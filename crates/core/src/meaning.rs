//! Searching by meaning: file text and names turned into vectors by a model, and
//! queries compared against them.

use anyhow::Result;
use std::path::{MAIN_SEPARATOR, PathBuf};

use anyhow::Context;
use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{Kind, Level, hash::Hash};

/// A model that turns text into vectors of unit length, so the dot product of two
/// is how alike their meanings are.
pub trait Embedder: Send {
    /// Names the model and how it's used, so vectors of different models are never
    /// compared.
    fn id(&self) -> &str;
    /// Vectors for text to be found.
    fn passages(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    /// The vector for a search.
    fn query(&mut self, text: &str) -> Result<Vec<f32>>;
    /// Scores below this mean unrelated.
    fn min_score(&self) -> f32;
    /// Whether the model runs on this computer. Text is only sent to one that
    /// doesn't from folders the settings don't keep private.
    fn is_local(&self) -> bool {
        true
    }
}

/// How long a piece of text embedded on its own is, in bytes. About a paragraph:
/// long enough to carry a topic, short enough not to blur several together.
const CHUNK_BYTES: usize = 1000;
/// Changes when chunks are cut differently, so text is embedded again.
const CHUNKING: &str = "lines-1";

/// Pieces of a file's text, as byte ranges, cut at line ends where they can be.
pub(crate) fn chunks(text: &str) -> Vec<(usize, usize)> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut end = 0;
    for line in text.split_inclusive('\n') {
        let line_end = end + line.len();
        if line_end - start > CHUNK_BYTES && end > start {
            chunks.push((start, end));
            start = end;
        }
        // A line too long on its own is cut at spaces, or anywhere.
        while line_end - start > CHUNK_BYTES {
            let mut cut = start + CHUNK_BYTES;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            if let Some(space) = text[start..cut].rfind(' ').filter(|&s| s > CHUNK_BYTES / 2) {
                cut = start + space + 1;
            }
            chunks.push((start, cut));
            start = cut;
        }
        end = line_end;
    }
    if end > start && !text[start..end].trim().is_empty() {
        chunks.push((start, end));
    }
    chunks
}

/// The text embedded for a chunk. Spreadsheet rows carry the header row, so a row
/// of numbers says what its columns are.
fn passage(text: &str, (start, end): (usize, usize), kind: Kind) -> String {
    let chunk = text[start..end].trim();
    match (kind, text.lines().next()) {
        (Kind::Sheet, Some(header)) if start > 0 => format!("{header}\n{chunk}"),
        _ => chunk.to_owned(),
    }
}

