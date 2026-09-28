use std::{fs, path::Path};

use anyhow::{Context, Result};
use rusqlite::Connection;

const SCHEMA_VERSION: i64 = 4;

const SCHEMA: &str = "
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    ext TEXT NOT NULL,
    kind TEXT NOT NULL,
    size INTEGER,
    mtime INTEGER NOT NULL,
    project_id INTEGER,
    scan_id INTEGER NOT NULL
);
CREATE INDEX files_mtime ON files (mtime);

CREATE VIRTUAL TABLE files_fts USING fts5 (
    name, dirs, body,
    content = '', contentless_delete = 1,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TABLE texts (
    id INTEGER PRIMARY KEY,
    text TEXT NOT NULL
);
-- How much of each file's text the last scan kept, so a new limit re-reads them.
CREATE TABLE text_limit (bytes INTEGER NOT NULL);
CREATE TRIGGER files_deleted AFTER DELETE ON files BEGIN
    DELETE FROM files_fts WHERE rowid = old.id;
    DELETE FROM texts WHERE id = old.id;
END;
";

pub(crate) fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version != SCHEMA_VERSION {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "DROP TRIGGER IF EXISTS files_deleted;
             DROP TABLE IF EXISTS text_limit;
             DROP TABLE IF EXISTS texts;
             DROP TABLE IF EXISTS files_fts;
             DROP TABLE IF EXISTS files;",
        )?;
        tx.execute_batch(SCHEMA)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
    }
    Ok(conn)
}
