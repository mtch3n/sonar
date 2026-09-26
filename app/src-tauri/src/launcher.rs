//! Everything between a keystroke and a result: sending the query to the file index
//! and plugins, streaming their sections to the search window, and carrying out the
//! result someone picks.

use std::{
    collections::HashMap,
    path::{MAIN_SEPARATOR, Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::Serialize;
use sonar_core::{Hit, Index, Paths, Query};
use sonar_plugins::{
    Action, External, Item, Manifest,
    store::{self, Found, Marketplace, Repo, Source},
    strip_keyword,
};
use tauri::{AppHandle, State, ipc::Channel};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;
use tokio::{sync::OnceCell, task::JoinSet};

use crate::{
    host,
    settings::{Settings, Theme},
    window,
};

/// Typing this and a space lists installed plugins and the marketplaces' plugins.
const PLUGINS_KEYWORD: &str = "plugins";
const CALCULATOR: &str = "calculator";
/// How long an external plugin may take to answer before it is restarted.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

pub struct Launcher {
    paths: Paths,
    index: Mutex<Index>,
    settings: Arc<RwLock<Settings>>,
    session: RwLock<Arc<Session>>,
    results: Mutex<Results>,
    indexing: AtomicBool,
    /// Problems with the settings, plugins or shortcut, shown when the bar opens.
    notices: Mutex<Vec<String>>,
}

/// The plugins for one opening of the search bar. External plugins run only while
/// the bar is open, so edits to a plugin apply the next time it opens.
struct Session {
    calculator: Option<Option<String>>,
    plugins: Vec<Plugin>,
    /// Plugins that are installed but turned off in the settings.
    disabled: Vec<Manifest>,
    catalog: OnceCell<Arc<Catalog>>,
}

/// Every marketplace in the settings, as far as it could be downloaded.
type Catalog = Vec<(Repo, Result<Marketplace, String>)>;

struct Plugin {
    external: Arc<External>,
    keyword: Option<String>,
}

/// The actions behind the rows on screen. The window only ever sends back a row id,
/// so it can't ask Sonar to open or run anything a plugin didn't offer.
#[derive(Default)]
struct Results {
    generation: u64,
    rows: HashMap<String, Entry>,
    next: u64,
}

struct Entry {
    generation: u64,
    action: Command,
    alt: Option<Command>,
}

/// What choosing a row does.
#[derive(Clone)]
enum Command {
    Plugin {
        action: Action,
        dir: Option<PathBuf>,
    },
    Install {
        id: String,
        name: String,
        source: Source,
        update: bool,
    },
    Uninstall {
        id: String,
        name: String,
    },
    Add(Repo),
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    key: String,
    title: String,
    rank: u32,
    rows: Vec<Row>,
    message: Option<String>,
    /// The message is an error rather than a note like "No results".
    warning: bool,
    pending: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    id: String,
    title: String,
    subtitle: Option<String>,
    meta: Option<String>,
    /// A glyph name the window knows, like a file kind or `calculator`.
    icon: String,
    /// A plugin's own icon, as a `data:` URL.
    image: Option<String>,
    action: &'static str,
    alt: Option<&'static str>,
}

/// How the search window should look, and anything it should tell the user.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    theme: Theme,
    accent: String,
    width: u32,
    rows: u32,
    indexing: bool,
    notices: Vec<String>,
}

/// What the window does after a row was chosen.
#[derive(Serialize)]
#[serde(tag = "then", rename_all = "camelCase")]
pub enum Outcome {
    Close,
    Fill { text: String },
    Refresh { notice: String },
}

/// A row before it has an id.
struct Draft {
    title: String,
    subtitle: Option<String>,
    meta: Option<String>,
    icon: &'static str,
    image: Option<String>,
    action: Command,
    alt: Option<Command>,
}

impl Launcher {
    pub fn open(paths: Paths) -> anyhow::Result<Launcher> {
        let index = Index::open(&paths.db)?;
        let launcher = Launcher {
            index: Mutex::new(index),
            settings: Arc::default(),
            session: RwLock::new(Arc::new(Session::empty())),
            results: Mutex::default(),
            indexing: AtomicBool::new(true),
            notices: Mutex::default(),
            paths,
        };
        launcher.reload();
        Ok(launcher)
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// Shared with the indexer and updater, which read it on every round.
    pub fn settings(&self) -> Arc<RwLock<Settings>> {
        self.settings.clone()
    }

    pub fn current_settings(&self) -> Settings {
        read(&self.settings).clone()
    }

    pub fn set_indexing(&self, indexing: bool) {
        self.indexing.store(indexing, Ordering::Relaxed);
    }

    /// Reads the settings and plugin folders again and stops the plugins that ran
    /// while the bar was last open.
    pub fn reload(&self) {
        let mut notices = Vec::new();
        match Settings::load(&self.paths.settings) {
            Ok(settings) => *write(&self.settings) = settings,
            Err(err) => notices.push(format!(
                "settings.toml {err}. Sonar is using the last settings that worked."
            )),
        }
        let settings = self.current_settings();
        let session = Session::load(&self.paths.plugins, &settings, &mut notices);
        *write(&self.session) = Arc::new(session);
        *lock(&self.notices) = notices;
    }

    /// Stops the plugins' programs once the bar closes; the next search starts them
    /// again.
    pub fn stop_plugins(&self) {
        let settings = self.current_settings();
        let session = Session::load(&self.paths.plugins, &settings, &mut Vec::new());
        *write(&self.session) = Arc::new(session);
    }

    /// Every installed plugin, on or off.
    pub fn installed(&self) -> Vec<Manifest> {
        let session = self.session();
        session
            .plugins
            .iter()
            .map(|plugin| plugin.external.manifest.clone())
            .chain(session.disabled.iter().cloned())
            .collect()
    }

    pub fn add_notice(&self, notice: String) {
        lock(&self.notices).push(notice);
    }

    pub fn view(&self) -> View {
        let settings = self.current_settings();
        View {
            theme: settings.appearance.theme,
            accent: settings.appearance.accent,
            width: settings.appearance.width,
            rows: settings.appearance.rows,
            indexing: self.indexing.load(Ordering::Relaxed),
            notices: lock(&self.notices).clone(),
        }
    }

    fn session(&self) -> Arc<Session> {
        read(&self.session).clone()
    }

    async fn search(&self, query: &str, out: &Channel<Vec<Section>>) -> Result<(), String> {
        let generation = lock(&self.results).begin();
        let session = self.session();
        let send = |sections: Vec<Section>| {
            let _ = out.send(sections);
        };

        if let Some(rest) = strip_keyword(query, PLUGINS_KEYWORD) {
            return self.plugins_view(generation, &session, rest, send).await;
        }
        if let Some(Some(keyword)) = &session.calculator
            && let Some(rest) = strip_keyword(query, keyword)
        {
            let rows = self.rows(
                generation,
                sonar_plugins::calculate(rest).map(calculator_draft),
            );
            let empty = rows
                .is_empty()
                .then(|| "Type a calculation, like 2^10 or 5 km to miles".to_owned());
            send(vec![section(CALCULATOR, "Calculator", 0, rows, empty)]);
            return Ok(());
        }
        for plugin in &session.plugins {
            if let Some(keyword) = &plugin.keyword
                && let Some(rest) = strip_keyword(query, keyword)
            {
                let manifest = &plugin.external.manifest;
                send(vec![pending(&manifest.id, &manifest.name, 0)]);
                if let Some(answer) = plugin.external.search(rest).await {
                    send(vec![self.plugin_section(generation, manifest, 0, answer)]);
                }
                return Ok(());
            }
        }

        let mut sections = Vec::new();
        let suggestions = self.rows(generation, suggestions(&session, query));
        if !suggestions.is_empty() {
            sections.push(section("keywords", "Plugins", 0, suggestions, None));
        }
        if session.calculator == Some(None)
            && let Some(item) = sonar_plugins::calculate(query)
        {
            let rows = self.rows(generation, Some(calculator_draft(item)));
            sections.push(section(CALCULATOR, "Calculator", 10, rows, None));
        }
        sections.push(self.files(generation, query));
        send(sections);

        let mut asked = JoinSet::new();
        let global = session
            .plugins
            .iter()
            .filter(|plugin| plugin.keyword.is_none());
        for (rank, plugin) in (30..).zip(global) {
            let external = plugin.external.clone();
            let query = query.to_owned();
            asked.spawn(async move {
                let answer = external.search(&query).await;
                (rank, external, answer)
            });
        }
        while let Some(Ok((rank, external, answer))) = asked.join_next().await {
            let answer = answer.filter(|answer| !matches!(answer, Ok(items) if items.is_empty()));
            if let Some(answer) = answer {
                send(vec![self.plugin_section(
                    generation,
                    &external.manifest,
                    rank,
                    answer,
                )]);
            }
        }
        Ok(())
    }

    fn files(&self, generation: u64, text: &str) -> Section {
        let home = &self.paths.home;
        let mut query = match Query::parse(text, home) {
            Ok(query) => query,
            Err(err) => return failed("files", "Files", 20, err.to_string()),
        };
        query.limit.get_or_insert(read(&self.settings).search.limit);
        let hits = lock(&self.index).search(&query);
        match hits {
            Ok(hits) => {
                let now = jiff::Timestamp::now().as_second();
                let drafts = hits.into_iter().map(|hit| file_draft(hit, home, now));
                section("files", "Files", 20, self.rows(generation, drafts), None)
            }
            Err(err) => failed("files", "Files", 20, format!("{err:#}")),
        }
    }

    fn plugin_section(
        &self,
        generation: u64,
        manifest: &Manifest,
        rank: u32,
        answer: Result<Vec<Item>, String>,
    ) -> Section {
        match answer {
            Ok(items) => {
                let drafts = items.into_iter().map(|item| plugin_draft(item, manifest));
                let rows = self.rows(generation, drafts);
                let empty = rows.is_empty().then(|| "No results".to_owned());
                section(&manifest.id, &manifest.name, rank, rows, empty)
            }
            Err(err) => failed(&manifest.id, &manifest.name, rank, err),
        }
    }

    async fn plugins_view(
        &self,
        generation: u64,
        session: &Session,
        filter: &str,
        send: impl Fn(Vec<Section>),
    ) -> Result<(), String> {
        let wanted = |name: &str, description: Option<&String>| {
            let filter = filter.to_lowercase();
            name.to_lowercase().contains(&filter)
                || description.is_some_and(|d| d.to_lowercase().contains(&filter))
        };
        let mut first = Vec::new();

        if let Some(repo) = filter.contains('/').then(|| Repo::parse(filter)).flatten() {
            let draft = Draft {
                title: format!("Add {repo}"),
                subtitle: Some(
                    "Install its plugin, or list its plugins if it's a marketplace".into(),
                ),
                meta: None,
                icon: "github",
                image: None,
                action: Command::Add(repo),
                alt: None,
            };
            let rows = self.rows(generation, Some(draft));
            first.push(section("add", "From GitHub", 0, rows, None));
        }

        let enabled = session
            .plugins
            .iter()
            .map(|plugin| (&plugin.external.manifest, plugin.keyword.as_deref(), true));
        let disabled = session
            .disabled
            .iter()
            .map(|manifest| (manifest, None, false));
        let drafts = enabled
            .chain(disabled)
            .filter(|(manifest, ..)| wanted(&manifest.name, manifest.description.as_ref()))
            .map(|(manifest, keyword, on)| installed_draft(manifest, keyword, on));
        let rows = self.rows(generation, drafts);
        let empty = (rows.is_empty() && filter.is_empty())
            .then(|| "No plugins yet. Install one below, or add a GitHub repository by typing plugins owner/name".to_owned());
        first.push(section("installed", "Installed", 10, rows, empty));

        let marketplaces = read(&self.settings).marketplaces();
        let cached = session.catalog.get().cloned();
        if cached.is_none() {
            first.extend(
                marketplaces.iter().zip(20..).map(|(repo, rank)| {
                    pending(&format!("market:{repo}"), &repo.to_string(), rank)
                }),
            );
        }
        send(first);

        let catalog = match cached {
            Some(catalog) => catalog,
            None => {
                let fetched = session
                    .catalog
                    .get_or_init(|| async move {
                        let fetched = tokio::task::spawn_blocking(move || {
                            marketplaces
                                .into_iter()
                                .map(|repo| {
                                    let market = store::marketplace(&repo);
                                    (repo, market)
                                })
                                .collect()
                        })
                        .await
                        .unwrap_or_default();
                        Arc::new(fetched)
                    })
                    .await;
                fetched.clone()
            }
        };
        let installed: Vec<String> = session
            .plugins
            .iter()
            .map(|p| p.external.manifest.id.clone())
            .chain(session.disabled.iter().map(|m| m.id.clone()))
            .collect();
        let mut sections = Vec::new();
        for ((repo, market), rank) in catalog.iter().zip(20..) {
            let key = format!("market:{repo}");
            match market {
                Ok(market) => {
                    let drafts = market
                        .plugins
                        .iter()
                        .filter(|listing| !installed.contains(&listing.id))
                        .filter(|listing| wanted(&listing.name, listing.description.as_ref()))
                        .map(|listing| Draft {
                            title: listing.name.clone(),
                            subtitle: listing.description.clone(),
                            meta: Some(listing.source.repo.to_string()),
                            icon: "plugin",
                            image: None,
                            action: Command::Install {
                                id: listing.id.clone(),
                                name: listing.name.clone(),
                                source: listing.source.clone(),
                                update: false,
                            },
                            alt: None,
                        });
                    let rows = self.rows(generation, drafts);
                    let empty = rows.is_empty().then(|| match filter {
                        "" => "Everything here is installed".to_owned(),
                        _ => "No plugins match".to_owned(),
                    });
                    sections.push(section(&key, &market.name, rank, rows, empty));
                }
                Err(err) => {
                    let title = repo.to_string();
                    sections.push(failed(&key, &title, rank, err.clone()));
                }
            }
        }
        send(sections);
        Ok(())
    }

    fn rows(&self, generation: u64, drafts: impl IntoIterator<Item = Draft>) -> Vec<Row> {
        let mut results = lock(&self.results);
        drafts
            .into_iter()
            .filter_map(|draft| results.add(generation, draft))
            .collect()
    }

    async fn activate(&self, app: &AppHandle, id: &str, alt: bool) -> Result<Outcome, String> {
        let command = {
            let results = lock(&self.results);
            let entry = results
                .rows
                .get(id)
                .ok_or("That result is gone; the list changed. Try again.")?;
            let command = if alt {
                entry.alt.clone()
            } else {
                Some(entry.action.clone())
            };
            command.ok_or("That result has no second action")?
        };
        let plugins = self.paths.plugins.clone();
        let settings_path = self.paths.settings.clone();
        match command {
            Command::Plugin { action, dir } => self.act(app, action, dir.as_deref()),
            Command::Install {
                id,
                name,
                source,
                update,
            } => {
                let verb = if update { "Updated" } else { "Installed" };
                blocking(move || store::install(&plugins, &id, &source)).await?;
                self.reload();
                Ok(Outcome::Refresh {
                    notice: format!("{verb} {name}"),
                })
            }
            Command::Uninstall { id, name } => {
                blocking(move || store::uninstall(&plugins, &id)).await?;
                self.reload();
                Ok(Outcome::Refresh {
                    notice: format!("Removed {name}"),
                })
            }
            Command::Add(repo) => {
                let notice = blocking(move || match store::probe(&repo)? {
                    Found::Marketplace(market) => {
                        crate::settings::add_marketplace(&settings_path, &repo)?;
                        Ok(format!(
                            "Added the {} marketplace with {} plugins",
                            market.name,
                            market.plugins.len()
                        ))
                    }
                    Found::Plugin => {
                        let source = Source {
                            repo: repo.clone(),
                            path: String::new(),
                        };
                        let manifest = store::install(&plugins, &store::id_for(&repo), &source)?;
                        Ok(format!("Installed {}", manifest.name))
                    }
                })
                .await?;
                self.reload();
                Ok(Outcome::Refresh { notice })
            }
        }
    }

    fn act(&self, app: &AppHandle, action: Action, dir: Option<&Path>) -> Result<Outcome, String> {
        let dir = dir.unwrap_or(&self.paths.home);
        match action {
            Action::Open(target) if target.contains("://") || target.starts_with("mailto:") => {
                host::open(app, &target)?;
            }
            Action::Open(path) => host::open(app, &dir.join(path).to_string_lossy())?,
            Action::Reveal(path) => app
                .opener()
                .reveal_item_in_dir(dir.join(path))
                .map_err(|err| err.to_string())?,
            Action::Copy(text) => {
                app.clipboard()
                    .write_text(text)
                    .map_err(|err| err.to_string())?;
                return Ok(close(app));
            }
            Action::Run(argv) => {
                run(&argv, dir)?;
                return Ok(close(app));
            }
            Action::Fill(text) => return Ok(Outcome::Fill { text }),
        }
        Ok(close(app))
    }
}

impl Session {
    fn empty() -> Session {
        Session {
            calculator: None,
            plugins: Vec::new(),
            disabled: Vec::new(),
            catalog: OnceCell::new(),
        }
    }

    fn load(dir: &Path, settings: &Settings, notices: &mut Vec<String>) -> Session {
        let calculator = settings.plugin(CALCULATOR);
        let (manifests, problems) = sonar_plugins::discover(dir);
        notices.extend(problems);
        let mut plugins: Vec<Plugin> = Vec::new();
        let mut disabled = Vec::new();
        let mut keywords = vec![PLUGINS_KEYWORD.to_owned()];
        keywords.extend(calculator.keyword.clone());
        for manifest in manifests {
            let config = settings.plugin(&manifest.id);
            if !config.enabled {
                disabled.push(manifest);
                continue;
            }
            let keyword = config.keyword.or_else(|| manifest.keyword.clone());
            if let Some(keyword) = &keyword {
                if keywords.contains(keyword) {
                    notices.push(format!(
                        "Plugin {} wants the keyword `{keyword}`, which is taken; set another under [plugins.{}] in settings.toml",
                        manifest.name, manifest.id
                    ));
                    disabled.push(manifest);
                    continue;
                }
                keywords.push(keyword.clone());
            }
            plugins.push(Plugin {
                external: Arc::new(External::new(manifest, host::prepare_plugin, ANSWER_WITHIN)),
                keyword,
            });
        }
        Session {
            calculator: calculator.enabled.then_some(calculator.keyword),
            plugins,
            disabled,
            catalog: OnceCell::new(),
        }
    }
}

impl Results {
    /// Starts a new search. Rows from the search before stay usable, so pressing
    /// Enter the moment a new keystroke lands still opens what was on screen.
    fn begin(&mut self) -> u64 {
        self.generation += 1;
        let keep = self.generation - 1;
        self.rows.retain(|_, entry| entry.generation >= keep);
        self.generation
    }

    fn add(&mut self, generation: u64, draft: Draft) -> Option<Row> {
        if generation < self.generation.saturating_sub(1) {
            return None;
        }
        self.next += 1;
        let id = self.next.to_string();
        let row = Row {
            id: id.clone(),
            title: draft.title,
            subtitle: draft.subtitle,
            meta: draft.meta,
            icon: draft.icon.to_owned(),
            image: draft.image,
            action: draft.action.label(),
            alt: draft.alt.as_ref().map(Command::label),
        };
        self.rows.insert(
            id,
            Entry {
                generation,
                action: draft.action,
                alt: draft.alt,
            },
        );
        Some(row)
    }
}

impl Command {
    fn label(&self) -> &'static str {
        match self {
            Command::Plugin { action, .. } => match action {
                Action::Open(_) => "Open",
                Action::Reveal(_) => "Show in folder",
                Action::Copy(_) => "Copy",
                Action::Run(_) => "Run",
                Action::Fill(_) => "Use",
            },
            Command::Install { update: false, .. } => "Install",
            Command::Install { update: true, .. } => "Update",
            Command::Uninstall { .. } => "Remove",
            Command::Add(_) => "Add",
        }
    }
}

fn suggestions(session: &Session, query: &str) -> Vec<Draft> {
    let fill = |keyword: &str| Command::Plugin {
        action: Action::Fill(format!("{keyword} ")),
        dir: None,
    };
    let mut drafts = Vec::new();
    if query == PLUGINS_KEYWORD {
        drafts.push(Draft {
            title: "Plugins".into(),
            subtitle: Some("Install, update and remove plugins".into()),
            meta: None,
            icon: "plugin",
            image: None,
            action: fill(PLUGINS_KEYWORD),
            alt: None,
        });
    }
    if let Some(Some(keyword)) = &session.calculator
        && query == keyword
    {
        drafts.push(Draft {
            title: "Calculator".into(),
            subtitle: Some("Arithmetic and unit conversions".into()),
            meta: None,
            icon: "calculator",
            image: None,
            action: fill(keyword),
            alt: None,
        });
    }
    for plugin in &session.plugins {
        let manifest = &plugin.external.manifest;
        if plugin.keyword.as_deref() == Some(query) {
            drafts.push(Draft {
                title: manifest.name.clone(),
                subtitle: manifest.description.clone(),
                meta: None,
                icon: "plugin",
                image: manifest.icon.clone(),
                action: fill(query),
                alt: None,
            });
        }
    }
    drafts
}

fn calculator_draft(item: Item) -> Draft {
    Draft {
        title: item.title,
        subtitle: None,
        meta: None,
        icon: "calculator",
        image: None,
        action: Command::Plugin {
            action: item.action,
            dir: None,
        },
        alt: None,
    }
}

fn plugin_draft(item: Item, manifest: &Manifest) -> Draft {
    let command = |action| Command::Plugin {
        action,
        dir: Some(manifest.dir.clone()),
    };
    Draft {
        title: item.title,
        subtitle: item.subtitle,
        meta: None,
        icon: "plugin",
        image: manifest.icon.clone(),
        action: command(item.action),
        alt: item.alt.map(command),
    }
}

fn installed_draft(manifest: &Manifest, keyword: Option<&str>, enabled: bool) -> Draft {
    let source = store::source_of(&manifest.dir);
    let meta = match (keyword, enabled) {
        (_, false) => Some("Off".to_owned()),
        (Some(keyword), true) => Some(format!("Keyword: {keyword}")),
        (None, true) => None,
    };
    let reveal = Command::Plugin {
        action: Action::Reveal(manifest.dir.to_string_lossy().into_owned()),
        dir: None,
    };
    let (action, alt) = match source {
        Some(source) => (
            Command::Install {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                source,
                update: true,
            },
            Some(Command::Uninstall {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
            }),
        ),
        None => (reveal, None),
    };
    Draft {
        title: manifest.name.clone(),
        subtitle: manifest.description.clone(),
        meta,
        icon: "plugin",
        image: manifest.icon.clone(),
        action,
        alt,
    }
}

fn file_draft(hit: Hit, home: &Path, now: i64) -> Draft {
    let folder = Path::new(&hit.path)
        .parent()
        .map(|parent| match parent.strip_prefix(home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
            Err(_) => parent.display().to_string(),
        })
        .unwrap_or_default();
    let meta = match hit.size {
        Some(bytes) => format!("{} · {}", age(now, hit.mtime), size(bytes)),
        None => age(now, hit.mtime),
    };
    let open = |action| Command::Plugin { action, dir: None };
    Draft {
        title: hit.name,
        subtitle: Some(folder),
        meta: Some(meta),
        icon: hit.kind.as_str(),
        image: None,
        action: open(Action::Open(hit.path.clone())),
        alt: Some(open(Action::Reveal(hit.path))),
    }
}

fn section(key: &str, title: &str, rank: u32, rows: Vec<Row>, message: Option<String>) -> Section {
    Section {
        key: key.to_owned(),
        title: title.to_owned(),
        rank,
        rows,
        message,
        warning: false,
        pending: false,
    }
}

fn failed(key: &str, title: &str, rank: u32, error: String) -> Section {
    Section {
        warning: true,
        ..section(key, title, rank, Vec::new(), Some(error))
    }
}

fn pending(key: &str, title: &str, rank: u32) -> Section {
    Section {
        pending: true,
        ..section(key, title, rank, Vec::new(), None)
    }
}

fn close(app: &AppHandle) -> Outcome {
    window::hide(app);
    Outcome::Close
}

fn run(argv: &[String], dir: &Path) -> Result<(), String> {
    let program = Path::new(&argv[0]);
    let program = if program.components().count() > 1 {
        dir.join(program)
    } else {
        program.to_owned()
    };
    let mut command = std::process::Command::new(&program);
    command.args(&argv[1..]).current_dir(dir);
    host::clean(&mut command);
    let mut child = command
        .spawn()
        .map_err(|err| format!("couldn't run {}: {err}", argv[0]))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| err.to_string())?
}

