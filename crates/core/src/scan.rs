use std::{
    fs::{File, Metadata},
    io::Read,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::Result;
use ignore::WalkBuilder;
use rusqlite::{Connection, OptionalExtension, Statement, Transaction, params};

use crate::{
    Kind, Level, Levels, Rules,
    hash::{self, Hash},
    meaning, text,
    words::words,
};

const PROJECT_MARKERS: &[&str] = &[
    ".git",
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "CMakeLists.txt",
    "composer.json",
    "Gemfile",
    "pubspec.yaml",
    "deno.json",
];

#[derive(Debug, Default)]
pub struct ScanStats {
    pub files: u64,
    pub folders: u64,
    pub projects: u64,
    pub removed: u64,
}

/// What a scan keeps of each file.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanOptions {
    /// How much of each file's text is kept, in bytes.
    pub text_limit: usize,
    pub levels: Levels,
}

impl Default for ScanOptions {
    fn default() -> ScanOptions {
        ScanOptions {
            text_limit: text::DEFAULT_TEXT_LIMIT,
            levels: Levels::default(),
        }
    }
}

pub(crate) fn scan(
    conn: &mut Connection,
    root: &Path,
    rules: &Rules,
    options: &ScanOptions,
) -> Result<ScanStats> {
    let tx = conn.transaction()?;
    let scan_id: i64 =
        tx.query_row("SELECT coalesce(max(scan_id), 0) + 1 FROM files", [], |r| {
            r.get(0)
        })?;
    let last_limit: Option<i64> = tx
        .query_row("SELECT bytes FROM text_limit", [], |r| r.get(0))
        .optional()?;
    let text_limit = options.text_limit;
    let reread = last_limit != Some(text_limit as i64);
    let mut stats = ScanStats::default();
    {
        let mut writer = Writer::new(&tx, scan_id, reread, text_limit)?;
        let rules = rules.clone();
        let walker = WalkBuilder::new(root)
            .hidden(false)
            .filter_entry(move |e| {
                let is_dir = e.file_type().is_some_and(|t| t.is_dir());
                let in_bundle = e
                    .path()
                    .parent()
                    .is_some_and(|p| Kind::of_bundle(&lowercase_ext(p)).is_some());
                !in_bundle && !hidden_by_os(e) && !rules.excludes(e.path(), is_dir)
            })
            .build();

        let mut project: Option<(PathBuf, i64)> = None;
        for entry in walker.flatten() {
            if entry.depth() == 0 {
                continue;
            }
            let path = entry.path();
            let (Some(path_str), Some(name), Some(file_type), Ok(meta)) = (
                path.to_str(),
                entry.file_name().to_str(),
                entry.file_type(),
                entry.metadata(),
            ) else {
                continue;
            };
            if project.as_ref().is_some_and(|(p, _)| !path.starts_with(p)) {
                project = None;
            }
            let project_id = project.as_ref().map(|(_, id)| *id);
            let dirs = path
                .parent()
                .and_then(|p| p.strip_prefix(root).ok())
                .and_then(Path::to_str)
                .map(words)
                .unwrap_or_default();
            let stamp = Stamp::of(&meta);
            let ext = lowercase_ext(path);
            let row = |kind: Kind, size: Option<i64>| Row {
                path: path_str,
                name,
                ext: match kind {
                    Kind::Folder | Kind::Project => "",
                    _ => &ext,
                },
                kind,
                size,
                stamp,
                level: options.levels.get(kind),
                project_id,
                dirs: &dirs,
            };

            if let Some(kind) = file_type.is_dir().then(|| Kind::of_bundle(&ext)).flatten() {
                if writer.put(&row(kind, None), None)?.is_some() {
                    stats.files += 1;
                }
            } else if file_type.is_dir() {
                let is_project =
                    project.is_none() && PROJECT_MARKERS.iter().any(|m| path.join(m).exists());
                let kind = if is_project {
                    Kind::Project
                } else {
                    Kind::Folder
                };
                let id = writer
                    .put(&row(kind, None), None)?
                    .expect("folders are always indexed");
                if is_project {
                    project = Some((path.to_owned(), id));
                    stats.projects += 1;
                } else {
                    stats.folders += 1;
                }
            } else if file_type.is_file() {
                let head = if Kind::needs_head(&ext, is_executable(&meta)) {
                    read_head(path)
                } else {
                    Vec::new()
                };
                let kind = Kind::of_file(name, &ext, project_id.is_some(), &head);
                let row = row(kind, Some(meta.len() as i64));
                let readable = row.level >= Level::Text
                    && (project_id.is_none() || kind == Kind::Code)
                    && text::is_readable(kind, &ext)
                    && !is_placeholder(&meta);
                if writer.put(&row, readable.then_some(path))?.is_some() {
                    stats.files += 1;
                }
            }
        }
    }
    stats.removed = tx.execute("DELETE FROM files WHERE scan_id <> ?1", [scan_id])? as u64;
    if reread {
        tx.execute("DELETE FROM text_limit", [])?;
        tx.execute(
            "INSERT INTO text_limit (bytes) VALUES (?1)",
            [text_limit as i64],
        )?;
    }
    forget_unused(&tx)?;
    tx.commit()?;
    Ok(stats)
}

