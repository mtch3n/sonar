//! `settings.toml`: what people can tweak without rebuilding Sonar. The file is read
//! again every time the search bar opens; a broken file keeps the last good settings.

use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};
use sonar_plugins::store::Repo;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub shortcut: String,
    pub marketplaces: Vec<String>,
    pub appearance: Appearance,
    pub search: Search,
    pub index: Index,
    pub updates: Updates,
    pub plugins: BTreeMap<String, PluginSettings>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub theme: Theme,
    pub accent: String,
    pub width: u32,
    pub rows: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Search {
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Index {
    pub rescan_minutes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Updates {
    pub check: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginSettings {
    pub enabled: bool,
    pub keyword: Option<String>,
}

#[cfg(target_os = "linux")]
const DEFAULT_SHORTCUT: &str = "ctrl+alt+space";
#[cfg(not(target_os = "linux"))]
const DEFAULT_SHORTCUT: &str = "alt+space";

pub const OFFICIAL_MARKETPLACE: &str = "mtch3n/sonar";

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            shortcut: DEFAULT_SHORTCUT.to_owned(),
            marketplaces: vec![OFFICIAL_MARKETPLACE.to_owned()],
            appearance: Appearance::default(),
            search: Search::default(),
            index: Index::default(),
            updates: Updates::default(),
            plugins: BTreeMap::new(),
        }
    }
}

impl Default for Appearance {
    fn default() -> Appearance {
        Appearance {
            theme: Theme::System,
            accent: "#ff5a1f".to_owned(),
            width: 720,
            rows: 8,
        }
    }
}

impl Default for Search {
    fn default() -> Search {
        Search {
            limit: sonar_core::DEFAULT_LIMIT,
        }
    }
}

impl Default for Index {
    fn default() -> Index {
        Index { rescan_minutes: 5 }
    }
}

impl Default for Updates {
    fn default() -> Updates {
        Updates { check: true }
    }
}

impl Default for PluginSettings {
    fn default() -> PluginSettings {
        PluginSettings {
            enabled: true,
            keyword: None,
        }
    }
}

impl Settings {
    /// Reads `path`, writing the default file first if there is none.
    pub fn load(path: &Path) -> Result<Settings, String> {
        if !path.exists() {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|err| err.to_string())?;
            }
            fs::write(path, template())
                .map_err(|err| format!("writing {}: {err}", path.display()))?;
        }
        let text =
            fs::read_to_string(path).map_err(|err| format!("reading {}: {err}", path.display()))?;
        Settings::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Settings, String> {
        let settings: Settings = toml::from_str(text).map_err(|err| {
            let line = err
                .span()
                .map(|span| text[..span.start].matches('\n').count() + 1);
            match line {
                Some(line) => format!("line {line}: {}", err.message()),
                None => err.message().to_owned(),
            }
        })?;
        settings.check()?;
        Ok(settings)
    }

    pub fn check(&self) -> Result<(), String> {
        Shortcut::parse(&self.shortcut)?;
        for repo in &self.marketplaces {
            Repo::parse(repo)
                .ok_or_else(|| format!("marketplace `{repo}` isn't a GitHub owner/name"))?;
        }
        let a = &self.appearance;
        if !is_hex_color(&a.accent) {
            return Err(format!("accent `{}` isn't a color like #ff5a1f", a.accent));
        }
        within("width", a.width, 480, 1600)?;
        within("rows", a.rows, 3, 20)?;
        within("limit", self.search.limit, 1, 500)?;
        within("rescan_minutes", self.index.rescan_minutes, 1, 24 * 60)?;
        for (id, plugin) in &self.plugins {
            if let Some(keyword) = &plugin.keyword {
                sonar_plugins::check_keyword(keyword)
                    .map_err(|err| format!("plugins.{id}: {err}"))?;
            }
        }
        Ok(())
    }

    pub fn plugin(&self, id: &str) -> PluginSettings {
        self.plugins.get(id).cloned().unwrap_or_default()
    }

    pub fn shortcut(&self) -> Shortcut {
        Shortcut::parse(&self.shortcut).expect("checked when loaded")
    }

    pub fn marketplaces(&self) -> Vec<Repo> {
        self.marketplaces
            .iter()
            .filter_map(|repo| Repo::parse(repo))
            .collect()
    }
}

