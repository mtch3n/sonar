mod db;
mod documents;
mod kind;
mod query;
mod rules;
mod scan;
mod search;
mod text;
mod words;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;

pub use kind::Kind;
pub use query::{DEFAULT_LIMIT, ParseError, Query, Term, Within};
pub use rules::Rules;
pub use scan::ScanStats;
pub use search::Hit;
pub use text::DEFAULT_TEXT_LIMIT;

#[derive(Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub db: PathBuf,
    /// Folders Sonar writes for the plugins it ships with.
    pub bundled: PathBuf,
    pub rules: PathBuf,
    pub settings: PathBuf,
    pub plugins: PathBuf,
    /// Where each plugin may keep files, in a folder named by its id.
    pub plugin_data: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Paths> {
        let home = dirs::home_dir().context("can't find the home folder")?;
        let data = dirs::data_local_dir().context("can't find the data folder")?;
        let config = dirs::config_dir()
            .context("can't find the config folder")?
            .join("sonar");
        Ok(Paths {
            home,
            db: data.join("sonar").join("index.db"),
            bundled: data.join("sonar").join("bundled"),
            rules: config.join("ignore"),
            settings: config.join("settings.toml"),
            plugins: config.join("plugins"),
            plugin_data: data.join("sonar").join("plugins"),
        })
    }
}

pub struct Index {
    conn: Connection,
}

impl Index {
    pub fn open(path: &Path) -> Result<Index> {
        Ok(Index {
            conn: db::open(path)?,
        })
    }

    /// Indexes everything under `root`, keeping up to `text_limit` bytes of each
    /// readable file's text.
    pub fn scan(&mut self, root: &Path, rules: &Rules, text_limit: usize) -> Result<ScanStats> {
        scan::scan(&mut self.conn, root, rules, text_limit)
    }

    /// The folders the last scan went into, for watching them for changes.
    pub fn folders(&self) -> Result<Vec<PathBuf>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM files WHERE kind IN ('folder', 'project')")?;
        let paths = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(paths
            .map(|p| p.map(PathBuf::from))
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn search(&self, query: &Query) -> Result<Vec<Hit>> {
        search::search(&self.conn, query, jiff::Timestamp::now().as_second())
    }

    pub fn is_empty(&self) -> Result<bool> {
        let any: bool = self
            .conn
            .query_row("SELECT EXISTS (SELECT 1 FROM files)", [], |r| r.get(0))?;
        Ok(!any)
    }
}
