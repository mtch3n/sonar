//! `settings.toml`: what people can tweak without rebuilding Sonar. The file is read
//! again every time the search bar opens; a broken file keeps the last good settings.

use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};
use sonar_core::{Kind, Level, Levels, ScanOptions};
use sonar_plugins::store::Repo;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub shortcut: String,
    pub marketplaces: Vec<String>,
    pub appearance: Appearance,
    pub search: Search,
    pub files: Files,
    pub index: Index,
    pub meaning: Meaning,
    pub updates: Updates,
    pub plugins: BTreeMap<String, PluginSettings>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub theme: Theme,
    pub monitor: Monitor,
    pub accent: String,
    pub width: u32,
    pub rows: u32,
}

/// The accent that follows the desktop's own.
pub const SYSTEM_ACCENT: &str = "system";
/// Sonar's own accent, for when the desktop has none.
const DEFAULT_ACCENT: &str = "#ff5a1f";

impl Appearance {
    /// The accent as a color, with `system` looked up from the desktop by `system`.
    pub fn accent_color(&self, system: impl FnOnce() -> Option<String>) -> String {
        if self.accent == SYSTEM_ACCENT {
            system().unwrap_or_else(|| DEFAULT_ACCENT.to_owned())
        } else {
            self.accent.clone()
        }
    }
}

/// The screen the search bar opens on.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Monitor {
    /// The one the pointer is on.
    Active,
    /// The main screen, as the desktop calls it.
    Main,
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Files {
    /// The command that opens projects and code, like `code` or `zed`. Empty opens
    /// them in their default app.
    pub editor: String,
    /// The terminal to open folders in, like `ptyxis` or `open -a iTerm`. Empty uses
    /// the first one found.
    pub terminal: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Index {
    pub rescan_minutes: u64,
    /// How much of each file's text is searched, in KB.
    pub text_kb: u64,
    /// The level of kinds that differ from their default, like `sheet = "name"`.
    pub kinds: BTreeMap<String, String>,
}

impl Index {
    /// What a scan keeps of each file. Assumes the settings were checked.
    pub fn scan_options(&self) -> ScanOptions {
        let mut levels = Levels::default();
        for (kind, level) in &self.kinds {
            if let (Some(kind), Some(level)) = (Kind::from_name(kind), Level::from_name(level)) {
                levels.set(kind, level);
            }
        }
        ScanOptions {
            text_limit: self.text_kb as usize * 1024,
            levels,
        }
    }

    fn check(&self) -> Result<(), String> {
        within("rescan_minutes", self.rescan_minutes, 1, 24 * 60)?;
        within("text_kb", self.text_kb, 1, 16 * 1024)?;
        for (kind, level) in &self.kinds {
            let known = Kind::ALL.iter().any(|k| k.as_str() == kind);
            if !known {
                let kinds: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
                return Err(format!(
                    "index.kinds: unknown kind `{kind}`; use one of: {}",
                    kinds.join(", ")
                ));
            }
            if Level::from_name(level).is_none() {
                return Err(format!(
                    "index.kinds.{kind} is `{level}`; use skip, name, text or meaning"
                ));
            }
        }
        Ok(())
    }
}

/// Searching by meaning, with a model that runs on this computer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Meaning {
    pub enabled: bool,
    /// One of `sonar_models::MODELS`.
    pub model: String,
}

impl Default for Meaning {
    fn default() -> Meaning {
        Meaning {
            enabled: false,
            model: sonar_models::DEFAULT_MODEL.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Updates {
    pub check: bool,
}

/// `[plugins.<id>]`. Unknown keys aren't mistakes here: they are the plugin's own
/// settings, checked against what the plugin declares once it is loaded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    pub enabled: bool,
    pub keyword: Option<String>,
    #[serde(flatten)]
    pub values: serde_json::Map<String, serde_json::Value>,
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
            files: Files::default(),
            index: Index::default(),
            meaning: Meaning::default(),
            updates: Updates::default(),
            plugins: BTreeMap::new(),
        }
    }
}