/// Adds `repo` to `marketplaces` in the settings file, keeping the rest of the file
/// as the person wrote it.
pub fn add_marketplace(path: &Path, repo: &Repo) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    Settings::parse(&text).map_err(|err| format!("fix settings.toml first ({err})"))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|err: toml_edit::TomlError| err.to_string())?;
    let list = doc
        .entry("marketplaces")
        .or_insert_with(|| toml_edit::value(toml_edit::Array::new()))
        .as_array_mut()
        .ok_or("`marketplaces` in settings.toml isn't a list")?;
    let name = repo.to_string();
    if !list
        .iter()
        .any(|item| item.as_str().and_then(Repo::parse).as_ref() == Some(repo))
    {
        list.push(name);
    }
    fs::write(path, doc.to_string()).map_err(|err| err.to_string())
}

/// Writes `settings` into the settings file, keeping the comments and layout of what
/// is there. A file too broken to read is kept next to it as `settings.toml.bak`.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    settings.check()?;
    let text = fs::read_to_string(path).unwrap_or_else(|_| template());
    let mut written = write_into(&text, settings)?;
    // What was there may hold mistakes the form doesn't cover, like a misspelled key.
    // Start again from the default file then, and keep the old one.
    if Settings::parse(&written).is_err() {
        let _ = fs::copy(path, path.with_extension("toml.bak"));
        written = write_into(&template(), settings)?;
    }
    fs::write(path, written).map_err(|err| format!("writing {}: {err}", path.display()))
}

