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
    let built = builder.build();
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
    /// Editors found on this computer, suggested for `files.editor`.
    editors: Vec<String>,
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

/// The calculator, the browser search and every installed plugin, on or off.
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
    plugins.push(PluginInfo {
        id: sonar_plugins::browser::ID.into(),
        name: "Browser".into(),
        description: Some(
            "Bookmarks and history from Chrome, Edge, Brave and other Chromium browsers".into(),
        ),
        keyword: None,
        image: None,
        icon: "bookmark",
        settings: sonar_plugins::browser::settings(),
    });
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
        editors: installed_editors(),
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

/// Editor commands, most used first, that open a file or folder given after them.
const EDITORS: [&str; 16] = [
    "code",
    "cursor",
    "zed",
    "zeditor",
    "codium",
    "windsurf",
    "subl",
    "idea",
    "pycharm",
    "webstorm",
    "rustrover",
    "kate",
    "gnome-text-editor",
    "gedit",
    "emacs",
    "notepad++",
];

/// Editors on `PATH`, and on macOS the editor apps in /Applications.
fn installed_editors() -> Vec<String> {
    let dirs: Vec<_> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let found = |name: &str| {
        let exts: &[&str] = if cfg!(windows) {
            &["exe", "cmd", "bat"]
        } else {
            &[""]
        };
        dirs.iter().any(|dir| {
            exts.iter()
                .any(|ext| dir.join(name).with_extension(ext).is_file())
        })
    };
    let mut editors: Vec<String> = EDITORS
        .iter()
        .filter(|name| found(name))
        .map(|name| name.to_string())
        .collect();
    if cfg!(target_os = "macos") {
        for app in [
            "Visual Studio Code",
            "Cursor",
            "Zed",
            "Sublime Text",
            "TextEdit",
        ] {
            if std::path::Path::new(&format!("/Applications/{app}.app")).exists() {
                editors.push(format!("open -a '{app}'"));
            }
        }
    }
    editors
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