/// How long ago a unix time was, like `3d ago`.
fn age(now: i64, then: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let seconds = (now - then).max(0);
    match seconds {
        s if s < MINUTE => "just now".to_owned(),
        s if s < HOUR => format!("{}m ago", s / MINUTE),
        s if s < DAY => format!("{}h ago", s / HOUR),
        s if s < 30 * DAY => format!("{}d ago", s / DAY),
        s if s < 365 * DAY => format!("{}mo ago", s / (30 * DAY)),
        s => format!("{}y ago", s / (365 * DAY)),
    }
}

/// A byte count for people, like `2.4 MB`.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[tauri::command]
pub async fn search(
    launcher: State<'_, Launcher>,
    query: String,
    on_results: Channel<Vec<Section>>,
) -> Result<(), String> {
    launcher.search(query.trim_start(), &on_results).await
}

#[tauri::command]
pub async fn activate(
    app: AppHandle,
    launcher: State<'_, Launcher>,
    id: String,
    alt: bool,
) -> Result<Outcome, String> {
    launcher.activate(&app, &id, alt).await
}

#[tauri::command]
pub fn view(launcher: State<'_, Launcher>) -> View {
    launcher.view()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_and_sizes() {
        assert_eq!(age(1000, 990), "just now");
        assert_eq!(age(100_000, 100_000 - 3 * 86_400), "3d ago");
        assert_eq!(size(419), "419 B");
        assert_eq!(size(481_587), "470.3 KB");
    }

    #[test]
    fn rows_from_two_searches_ago_are_forgotten() {
        let mut results = Results::default();
        let draft = || Draft {
            title: "x".into(),
            subtitle: None,
            meta: None,
            icon: "other",
            image: None,
            action: Command::Add(Repo::parse("a/b").unwrap()),
            alt: None,
        };
        let first = results.begin();
        let old = results.add(first, draft()).unwrap().id;
        let second = results.begin();
        let recent = results.add(second, draft()).unwrap().id;
        assert!(
            results.rows.contains_key(&old),
            "still on screen while typing"
        );
        results.begin();
        assert!(!results.rows.contains_key(&old));
        assert!(results.rows.contains_key(&recent));
        assert!(
            results.add(first, draft()).is_none(),
            "late answers to old searches are dropped"
        );
    }
}