/// `text` with the values of `settings`, keeping its comments where it can.
fn write_into(text: &str, settings: &Settings) -> Result<String, String> {
    use toml_edit::{Array, DocumentMut, Item, Table, value};

    let mut doc: DocumentMut = text
        .parse()
        .or_else(|_| template().parse())
        .map_err(|err: toml_edit::TomlError| err.to_string())?;
    let a = &settings.appearance;
    let theme = match a.theme {
        Theme::System => "system",
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    let marketplaces: Array = settings.marketplaces.iter().map(String::as_str).collect();
    set(&mut doc["shortcut"], settings.shortcut.as_str().into());
    set(&mut doc["marketplaces"], marketplaces.into());
    set(&mut doc["appearance"]["theme"], theme.into());
    set(&mut doc["appearance"]["accent"], a.accent.as_str().into());
    set(&mut doc["appearance"]["width"], i64::from(a.width).into());
    set(&mut doc["appearance"]["rows"], i64::from(a.rows).into());
    set(
        &mut doc["search"]["limit"],
        (settings.search.limit as i64).into(),
    );
    set(
        &mut doc["index"]["rescan_minutes"],
        (settings.index.rescan_minutes as i64).into(),
    );
    set(&mut doc["updates"]["check"], settings.updates.check.into());

    // Only plugins that differ from their defaults get a table.
    let mut plugins = Table::new();
    plugins.set_implicit(true);
    for (id, plugin) in &settings.plugins {
        if *plugin == PluginSettings::default() {
            continue;
        }
        let mut table = Table::new();
        if !plugin.enabled {
            table["enabled"] = value(false);
        }
        if let Some(keyword) = &plugin.keyword {
            table["keyword"] = value(keyword.as_str());
        }
        plugins.insert(id, Item::Table(table));
    }
    if plugins.is_empty() {
        doc.remove("plugins");
    } else {
        doc["plugins"] = Item::Table(plugins);
    }
    Ok(doc.to_string())
}

/// Replaces a value, keeping the comment written after it.
fn set(item: &mut toml_edit::Item, new: toml_edit::Value) {
    let decor = item.as_value().map(|old| old.decor().clone());
    *item = toml_edit::Item::Value(new);
    if let (Some(decor), Some(value)) = (decor, item.as_value_mut()) {
        *value.decor_mut() = decor;
    }
}

fn within<T: PartialOrd + std::fmt::Display>(
    name: &str,
    value: T,
    min: T,
    max: T,
) -> Result<(), String> {
    if value < min || value > max {
        Err(format!("{name} is {value}; use {min} to {max}"))
    } else {
        Ok(())
    }
}

fn is_hex_color(text: &str) -> bool {
    text.strip_prefix('#')
        .is_some_and(|hex| matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A keyboard shortcut such as `ctrl+alt+space`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
    key: String,
}

impl Shortcut {
    pub fn parse(text: &str) -> Result<Shortcut, String> {
        let err = |why: &str| {
            format!("shortcut `{text}` {why}; write it like \"alt+space\" or \"ctrl+shift+k\"")
        };
        let mut parts: Vec<String> = text.split('+').map(|p| p.trim().to_lowercase()).collect();
        let key = parts
            .pop()
            .filter(|k| !k.is_empty())
            .ok_or_else(|| err("has no key"))?;
        let mut shortcut = Shortcut {
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            key: String::new(),
        };
        for part in &parts {
            match part.as_str() {
                "ctrl" | "control" => shortcut.ctrl = true,
                "alt" | "option" => shortcut.alt = true,
                "shift" => shortcut.shift = true,
                "super" | "cmd" | "command" | "meta" | "win" => shortcut.meta = true,
                other => return Err(err(&format!("has an unknown modifier `{other}`"))),
            }
        }
        let function_key = key
            .strip_prefix('f')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=24).contains(&n));
        let single = key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric());
        if !(single || function_key || key == "space") {
            return Err(err(&format!(
                "uses `{key}`, which isn't a letter, digit, space or F1 to F24"
            )));
        }
        if parts.is_empty() && !function_key {
            return Err(err("needs a modifier such as ctrl or alt"));
        }
        shortcut.key = key;
        Ok(shortcut)
    }

    /// For the global shortcut plugin on macOS and Windows.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub fn accelerator(&self) -> String {
        let mut parts = self.modifiers(["ctrl", "alt", "shift", "super"]);
        parts.push(self.key.clone());
        parts.join("+")
    }

    /// For GNOME's custom keyboard shortcuts.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn gnome(&self) -> String {
        let mods: String = self
            .modifiers(["<Control>", "<Alt>", "<Shift>", "<Super>"])
            .concat();
        let key = if self.key.starts_with('f') && self.key.len() > 1 {
            self.key.to_uppercase()
        } else {
            self.key.clone()
        };
        format!("{mods}{key}")
    }

    /// How the shortcut is written on this platform, for menus.
    pub fn label(&self) -> String {
        let key = match self.key.as_str() {
            "space" => "Space".to_owned(),
            key => key.to_uppercase(),
        };
        if cfg!(target_os = "macos") {
            format!("{}{key}", self.modifiers(["⌃", "⌥", "⇧", "⌘"]).concat())
        } else {
            let meta = if cfg!(windows) { "Win" } else { "Super" };
            let mut parts = self.modifiers(["Ctrl", "Alt", "Shift", meta]);
            parts.push(key);
            parts.join("+")
        }
    }

    fn modifiers(&self, names: [&str; 4]) -> Vec<String> {
        [self.ctrl, self.alt, self.shift, self.meta]
            .into_iter()
            .zip(names)
            .filter(|(on, _)| *on)
            .map(|(_, name)| name.to_owned())
            .collect()
    }
}