/// How long the cache keeps what it learned from content no file has anymore, so
/// a file that comes back, like one restored from the trash, isn't read again.
const KEEP_UNUSED_SECS: i64 = 30 * 24 * 60 * 60;

fn forget_unused(tx: &Transaction) -> Result<()> {
    let now = jiff::Timestamp::now().as_second();
    tx.execute(
        "UPDATE cache.contents SET unused_since = NULL
         WHERE unused_since IS NOT NULL AND hash IN (SELECT hash FROM main.files)",
        [],
    )?;
    tx.execute(
        "UPDATE cache.contents SET unused_since = ?1
         WHERE unused_since IS NULL AND hash NOT IN (SELECT hash FROM main.files WHERE hash IS NOT NULL)",
        [now],
    )?;
    tx.execute(
        "DELETE FROM cache.contents WHERE unused_since < ?1",
        [now - KEEP_UNUSED_SECS],
    )?;
    meaning::forget_unused(tx)?;
    Ok(())
}

/// When a file was last written, as its metadata says.
#[derive(Clone, Copy, PartialEq)]
struct Stamp {
    /// Seconds, for searching and showing.
    mtime: i64,
    mtime_ns: i64,
    ctime_ns: i64,
}

impl Stamp {
    fn of(meta: &Metadata) -> Stamp {
        let mtime_ns = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as i64);
        Stamp {
            mtime: mtime_ns.div_euclid(1_000_000_000),
            mtime_ns,
            ctime_ns: ctime_ns(meta),
        }
    }
}

#[cfg(unix)]
fn ctime_ns(meta: &Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    meta.ctime() * 1_000_000_000 + meta.ctime_nsec()
}

/// Windows has no changed time in its standard metadata, so the modified time
/// stands alone there.
#[cfg(not(unix))]
fn ctime_ns(_: &Metadata) -> i64 {
    0
}

struct Row<'a> {
    path: &'a str,
    name: &'a str,
    ext: &'a str,
    kind: Kind,
    size: Option<i64>,
    stamp: Stamp,
    level: Level,
    project_id: Option<i64>,
    dirs: &'a str,
}

struct Writer<'t> {
    find: Statement<'t>,
    insert: Statement<'t>,
    update: Statement<'t>,
    fts_insert: Statement<'t>,
    fts_delete: Statement<'t>,
    set_hash: Statement<'t>,
    cached_text: Statement<'t>,
    cache_text: Statement<'t>,
    scan_id: i64,
    /// Whether every file's text is read again, because the text limit changed.
    reread: bool,
    text_limit: usize,
}

/// A file's row: its id, kind, size, times, level, whether it's outside projects,
/// and whether the cache has its text, if it has any.
type Found = (i64, String, Option<i64>, i64, i64, i64, bool, bool);

