//! Bookmarks and history from Chromium browsers: Chrome, Chromium, Brave, Edge,
//! Vivaldi and Arc, in every profile. Everything is read from the browsers' own files,
//! so no extension is needed and nothing leaves the computer.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags, params_from_iter};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{Action, Field, Item, Setting};

/// The browser integration's id in `settings.toml`.
pub const ID: &str = "browser";
/// Its settings keys; each profile also has its own, from [`Profile::key`].
pub const BOOKMARKS: &str = "bookmarks";
pub const HISTORY: &str = "history";
pub const RESULTS: &str = "results";

/// Words shorter than this, all together, match too much to be worth showing.
const MIN_QUERY: usize = 2;

/// What to search, how many results to show, and a switch for every profile found.
pub fn settings() -> Vec<Setting> {
    let profiles = installed_profiles();
    let mut settings = vec![
        Setting {
            key: BOOKMARKS.into(),
            title: "Search bookmarks".into(),
            description: None,
            field: Field::Toggle { default: true },
        },
        Setting {
            key: HISTORY.into(),
            title: "Search history".into(),
            description: Some("Pages you visited".into()),
            field: Field::Toggle { default: true },
        },
        Setting {
            key: RESULTS.into(),
            title: "Results".into(),
            description: Some("Shown above your files".into()),
            field: Field::Number {
                default: 3.0,
                min: Some(1.0),
                max: Some(10.0),
            },
        },
    ];
    settings.extend(profiles.iter().map(|profile| Setting {
        key: profile.key(),
        title: format!("{} · {}", profile.browser, profile.name),
        description: None,
        field: Field::Toggle { default: true },
    }));
    settings
}

/// A browser Sonar knows where to find.
struct Browser {
    name: &'static str,
    /// The browser's user data folder, under the platform's base folder.
    #[cfg(target_os = "linux")]
    data: &'static str,
    #[cfg(target_os = "macos")]
    data: &'static str,
    #[cfg(windows)]
    data: &'static str,
    /// Linux commands, macOS app names or Windows executables that start it.
    #[cfg(target_os = "linux")]
    commands: &'static [&'static str],
    #[cfg(target_os = "macos")]
    app: &'static str,
    #[cfg(windows)]
    exe: &'static str,
}

#[cfg(target_os = "linux")]
const BROWSERS: &[Browser] = &[
    Browser {
        name: "Chrome",
        data: "google-chrome",
        commands: &["google-chrome-stable", "google-chrome"],
    },
    Browser {
        name: "Chromium",
        data: "chromium",
        commands: &["chromium", "chromium-browser"],
    },
    Browser {
        name: "Brave",
        data: "BraveSoftware/Brave-Browser",
        commands: &["brave-browser", "brave"],
    },
    Browser {
        name: "Edge",
        data: "microsoft-edge",
        commands: &["microsoft-edge-stable", "microsoft-edge"],
    },
    Browser {
        name: "Vivaldi",
        data: "vivaldi",
        commands: &["vivaldi-stable", "vivaldi"],
    },
];

#[cfg(target_os = "macos")]
const BROWSERS: &[Browser] = &[
    Browser {
        name: "Chrome",
        data: "Google/Chrome",
        app: "Google Chrome",
    },
    Browser {
        name: "Chromium",
        data: "Chromium",
        app: "Chromium",
    },
    Browser {
        name: "Brave",
        data: "BraveSoftware/Brave-Browser",
        app: "Brave Browser",
    },
    Browser {
        name: "Edge",
        data: "Microsoft Edge",
        app: "Microsoft Edge",
    },
    Browser {
        name: "Vivaldi",
        data: "Vivaldi",
        app: "Vivaldi",
    },
    Browser {
        name: "Arc",
        data: "Arc/User Data",
        app: "Arc",
    },
];

#[cfg(windows)]
const BROWSERS: &[Browser] = &[
    Browser {
        name: "Chrome",
        data: r"Google\Chrome\User Data",
        exe: "chrome.exe",
    },
    Browser {
        name: "Chromium",
        data: r"Chromium\User Data",
        exe: "chromium.exe",
    },
    Browser {
        name: "Brave",
        data: r"BraveSoftware\Brave-Browser\User Data",
        exe: "brave.exe",
    },
    Browser {
        name: "Edge",
        data: r"Microsoft\Edge\User Data",
        exe: "msedge.exe",
    },
    Browser {
        name: "Vivaldi",
        data: r"Vivaldi\User Data",
        exe: "vivaldi.exe",
    },
];

