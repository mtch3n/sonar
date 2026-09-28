//! Files that are copies of each other, or look like it: the same content, pictures
//! and videos that look the same by their perceptual fingerprints, and names that
//! are another's with "(1)" or "copy" added. Nothing is ever removed; the groups
//! only point at the copies.

use std::{collections::HashMap, path::Path};

use anyhow::Result;
use rusqlite::{Connection, params, params_from_iter, types::Value};

use crate::{Hit, Kind, hash};

/// Smaller files are left out of exact duplicates and look-alikes: they cost next
/// to nothing, empty or boilerplate files would crowd out the ones that matter, and
/// icons of flat color all look alike to a fingerprint.
pub const MIN_SIZE: u64 = 16 * 1024;
/// How many bits two fingerprints may differ in and still look the same, unless a
/// search asks otherwise.
pub const DEFAULT_DISTANCE: u32 = 8;

/// How the files of a group are alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Likeness {
    /// The same content, byte for byte.
    Same,
    /// Pictures or videos that look the same, their fingerprints differing in at
    /// most this many bits.
    Looks(u32),
    /// Names that are one name with a copy's mark, like "report (1).pdf".
    Name,
}

/// Which kinds of alikeness to look for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub same: bool,
    pub looks: Option<u32>,
    pub names: bool,
}

impl Default for Wanted {
    fn default() -> Wanted {
        Wanted {
            same: true,
            looks: Some(DEFAULT_DISTANCE),
            names: true,
        }
    }
}

#[derive(Debug)]
pub struct Group {
    pub likeness: Likeness,
    pub files: Vec<Hit>,
    /// What keeping only the largest file would free, in bytes.
    pub wasted: u64,
}

/// A file and its content hash, if it has one.
type Hashed = (File, Option<Vec<u8>>);
/// A file with its fingerprint's algorithm and bits, its length if it's a video,
/// and its content hash.
type Printed = (File, String, u64, Option<f64>, Vec<u8>);

/// A file as the duplicate finder sees it.
struct File {
    id: i64,
    path: String,
    name: String,
    kind: String,
    size: u64,
    mtime: i64,
}

/// Groups of files alike in the ways `wanted` asks, among the files `filters` (SQL
/// after a WHERE on `files f`) allow, most space wasted first.
pub(crate) fn groups(
    conn: &Connection,
    wanted: Wanted,
    filters: &str,
    args: &[Value],
) -> Result<Vec<Group>> {
    let mut groups = Vec::new();
    if wanted.same {
        groups.extend(same(conn, filters, args)?);
    }
    if let Some(distance) = wanted.looks {
        groups.extend(looks(conn, distance, filters, args)?);
    }
    if wanted.names {
        groups.extend(names(conn, filters, args)?);
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.wasted));
    Ok(groups)
}

