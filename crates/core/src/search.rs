use std::path::MAIN_SEPARATOR;

use anyhow::Result;
use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    DEFAULT_LIMIT, Embedder, Kind, Level, Query, Term, Within,
    meaning::{Source, Store},
    text,
    words::words,
};

const AFTER_SEPARATOR: char = (MAIN_SEPARATOR as u8 + 1) as char;

#[derive(Debug)]
pub struct Hit {
    pub path: String,
    pub name: String,
    pub kind: Kind,
    pub size: Option<u64>,
    pub mtime: i64,
    /// The line that matched, when the file matched by its text and not its name.
    pub line: Option<String>,
}

/// The columns a word is matched against.
#[derive(Clone, Copy, PartialEq)]
enum Scope {
    /// Names and folders only.
    Names,
    /// Names, folders and, for long enough words, the text inside files.
    Everything,
}

/// A model and its vectors, to search by meaning as well as words.
pub(crate) struct Meaning<'a> {
    pub embedder: &'a mut dyn Embedder,
    pub store: &'a mut Option<Store>,
}

/// How many vectors a search by meaning looks at, before filters.
const NEAREST: usize = 400;
/// Reciprocal rank fusion's constant: higher evens out the weight of top ranks.
const FUSION_K: f64 = 60.0;

pub(crate) fn search(
    conn: &Connection,
    q: &Query,
    now: i64,
    meaning: Option<Meaning>,
) -> Result<Vec<Hit>> {
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT);
    let (filters, filter_args) = filters(q);
    let text = join(q.terms.iter().map(|t| term_expr(t, Scope::Everything)), " ");

    let mut sql = String::from("SELECT f.id, ");
    let mut args: Vec<Value> = Vec::new();
    match &text {
        Some(expr) => {
            let by_name =
                join(q.terms.iter().map(|t| term_expr(t, Scope::Names)), " ").unwrap_or_default();
            sql.push_str(
                "f.id IN (SELECT rowid FROM files_fts WHERE files_fts MATCH ?) AS by_name
                 FROM files_fts JOIN files f ON f.id = files_fts.rowid WHERE files_fts MATCH ?",
            );
            args.push(Value::Text(by_name));
            args.push(Value::Text(expr.clone()));
        }
        None => sql.push_str("1 AS by_name FROM files f WHERE 1"),
    }
    sql.push_str(&filters);
    args.extend(filter_args.iter().cloned());
    if text.is_some() {
        sql.push_str(
            " ORDER BY by_name DESC, bm25(files_fts, 10.0, 1.0, 1.0)
                * (CASE WHEN f.project_id IS NULL THEN 1.0 ELSE 0.3 END)
                * (1.0 + 1.0 / (1.0 + max(0, ? - f.mtime) / 604800.0))",
        );
        args.push(Value::Integer(now));
    } else {
        sql.push_str(" ORDER BY f.project_id IS NOT NULL, f.mtime DESC");
    }
    sql.push_str(" LIMIT ?");
    args.push(Value::Integer(limit as i64));
    let by_words: Vec<(i64, bool)> = conn
        .prepare(&sql)?
        .query_map(params_from_iter(args), |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;

    let by_meaning = match (meaning, meaning_text(q)) {
        (Some(meaning), Some(text)) => nearest_files(conn, &text, meaning, &filters, &filter_args)?,
        _ => Vec::new(),
    };

    // Reciprocal rank fusion: a file's score adds up over both lists, so one found
    // by its words and its meaning comes first, and either alone still counts.
    let mut fused: Vec<(i64, f64, Option<Found>)> = Vec::new();
    for (rank, (id, by_name)) in by_words.iter().enumerate() {
        let found = if *by_name { Found::Name } else { Found::Words };
        fused.push((*id, 1.0 / (FUSION_K + rank as f64 + 1.0), Some(found)));
    }
    for (rank, (id, _, chunk)) in by_meaning.iter().enumerate() {
        let score = 1.0 / (FUSION_K + rank as f64 + 1.0);
        match fused.iter_mut().find(|(known, ..)| known == id) {
            Some(entry) => entry.1 += score,
            None => fused.push((*id, score, chunk.map(Found::Meaning))),
        }
    }
    if !by_meaning.is_empty() {
        fused.sort_by(|a, b| b.1.total_cmp(&a.1));
    }
    fused.truncate(limit);

    let wanted: Vec<String> = q
        .terms
        .iter()
        .flat_map(|t| {
            words(&t.text)
                .to_lowercase()
                .split(' ')
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|w| !w.is_empty())
        .collect();
    let mut row = conn.prepare(
        "SELECT f.path, f.name, f.kind, f.size, f.mtime, c.text
         FROM files f LEFT JOIN cache.contents c ON c.hash = f.hash WHERE f.id = ?1",
    )?;
    let mut hits = Vec::with_capacity(fused.len());
    for (id, _, found) in fused {
        let (mut hit, text) = row.query_row([id], |r| {
            let kind: String = r.get(2)?;
            let size: Option<i64> = r.get(3)?;
            let hit = Hit {
                path: r.get(0)?,
                name: r.get(1)?,
                kind: Kind::from_name(&kind).unwrap_or(Kind::Other),
                size: size.map(|s| s as u64),
                mtime: r.get(4)?,
                line: None,
            };
            Ok((hit, r.get::<_, Option<String>>(5)?))
        })?;
        hit.line = match (found, text) {
            (Some(Found::Words), Some(text)) => text::matching_line(&text, &wanted),
            (Some(Found::Meaning((start, end))), Some(text)) => {
                text.get(start..end).and_then(|chunk| {
                    text::matching_line(chunk, &wanted).or_else(|| text::first_line(chunk))
                })
            }
            _ => None,
        };
        hits.push(hit);
    }
    Ok(hits)
}

/// How a file was found, for the line shown with it.
#[derive(Clone)]
enum Found {
    Name,
    Words,
    /// By the meaning of this byte range of its text, or of its name when `None`.
    Meaning((usize, usize)),
}

/// The text to search by meaning: the query's words, when there are enough of them
/// to carry a meaning and none asks for exact or name-only matches.
fn meaning_text(q: &Query) -> Option<String> {
    if q.terms.iter().any(|t| t.exact || t.name_only) {
        return None;
    }
    let text = q
        .terms
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let long_enough = text.chars().count() >= 4 || (!text.is_ascii() && text.chars().count() >= 2);
    long_enough.then_some(text)
}

/// A file found by meaning: its id, score, and the byte range of its text that
/// matched, or `None` when its name did.
type Near = (i64, f32, Option<(usize, usize)>);

/// Files whose text or name is nearest in meaning to `text`, best first, with the
/// chunk that matched when it was their text.
fn nearest_files(
    conn: &Connection,
    text: &str,
    meaning: Meaning,
    filters: &str,
    filter_args: &[Value],
) -> Result<Vec<Near>> {
    Store::refresh(conn, meaning.store, meaning.embedder.id())?;
    let Some(store) = meaning.store.as_ref() else {
        return Ok(Vec::new());
    };
    let query = meaning.embedder.query(text)?;
    let nearest = store.nearest(&query, NEAREST, meaning.embedder.min_score());
    if nearest.is_empty() {
        return Ok(Vec::new());
    }

    let mut hashes: Vec<Value> = Vec::new();
    let mut names: Vec<Value> = Vec::new();
    for (_, source) in &nearest {
        match source {
            Source::Chunk { hash, .. } => hashes.push(Value::Blob(hash.to_vec())),
            Source::Name(name) => names.push(Value::Text(name.clone())),
        }
    }
    let mut sql = format!(
        "SELECT f.id, f.hash, f.name FROM files f
         WHERE f.level >= {} AND (f.hash IN ({}) OR f.name IN ({}))",
        Level::Meaning.as_int(),
        placeholders(hashes.len()),
        placeholders(names.len()),
    );
    sql.push_str(filters);
    let args = hashes
        .into_iter()
        .chain(names)
        .chain(filter_args.iter().cloned());
    let files: Vec<(i64, Option<Vec<u8>>, String)> = conn
        .prepare(&sql)?
        .query_map(params_from_iter(args), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<Result<_, _>>()?;

    // Each file takes the score of its best match.
    let mut best: Vec<Near> = Vec::new();
    for (score, source) in nearest {
        for (id, hash, name) in &files {
            let matched = match source {
                Source::Chunk {
                    hash: h,
                    start,
                    end,
                } => (hash.as_deref() == Some(&h[..])).then_some(Some((*start, *end))),
                Source::Name(n) => (n == name).then_some(None),
            };
            if let Some(chunk) = matched
                && !best.iter().any(|(known, ..)| known == id)
            {
                best.push((*id, score, chunk));
            }
        }
    }
    Ok(best)
}

fn placeholders(n: usize) -> String {
    if n == 0 {
        return "NULL".to_owned();
    }
    vec!["?"; n].join(", ")
}

/// The conditions of a query's filters, as SQL to add after a WHERE on `files f`,
/// and their arguments.
fn filters(q: &Query) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let mut args = Vec::new();
    if q.kinds.is_empty() && q.exts.is_empty() && q.within.is_empty() {
        sql.push_str(" AND f.project_id IS NULL");
    }
    let excluded = q.exclude.iter().map(|word| {
        let term = Term {
            text: word.clone(),
            exact: false,
            name_only: false,
        };
        term_expr(&term, Scope::Names)
    });
    if let Some(expr) = join(excluded, " OR ") {
        sql.push_str(" AND f.id NOT IN (SELECT rowid FROM files_fts WHERE files_fts MATCH ?)");
        args.push(Value::Text(expr));
    }
    in_list(
        &mut sql,
        &mut args,
        "f.kind",
        q.kinds.iter().map(|k| k.as_str().to_owned()),
    );
    in_list(&mut sql, &mut args, "f.ext", q.exts.iter().cloned());
    if !q.within.is_empty() {
        let conditions: Vec<&str> = q
            .within
            .iter()
            .map(|w| match w {
                Within::Path(path) => {
                    args.push(Value::Text(format!("{path}{MAIN_SEPARATOR}")));
                    args.push(Value::Text(format!("{path}{AFTER_SEPARATOR}")));
                    "(f.path >= ? AND f.path < ?)"
                }
                Within::Folder(folder) => {
                    let folder = escape_like(folder);
                    args.push(Value::Text(format!(
                        "%{MAIN_SEPARATOR}{folder}{MAIN_SEPARATOR}%"
                    )));
                    "f.path LIKE ? ESCAPE '^'"
                }
            })
            .collect();
        sql.push_str(&format!(" AND ({})", conditions.join(" OR ")));
    }
    let bounds = [
        (" AND f.mtime >= ?", q.modified_after),
        (" AND f.mtime < ?", q.modified_before),
        (" AND f.size > ?", q.size_above.map(|b| b as i64)),
        (" AND f.size < ?", q.size_below.map(|b| b as i64)),
    ];
    for (condition, bound) in bounds {
        if let Some(value) = bound {
            sql.push_str(condition);
            args.push(Value::Integer(value));
        }
    }
    (sql, args)
}

fn term_expr(term: &Term, scope: Scope) -> Option<String> {
    let phrase = words(&term.text);
    if phrase.is_empty() {
        return None;
    }
    let star = if term.exact { "" } else { "*" };
    // Short prefixes of Latin words would match the text of nearly every file, but
    // a single CJK character is a word of its own.
    let text_worthy = term.text.chars().count() >= 3 || !term.text.is_ascii();
    let column = if term.name_only {
        "name : "
    } else if scope == Scope::Names || !text_worthy {
        "{name dirs} : "
    } else {
        ""
    };
    Some(format!("{column}\"{phrase}\"{star}"))
}

fn join(parts: impl Iterator<Item = Option<String>>, separator: &str) -> Option<String> {
    let parts: Vec<String> = parts.flatten().collect();
    (!parts.is_empty()).then(|| parts.join(separator))
}

fn in_list(
    sql: &mut String,
    args: &mut Vec<Value>,
    column: &str,
    values: impl Iterator<Item = String>,
) {
    let values: Vec<Value> = values.map(Value::Text).collect();
    if values.is_empty() {
        return;
    }
    let placeholders = vec!["?"; values.len()].join(", ");
    sql.push_str(&format!(" AND {column} IN ({placeholders})"));
    args.extend(values);
}

fn escape_like(text: &str) -> String {
    text.replace('^', "^^")
        .replace('%', "^%")
        .replace('_', "^_")
}
