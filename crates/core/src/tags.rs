//! Tags on files, searched with `tag:`. They come from several places:
//!
//! - rules, as a file is indexed: screenshots, and a project's language;
//! - tags the file carries itself, as Linux desktops keep them in `user.xdg.tags`;
//! - processors, like Describe's tags, and the labels it picks;
//! - labels found by meaning: a file whose text is close to a label's description,
//!   or to the files tagged with it by hand;
//! - tags given by hand, kept by content so they follow a file that's moved, and
//!   tags taken off by hand, which nothing puts back.

use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Embedder, Kind, hash, meaning::from_blob};

/// Where a tag on content came from.
pub(crate) const MANUAL: &str = "manual";
/// A tag taken off by hand; it hides the tag from every other source.
pub(crate) const REMOVED: &str = "removed";
pub(crate) const PLUGIN: &str = "plugin";
pub(crate) const PLUGIN_LABEL: &str = "plugin-label";
const MEANING: &str = "meaning";
const EXAMPLES: &str = "examples";

/// Tags made of a file's name, place and kind while it's indexed.
pub(crate) fn rules(path: &Path, name: &str, kind: Kind) -> Vec<String> {
    let mut tags = Vec::new();
    let lower = name.to_lowercase();
    let in_screenshots = path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|f| f.to_string_lossy().to_lowercase().contains("screenshot"));
    if kind == Kind::Image
        && (in_screenshots
            || [
                "screenshot",
                "screen shot",
                "螢幕截圖",
                "截圖",
                "スクリーンショット",
            ]
            .iter()
            .any(|s| lower.contains(s)))
    {
        tags.push("screenshot".to_owned());
    }
    if kind == Kind::Project {
        for (marker, language) in [
            ("Cargo.toml", "rust"),
            ("package.json", "javascript"),
            ("tsconfig.json", "typescript"),
            ("pyproject.toml", "python"),
            ("requirements.txt", "python"),
            ("go.mod", "go"),
            ("pom.xml", "java"),
            ("build.gradle", "java"),
            ("build.gradle.kts", "kotlin"),
            ("composer.json", "php"),
            ("Gemfile", "ruby"),
            ("pubspec.yaml", "dart"),
            ("deno.json", "typescript"),
            ("CMakeLists.txt", "c++"),
        ] {
            if path.join(marker).exists() && !tags.iter().any(|t| t == language) {
                tags.push(language.to_owned());
            }
        }
    }
    tags
}