/// One profile of one browser.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub browser: &'static str,
    /// The name people gave the profile, like "Work".
    pub name: String,
    pub dir: PathBuf,
    /// The start of a command that opens a URL in this profile; the URL goes last.
    /// `None` when the browser's program couldn't be found.
    pub open: Option<Vec<String>>,
}

impl Profile {
    /// The profile's switch in `settings.toml`, like `chrome-profile-7`.
    pub fn key(&self) -> String {
        let folder = self.dir.file_name().unwrap_or_default().to_string_lossy();
        format!("{}-{folder}", self.browser)
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Bookmark {
    title: String,
    url: String,
    profile: usize,
}

/// A result, and whether it's a bookmark rather than a page from history.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub item: Item,
    pub bookmark: bool,
}

/// The browsers' bookmarks, read when the search bar opens, and their history,
/// searched as you type.
pub struct Browsers {
    profiles: Vec<Profile>,
    bookmarks: Vec<Bookmark>,
    history: bool,
    limit: usize,
}

impl Browsers {
    /// The profiles `settings` leaves on, searched the way it says. `settings` holds
    /// a value for every key [`settings`] declares.
    pub fn load(settings: &Map<String, Value>) -> Browsers {
        let on = |key: &str| settings.get(key) != Some(&Value::Bool(false));
        let profiles = installed_profiles()
            .into_iter()
            .filter(|profile| on(&profile.key()))
            .collect();
        let limit = settings[RESULTS].as_u64().unwrap_or(3) as usize;
        Browsers::from_profiles(profiles, on(BOOKMARKS), on(HISTORY), limit)
    }

    pub fn from_profiles(
        profiles: Vec<Profile>,
        bookmarks: bool,
        history: bool,
        limit: usize,
    ) -> Browsers {
        let bookmarks = match bookmarks {
            true => profiles
                .iter()
                .enumerate()
                .flat_map(|(i, profile)| read_bookmarks(&profile.dir.join("Bookmarks"), i))
                .collect(),
            false => Vec::new(),
        };
        Browsers {
            profiles,
            bookmarks,
            history,
            limit,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Bookmarks whose title or address has every word of `query`, then pages from
    /// history, most visited first.
    pub fn search(&self, query: &str) -> Vec<Found> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.concat().chars().count() < MIN_QUERY {
            return Vec::new();
        }
        let mut seen = HashSet::new();
        let mut found = Vec::new();

        let mut bookmarks: Vec<&Bookmark> = self
            .bookmarks
            .iter()
            .filter(|b| matches(&words, &b.title, &b.url))
            .collect();
        // Titles that start with what was typed first, then the shortest addresses,
        // which tend to be the site rather than one page of it.
        bookmarks.sort_by_key(|b| (!b.title.to_lowercase().starts_with(&words[0]), b.url.len()));
        for bookmark in bookmarks {
            if found.len() == self.limit {
                return found;
            }
            if seen.insert(bookmark.url.clone()) {
                found.push(self.found(&bookmark.title, &bookmark.url, bookmark.profile, true));
            }
        }

        if self.history {
            let mut pages: Vec<(i64, String, String, usize)> = Vec::new();
            for (i, profile) in self.profiles.iter().enumerate() {
                pages.extend(
                    search_history(&profile.dir.join("History"), &words, self.limit * 3)
                        .into_iter()
                        .map(|(visits, url, title)| (visits, url, title, i)),
                );
            }
            pages.sort_by_key(|page| std::cmp::Reverse(page.0));
            // History keeps a page once per address, so the same page under a
            // slightly different one, like with #inbox, would show twice.
            let mut titles = HashSet::new();
            for (_, url, title, profile) in pages {
                if found.len() == self.limit {
                    break;
                }
                let repeat = !title.is_empty() && !titles.insert((profile, title.clone()));
                if seen.insert(url.clone()) && !repeat {
                    found.push(self.found(&title, &url, profile, false));
                }
            }
        }
        found
    }

    fn found(&self, title: &str, url: &str, profile: usize, bookmark: bool) -> Found {
        let profile = &self.profiles[profile];
        let address = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');
        // Name the profile only when there's more than one to tell apart.
        let subtitle = match self.profiles.len() {
            1 => address.to_owned(),
            _ if self.browsers() == 1 => format!("{address} · {}", profile.name),
            _ => format!("{address} · {} {}", profile.browser, profile.name),
        };
        let action = match &profile.open {
            Some(open) => Action::Run(open.iter().cloned().chain([url.to_owned()]).collect()),
            None => Action::Open(url.to_owned()),
        };
        Found {
            item: Item {
                title: if title.trim().is_empty() {
                    address.to_owned()
                } else {
                    title.to_owned()
                },
                subtitle: Some(subtitle),
                action,
                alt: Some(Action::Copy(url.to_owned())),
            },
            bookmark,
        }
    }

    fn browsers(&self) -> usize {
        self.profiles
            .iter()
            .map(|p| p.browser)
            .collect::<HashSet<_>>()
            .len()
    }
}

fn matches(words: &[String], title: &str, url: &str) -> bool {
    let title = title.to_lowercase();
    let url = url.to_lowercase();
    words
        .iter()
        .all(|w| title.contains(w.as_str()) || url.contains(w.as_str()))
}

#[derive(Deserialize)]
struct BookmarksFile {
    roots: std::collections::HashMap<String, serde_json::Value>,
}

fn read_bookmarks(path: &Path, profile: usize) -> Vec<Bookmark> {
    let Some(file) = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<BookmarksFile>(&text).ok())
    else {
        return Vec::new();
    };
    let mut bookmarks = Vec::new();
    let mut stack: Vec<&serde_json::Value> = file.roots.values().collect();
    while let Some(node) = stack.pop() {
        match node.get("type").and_then(|t| t.as_str()) {
            Some("url") => {
                let text = |key| node.get(key).and_then(|v: &serde_json::Value| v.as_str());
                if let (Some(title), Some(url)) = (text("name"), text("url")) {
                    bookmarks.push(Bookmark {
                        title: title.to_owned(),
                        url: url.to_owned(),
                        profile,
                    });
                }
            }
            _ => {
                if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
                    stack.extend(children.iter().rev());
                }
            }
        }
    }
    bookmarks
}

