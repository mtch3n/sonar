use std::{fs, path::Path};

use anyhow::{Context, Result};
use rusqlite::Connection;

const SCHEMA_VERSION: i64 = 5;

const SCHEMA: &str = "
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    ext TEXT NOT NULL,
    kind TEXT NOT NULL,
    size INTEGER,
    mtime INTEGER NOT NULL,
    -- Nanosecond modified and changed times, so any write is seen: programs can set
    -- the modified time, like rsync -a and cp -p do, but not the changed time.
    mtime_ns INTEGER NOT NULL,
    ctime_ns INTEGER NOT NULL,
    level INTEGER NOT NULL,
    -- The hash of the file's content, for files whose content is read.
    hash BLOB,
    project_id INTEGER,
    scan_id INTEGER NOT NULL
);
CREATE INDEX files_mtime ON files (mtime);
CREATE INDEX files_hash ON files (hash) WHERE hash IS NOT NULL;

CREATE VIRTUAL TABLE files_fts USING fts5 (
    name, dirs, body,
    content = '', contentless_delete = 1,
    tokenize = 'unicode61 remove_diacritics 2'
);
-- How much of each file's text the last scan kept, so a new limit re-reads them.
CREATE TABLE text_limit (bytes INTEGER NOT NULL);
CREATE TRIGGER files_deleted AFTER DELETE ON files BEGIN
    DELETE FROM files_fts WHERE rowid = old.id;
END;
";

const CACHE_VERSION: i64 = 2;

/// What Sonar learned from each file's content, by its hash. It's kept in a file of
/// its own, so it outlives the index when that is rebuilt, and entries no file has
/// needed for a while are removed.
const CACHE_SCHEMA: &str = "
CREATE TABLE cache.contents (
    hash BLOB PRIMARY KEY,
    text_limit INTEGER NOT NULL,
    -- NULL when the file has no text, so a broken document isn't read again.
    text TEXT,
    unused_since INTEGER
) WITHOUT ROWID;
-- Pieces of text by meaning: vectors of each chunk of the text with a hash.
CREATE TABLE cache.vectors (
    model TEXT NOT NULL,
    hash BLOB NOT NULL,
    chunk INTEGER NOT NULL,
    start INTEGER NOT NULL,
    end INTEGER NOT NULL,
    vector BLOB NOT NULL,
    PRIMARY KEY (model, hash, chunk)
) WITHOUT ROWID;
-- The text each model has embedded, and how it was cut into chunks.
CREATE TABLE cache.embedded (
    model TEXT NOT NULL,
    hash BLOB NOT NULL,
    policy TEXT NOT NULL,
    PRIMARY KEY (model, hash)
) WITHOUT ROWID;
CREATE TABLE cache.names (
    model TEXT NOT NULL,
    name TEXT NOT NULL,
    vector BLOB NOT NULL,
    PRIMARY KEY (model, name)
) WITHOUT ROWID;
CREATE TABLE cache.vectors_version (version INTEGER NOT NULL);
INSERT INTO cache.vectors_version VALUES (0);
";

pub(crate) fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // The app scans, embeds and searches on connections of their own.
    conn.busy_timeout(std::time::Duration::from_secs(60))?;

    let cache = path.with_file_name("cache.db");
    conn.execute(
        "ATTACH DATABASE ?1 AS cache",
        [cache.to_str().context("the cache path isn't UTF-8")?],
    )
    .with_context(|| format!("opening {}", cache.display()))?;
    conn.pragma_update_and_check(Some("cache"), "journal_mode", "WAL", |_| Ok(()))?;
    conn.pragma_update(Some("cache"), "synchronous", "NORMAL")?;

    let version: i64 = conn.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    if version != SCHEMA_VERSION {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "DROP TRIGGER IF EXISTS main.files_deleted;
             DROP TABLE IF EXISTS main.text_limit;
             DROP TABLE IF EXISTS main.texts;
             DROP TABLE IF EXISTS main.files_fts;
             DROP TABLE IF EXISTS main.files;",
        )?;
        tx.execute_batch(SCHEMA)?;
        tx.pragma_update(Some("main"), "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
    }
    let version: i64 = conn.query_row("PRAGMA cache.user_version", [], |r| r.get(0))?;
    if version != CACHE_VERSION {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "DROP TABLE IF EXISTS cache.contents;
             DROP TABLE IF EXISTS cache.vectors;
             DROP TABLE IF EXISTS cache.embedded;
             DROP TABLE IF EXISTS cache.names;
             DROP TABLE IF EXISTS cache.vectors_version;",
        )?;
        tx.execute_batch(CACHE_SCHEMA)?;
        tx.pragma_update(Some("cache"), "user_version", CACHE_VERSION)?;
        tx.commit()?;
    }
    Ok(conn)
}
