//! Plugins that ship inside Sonar. Sonar's own program runs them when started with
//! `--plugin <id>`; to the rest of Sonar they're plugins like any other, whose folders
//! are written out each time the plugin folders are read.

use std::{fs, path::Path};

use serde::Serialize;
use sonar_plugins::{Setting, apps, browser, calculator, processes, system, windows};

struct Bundled {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    keyword: Option<&'static str>,
    /// Where results go when the plugin has no keyword: `top` or `bottom`.
    position: &'static str,
    icon: &'static str,
    /// Built when written, so choices can come from this computer.
    settings: fn() -> Vec<Setting>,
    serve: fn(),
}

const BUNDLED: &[Bundled] = &[
    Bundled {
        id: apps::ID,
        name: "Apps",
        description: "Open installed apps by name",
        keyword: None,
        position: "top",
        icon: include_str!("../icons/plugins/apps.svg"),
        settings: apps::settings,
        serve: apps::serve,
    },
    Bundled {
        id: calculator::ID,
        name: "Calculator",
        description: "Arithmetic, units, currencies, time zones and dates",
        keyword: None,
        position: "top",
        icon: include_str!("../icons/plugins/calculator.svg"),
        settings: calculator::settings,
        serve: calculator::serve,
    },
    Bundled {
        id: browser::ID,
        name: "Browser",
        description: "Bookmarks and history from Chrome, Edge, Brave and other Chromium browsers",
        keyword: None,
        position: "top",
        icon: include_str!("../icons/plugins/browser.svg"),
        settings: browser::settings,
        serve: browser::serve,
    },
    Bundled {
        id: system::ID,
        name: "System",
        description: "Lock, sleep, restart, shut down, log out and empty the trash",
        keyword: None,
        position: "top",
        icon: include_str!("../icons/plugins/system.svg"),
        settings: system::settings,
        serve: system::serve,
    },
    Bundled {
        id: processes::ID,
        name: "Processes",
        description: "Find a running program and end it",
        keyword: Some(processes::KEYWORD),
        position: "bottom",
        icon: include_str!("../icons/plugins/processes.svg"),
        settings: processes::settings,
        serve: processes::serve,
    },
    Bundled {
        id: windows::ID,
        name: "Windows",
        description: "Switch to an open window",
        keyword: Some(windows::KEYWORD),
        position: "bottom",
        icon: include_str!("../icons/plugins/windows.svg"),
        settings: windows::settings,
        serve: windows::serve,
    },
];

/// Runs the bundled plugin `id`, if there is one by that name.
pub fn serve(id: &str) -> bool {
    let Some(plugin) = BUNDLED.iter().find(|p| p.id == id) else {
        return false;
    };
    (plugin.serve)();
    true
}

#[derive(Serialize)]
struct PluginFile<'a> {
    name: &'a str,
    description: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    keyword: Option<&'a str>,
    command: Vec<String>,
    position: &'a str,
    icon: &'a str,
    settings: Vec<Setting>,
}

/// Writes a folder for each bundled plugin into `dir`, started by `program`.
pub fn write(dir: &Path, program: &Path) -> Result<(), String> {
    for plugin in BUNDLED {
        let folder = dir.join(plugin.id);
        fs::create_dir_all(&folder).map_err(|err| err.to_string())?;
        let file = PluginFile {
            name: plugin.name,
            description: plugin.description,
            keyword: plugin.keyword,
            command: vec![
                program.to_string_lossy().into_owned(),
                "--plugin".into(),
                plugin.id.into(),
            ],
            position: plugin.position,
            icon: "icon.svg",
            settings: (plugin.settings)(),
        };
        let text = toml::to_string(&file).map_err(|err| err.to_string())?;
        write_if_changed(&folder.join(sonar_plugins::MANIFEST), &text)?;
        write_if_changed(&folder.join("icon.svg"), plugin.icon)?;
    }
    Ok(())
}

fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    fs::write(path, text).map_err(|err| format!("writing {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_plugins_read_back_as_plugins() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), Path::new("/opt/sonar/sonar-app")).unwrap();
        let (manifests, problems) = sonar_plugins::discover(tmp.path());
        assert!(problems.is_empty(), "{problems:?}");
        let ids: Vec<&str> = manifests.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "apps",
                "browser",
                "calculator",
                "processes",
                "system",
                "windows"
            ]
        );
        assert_eq!(manifests[3].keyword.as_deref(), Some("kill"));
        let calculator = &manifests[2];
        assert_eq!(calculator.id, "calculator");
        assert_eq!(
            calculator.command,
            ["/opt/sonar/sonar-app", "--plugin", "calculator"]
        );
        assert_eq!(calculator.position, sonar_plugins::Position::Top);
        assert!(
            calculator
                .icon
                .as_deref()
                .unwrap()
                .starts_with("data:image/svg+xml")
        );
        let keys: Vec<&str> = calculator.settings.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["currency", "rates"]);
    }
}
