use std::path::MAIN_SEPARATOR;

use anyhow::Result;
use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{DEFAULT_LIMIT, Kind, Query, Term, Within, words::words};

const AFTER_SEPARATOR: char = (MAIN_SEPARATOR as u8 + 1) as char;

#[derive(Debug)]
pub struct Hit {
    pub path: String,
    pub name: String,
    pub kind: Kind,
    pub size: Option<u64>,
    pub mtime: i64,
}

pub(crate) fn search(conn: &Connection, q: &Query, now: i64) -> Result<Vec<Hit>> {
    let mut sql = String::from("SELECT f.path, f.name, f.kind, f.size, f.mtime FROM ");
    let mut args: Vec<Value> = Vec::new();

    let text = join(q.terms.iter().map(term_expr), " ");
    match &text {
        Some(expr) => {
            sql.push_str(
                "files_fts JOIN files f ON f.id = files_fts.rowid WHERE files_fts MATCH ?",
            );
            args.push(Value::Text(expr.clone()));
        }
        None => sql.push_str("files f WHERE 1"),
    }

    if q.kinds.is_empty() && q.exts.is_empty() && q.within.is_empty() {
        sql.push_str(" AND f.project_id IS NULL");
    }
    let excluded = q.exclude.iter().map(|word| {
        term_expr(&Term {
            text: word.clone(),
            exact: false,
            name_only: false,
        })
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

    if text.is_some() {
        sql.push_str(
            " ORDER BY bm25(files_fts, 10.0, 1.0)
                * (CASE WHEN f.project_id IS NULL THEN 1.0 ELSE 0.3 END)
                * (1.0 + 1.0 / (1.0 + max(0, ? - f.mtime) / 604800.0))",
        );
        args.push(Value::Integer(now));
    } else {
        sql.push_str(" ORDER BY f.project_id IS NOT NULL, f.mtime DESC");
    }
    sql.push_str(" LIMIT ?");
    args.push(Value::Integer(q.limit.unwrap_or(DEFAULT_LIMIT) as i64));

    let mut stmt = conn.prepare(&sql)?;
    let hits = stmt
        .query_map(params_from_iter(args), |r| {
            let kind: String = r.get(2)?;
            let size: Option<i64> = r.get(3)?;
            Ok(Hit {
                path: r.get(0)?,
                name: r.get(1)?,
                kind: Kind::from_name(&kind).unwrap_or(Kind::Other),
                size: size.map(|s| s as u64),
                mtime: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(hits)
}

fn term_expr(term: &Term) -> Option<String> {
    let phrase = words(&term.text);
    if phrase.is_empty() {
        return None;
    }
    let star = if term.exact { "" } else { "*" };
    let column = if term.name_only { "name : " } else { "" };
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