/// Pages from one profile's history with every word in the title or address, as
/// `(visits, url, title)`. The browser keeps the file locked while it runs, so it's
/// opened as unchanging: nothing is locked and nothing is written.
fn search_history(path: &Path, words: &[String], limit: usize) -> Vec<(i64, String, String)> {
    if !path.is_file() {
        return Vec::new();
    }
    let uri = format!("file:{}?immutable=1", uri_path(path));
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let Ok(conn) = Connection::open_with_flags(uri, flags) else {
        return Vec::new();
    };
    let mut sql = String::from(
        "SELECT visit_count + 2 * typed_count, url, title FROM urls
         WHERE hidden = 0 AND url LIKE 'http%'",
    );
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    for word in words {
        sql.push_str(" AND (lower(title) LIKE ? ESCAPE '^' OR lower(url) LIKE ? ESCAPE '^')");
        let pattern = format!("%{}%", escape_like(word));
        args.push(rusqlite::types::Value::Text(pattern.clone()));
        args.push(rusqlite::types::Value::Text(pattern));
    }
    sql.push_str(" ORDER BY 1 DESC, last_visit_time DESC LIMIT ?");
    args.push(rusqlite::types::Value::Integer(limit as i64));
    // A read that lands while the browser writes can fail; the next keystroke retries.
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    stmt.query_map(params_from_iter(args), |r| {
        Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?))
    })
    .map(|rows| {
        rows.flatten()
            .map(|(visits, url, title)| (visits, url, title.unwrap_or_default()))
            .collect()
    })
    .unwrap_or_default()
}

/// A path as the path part of an SQLite `file:` URI.
fn uri_path(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let path = path
        .replace('%', "%25")
        .replace('?', "%3f")
        .replace('#', "%23");
    if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    }
}

fn escape_like(text: &str) -> String {
    text.replace('^', "^^")
        .replace('%', "^%")
        .replace('_', "^_")
}

#[derive(Deserialize)]
struct LocalState {
    profile: ProfileState,
}

#[derive(Deserialize)]
struct ProfileState {
    #[serde(default)]
    info_cache: std::collections::HashMap<String, ProfileInfo>,
}

#[derive(Deserialize)]
struct ProfileInfo {
    name: String,
}

/// The profiles in one browser's user data folder: those listed in its `Local
/// State`, or just `Default` when there's no list.
fn profiles_in(data: &Path) -> Vec<(String, PathBuf)> {
    let listed = fs::read_to_string(data.join("Local State"))
        .ok()
        .and_then(|text| serde_json::from_str::<LocalState>(&text).ok())
        .map(|state| state.profile.info_cache)
        .unwrap_or_default();
    let mut profiles: Vec<(String, PathBuf)> = if listed.is_empty() {
        vec![("Default".to_owned(), data.join("Default"))]
    } else {
        listed
            .into_iter()
            .map(|(dir, info)| (info.name, data.join(dir)))
            .collect()
    };
    profiles.retain(|(_, dir)| dir.is_dir());
    profiles.sort_by(|a, b| a.1.cmp(&b.1));
    profiles
}

