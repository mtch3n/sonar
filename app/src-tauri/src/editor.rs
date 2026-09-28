//! The Settings window: a form over `settings.toml`.

use serde::Serialize;
use sonar_plugins::{Setting, calculator, currency::Rates};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::{
    host,
    launcher::Launcher,
    settings::{self, Settings},
};

const LABEL: &str = "settings";

/// Opens the Settings window, or brings it forward if it's open.
pub fn open(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let builder =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#settings".into()))
            .title("Sonar Settings")
            .inner_size(680.0, 760.0)
            .min_inner_size(560.0, 480.0)
            .center();
    // The window draws its own title bar to match the rest of Sonar. macOS keeps its
    // traffic lights over it; elsewhere the page has its own buttons, and on Linux
    // its own rounded corners.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    #[cfg(target_os = "linux")]
    let builder = builder.decorations(false).transparent(true);
    #[cfg(windows)]
    let builder = builder.decorations(false).shadow(true);
    match builder.build() {
        Ok(window) => crate::window::plain_scrolling(&window),
        Err(err) => eprintln!("sonar: couldn't open the settings window: {err}"),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Editor {
    settings: Settings,
    plugins: Vec<PluginInfo>,
    path: String,
    /// Editors and terminals found on this computer, offered for `files.editor` and
    /// `files.terminal`.
    editors: Vec<Tool>,
    terminals: Vec<Tool>,
    /// When the saved exchange rates were published, in Unix seconds.
    rates_published: Option<i64>,
    /// The desktop's accent, which `system` stands for.
    system_accent: Option<String>,
    /// Why the file can't be read, if it can't; the form then shows the last
    /// settings that worked.
    problem: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    id: String,
    name: String,
    description: Option<String>,
    /// The plugin's own keyword, used when the settings don't give another.
    keyword: Option<String>,
    image: Option<String>,
    icon: &'static str,
    /// What the Settings window draws a form for.
    settings: Vec<Setting>,
    /// Why it can't run here, like a program it needs that isn't installed.
    problem: Option<String>,
}

/// Every plugin, Sonar's own and installed, on or off.
fn plugins(launcher: &Launcher) -> Vec<PluginInfo> {
    launcher
        .installed()
        .into_iter()
        .map(|manifest| PluginInfo {
            problem: manifest.missing(),
            id: manifest.id,
            name: manifest.name,
            description: manifest.description,
            keyword: manifest.keyword,
            image: manifest.icon,
            icon: "plugin",
            settings: manifest.settings,
        })
        .collect()
}

#[tauri::command]
pub fn settings_get(launcher: State<'_, Launcher>) -> Editor {
    let path = &launcher.paths().settings;
    Editor {
        settings: launcher.current_settings(),
        plugins: plugins(&launcher),
        editors: tools(sonar_apps::editors()),
        terminals: tools(sonar_apps::terminals()),
        system_accent: host::system_accent(),
        rates_published: Rates::load(&rates_file(&launcher)).map(|r| r.published),
        path: path.display().to_string(),
        problem: Settings::load(path).err(),
    }
}

#[tauri::command]
pub fn settings_save(
    app: AppHandle,
    launcher: State<'_, Launcher>,
    settings: Settings,
) -> Result<(), String> {
    check_values(&plugins(&launcher), &settings)?;
    settings::save(&launcher.paths().settings, &settings)?;
    crate::reload(&app);
    let _ = app.emit("sonar://view", ());
    Ok(())
}

#[derive(Serialize)]
pub struct Tool {
    name: String,
    /// The command as `settings.toml` holds it.
    command: String,
}

fn tools(apps: Vec<sonar_apps::App>) -> Vec<Tool> {
    apps.into_iter()
        .map(|app| Tool {
            command: app.command_line(),
            name: app.name,
        })
        .collect()
}

/// Refuses values that don't fit what their plugin declares, naming the first one.
fn check_values(plugins: &[PluginInfo], settings: &Settings) -> Result<(), String> {
    for plugin in plugins {
        let values = settings.plugin(&plugin.id).values;
        let (_, problems) = sonar_plugins::resolve(&plugin.settings, &values);
        if let Some(problem) = problems.first() {
            return Err(format!("{}: {problem}", plugin.name));
        }
    }
    Ok(())
}

fn rates_file(launcher: &Launcher) -> std::path::PathBuf {
    launcher
        .paths()
        .plugin_data
        .join(calculator::ID)
        .join(calculator::RATES_FILE)
}

/// Downloads exchange rates now, for the calculator to use from the next search on,
/// and says when they were published.
#[tauri::command]
pub async fn rates_update(launcher: State<'_, Launcher>) -> Result<i64, String> {
    let path = rates_file(&launcher);
    tauri::async_runtime::spawn_blocking(move || {
        let rates = Rates::download()?;
        rates.save(&path)?;
        Ok(rates.published)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn settings_open_file(app: AppHandle, launcher: State<'_, Launcher>) -> Result<(), String> {
    host::open(&app, &launcher.paths().settings.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_refuses_values_the_plugin_doesnt_take() {
        let calculator = PluginInfo {
            id: sonar_plugins::calculator::ID.into(),
            name: "Calculator".into(),
            description: None,
            keyword: None,
            image: None,
            icon: "calculator",
            settings: sonar_plugins::calculator::settings(),
            problem: None,
        };
        let plugins = [calculator];
        let good = Settings::parse("[plugins.calculator]\ncurrency = \"JPY\"\n").unwrap();
        assert_eq!(check_values(&plugins, &good), Ok(()));
        let bad = Settings::parse("[plugins.calculator]\nrates = \"yes\"\n").unwrap();
        assert_eq!(
            check_values(&plugins, &bad),
            Err("Calculator: `rates` must be true or false".into())
        );
    }
}