fn template() -> String {
    format!(
        r##"# Sonar settings. Sonar reads this file again each time the search bar opens.

# Keys that open the search bar, like "alt+space" or "ctrl+shift+k".
shortcut = "{DEFAULT_SHORTCUT}"

# GitHub repositories whose plugins you can install by typing "plugins".
marketplaces = ["{OFFICIAL_MARKETPLACE}"]

[appearance]
theme = "system"    # "system", "light" or "dark"
accent = "#ff5a1f"  # color of the selection and the text cursor
width = 720         # 480 to 1600
rows = 8            # results shown before the list scrolls, 3 to 20

[search]
limit = 20          # results to find when the query has no limit: filter

[index]
rescan_minutes = 5  # how often to look for new and changed files

[updates]
check = true        # look for new versions of Sonar on GitHub

# Turn a plugin off, or give it another keyword:
#
# [plugins.calculator]
# enabled = false
#
# [plugins.web-search]
# keyword = "w"
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_written_file_holds_the_defaults() {
        assert_eq!(Settings::parse(&template()).unwrap(), Settings::default());
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
    }

    #[test]
    fn mistakes_name_the_line() {
        let err = Settings::parse("[appearance]\nwidht = 900\n").unwrap_err();
        assert!(err.starts_with("line 2: unknown field `widht`"), "{err}");
        for (text, says) in [
            ("[appearance]\naccent = \"orange\"", "accent"),
            ("[appearance]\nrows = 40", "rows is 40"),
            ("shortcut = \"alt+space+k\"", "unknown modifier"),
            ("marketplaces = [\"nope\"]", "marketplace"),
            ("[plugins.web]\nkeyword = \"two words\"", "plugins.web"),
        ] {
            let err = Settings::parse(text).unwrap_err();
            assert!(err.contains(says), "{text}: {err}");
        }
    }

    #[test]
    fn shortcuts() {
        let s = Shortcut::parse("Ctrl+Alt+Space").unwrap();
        assert_eq!(s.accelerator(), "ctrl+alt+space");
        assert_eq!(s.gnome(), "<Control><Alt>space");
        let s = Shortcut::parse("super+shift+k").unwrap();
        assert_eq!(s.gnome(), "<Shift><Super>k");
        assert_eq!(Shortcut::parse("f12").unwrap().gnome(), "F12");
        for bad in ["space", "alt+", "alt+enter", "hyper+k", ""] {
            assert!(Shortcut::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn saving_keeps_comments_and_writes_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        fs::write(&path, template()).unwrap();
        let mut settings = Settings::default();
        settings.appearance.theme = Theme::Dark;
        settings.appearance.width = 900;
        settings.plugins.insert(
            "calculator".into(),
            PluginSettings {
                enabled: false,
                keyword: None,
            },
        );
        settings.plugins.insert(
            "web-search".into(),
            PluginSettings {
                enabled: true,
                keyword: Some("w".into()),
            },
        );
        settings
            .plugins
            .insert("same".into(), PluginSettings::default());
        save(&path, &settings).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# color of the selection"), "{text}");
        assert!(text.contains("theme = \"dark\"    # \"system\""), "{text}");
        assert!(text.contains("[plugins.web-search]"), "{text}");
        let mut expected = settings.clone();
        expected.plugins.remove("same");
        assert_eq!(Settings::parse(&text).unwrap(), expected);

        settings.appearance.rows = 99;
        assert!(
            save(&path, &settings).is_err(),
            "invalid settings are not written"
        );

        fs::write(&path, "[appearance]\nwidht = 900\n").unwrap();
        save(&path, &Settings::default()).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        let old = fs::read_to_string(path.with_extension("toml.bak")).unwrap();
        assert!(old.contains("widht"));
    }

    #[test]
    fn adding_a_marketplace_keeps_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        fs::write(&path, template()).unwrap();
        let repo = Repo::parse("https://github.com/alice/sonar-market").unwrap();
        add_marketplace(&path, &repo).unwrap();
        add_marketplace(&path, &repo).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# Keys that open the search bar"));
        assert_eq!(
            Settings::parse(&text).unwrap().marketplaces,
            [OFFICIAL_MARKETPLACE, "alice/sonar-market"]
        );
    }
}