impl<'t> Writer<'t> {
    fn new(
        tx: &'t Transaction,
        scan_id: i64,
        reread: bool,
        text_limit: usize,
    ) -> Result<Writer<'t>> {
        Ok(Writer {
            find: tx.prepare(
                "SELECT id, kind, size, mtime_ns, ctime_ns, level, project_id IS NULL,
                        hash IS NULL OR hash IN (SELECT hash FROM cache.contents)
                 FROM files WHERE path = ?1",
            )?,
            insert: tx.prepare(
                "INSERT INTO files (path, name, ext, kind, size, mtime, mtime_ns, ctime_ns,
                                    level, hash, project_id, scan_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 RETURNING id",
            )?,
            update: tx.prepare(
                "UPDATE files SET kind = ?2, size = ?3, mtime = ?4, mtime_ns = ?5, ctime_ns = ?6,
                                  level = ?7, project_id = ?8, scan_id = ?9
                 WHERE id = ?1",
            )?,
            fts_insert: tx.prepare(
                "INSERT INTO files_fts (rowid, name, dirs, body) VALUES (?1, ?2, ?3, ?4)",
            )?,
            fts_delete: tx.prepare("DELETE FROM files_fts WHERE rowid = ?1")?,
            set_hash: tx.prepare("UPDATE files SET hash = ?2 WHERE id = ?1")?,
            cached_text: tx
                .prepare("SELECT text FROM cache.contents WHERE hash = ?1 AND text_limit = ?2")?,
            cache_text: tx.prepare(
                "INSERT OR REPLACE INTO cache.contents (hash, text_limit, text) VALUES (?1, ?2, ?3)",
            )?,
            scan_id,
            reread,
            text_limit,
        })
    }

    /// Writes a row, and when the file is new, has changed since the last scan, or
    /// the text limit has, indexes its text again, reading it from `content` unless
    /// the cache has the text of the same content. Returns the row's id, or `None`
    /// when the file's kind is skipped.
    fn put(&mut self, row: &Row, content: Option<&Path>) -> Result<Option<i64>> {
        if row.level == Level::Skip {
            return Ok(None);
        }
        let found: Option<Found> = self
            .find
            .query_row([row.path], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })
            .optional()?;
        let stamp = row.stamp;
        let id = match &found {
            Some((id, ..)) => {
                self.update.execute(params![
                    id,
                    row.kind.as_str(),
                    row.size,
                    stamp.mtime,
                    stamp.mtime_ns,
                    stamp.ctime_ns,
                    row.level.as_int(),
                    row.project_id,
                    self.scan_id
                ])?;
                *id
            }
            None => self.insert.query_row(
                params![
                    row.path,
                    row.name,
                    row.ext,
                    row.kind.as_str(),
                    row.size,
                    stamp.mtime,
                    stamp.mtime_ns,
                    stamp.ctime_ns,
                    row.level.as_int(),
                    None::<Vec<u8>>,
                    row.project_id,
                    self.scan_id
                ],
                |r| r.get(0),
            )?,
        };
        let unchanged = !self.reread
            && found.is_some_and(
                |(_, kind, size, mtime_ns, ctime_ns, level, outside, cached)| {
                    cached
                        && kind == row.kind.as_str()
                        && size == row.size
                        && mtime_ns == stamp.mtime_ns
                        && ctime_ns == stamp.ctime_ns
                        && level == row.level.as_int()
                        && outside == row.project_id.is_none()
                },
            );
        if unchanged {
            return Ok(Some(id));
        }

        let hash = content.and_then(hash::of_file);
        let text = match (content, hash) {
            (Some(path), Some(hash)) => self.text(path, row.ext, &hash)?,
            _ => None,
        };
        self.set_hash.execute(params![id, hash])?;
        let name_words = format!("{} {}", row.name, words(row.name));
        let body = text.as_deref().map(words).unwrap_or_default();
        self.fts_delete.execute([id])?;
        self.fts_insert
            .execute(params![id, name_words, row.dirs, body])?;
        Ok(Some(id))
    }

    /// The text of the file at `path`, from the cache when it has this content.
    fn text(&mut self, path: &Path, ext: &str, hash: &Hash) -> Result<Option<String>> {
        let limit = self.text_limit as i64;
        let cached: Option<Option<String>> = self
            .cached_text
            .query_row(params![hash, limit], |r| r.get(0))
            .optional()?;
        if let Some(text) = cached {
            return Ok(text);
        }
        let text = text::read(path, ext, self.text_limit);
        self.cache_text.execute(params![hash, limit, text])?;
        Ok(text)
    }
}

#[cfg(unix)]
fn is_executable(meta: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &Metadata) -> bool {
    false
}

#[cfg(windows)]
fn hidden_by_os(entry: &ignore::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    entry
        .metadata()
        .is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
}

#[cfg(not(windows))]
fn hidden_by_os(_: &ignore::DirEntry) -> bool {
    false
}

/// Whether the file is only a stand-in for one in the cloud, which reading would
/// download.
#[cfg(windows)]
fn is_placeholder(meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x40000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x400000;
    meta.file_attributes()
        & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

#[cfg(target_os = "macos")]
fn is_placeholder(meta: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    const SF_DATALESS: u32 = 0x40000000;
    meta.st_flags() & SF_DATALESS != 0
}

#[cfg(not(any(windows, target_os = "macos")))]
fn is_placeholder(_: &Metadata) -> bool {
    false
}

fn lowercase_ext(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn read_head(path: &Path) -> Vec<u8> {
    let mut head = Vec::with_capacity(4);
    let _ = File::open(path).and_then(|f| f.take(4).read_to_end(&mut head));
    head
}