/// The words of a file name, without its extension, as a model reads them best:
/// separators become spaces and camelCase is split, while CJK text stays whole.
fn name_passage(name: &str) -> String {
    let stem = name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map_or(name, |(stem, _)| stem);
    let mut out = String::with_capacity(stem.len() + 4);
    let mut prev: Option<char> = None;
    for c in stem.chars() {
        if c.is_alphanumeric() {
            if prev.is_some_and(|p| p.is_lowercase() && c.is_uppercase()) {
                out.push(' ');
            }
            out.push(c);
            prev = Some(c);
        } else {
            out.push(' ');
            prev = None;
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Default)]
pub struct EmbedStats {
    pub names: u64,
    pub files: u64,
    pub chunks: u64,
    /// Whether it stopped early because it was asked to.
    pub stopped: bool,
}

/// Names embedded per model call.
const NAME_BATCH: usize = 256;
/// Chunks embedded per model call, and files per transaction.
const CHUNK_BATCH: usize = 64;
const FILE_BATCH: usize = 16;

/// Embeds the names and text of files searched by meaning that `embedder` hasn't
/// embedded yet, in small transactions so searches and scans aren't held up.
/// `progress` is told what's done after each batch, and stops it by returning false.
pub(crate) fn embed(
    conn: &mut Connection,
    embedder: &mut dyn Embedder,
    private: &[PathBuf],
    progress: &mut dyn FnMut(&EmbedStats) -> bool,
) -> Result<EmbedStats> {
    let model = embedder.id().to_owned();
    let meaning = Level::Meaning.as_int();
    let mut stats = EmbedStats::default();
    // Names bind three arguments, and text four, before those.
    let (shared_names, shared_names_args) = shareable(&*embedder, private, 4)?;
    let (shared, shared_args) = shareable(&*embedder, private, 5)?;

    loop {
        if !progress(&stats) {
            stats.stopped = true;
            return Ok(stats);
        }
        let names: Vec<String> = conn
            .prepare(&format!(
                "SELECT DISTINCT name FROM files
                 WHERE level >= ?1
                   AND name NOT IN (SELECT name FROM cache.names WHERE model = ?2){shared_names}
                 LIMIT ?3"
            ))?
            .query_map(
                params_from_iter(
                    [
                        Value::Integer(meaning),
                        Value::Text(model.clone()),
                        Value::Integer(NAME_BATCH as i64),
                    ]
                    .into_iter()
                    .chain(shared_names_args.iter().cloned()),
                ),
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        if names.is_empty() {
            break;
        }
        let passages: Vec<String> = names.iter().map(|n| name_passage(n)).collect();
        let vectors = embedder.passages(&passages)?;
        let tx = conn.transaction()?;
        {
            let mut put = tx.prepare(
                "INSERT OR REPLACE INTO cache.names (model, name, vector) VALUES (?1, ?2, ?3)",
            )?;
            for (name, vector) in names.iter().zip(&vectors) {
                put.execute(params![model, name, to_blob(vector)])?;
            }
        }
        bump_version(&tx)?;
        tx.commit()?;
        stats.names += names.len() as u64;
    }

    loop {
        if !progress(&stats) {
            stats.stopped = true;
            return Ok(stats);
        }
        let pending: Vec<(Hash, String, String, String)> = conn
            .prepare(&format!(
                "SELECT t.hash, t.policy, t.text,
                        (SELECT kind FROM files f WHERE f.hash = t.hash LIMIT 1)
                 FROM cache.full_texts t
                 WHERE t.hash IN (
                       SELECT hash FROM files
                       WHERE level >= ?1 AND hash IS NOT NULL{shared})
                   AND NOT EXISTS (
                       SELECT 1 FROM cache.embedded e
                       WHERE e.model = ?2 AND e.hash = t.hash
                         AND e.policy = ?3 || '/' || t.policy)
                 LIMIT ?4"
            ))?
            .query_map(
                params_from_iter(
                    [
                        Value::Integer(meaning),
                        Value::Text(model.clone()),
                        Value::Text(CHUNKING.to_owned()),
                        Value::Integer(FILE_BATCH as i64),
                    ]
                    .into_iter()
                    .chain(shared_args.iter().cloned()),
                ),
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?
            .collect::<rusqlite::Result<_>>()?;
        if pending.is_empty() {
            break;
        }

        let mut embedded = Vec::with_capacity(pending.len());
        for (hash, policy, text, kind) in &pending {
            let kind = Kind::from_name(kind).unwrap_or(Kind::Other);
            let ranges = chunks(text);
            let mut vectors = Vec::with_capacity(ranges.len());
            for batch in ranges.chunks(CHUNK_BATCH) {
                let passages: Vec<String> = batch.iter().map(|&r| passage(text, r, kind)).collect();
                vectors.extend(embedder.passages(&passages)?);
                if !progress(&stats) {
                    stats.stopped = true;
                    return Ok(stats);
                }
            }
            embedded.push((hash, policy, ranges, vectors));
        }

        let tx = conn.transaction()?;
        {
            let mut clear =
                tx.prepare("DELETE FROM cache.vectors WHERE model = ?1 AND hash = ?2")?;
            let mut put = tx.prepare(
                "INSERT INTO cache.vectors (model, hash, chunk, start, end, vector)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            let mut done = tx.prepare(
                "INSERT OR REPLACE INTO cache.embedded (model, hash, policy) VALUES (?1, ?2, ?3)",
            )?;
            for (hash, policy, ranges, vectors) in &embedded {
                clear.execute(params![model, hash])?;
                for (i, ((start, end), vector)) in ranges.iter().zip(vectors).enumerate() {
                    put.execute(params![
                        model,
                        hash,
                        i as i64,
                        *start as i64,
                        *end as i64,
                        to_blob(vector)
                    ])?;
                }
                done.execute(params![model, hash, format!("{CHUNKING}/{policy}")])?;
                stats.chunks += ranges.len() as u64;
            }
        }
        bump_version(&tx)?;
        tx.commit()?;
        stats.files += pending.len() as u64;
    }
    Ok(stats)
}

/// A condition on `files` that leaves out private folders when the model isn't
/// local, to add after a WHERE, and its arguments, numbered from `first`.
fn shareable(
    embedder: &dyn Embedder,
    private: &[PathBuf],
    first: usize,
) -> Result<(String, Vec<Value>)> {
    if embedder.is_local() || private.is_empty() {
        return Ok((String::new(), Vec::new()));
    }
    let mut sql = String::new();
    let mut args = Vec::new();
    for folder in private {
        let folder = folder
            .to_str()
            .context("a private folder's path isn't UTF-8")?;
        let folder = folder.trim_end_matches(MAIN_SEPARATOR);
        let n = first + args.len();
        sql.push_str(&format!(
            " AND NOT (path = ?{n} OR (path >= ?{} AND path < ?{}))",
            n + 1,
            n + 2
        ));
        args.push(Value::Text(folder.to_owned()));
        args.push(Value::Text(format!("{folder}{MAIN_SEPARATOR}")));
        args.push(Value::Text(format!(
            "{folder}{}",
            (MAIN_SEPARATOR as u8 + 1) as char
        )));
    }
    Ok((sql, args))
}

/// How many names and files `model` has yet to embed.
pub(crate) fn pending(conn: &Connection, model: &str) -> Result<(u64, u64)> {
    let meaning = Level::Meaning.as_int();
    let names: i64 = conn.query_row(
        "SELECT count(DISTINCT name) FROM files
         WHERE level >= ?1 AND name NOT IN (SELECT name FROM cache.names WHERE model = ?2)",
        params![meaning, model],
        |r| r.get(0),
    )?;
    let files: i64 = conn.query_row(
        "SELECT count(*) FROM cache.full_texts t
         WHERE t.hash IN (SELECT hash FROM files WHERE level >= ?1 AND hash IS NOT NULL)
           AND NOT EXISTS (
               SELECT 1 FROM cache.embedded e
               WHERE e.model = ?2 AND e.hash = t.hash
                 AND e.policy = ?3 || '/' || t.policy)",
        params![meaning, model, CHUNKING],
        |r| r.get(0),
    )?;
    Ok((names as u64, files as u64))
}

/// Counts changes to the vectors, so searches know when to load them again.
fn bump_version(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE cache.vectors_version SET version = version + 1", [])?;
    Ok(())
}

fn version(conn: &Connection) -> Result<i64> {
    Ok(
        conn.query_row("SELECT version FROM cache.vectors_version", [], |r| {
            r.get(0)
        })?,
    )
}

fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub(crate) fn from_blob(blob: &[u8]) -> impl Iterator<Item = f32> + '_ {
    blob.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// What a vector in the store stands for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Source {
    /// A piece of the text of the files with this content.
    Chunk {
        hash: Hash,
        start: usize,
        end: usize,
    },
    /// The files with this name.
    Name(String),
}

/// Every vector of one model, held in memory, since comparing a query against tens
/// of thousands of them takes milliseconds while reading them takes longer.
pub(crate) struct Store {
    model: String,
    version: i64,
    dims: usize,
    vectors: Vec<f32>,
    sources: Vec<Source>,
}

impl Store {
    /// The store for `model`, loaded again only when the vectors have changed.
    pub(crate) fn refresh(conn: &Connection, store: &mut Option<Store>, model: &str) -> Result<()> {
        let version = version(conn)?;
        if store
            .as_ref()
            .is_some_and(|s| s.model == model && s.version == version)
        {
            return Ok(());
        }
        let mut loaded = Store {
            model: model.to_owned(),
            version,
            dims: 0,
            vectors: Vec::new(),
            sources: Vec::new(),
        };
        let mut chunks =
            conn.prepare("SELECT hash, start, end, vector FROM cache.vectors WHERE model = ?1")?;
        let mut rows = chunks.query([model])?;
        while let Some(row) = rows.next()? {
            let source = Source::Chunk {
                hash: row.get(0)?,
                start: row.get::<_, i64>(1)? as usize,
                end: row.get::<_, i64>(2)? as usize,
            };
            loaded.push(source, row.get_ref(3)?.as_blob()?);
        }
        let mut names = conn.prepare("SELECT name, vector FROM cache.names WHERE model = ?1")?;
        let mut rows = names.query([model])?;
        while let Some(row) = rows.next()? {
            loaded.push(Source::Name(row.get(0)?), row.get_ref(1)?.as_blob()?);
        }
        *store = Some(loaded);
        Ok(())
    }

    fn push(&mut self, source: Source, blob: &[u8]) {
        let dims = blob.len() / 4;
        if self.dims == 0 {
            self.dims = dims;
        }
        // A vector of another length is from a model changed under the same id.
        if dims == self.dims {
            self.vectors.extend(from_blob(blob));
            self.sources.push(source);
        }
    }

    /// The `k` sources most like `query`, scoring at least `min`, best first.
    pub(crate) fn nearest(&self, query: &[f32], k: usize, min: f32) -> Vec<(f32, &Source)> {
        if query.len() != self.dims {
            return Vec::new();
        }
        let mut scored: Vec<(f32, usize)> = self
            .vectors
            .chunks_exact(self.dims)
            .enumerate()
            .map(|(i, v)| (v.iter().zip(query).map(|(a, b)| a * b).sum(), i))
            .filter(|&(score, _)| score >= min)
            .collect();
        let k = k.min(scored.len());
        if k == 0 {
            return Vec::new();
        }
        scored.select_nth_unstable_by(k - 1, |a, b| b.0.total_cmp(&a.0));
        scored.truncate(k);
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored
            .into_iter()
            .map(|(score, i)| (score, &self.sources[i]))
            .collect()
    }
}

/// Forgets vectors of content the cache no longer has, and of names no file has.
pub(crate) fn forget_unused(conn: &Connection) -> Result<()> {
    let removed = conn.execute(
        "DELETE FROM cache.vectors WHERE hash NOT IN (SELECT hash FROM cache.full_texts)",
        [],
    )? + conn.execute(
        "DELETE FROM cache.embedded WHERE hash NOT IN (SELECT hash FROM cache.full_texts)",
        [],
    )? + conn.execute(
        "DELETE FROM cache.names WHERE name NOT IN (SELECT name FROM main.files)",
        [],
    )?;
    if removed > 0 {
        bump_version(conn)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_cover_the_text_at_line_ends() {
        let line = "word ".repeat(30) + "\n";
        let text = line.repeat(20);
        let pieces = chunks(&text);
        assert!(pieces.len() > 1);
        assert_eq!(pieces[0].0, 0);
        assert_eq!(pieces.last().unwrap().1, text.len());
        for window in pieces.windows(2) {
            assert_eq!(window[0].1, window[1].0);
        }
        for &(start, end) in &pieces {
            assert!(end - start <= CHUNK_BYTES);
            assert!(text[..end].ends_with('\n'));
        }
    }

    #[test]
    fn long_lines_are_cut_at_spaces() {
        let text = "日本語 ".repeat(400);
        let pieces = chunks(&text);
        assert!(pieces.len() > 1);
        for &(start, end) in &pieces {
            assert!(end - start <= CHUNK_BYTES);
            assert!(text.is_char_boundary(start) && text.is_char_boundary(end));
        }
        assert_eq!(chunks("   \n\n"), []);
    }

    #[test]
    fn sheet_rows_carry_their_header() {
        let text = format!("Item\tCost\n{}", "Paper\t12\n".repeat(200));
        let pieces = chunks(&text);
        assert!(passage(&text, pieces[1], Kind::Sheet).starts_with("Item\tCost\nPaper"));
        assert!(passage(&text, pieces[1], Kind::Doc).starts_with("Paper"));
    }

    #[test]
    fn names_lose_their_extension() {
        assert_eq!(name_passage("backupPhotos.sh"), "backup Photos");
        assert_eq!(name_passage(".bashrc"), "bashrc");
        assert_eq!(name_passage("2024_報銷發票.pdf"), "2024 報銷發票");
    }
}
