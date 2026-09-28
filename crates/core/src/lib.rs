mod db;
mod documents;
mod hash;
mod kind;
mod level;
mod meaning;
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
pub use level::{Level, Levels};
pub use meaning::{EmbedStats, Embedder};
pub use query::{DEFAULT_LIMIT, ParseError, Query, Term, Within};
pub use rules::Rules;
pub use scan::{ScanOptions, ScanStats};
pub use search::Hit;
pub use text::DEFAULT_TEXT_LIMIT;

#[derive(Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub db: PathBuf,
    /// Where the models that search by meaning are downloaded.
    pub models: PathBuf,
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
            models: data.join("sonar").join("models"),
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
    /// The vectors of the model last searched with, loaded on first use.
    store: Option<meaning::Store>,
}

impl Index {
    pub fn open(path: &Path) -> Result<Index> {
        Ok(Index {
            conn: db::open(path)?,
            store: None,
        })
    }

    /// Indexes everything under `root` that `rules` don't exclude.
    pub fn scan(&mut self, root: &Path, rules: &Rules, options: &ScanOptions) -> Result<ScanStats> {
        scan::scan(&mut self.conn, root, rules, options)
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

    /// Files matching `query` by their names and words.
    pub fn search(&self, query: &Query) -> Result<Vec<Hit>> {
        search::search(&self.conn, query, jiff::Timestamp::now().as_second(), None)
    }

    /// Files matching `query` by their names and words, or by meaning as `embedder`
    /// understands it.
    pub fn search_with(&mut self, query: &Query, embedder: &mut dyn Embedder) -> Result<Vec<Hit>> {
        let meaning = search::Meaning {
            embedder,
            store: &mut self.store,
        };
        search::search(
            &self.conn,
            query,
            jiff::Timestamp::now().as_second(),
            Some(meaning),
        )
    }

    /// Embeds what files searched by meaning need and `embedder` hasn't embedded
    /// yet. `progress` hears what's done after each batch, and stops it early by
    /// returning false.
    /// Files under `private` are left out when the model isn't on this computer.
    pub fn embed(
        &mut self,
        embedder: &mut dyn Embedder,
        private: &[PathBuf],
        progress: &mut dyn FnMut(&EmbedStats) -> bool,
    ) -> Result<EmbedStats> {
        meaning::embed(&mut self.conn, embedder, private, progress)
    }

    /// Forgets what was learned from the file at `path`, or from everything under
    /// it, so the next scan reads and embeds it afresh. Returns how many files that
    /// covers.
    pub fn forget(&mut self, path: &Path) -> Result<u64> {
        scan::forget(&mut self.conn, path)
    }

    /// How many names and files model `model` has yet to embed.
    pub fn pending_meaning(&self, model: &str) -> Result<(u64, u64)> {
        meaning::pending(&self.conn, model)
    }

    pub fn is_empty(&self) -> Result<bool> {
        let any: bool = self
            .conn
            .query_row("SELECT EXISTS (SELECT 1 FROM files)", [], |r| r.get(0))?;
        Ok(!any)
    }
}
