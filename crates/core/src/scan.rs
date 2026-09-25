use std::{
    fs::{File, Metadata},
    io::Read,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::Result;
use ignore::WalkBuilder;
use rusqlite::{Connection, OptionalExtension, Statement, Transaction, params};

use crate::{Kind, Rules, words::words};

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

pub(crate) fn scan(conn: &mut Connection, root: &Path, rules: &Rules) -> Result<ScanStats> {
    let tx = conn.transaction()?;
    let scan_id: i64 =
        tx.query_row("SELECT coalesce(max(scan_id), 0) + 1 FROM files", [], |r| {
            r.get(0)
        })?;
    let mut stats = ScanStats::default();
    {
        let mut writer = Writer::new(&tx, scan_id)?;
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
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);

            let ext = lowercase_ext(path);

            if let Some(kind) = file_type.is_dir().then(|| Kind::of_bundle(&ext)).flatten() {
                writer.put(&Row {
                    path: path_str,
                    name,
                    ext: &ext,
                    kind,
                    size: None,
                    mtime,
                    project_id,
                    dirs: &dirs,
                })?;
                stats.files += 1;
            } else if file_type.is_dir() {
                let is_project =
                    project.is_none() && PROJECT_MARKERS.iter().any(|m| path.join(m).exists());
                let kind = if is_project {
                    Kind::Project
                } else {
                    Kind::Folder
                };
                let id = writer.put(&Row {
                    path: path_str,
                    name,
                    ext: "",
                    kind,
                    size: None,
                    mtime,
                    project_id,
                    dirs: &dirs,
                })?;
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
                writer.put(&Row {
                    path: path_str,
                    name,
                    ext: &ext,
                    kind,
                    size: Some(meta.len() as i64),
                    mtime,
                    project_id,
                    dirs: &dirs,
                })?;
                stats.files += 1;
            }
        }
    }
    stats.removed = tx.execute("DELETE FROM files WHERE scan_id <> ?1", [scan_id])? as u64;
    tx.commit()?;
    Ok(stats)
}

struct Row<'a> {
    path: &'a str,
    name: &'a str,
    ext: &'a str,
    kind: Kind,
    size: Option<i64>,
    mtime: i64,
    project_id: Option<i64>,
    dirs: &'a str,
}

struct Writer<'t> {
    insert: Statement<'t>,
    update: Statement<'t>,
    fts: Statement<'t>,
    scan_id: i64,
}

impl<'t> Writer<'t> {
    fn new(tx: &'t Transaction, scan_id: i64) -> Result<Writer<'t>> {
        Ok(Writer {
            insert: tx.prepare(
                "INSERT INTO files (path, name, ext, kind, size, mtime, project_id, scan_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (path) DO NOTHING
                 RETURNING id",
            )?,
            update: tx.prepare(
                "UPDATE files SET kind = ?2, size = ?3, mtime = ?4, project_id = ?5, scan_id = ?6
                 WHERE path = ?1
                 RETURNING id",
            )?,
            fts: tx.prepare("INSERT INTO files_fts (rowid, name, dirs) VALUES (?1, ?2, ?3)")?,
            scan_id,
        })
    }

    fn put(&mut self, row: &Row) -> Result<i64> {
        let inserted: Option<i64> = self
            .insert
            .query_row(
                params![
                    row.path,
                    row.name,
                    row.ext,
                    row.kind.as_str(),
                    row.size,
                    row.mtime,
                    row.project_id,
                    self.scan_id
                ],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = inserted {
            let name_words = format!("{} {}", row.name, words(row.name));
            self.fts.execute(params![id, name_words, row.dirs])?;
            return Ok(id);
        }
        Ok(self.update.query_row(
            params![
                row.path,
                row.kind.as_str(),
                row.size,
                row.mtime,
                row.project_id,
                self.scan_id
            ],
            |r| r.get(0),
        )?)
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
