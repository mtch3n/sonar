mod db;
mod kind;
mod query;
mod rules;
mod scan;
mod search;
mod words;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;

pub use kind::Kind;
pub use query::{ParseError, Query, Term, Within};
pub use rules::Rules;
pub use scan::ScanStats;
pub use search::Hit;

#[derive(Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub db: PathBuf,
    pub rules: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Paths> {
        let home = dirs::home_dir().context("can't find the home folder")?;
        let data = dirs::data_local_dir().context("can't find the data folder")?;
        let config = dirs::config_dir().context("can't find the config folder")?;
        Ok(Paths {
            home,
            db: data.join("sonar").join("index.db"),
            rules: config.join("sonar").join("ignore"),
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

    pub fn scan(&mut self, root: &Path, rules: &Rules) -> Result<ScanStats> {
        scan::scan(&mut self.conn, root, rules)
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