fn installed_profiles() -> Vec<Profile> {
    #[cfg(windows)]
    let base = dirs::data_local_dir();
    #[cfg(not(windows))]
    let base = dirs::config_dir();
    let Some(base) = base else {
        return Vec::new();
    };
    let mut all = Vec::new();
    for browser in BROWSERS {
        let data = base.join(browser.data);
        let profiles = profiles_in(&data);
        if profiles.is_empty() {
            continue;
        }
        let program = program(browser);
        for (name, dir) in profiles {
            let open = program.as_ref().map(|start| {
                let folder = dir.file_name().unwrap_or_default().to_string_lossy();
                let mut argv = start.clone();
                argv.push(format!("--profile-directory={folder}"));
                argv
            });
            all.push(Profile {
                browser: browser.name,
                name,
                dir,
                open,
            });
        }
    }
    all
}

/// The command that starts `browser`, before its own arguments.
#[cfg(target_os = "linux")]
fn program(browser: &Browser) -> Option<Vec<String>> {
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    browser
        .commands
        .iter()
        .find(|command| dirs.iter().any(|dir| dir.join(command).is_file()))
        .map(|command| vec![command.to_string()])
}

#[cfg(target_os = "macos")]
fn program(browser: &Browser) -> Option<Vec<String>> {
    let installed = [
        "/Applications",
        &format!("{}/Applications", std::env::var("HOME").ok()?),
    ]
    .iter()
    .any(|apps| {
        Path::new(apps)
            .join(format!("{}.app", browser.app))
            .exists()
    });
    installed.then(|| {
        ["open", "-na", browser.app, "--args"]
            .map(str::to_owned)
            .to_vec()
    })
}

