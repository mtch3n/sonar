//! How much Sonar indexes of each kind of file.

use std::fmt;

use crate::Kind;

/// How much of a file is indexed. Each level includes the ones before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Left out of the index.
    Skip,
    /// Found by its name and folder only.
    Name,
    /// Its text is searched for words too.
    Text,
    /// Its text is searched by meaning too.
    Meaning,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Skip, Level::Name, Level::Text, Level::Meaning];

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Skip => "skip",
            Level::Name => "name",
            Level::Text => "text",
            Level::Meaning => "meaning",
        }
    }

    pub fn from_name(name: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.as_str() == name)
    }

    pub(crate) fn as_int(self) -> i64 {
        self as i64
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The level of each kind of file.
#[derive(Clone, Debug, PartialEq)]
pub struct Levels([Level; Kind::ALL.len()]);

impl Default for Levels {
    fn default() -> Levels {
        Levels(Kind::ALL.map(Levels::default_for))
    }
}

impl Levels {
    /// Documents and scripts are searched by meaning. Spreadsheets and config files
    /// are searched for words only: big data files would crowd out everything else,
    /// and config files often hold secrets. Code is found by name, as its text would
    /// bury everything else under matches.
    pub fn default_for(kind: Kind) -> Level {
        match kind {
            Kind::Pdf | Kind::Doc | Kind::Slides | Kind::Script => Level::Meaning,
            Kind::Sheet | Kind::Config => Level::Text,
            _ => Level::Name,
        }
    }

    /// The highest level a kind can have. Keys are never read, and folders, apps,
    /// archives and other files have no text of their own.
    pub fn max_for(kind: Kind) -> Level {
        match kind {
            Kind::Key | Kind::Folder | Kind::Project | Kind::App | Kind::Archive | Kind::Other => {
                Level::Name
            }
            _ => Level::Meaning,
        }
    }

    pub fn get(&self, kind: Kind) -> Level {
        self.0[kind as usize]
    }

    /// Sets a kind's level, lowered to the highest it can have. Folders and projects
    /// are always found by name, since the files inside them are placed by them.
    pub fn set(&mut self, kind: Kind, level: Level) {
        self.0[kind as usize] = match kind {
            Kind::Folder | Kind::Project => Level::Name,
            _ => level.min(Levels::max_for(kind)),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_never_read() {
        let mut levels = Levels::default();
        levels.set(Kind::Key, Level::Meaning);
        assert_eq!(levels.get(Kind::Key), Level::Name);
        levels.set(Kind::Key, Level::Skip);
        assert_eq!(levels.get(Kind::Key), Level::Skip);
    }

    #[test]
    fn levels_are_ordered() {
        assert!(Level::Skip < Level::Name && Level::Text < Level::Meaning);
        assert_eq!(Level::from_name("text"), Some(Level::Text));
        assert_eq!(Level::from_name("words"), None);
    }
}