fn files(conn: &Connection, sql: &str, args: &[Value]) -> Result<Vec<Hashed>> {
    Ok(conn
        .prepare(sql)?
        .query_map(params_from_iter(args.iter().cloned()), |r| {
            Ok((
                File {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    name: r.get(2)?,
                    kind: r.get(3)?,
                    size: r.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                    mtime: r.get(5)?,
                },
                r.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?)
}

const COLUMNS: &str = "f.id, f.path, f.name, f.kind, f.size, f.mtime, f.hash";
const NOT_FOLDERS: &str = "f.kind NOT IN ('folder', 'project', 'app')";

/// Files of the same size, hashed where they weren't already, grouped by hash.
fn same(conn: &Connection, filters: &str, args: &[Value]) -> Result<Vec<Group>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM files f
         WHERE {NOT_FOLDERS} AND f.size >= {MIN_SIZE}{filters}
           AND f.size IN (
               SELECT size FROM files f
               WHERE {NOT_FOLDERS} AND f.size >= {MIN_SIZE}{filters}
               GROUP BY size HAVING count(*) > 1)"
    );
    let mut both = args.to_vec();
    both.extend(args.iter().cloned());
    let mut by_hash: HashMap<Vec<u8>, Vec<File>> = HashMap::new();
    let mut remember = conn.prepare("UPDATE files SET hash = ?2 WHERE id = ?1")?;
    for (file, hash) in files(conn, &sql, &both)? {
        let hash = match hash {
            Some(hash) => hash,
            None => {
                let Some(hash) = hash::of_file(Path::new(&file.path)) else {
                    continue;
                };
                // Kept, so the next search doesn't read the file again.
                remember.execute(params![file.id, hash.to_vec()])?;
                hash.to_vec()
            }
        };
        by_hash.entry(hash).or_default().push(file);
    }
    Ok(by_hash
        .into_values()
        .filter(|files| files.len() > 1)
        .map(|files| group(Likeness::Same, files))
        .collect())
}

/// Pictures and videos whose fingerprints are within `distance` bits, and videos
/// also of about the same length, joined into groups through one another.
fn looks(conn: &Connection, distance: u32, filters: &str, args: &[Value]) -> Result<Vec<Group>> {
    let sql = format!(
        "SELECT {COLUMNS}, o.algo, o.fingerprint, o.duration
         FROM files f JOIN cache.outputs o ON o.hash = f.hash
         WHERE o.fingerprint IS NOT NULL AND f.size >= {MIN_SIZE}{filters}"
    );
    let rows: Vec<Printed> = conn
        .prepare(&sql)?
        .query_map(params_from_iter(args.iter().cloned()), |r| {
            Ok((
                File {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    name: r.get(2)?,
                    kind: r.get(3)?,
                    size: r.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                    mtime: r.get(5)?,
                },
                r.get(7)?,
                r.get::<_, i64>(8)? as u64,
                r.get(9)?,
                r.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    // Union-find over the files, joining any two that look alike.
    let mut parent: Vec<usize> = (0..rows.len()).collect();
    let mut farthest = vec![0u32; rows.len()];
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..rows.len() {
        for j in i + 1..rows.len() {
            let (a, b) = (&rows[i], &rows[j]);
            // Exact copies are their own group.
            if a.1 != b.1 || a.4 == b.4 {
                continue;
            }
            let apart = (a.2 ^ b.2).count_ones();
            if apart > distance || !same_length(a.3, b.3) {
                continue;
            }
            let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
            if ri != rj {
                parent[ri] = rj;
                farthest[rj] = farthest[rj].max(farthest[ri]).max(apart);
            } else {
                farthest[ri] = farthest[ri].max(apart);
            }
        }
    }
    let mut members: HashMap<usize, Vec<File>> = HashMap::new();
    let mut roots = Vec::with_capacity(rows.len());
    for i in 0..rows.len() {
        roots.push(root(&mut parent, i));
    }
    for (row, root) in rows.into_iter().zip(roots) {
        members.entry(root).or_default().push(row.0);
    }
    Ok(members
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(root, files)| group(Likeness::Looks(farthest[root]), files))
        .collect())
}

/// Whether two videos are about the same length: within two seconds, or 2% of the
/// longer. Pictures have no length and always are.
fn same_length(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= 2f64.max(a.max(b) * 0.02),
        _ => true,
    }
}

/// Files whose names are the same once a copy's mark is taken off.
fn names(conn: &Connection, filters: &str, args: &[Value]) -> Result<Vec<Group>> {
    let sql = format!("SELECT {COLUMNS} FROM files f WHERE {NOT_FOLDERS}{filters}");
    let mut by_name: HashMap<String, Vec<Hashed>> = HashMap::new();
    for (file, hash) in files(conn, &sql, args)? {
        let plain = plain_name(&file.name);
        by_name.entry(plain).or_default().push((file, hash));
    }
    Ok(by_name
        .into_values()
        .filter(|files| {
            // One of them has to be marked as a copy, and they can't all be the same
            // content, which is a group of its own.
            files.len() > 1
                && files
                    .iter()
                    .any(|(f, _)| plain_name(&f.name) != f.name.to_lowercase())
                && !files.iter().all(|(_, h)| h.is_some() && h == &files[0].1)
        })
        .map(|files| group(Likeness::Name, files.into_iter().map(|(f, _)| f).collect()))
        .collect())
}

/// A file name without a copy's mark, lowercase: "Report (2).PDF", "report copy.pdf"
/// and "report - Copy.pdf" are all "report.pdf".
pub(crate) fn plain_name(name: &str) -> String {
    let lower = name.to_lowercase();
    let (stem, ext) = match lower.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_owned(), format!(".{ext}")),
        _ => (lower.clone(), String::new()),
    };
    let mut stem = stem.trim_end().to_owned();
    loop {
        let before = stem.clone();
        // "(2)" at the end.
        if let Some(open) = stem.rfind(" (")
            && stem.ends_with(')')
            && stem[open + 2..stem.len() - 1]
                .chars()
                .all(|c| c.is_ascii_digit())
            && stem.len() > open + 3
        {
            stem.truncate(open);
        }
        for mark in [" - copy", " copy", "_copy", "-copy", "的副本", " 副本"] {
            if let Some(rest) = stem.strip_suffix(mark) {
                stem = rest.to_owned();
            }
        }
        // "copy 2" at the end.
        if let Some((rest, number)) = stem.rsplit_once(" copy ")
            && number.chars().all(|c| c.is_ascii_digit())
        {
            stem = rest.to_owned();
        }
        stem = stem.trim_end().to_owned();
        if stem == before || stem.is_empty() {
            break;
        }
    }
    format!("{stem}{ext}")
}

fn group(likeness: Likeness, mut files: Vec<File>) -> Group {
    // The oldest first: usually the original.
    files.sort_by(|a, b| a.mtime.cmp(&b.mtime).then(a.path.cmp(&b.path)));
    let total: u64 = files.iter().map(|f| f.size).sum();
    let largest = files.iter().map(|f| f.size).max().unwrap_or(0);
    Group {
        likeness,
        wasted: total - largest,
        files: files
            .into_iter()
            .map(|f| Hit {
                path: f.path,
                name: f.name,
                kind: Kind::from_name(&f.kind).unwrap_or(Kind::Other),
                size: Some(f.size),
                mtime: f.mtime,
                line: None,
                tags: Vec::new(),
            })
            .collect(),
    }
}

/// Files that look like the one at `path`, closest first, with how many bits their
/// fingerprints differ in.
pub(crate) fn similar(
    conn: &Connection,
    path: &str,
    distance: u32,
    filters: &str,
    args: &[Value],
) -> Result<Vec<(Hit, u32)>> {
    let mine: Option<(String, i64, Option<f64>, Vec<u8>)> = conn
        .query_row(
            "SELECT o.algo, o.fingerprint, o.duration, f.hash
             FROM files f JOIN cache.outputs o ON o.hash = f.hash
             WHERE f.path = ?1 AND o.fingerprint IS NOT NULL",
            [path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();
    let Some((algo, bits, duration, hash)) = mine else {
        // Without a fingerprint, only exact copies can be found.
        let mut sql = format!(
            "SELECT {COLUMNS} FROM files f
             WHERE f.hash = (SELECT hash FROM files WHERE path = ?) AND f.path != ?{filters}"
        );
        sql.push_str(" ORDER BY f.mtime");
        let mut all = vec![Value::Text(path.to_owned()), Value::Text(path.to_owned())];
        all.extend(args.iter().cloned());
        return Ok(files(conn, &sql, &all)?
            .into_iter()
            .map(|(f, _)| (to_hit(f), 0))
            .collect());
    };
    let sql = format!(
        "SELECT {COLUMNS}, o.fingerprint, o.duration
         FROM files f JOIN cache.outputs o ON o.hash = f.hash
         WHERE o.algo = ? AND f.path != ?{filters}"
    );
    let mut all = vec![Value::Text(algo), Value::Text(path.to_owned())];
    all.extend(args.iter().cloned());
    let mut near: Vec<(Hit, u32)> = conn
        .prepare(&sql)?
        .query_map(params_from_iter(all), |r| {
            let file = File {
                id: r.get(0)?,
                path: r.get(1)?,
                name: r.get(2)?,
                kind: r.get(3)?,
                size: r.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                mtime: r.get(5)?,
            };
            let theirs: Vec<u8> = r.get::<_, Option<Vec<u8>>>(6)?.unwrap_or_default();
            let apart = if theirs == hash {
                0
            } else {
                (r.get::<_, i64>(7)? as u64 ^ bits as u64).count_ones()
            };
            Ok((file, apart, r.get::<_, Option<f64>>(8)?))
        })?
        .filter_map(|row| row.ok())
        .filter(|(_, apart, length)| *apart <= distance && same_length(duration, *length))
        .map(|(file, apart, _)| (to_hit(file), apart))
        .collect();
    near.sort_by_key(|(_, apart)| *apart);
    Ok(near)
}

/// Bytes as people read them, like "12.4 MB".
pub(crate) fn human_size(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < units.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", units[unit])
    }
}

fn to_hit(f: File) -> Hit {
    let _ = f.id;
    Hit {
        path: f.path,
        name: f.name,
        kind: Kind::from_name(&f.kind).unwrap_or(Kind::Other),
        size: Some(f.size),
        mtime: f.mtime,
        line: None,
        tags: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_marks_come_off() {
        for name in [
            "Report (2).PDF",
            "report copy.pdf",
            "report - Copy.pdf",
            "report copy 3.pdf",
            "report (1) (1).pdf",
            "report.pdf",
        ] {
            assert_eq!(plain_name(name), "report.pdf", "{name}");
        }
        assert_eq!(plain_name("報告的副本.docx"), "報告.docx");
        assert_eq!(plain_name("2024 (draft).pdf"), "2024 (draft).pdf");
        assert_eq!(plain_name("(1).txt"), "(1).txt");
    }

    #[test]
    fn video_lengths() {
        assert!(same_length(Some(600.0), Some(608.0)));
        assert!(!same_length(Some(60.0), Some(70.0)));
        assert!(same_length(None, None));
    }
}
