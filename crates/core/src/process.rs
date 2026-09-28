//! Processors: programs that learn something from a file that its text doesn't
//! say, like what a photo shows or a perceptual fingerprint of a video. What they
//! say is kept by the file's content hash, and their text and tags are searched
//! like the file's own text.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Kind, Level, hash, media, tags, words::words};

/// A file for a processor to look at.
pub struct Job<'a> {
    pub path: &'a Path,
    pub kind: Kind,
    /// The content hash, in hex.
    pub hash: String,
    /// Frames of a video, evenly spread, as many as the processor asked for.
    pub frames: Vec<PathBuf>,
    /// A video's length in seconds.
    pub duration: Option<f64>,
}

/// What a processor learned from a file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    /// Searched like the file's own text, like a description.
    pub text: Option<String>,
    pub tags: Vec<String>,
    pub labels: Vec<String>,
    pub fingerprint: Option<Fingerprint>,
}

/// A perceptual hash: files that look alike have hashes that differ in few bits.
#[derive(Clone, Debug, PartialEq)]
pub struct Fingerprint {
    /// How it was made; only fingerprints made the same way are compared.
    pub algo: String,
    pub bits: u64,
}

pub trait Processor {
    /// The plugin's id.
    fn id(&self) -> &str;
    /// Changes when the processor would say something different, so files are
    /// looked at again.
    fn version(&self) -> &str;
    fn kinds(&self) -> &[Kind];
    /// Frames it wants of each video; none means it's given the video itself.
    fn frames(&self) -> usize;
    /// Whether what it's given stays on this computer.
    fn is_local(&self) -> bool;
    fn process(&mut self, job: &Job) -> Result<Output>;
}

#[derive(Debug, Default)]
pub struct ProcessStats {
    pub done: u64,
    pub failed: u64,
    /// Files it would have looked at, had it gone on.
    pub left: u64,
    pub stopped: bool,
}

/// Where processing keeps what it needs between runs.
pub struct ProcessOptions<'a> {
    /// The folder scanned, for the folder words of each file.
    pub root: &'a Path,
    /// Frames of videos, by content hash.
    pub frames: &'a Path,
    /// Folders never given to a processor that isn't local.
    pub private: &'a [PathBuf],
}

pub(crate) fn process(
    conn: &mut Connection,
    processor: &mut dyn Processor,
    options: &ProcessOptions,
    progress: &mut dyn FnMut(&ProcessStats) -> bool,
) -> Result<ProcessStats> {
    let id = processor.id().to_owned();
    let version = processor.version().to_owned();
    let kinds: Vec<String> = processor
        .kinds()
        .iter()
        .map(|k| format!("'{}'", k.as_str()))
        .collect();
    if kinds.is_empty() {
        return Ok(ProcessStats::default());
    }
    let private = if processor.is_local() {
        &[][..]
    } else {
        options.private
    };
    let candidates: Vec<(i64, String, String, Option<Vec<u8>>)> = conn
        .prepare(&format!(
            "SELECT f.id, f.path, f.kind, f.hash FROM files f
             WHERE f.level >= ?1 AND f.kind IN ({})
               AND (f.hash IS NULL OR NOT EXISTS (
                   SELECT 1 FROM cache.outputs o
                   WHERE o.hash = f.hash AND o.processor = ?2 AND o.version = ?3))
             ORDER BY f.mtime DESC",
            kinds.join(", ")
        ))?
        .query_map(params![Level::Text.as_int(), id, version], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let candidates: Vec<_> = candidates
        .into_iter()
        .filter(|(_, path, ..)| !private.iter().any(|p| Path::new(path).starts_with(p)))
        .collect();

    let ffmpeg = media::available();
    let mut stats = ProcessStats {
        left: candidates.len() as u64,
        ..ProcessStats::default()
    };
    for (file, path, kind, hash) in candidates {
        if !progress(&stats) {
            stats.stopped = true;
            return Ok(stats);
        }
        stats.left -= 1;
        let path = PathBuf::from(path);
        let Some(hash) = hash.or_else(|| hash::of_file(&path).map(|h| h.to_vec())) else {
            continue;
        };
        conn.execute(
            "UPDATE files SET hash = ?2 WHERE id = ?1",
            params![file, hash],
        )?;
        let done: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM cache.outputs
                            WHERE hash = ?1 AND processor = ?2 AND version = ?3)",
            params![hash, id, version],
            |r| r.get(0),
        )?;
        if done {
            // Same content as a file already looked at, like a copy.
            refresh_text(conn, &hash, options.root)?;
            continue;
        }

        let kind = Kind::from_name(&kind).unwrap_or(Kind::Other);
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        let mut length = None;
        let output = video(
            kind,
            &path,
            &hex,
            processor.frames(),
            ffmpeg,
            options.frames,
        )
        .and_then(|(frames, duration)| {
            length = duration;
            processor.process(&Job {
                path: &path,
                kind,
                hash: hex,
                frames,
                duration,
            })
        });
        let tx = conn.transaction()?;
        match &output {
            Ok(output) => {
                tx.execute(
                    "INSERT OR REPLACE INTO cache.outputs
                         (hash, processor, version, text, tags, labels, algo, fingerprint, duration)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        hash,
                        id,
                        version,
                        output.text,
                        output.tags.join("\n"),
                        output.labels.join("\n"),
                        output.fingerprint.as_ref().map(|f| f.algo.as_str()),
                        output.fingerprint.as_ref().map(|f| f.bits as i64),
                        length,
                    ],
                )?;
                stats.done += 1;
            }
            Err(err) => {
                // Kept, so a file a processor can't read isn't tried on every run.
                tx.execute(
                    "INSERT OR REPLACE INTO cache.outputs (hash, processor, version, error)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![hash, id, version, format!("{err:#}")],
                )?;
                stats.failed += 1;
            }
        }
        refresh_text(&tx, &hash, options.root)?;
        tx.commit()?;
    }
    progress(&stats);
    Ok(stats)
}