/// Tags a file carries itself: on Linux, the comma-separated `user.xdg.tags` that
/// desktops like KDE's keep.
#[cfg(unix)]
pub(crate) fn own(path: &Path) -> Vec<String> {
    xattr::get(path, "user.xdg.tags")
        .ok()
        .flatten()
        .map(|value| {
            String::from_utf8_lossy(&value)
                .split(',')
                .map(clean)
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(unix))]
pub(crate) fn own(_: &Path) -> Vec<String> {
    Vec::new()
}

/// A tag as it's kept and matched: trimmed and lowercase, with spaces as dashes.
pub fn clean(tag: &str) -> String {
    tag.trim()
        .trim_start_matches('#')
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// Writes the tags a scan found for file `id`.
pub(crate) fn put_file_tags(conn: &Connection, id: i64, tags: &[String]) -> Result<()> {
    conn.execute("DELETE FROM file_tags WHERE file_id = ?1", [id])?;
    let mut put =
        conn.prepare_cached("INSERT OR IGNORE INTO file_tags (file_id, tag) VALUES (?1, ?2)")?;
    for tag in tags {
        put.execute(params![id, tag])?;
    }
    Ok(())
}

/// The tags of the file with id `id`, from every source, without those taken off.
pub(crate) fn of_file(conn: &Connection, id: i64) -> Result<Vec<String>> {
    let mut tags: Vec<String> = conn
        .prepare_cached(
            "SELECT tag FROM file_tags WHERE file_id = ?1
             UNION
             SELECT t.tag FROM files f JOIN cache.tags t ON t.hash = f.hash
             WHERE f.id = ?1 AND t.source != 'removed'
             EXCEPT
             SELECT t.tag FROM files f JOIN cache.tags t ON t.hash = f.hash
             WHERE f.id = ?1 AND t.source = 'removed'",
        )?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    tags.sort();
    Ok(tags)
}

/// The SQL condition, after a WHERE on `files f`, for files tagged `tag`, and its
/// arguments.
pub(crate) fn condition(tag: &str) -> (&'static str, [String; 3]) {
    (
        " AND (f.id IN (SELECT file_id FROM file_tags WHERE tag = ?)
               OR (f.hash IN (SELECT hash FROM cache.tags WHERE tag = ? AND source != 'removed')
                   AND f.hash NOT IN (SELECT hash FROM cache.tags WHERE tag = ? AND source = 'removed')))",
        [tag.to_owned(), tag.to_owned(), tag.to_owned()],
    )
}

/// The content hash of the file at `path`, reading the file if it wasn't hashed.
fn hash_of(conn: &Connection, path: &str) -> Result<Vec<u8>> {
    let known: Option<Option<Vec<u8>>> = conn
        .query_row("SELECT hash FROM files WHERE path = ?1", [path], |r| {
            r.get(0)
        })
        .optional()?;
    let known = known.with_context(|| format!("{path} isn't in the index"))?;
    if let Some(hash) = known {
        return Ok(hash);
    }
    let hash = hash::of_file(Path::new(path))
        .with_context(|| format!("couldn't read {path}"))?
        .to_vec();
    conn.execute(
        "UPDATE files SET hash = ?2 WHERE path = ?1",
        params![path, hash],
    )?;
    Ok(hash)
}

/// Tags the file at `path` by hand with `tag`, or takes it off. Either sticks to
/// the file's content, wherever it goes.
pub(crate) fn set(conn: &Connection, path: &str, tag: &str, on: bool) -> Result<()> {
    let tag = clean(tag);
    anyhow::ensure!(!tag.is_empty(), "a tag needs a name");
    let hash = hash_of(conn, path)?;
    conn.execute(
        "DELETE FROM cache.tags WHERE hash = ?1 AND tag = ?2 AND source IN ('manual', 'removed')",
        params![hash, tag],
    )?;
    conn.execute(
        "INSERT INTO cache.tags (hash, tag, source, score) VALUES (?1, ?2, ?3, 1)",
        params![hash, tag, if on { MANUAL } else { REMOVED }],
    )?;
    Ok(())
}

/// Tags given by hand, by how many files have each, most first.
pub(crate) fn manual(conn: &Connection) -> Result<Vec<(String, u64)>> {
    Ok(conn
        .prepare(
            "SELECT tag, count(*) FROM cache.tags WHERE source = 'manual'
             GROUP BY tag ORDER BY count(*) DESC, tag",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Keeps a processor's tags and labels for content `hash` among its tags.
pub(crate) fn put_plugin_tags(conn: &Connection, hash: &[u8]) -> Result<()> {
    conn.execute(
        "DELETE FROM cache.tags WHERE hash = ?1 AND source IN ('plugin', 'plugin-label')",
        [hash],
    )?;
    let outputs: Vec<(Option<String>, Option<String>)> = conn
        .prepare("SELECT tags, labels FROM cache.outputs WHERE hash = ?1 AND error IS NULL")?
        .query_map([hash], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut put = conn.prepare(
        "INSERT OR IGNORE INTO cache.tags (hash, tag, source, score) VALUES (?1, ?2, ?3, 1)",
    )?;
    for (tags, labels) in outputs {
        for (list, source) in [(tags, PLUGIN), (labels, PLUGIN_LABEL)] {
            for tag in list.unwrap_or_default().lines().map(clean) {
                if !tag.is_empty() {
                    put.execute(params![hash, tag, source])?;
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct LabelStats {
    /// Contents given a label by its description.
    pub by_meaning: u64,
    /// Contents given a label by being like the files tagged with it by hand.
    pub by_examples: u64,
}

/// How far above a model's cutoff a label's description has to score, and how far
/// ahead of the next label, for a file to get it: a description is a sentence,
/// and files near many sentences shouldn't get them all.
const LABEL_MARGIN: f32 = 0.15;
const AHEAD_BY: f32 = 0.03;
/// Hand-tagged files a label needs before others are tagged like them.
const MIN_EXAMPLES: usize = 2;

/// Gives labels to the content `embedder` has vectors of: each gets the label whose
/// description its text is closest to, when close enough, and every label whose
/// hand-tagged files it's as close to as they are to each other. Labels found this
/// way before are replaced.
pub(crate) fn learn(
    conn: &mut Connection,
    embedder: &mut dyn Embedder,
    labels: &BTreeMap<String, String>,
) -> Result<LabelStats> {
    let model = embedder.id().to_owned();
    // The text of each content, as the average of its chunks, and its best chunk
    // for each label's description.
    let mut contents: BTreeMap<Vec<u8>, Vec<Vec<f32>>> = BTreeMap::new();
    {
        let mut rows = conn.prepare("SELECT hash, vector FROM cache.vectors WHERE model = ?1")?;
        let mut rows = rows.query([&model])?;
        while let Some(row) = rows.next()? {
            let hash: Vec<u8> = row.get(0)?;
            let vector: Vec<f32> = from_blob(row.get_ref(1)?.as_blob()?).collect();
            contents.entry(hash).or_default().push(vector);
        }
    }
    let names: Vec<String> = labels.keys().cloned().collect();
    let descriptions: Vec<Vec<f32>> = labels
        .values()
        .map(|d| embedder.query(d))
        .collect::<Result<_>>()?;
    let cutoff = embedder.min_score() + LABEL_MARGIN;

    let mut found: Vec<(Vec<u8>, String, &str, f32)> = Vec::new();
    let mut means: BTreeMap<&Vec<u8>, Vec<f32>> = BTreeMap::new();
    for (hash, chunks) in &contents {
        let scores: Vec<f32> = descriptions.iter().map(|d| best_of(chunks, d)).collect();
        let mut ranked: Vec<(usize, f32)> = scores.into_iter().enumerate().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        if let Some(&(best, score)) = ranked.first()
            && score >= cutoff
            && ranked
                .get(1)
                .is_none_or(|(_, next)| score - next >= AHEAD_BY)
        {
            found.push((hash.clone(), names[best].clone(), MEANING, score));
        }
        means.insert(hash, unit(mean(chunks)));
    }

    // Labels learned from hand-tagged examples, when a label has enough of them.
    let manual: Vec<(Vec<u8>, String)> = conn
        .prepare("SELECT hash, tag FROM cache.tags WHERE source = 'manual'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut examples: BTreeMap<&str, Vec<&Vec<f32>>> = BTreeMap::new();
    for (hash, tag) in &manual {
        if let Some(mean) = means.get(hash) {
            examples.entry(tag).or_default().push(mean);
        }
    }
    let mut by_examples = 0;
    for (tag, vectors) in &examples {
        if vectors.len() < MIN_EXAMPLES {
            continue;
        }
        let center = unit(mean(
            &vectors.iter().map(|v| (*v).clone()).collect::<Vec<_>>(),
        ));
        // As close as the least typical example is.
        let radius = vectors
            .iter()
            .map(|v| dot(v, &center))
            .fold(f32::MAX, f32::min);
        for (hash, mean) in &means {
            let score = dot(mean, &center);
            if score >= radius.max(embedder.min_score()) {
                found.push(((*hash).clone(), (*tag).to_owned(), EXAMPLES, score));
                by_examples += 1;
            }
        }
    }

    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM cache.tags WHERE source IN ('meaning', 'examples')",
        [],
    )?;
    {
        let mut put = tx.prepare(
            "INSERT OR IGNORE INTO cache.tags (hash, tag, source, score) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (hash, tag, source, score) in &found {
            put.execute(params![hash, tag, source, score])?;
        }
    }
    tx.commit()?;
    Ok(LabelStats {
        by_meaning: found.iter().filter(|f| f.2 == MEANING).count() as u64,
        by_examples,
    })
}

/// How close a file's text is to a label: the average of its three closest chunks,
/// so one passage in a long document that happens to be near isn't enough.
fn best_of(chunks: &[Vec<f32>], label: &[f32]) -> f32 {
    let mut scores: Vec<f32> = chunks.iter().map(|c| dot(c, label)).collect();
    scores.sort_by(|a, b| b.total_cmp(a));
    let top = &scores[..scores.len().min(3)];
    top.iter().sum::<f32>() / top.len().max(1) as f32
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn mean(vectors: &[Vec<f32>]) -> Vec<f32> {
    let dims = vectors.first().map_or(0, Vec::len);
    let mut sum = vec![0.0; dims];
    for v in vectors {
        for (s, x) in sum.iter_mut().zip(v) {
            *s += x;
        }
    }
    sum
}

fn unit(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_tag_screenshots() {
        let tags = rules(
            Path::new("/home/me/Pictures/Screenshots/a.png"),
            "a.png",
            Kind::Image,
        );
        assert_eq!(tags, ["screenshot"]);
        let tags = rules(
            Path::new("/home/me/Desktop/Screenshot 2026-01-01.png"),
            "Screenshot 2026-01-01.png",
            Kind::Image,
        );
        assert_eq!(tags, ["screenshot"]);
        assert!(rules(Path::new("/home/me/a.png"), "a.png", Kind::Image).is_empty());
    }

    #[test]
    fn projects_are_tagged_with_their_language() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        std::fs::write(dir.path().join("package.json"), "").unwrap();
        let tags = rules(dir.path(), "app", Kind::Project);
        assert_eq!(tags, ["rust", "javascript"]);
    }

    #[test]
    fn tags_are_cleaned() {
        assert_eq!(clean("  #Bank Statement "), "bank-statement");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_tags_files_carry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "").unwrap();
        // Some file systems, like tmpfs on older kernels, have no user attributes.
        if xattr::set(&path, "user.xdg.tags", b"Taxes,2024 ").is_ok() {
            assert_eq!(own(&path), ["taxes", "2024"]);
        }
    }
}