impl Default for Appearance {
    fn default() -> Appearance {
        Appearance {
            theme: Theme::System,
            monitor: Monitor::Active,
            accent: DEFAULT_ACCENT.to_owned(),
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
        Index {
            rescan_minutes: 5,
            text_kb: (sonar_core::DEFAULT_TEXT_LIMIT / 1024) as u64,
            kinds: BTreeMap::new(),
        }
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
            values: serde_json::Map::new(),
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
        if a.accent != SYSTEM_ACCENT && !is_hex_color(&a.accent) {
            return Err(format!(
                "accent `{}` isn't \"system\" or a color like #ff5a1f",
                a.accent
            ));
        }
        within("width", a.width, 480, 1600)?;
        within("rows", a.rows, 3, 20)?;
        within("limit", self.search.limit, 1, 500)?;
        self.editor()?;
        self.terminal()?;
        self.index.check()?;
        if sonar_models::info(&self.meaning.model).is_none() {
            let models: Vec<&str> = sonar_models::MODELS.iter().map(|m| m.id).collect();
            return Err(format!(
                "meaning.model is `{}`; use one of: {}",
                self.meaning.model,
                models.join(", ")
            ));
        }
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

    /// The editor command split into the program and its arguments, or `None` when
    /// files open in their default app.
    pub fn editor(&self) -> Result<Option<Vec<String>>, String> {
        command("editor", &self.files.editor)
    }

    /// The terminal command, or `None` to use the first terminal found.
    pub fn terminal(&self) -> Result<Option<Vec<String>>, String> {
        command("terminal", &self.files.terminal)
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
    // A file from an older Sonar may lack a newer section; add it as a [section]
    // rather than letting it become an inline table.
    for name in [
        "appearance",
        "search",
        "files",
        "index",
        "meaning",
        "updates",
    ] {
        if !doc.contains_table(name) {
            doc.insert(name, Item::Table(Table::new()));
        }
    }
    set(&mut doc["shortcut"], settings.shortcut.as_str().into());
    set(&mut doc["marketplaces"], marketplaces.into());
    set(&mut doc["appearance"]["theme"], theme.into());
    let monitor = match a.monitor {
        Monitor::Active => "active",
        Monitor::Main => "main",
    };
    set(&mut doc["appearance"]["monitor"], monitor.into());
    set(&mut doc["appearance"]["accent"], a.accent.as_str().into());
    set(&mut doc["appearance"]["width"], i64::from(a.width).into());
    set(&mut doc["appearance"]["rows"], i64::from(a.rows).into());
    set(
        &mut doc["search"]["limit"],
        (settings.search.limit as i64).into(),
    );
    set(
        &mut doc["files"]["editor"],
        settings.files.editor.as_str().into(),
    );
    set(
        &mut doc["files"]["terminal"],
        settings.files.terminal.as_str().into(),
    );
    set(
        &mut doc["index"]["rescan_minutes"],
        (settings.index.rescan_minutes as i64).into(),
    );
    set(
        &mut doc["index"]["text_kb"],
        (settings.index.text_kb as i64).into(),
    );
    let mut kinds = Table::new();
    for (kind, level) in &settings.index.kinds {
        kinds[kind.as_str()] = value(level.as_str());
    }
    if kinds.is_empty() {
        if let Some(index) = doc["index"].as_table_mut() {
            index.remove("kinds");
        }
    } else {
        doc["index"]["kinds"] = Item::Table(kinds);
    }
    set(
        &mut doc["meaning"]["enabled"],
        settings.meaning.enabled.into(),
    );
    set(
        &mut doc["meaning"]["model"],
        settings.meaning.model.as_str().into(),
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
        for (key, setting) in &plugin.values {
            let setting = match setting {
                serde_json::Value::String(text) => value(text.as_str()),
                serde_json::Value::Bool(on) => value(*on),
                serde_json::Value::Number(n) => match n.as_i64() {
                    Some(n) => value(n),
                    None => value(n.as_f64().unwrap_or_default()),
                },
                _ => {
                    return Err(format!(
                        "plugins.{id}.{key} must be text, a number or true/false"
                    ));
                }
            };
            table[key.as_str()] = setting;
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

/// A command setting split into the program and its arguments; `None` when empty.
fn command(name: &str, text: &str) -> Result<Option<Vec<String>>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    shell_words::split(text)
        .map(Some)
        .map_err(|_| format!("{name} `{text}` has a quote that isn't closed"))
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
monitor = "active"  # screen the bar opens on: "active", the one the pointer is on, or "main"
accent = "#ff5a1f"  # color of the selection and the text cursor, or "system"
width = 720         # 480 to 1600
rows = 8            # results shown before the list scrolls, 3 to 20

[search]
limit = 20          # results to find when the query has no limit: filter

[files]
editor = ""         # opens projects and code, like "code" or "zed"; empty uses the default app
terminal = ""       # opens folders, like "ptyxis" or "open -a iTerm"; empty uses the first found

[index]
rescan_minutes = 5  # how often to rescan everything, for changes the watch missed
text_kb = 64        # how much of each file's text is searched, 1 to 16384

# How much of each kind of file is indexed: "skip", "name", "text" or "meaning".
# Documents, PDFs, slides and scripts are searched by meaning, spreadsheets and
# config files for words, and everything else by name. Keys are never read.
#
# [index.kinds]
# sheet = "name"
# code = "text"

[meaning]
enabled = false     # search by meaning too; downloads the model the first time
model = "multilingual"  # "multilingual", "english", "bge-small-en" or "multilingual-e5-small"

[updates]
check = true        # look for new versions of Sonar on GitHub

# Turn a plugin off, give it another keyword, or change its own settings:
#
# [plugins.calculator]
# currency = "EUR"  # what amounts like 100 usd are shown in
# rates = false     # stop downloading exchange rates
#
# [plugins.browser]
# history = false   # search bookmarks only
# results = 5       # 1 to 10
#
# [plugins.web-search]
# keyword = "w"
# first = "duckduckgo"
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
            ("[files]\neditor = \"code '--new\"", "quote"),
            ("[files]\nterminal = \"kitty '\"", "terminal"),
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
                ..PluginSettings::default()
            },
        );
        settings.plugins.insert(
            "web-search".into(),
            PluginSettings {
                keyword: Some("w".into()),
                values: serde_json::json!({"first": "ddg", "results": 5, "safe": false})
                    .as_object()
                    .unwrap()
                    .clone(),
                ..PluginSettings::default()
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
        assert!(text.contains("first = \"ddg\""), "{text}");
        assert!(text.contains("results = 5\n"), "{text}");
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
    fn kind_levels_are_checked_and_saved() {
        let settings =
            Settings::parse("[index.kinds]\nsheet = \"name\"\nkey = \"meaning\"\n").unwrap();
        let levels = settings.index.scan_options().levels;
        assert_eq!(levels.get(Kind::Sheet), Level::Name);
        assert_eq!(levels.get(Kind::Key), Level::Name, "keys are never read");
        assert_eq!(levels.get(Kind::Pdf), Level::Meaning);
        for (text, says) in [
            ("[index.kinds]\nmovie = \"skip\"", "unknown kind `movie`"),
            ("[index.kinds]\nsheet = \"all\"", "index.kinds.sheet"),
        ] {
            let err = Settings::parse(text).unwrap_err();
            assert!(err.contains(says), "{text}: {err}");
        }

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        fs::write(&path, template()).unwrap();
        save(&path, &settings).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\n[index.kinds]\n"), "{text}");
        assert_eq!(Settings::parse(&text).unwrap(), settings);
        save(&path, &Settings::default()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("\n[index.kinds]\n"), "{text}");
    }

    #[test]
    fn plugin_values_are_read_from_the_plugin_table() {
        let settings = Settings::parse(
            "[plugins.calculator]\ncurrency = \"EUR\"\nrates = false\n\n[plugins.web-search]\nkeyword = \"w\"\nfirst = \"github\"\n",
        )
        .unwrap();
        let web = settings.plugin("web-search");
        assert_eq!(web.keyword.as_deref(), Some("w"));
        assert_eq!(web.values["first"], "github");
        assert_eq!(settings.plugin("calculator").values["currency"], "EUR");
    }

    #[test]
    fn editors_split_into_program_and_arguments() {
        assert_eq!(Settings::default().editor(), Ok(None));
        let settings =
            Settings::parse("[files]\neditor = \"open -a 'Visual Studio Code'\"\n").unwrap();
        assert_eq!(
            settings.editor(),
            Ok(Some(vec![
                "open".into(),
                "-a".into(),
                "Visual Studio Code".into()
            ]))
        );
    }

    #[test]
    fn saving_adds_sections_an_older_file_lacks() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        let template = template();
        let (before, rest) = template.split_once("[files]").unwrap();
        let rest = &rest[rest.find("[index]").unwrap()..];
        let (index, rest) = rest.split_once("[meaning]").unwrap();
        let old = format!(
            "{before}{index}{}",
            &rest[rest.find("[updates]").unwrap()..]
        );
        assert!(!old.contains("[files]"));
        fs::write(&path, old).unwrap();
        let mut settings = Settings::default();
        settings.files.editor = "subl".into();
        save(&path, &settings).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[files]\neditor = \"subl\""), "{text}");
        assert!(!text.contains("meaning = {"), "{text}");
        assert!(text.contains("# color of the selection"), "{text}");
        assert_eq!(Settings::parse(&text).unwrap(), settings);
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