/// The frames and length of a video, when the processor wants frames; nothing for
/// other files.
fn video(
    kind: Kind,
    path: &Path,
    hex: &str,
    count: usize,
    ffmpeg: bool,
    frames: &Path,
) -> Result<(Vec<PathBuf>, Option<f64>)> {
    if kind != Kind::Video || count == 0 {
        return Ok((Vec::new(), None));
    }
    if !ffmpeg {
        anyhow::bail!("reading videos needs ffmpeg");
    }
    let duration = media::duration(path)?;
    let all = media::frames(path, duration, &frames.join(hex))?;
    Ok((media::spread(&all, count), Some(duration)))
}

/// Gathers what every processor said about content `hash` into the text searched
/// with it, and indexes that text again for the files that have it.
pub(crate) fn refresh_text(conn: &Connection, hash: &[u8], root: &Path) -> Result<()> {
    tags::put_plugin_tags(conn, hash)?;
    let parts: Vec<(Option<String>, Option<String>)> = conn
        .prepare(
            "SELECT text, tags FROM cache.outputs
             WHERE hash = ?1 AND error IS NULL ORDER BY processor",
        )?
        .query_map([hash], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let text: Vec<String> = parts
        .into_iter()
        .flat_map(|(text, tags)| {
            let tags = tags
                .filter(|t| !t.is_empty())
                .map(|t| t.replace('\n', ", "));
            text.into_iter().chain(tags)
        })
        .filter(|t| !t.trim().is_empty())
        .collect();
    if text.is_empty() {
        conn.execute("DELETE FROM cache.extras WHERE hash = ?1", [hash])?;
    } else {
        conn.execute(
            "INSERT INTO cache.extras (hash, text, version) VALUES (?1, ?2, 1)
             ON CONFLICT (hash) DO UPDATE SET text = excluded.text, version = version + 1
             WHERE text != excluded.text",
            params![hash, text.join("\n")],
        )?;
    }

    let files: Vec<(i64, String, String)> = conn
        .prepare("SELECT id, path, name FROM files WHERE hash = ?1")?
        .query_map([hash], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let full: Option<String> = conn
        .query_row(
            "SELECT text FROM cache.full_texts WHERE hash = ?1",
            [hash],
            |r| r.get(0),
        )
        .optional()?;
    let body = full.as_deref().map(words).unwrap_or_default();
    for (id, path, name) in files {
        let dirs = Path::new(&path)
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
            .and_then(Path::to_str)
            .map(words)
            .unwrap_or_default();
        conn.execute("DELETE FROM files_fts WHERE rowid = ?1", [id])?;
        conn.execute(
            "INSERT INTO files_fts (rowid, name, dirs, body) VALUES (?1, ?2, ?3, ?4)",
            params![id, format!("{name} {}", words(&name)), dirs, body],
        )?;
    }
    Ok(())
}

/// Frames of videos no file has anymore are removed.
pub(crate) fn forget_frames(conn: &Connection, frames: &Path) -> Result<()> {
    let Ok(entries) = fs::read_dir(frames) else {
        return Ok(());
    };
    let mut known = conn.prepare("SELECT EXISTS (SELECT 1 FROM files WHERE hash = ?1)")?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(hash) = name.to_str().and_then(from_hex) else {
            continue;
        };
        if !known.query_row([hash], |r| r.get::<_, bool>(0))? {
            fs::remove_dir_all(entry.path())
                .with_context(|| format!("removing {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    (hex.len() == 64)
        .then(|| {
            (0..32)
                .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
                .collect()
        })
        .flatten()
}