#[cfg(windows)]
fn program(browser: &Browser) -> Option<Vec<String>> {
    let key = format!(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{}",
        browser.exe
    );
    [
        windows_registry::CURRENT_USER,
        windows_registry::LOCAL_MACHINE,
    ]
    .iter()
    .find_map(|root| root.open(&key).and_then(|k| k.get_string("")).ok())
    .map(|exe| vec![exe])
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOKMARKS: &str = r#"{"roots": {
        "bookmark_bar": {"type": "folder", "children": [
            {"type": "url", "name": "Rust documentation", "url": "https://doc.rust-lang.org/"},
            {"type": "folder", "name": "Work", "children": [
                {"type": "url", "name": "Sonar issues", "url": "https://github.com/mtch3n/sonar/issues"}
            ]}
        ]},
        "other": {"type": "folder", "children": []},
        "synced": {"type": "folder", "children": [
            {"type": "url", "name": "Rust playground", "url": "https://play.rust-lang.org/"}
        ]}
    }}"#;

    fn history(dir: &Path, pages: &[(&str, &str, i64)]) {
        let conn = Connection::open(dir.join("History")).unwrap();
        conn.execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY AUTOINCREMENT, url LONGVARCHAR, title LONGVARCHAR,
             visit_count INTEGER DEFAULT 0 NOT NULL, typed_count INTEGER DEFAULT 0 NOT NULL,
             last_visit_time INTEGER NOT NULL, hidden INTEGER DEFAULT 0 NOT NULL);",
        )
        .unwrap();
        for (url, title, visits) in pages {
            conn.execute(
                "INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES (?1, ?2, ?3, 0)",
                rusqlite::params![url, title, visits],
            )
            .unwrap();
        }
    }

    fn profile(root: &Path, dir: &str, name: &str, open: Option<&str>) -> Profile {
        let dir = root.join(dir);
        fs::create_dir_all(&dir).unwrap();
        Profile {
            browser: "Chrome",
            name: name.into(),
            dir,
            open: open.map(|program| vec![program.to_owned(), "--profile-directory=x".into()]),
        }
    }

    fn titles(found: &[Found]) -> Vec<&str> {
        found.iter().map(|f| f.item.title.as_str()).collect()
    }

    #[test]
    fn finds_bookmarks_then_history() {
        let tmp = tempfile::tempdir().unwrap();
        let work = profile(tmp.path(), "Profile 1", "Work", Some("google-chrome"));
        fs::write(work.dir.join("Bookmarks"), BOOKMARKS).unwrap();
        history(
            &work.dir,
            &[
                ("https://doc.rust-lang.org/", "Rust documentation", 40),
                ("https://www.rust-lang.org/learn", "Learn Rust", 12),
                ("https://crates.io/", "crates.io: Rust Package Registry", 30),
                ("chrome://settings/", "Settings rust", 99),
                (
                    "https://crates.io/?q=",
                    "crates.io: Rust Package Registry",
                    5,
                ),
            ],
        );
        let browsers = Browsers::from_profiles(vec![work], true, true, 10);

        let found = browsers.search("rust");
        assert_eq!(
            titles(&found),
            [
                "Rust documentation",
                "Rust playground",
                "crates.io: Rust Package Registry",
                "Learn Rust"
            ],
            "bookmarks first, a bookmarked page isn't repeated, browser pages are left out"
        );
        assert!(found[0].bookmark && !found[2].bookmark);
        assert_eq!(found[0].item.subtitle.as_deref(), Some("doc.rust-lang.org"));
        assert_eq!(
            found[0].item.action,
            Action::Run(vec![
                "google-chrome".into(),
                "--profile-directory=x".into(),
                "https://doc.rust-lang.org/".into()
            ])
        );
        assert_eq!(
            found[0].item.alt,
            Some(Action::Copy("https://doc.rust-lang.org/".into()))
        );

        assert_eq!(titles(&browsers.search("sonar issues")), ["Sonar issues"]);
        assert_eq!(
            titles(&browsers.search("github work")),
            Vec::<&str>::new(),
            "folders aren't words"
        );
        assert!(browsers.search("r").is_empty(), "too short");
    }

    #[test]
    fn respects_the_limit_and_the_history_setting() {
        let tmp = tempfile::tempdir().unwrap();
        let only = profile(tmp.path(), "Default", "Person 1", None);
        fs::write(only.dir.join("Bookmarks"), BOOKMARKS).unwrap();
        history(
            &only.dir,
            &[("https://crates.io/", "crates.io: Rust Package Registry", 30)],
        );

        let two = Browsers::from_profiles(vec![only.clone()], true, true, 2);
        assert_eq!(titles(&two.search("rust")).len(), 2);
        let no_history = Browsers::from_profiles(vec![only.clone()], true, false, 10);
        assert_eq!(titles(&no_history.search("crates")), Vec::<&str>::new());
        let found = no_history.search("rust");
        assert_eq!(
            found[0].item.action,
            Action::Open("https://doc.rust-lang.org/".into()),
            "without the browser's program, the default browser opens it"
        );
        let history_only = Browsers::from_profiles(vec![only], false, true, 10);
        assert_eq!(
            titles(&history_only.search("rust")),
            ["crates.io: Rust Package Registry"]
        );
    }

    #[test]
    fn names_the_profile_when_there_are_several() {
        let tmp = tempfile::tempdir().unwrap();
        let work = profile(tmp.path(), "Profile 1", "Work", None);
        let home = profile(tmp.path(), "Profile 2", "Home", None);
        fs::write(home.dir.join("Bookmarks"), BOOKMARKS).unwrap();
        let browsers = Browsers::from_profiles(vec![work, home], true, false, 10);
        let found = browsers.search("playground");
        assert_eq!(
            found[0].item.subtitle.as_deref(),
            Some("play.rust-lang.org · Home")
        );
    }

    #[test]
    fn profiles_have_readable_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let profile = profile(tmp.path(), "Profile 7", "Work", None);
        assert_eq!(profile.key(), "chrome-profile-7");
    }

    #[test]
    fn lists_profiles_from_local_state() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        assert!(profiles_in(data).is_empty());
        fs::create_dir(data.join("Default")).unwrap();
        assert_eq!(
            profiles_in(data),
            [("Default".to_owned(), data.join("Default"))]
        );

        fs::create_dir(data.join("Profile 7")).unwrap();
        fs::write(
            data.join("Local State"),
            r#"{"profile": {"info_cache": {"Profile 7": {"name": "gojitech"}, "Profile 9": {"name": "gone"}}}}"#,
        )
        .unwrap();
        assert_eq!(
            profiles_in(data),
            [("gojitech".to_owned(), data.join("Profile 7"))]
        );
    }

    #[test]
    fn history_paths_become_uris() {
        assert_eq!(
            uri_path(Path::new("/home/me/Profile 1/History")),
            "/home/me/Profile 1/History"
        );
        assert_eq!(uri_path(Path::new("/a?b#c%d")), "/a%3fb%23c%25d");
        assert_eq!(
            uri_path(Path::new(r"C:\Users\me\History")),
            "/C:/Users/me/History"
        );
    }
}
