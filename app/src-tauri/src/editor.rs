//! The Settings window: a form over `settings.toml`.

use serde::Serialize;
use sonar_plugins::Setting;
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
    let built =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#settings".into()))
            .title("Sonar Settings")
            .inner_size(680.0, 760.0)
            .min_inner_size(560.0, 480.0)
            .center()
            .build();
    if let Err(err) = built {
        eprintln!("sonar: couldn't open the settings window: {err}");
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Editor {
    settings: Settings,
    plugins: Vec<PluginInfo>,
    path: String,
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
}

/// The calculator and every installed plugin, on or off.
fn plugins(launcher: &Launcher) -> Vec<PluginInfo> {
    let mut plugins = vec![PluginInfo {
        id: sonar_plugins::calculator::ID.into(),
        name: "Calculator".into(),
        description: Some("Arithmetic, units and currencies, built in".into()),
        keyword: None,
        image: None,
        icon: "calculator",
        settings: sonar_plugins::calculator::settings(),
    }];
    plugins.extend(launcher.installed().into_iter().map(|manifest| PluginInfo {
        id: manifest.id,
        name: manifest.name,
        description: manifest.description,
        keyword: manifest.keyword,
        image: manifest.icon,
        icon: "plugin",
        settings: manifest.settings,
    }));
    plugins
}

#[tauri::command]
pub fn settings_get(launcher: State<'_, Launcher>) -> Editor {
    let path = &launcher.paths().settings;
    Editor {
        settings: launcher.current_settings(),
        plugins: plugins(&launcher),
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
    for plugin in plugins(&launcher) {
        let values = settings.plugin(&plugin.id).values;
        let (_, problems) = sonar_plugins::resolve(&plugin.settings, &values);
        if let Some(problem) = problems.first() {
            return Err(format!("{}: {problem}", plugin.name));
        }
    }
    settings::save(&launcher.paths().settings, &settings)?;
    crate::reload(&app);
    let _ = app.emit("sonar://view", ());
    Ok(())
}

#[tauri::command]
pub fn settings_open_file(app: AppHandle, launcher: State<'_, Launcher>) -> Result<(), String> {
    host::open(&app, &launcher.paths().settings.to_string_lossy())
}
