//! Freedesktop `.desktop` entries: how Linux desktops list installed apps.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::{App, Launchable};

/// Apps whose entry's categories satisfy `wanted`, from the user's and the system's
/// application folders. An entry in the user's folder hides the system's one.
pub fn apps(wanted: impl Fn(&[String]) -> bool) -> Vec<App> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in application_dirs() {
        let Ok(files) = fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = files.flatten().map(|f| f.path()).collect();
        files.sort();
        for file in files {
            let Some(id) = file.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if !id.ends_with(".desktop") || !seen.insert(id) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            if let Some(entry) = Entry::parse(&text)
                && entry.usable()
                && wanted(&entry.categories)
                && let Some(command) = entry.command()
            {
                // The same app can be installed as a package and as a Flatpak.
                let flatpak = command.first().is_some_and(|p| p.ends_with("flatpak"));
                let name = if flatpak {
                    format!("{} (Flatpak)", entry.name)
                } else {
                    entry.name
                };
                apps.push(App { name, command });
            }
        }
    }
    apps
}

/// `$XDG_DATA_HOME/applications`, then each of `$XDG_DATA_DIRS`, where Flatpak adds
/// its exports.
fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    home.into_iter()
        .chain(system.split(':').map(PathBuf::from))
        .map(|dir| dir.join("applications"))
        .collect()
}

/// Every app shown in the desktop's menus, opened with `gio launch`, which starts
/// Flatpaks, D-Bus activated apps and terminal apps the way the desktop does.
pub fn launchable() -> Vec<Launchable> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in application_dirs() {
        let Ok(files) = fs::read_dir(&dir) else {
            continue;
        };
        for file in files.flatten().map(|f| f.path()) {
            let Some(id) = file.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if !id.ends_with(".desktop") || !seen.insert(id) {
                continue;
            }
            let Some(entry) = fs::read_to_string(&file)
                .ok()
                .and_then(|t| Entry::parse(&t))
            else {
                continue;
            };
            let shown =
                entry.is_app && !entry.hidden && entry.try_exec.as_deref().is_none_or(on_path);
            if !shown {
                continue;
            }
            let mut other_names = entry.keywords.clone();
            other_names.extend(entry.generic_name.clone());
            // The same app can be installed as a package and as a Flatpak.
            let flatpak = entry
                .command()
                .and_then(|c| c.first().cloned())
                .is_some_and(|p| p.ends_with("flatpak"));
            apps.push(Launchable {
                icon: entry.icon.as_deref().and_then(find_icon),
                name: if flatpak {
                    format!("{} (Flatpak)", entry.name)
                } else {
                    entry.name
                },
                other_names,
                open: vec![
                    "gio".into(),
                    "launch".into(),
                    file.to_string_lossy().into_owned(),
                ],
            });
        }
    }
    apps
}

/// The icon file of the app whose entry is `id`, like `org.gnome.Ptyxis.desktop`,
/// from the icon theme's usual folders.
pub fn icon(id: &str) -> Option<PathBuf> {
    let text = application_dirs()
        .iter()
        .find_map(|dir| fs::read_to_string(dir.join(id)).ok())?;
    find_icon(&Entry::parse(&text)?.icon?)
}

/// An `Icon=` value as a file: a path as it is, or a name looked up in the icon
/// theme's usual folders, largest first.
fn find_icon(name: &str) -> Option<PathBuf> {
    if Path::new(&name).is_absolute() {
        return Path::new(&name).is_file().then(|| PathBuf::from(name));
    }
    let data: Vec<PathBuf> = application_dirs()
        .iter()
        .filter_map(|apps| apps.parent().map(Path::to_path_buf))
        .collect();
    const SIZES: [&str; 7] = [
        "scalable", "512x512", "256x256", "128x128", "96x96", "64x64", "48x48",
    ];
    data.iter()
        .flat_map(|dir| SIZES.map(|size| dir.join("icons/hicolor").join(size).join("apps")))
        .chain(data.iter().map(|dir| dir.join("pixmaps")))
        .flat_map(|dir| ["svg", "png"].map(|ext| dir.join(format!("{name}.{ext}"))))
        .find(|path| path.is_file())
}

#[derive(Debug, Default, PartialEq)]
struct Entry {
    name: String,
    generic_name: Option<String>,
    keywords: Vec<String>,
    exec: String,
    icon: Option<String>,
    try_exec: Option<String>,
    categories: Vec<String>,
    is_app: bool,
    hidden: bool,
    in_terminal: bool,
}

impl Entry {
    /// The `[Desktop Entry]` group; the actions after it are extra menu items.
    fn parse(text: &str) -> Option<Entry> {
        let mut entry = Entry::default();
        let mut in_main = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_main = line == "[Desktop Entry]";
                continue;
            }
            let Some((key, value)) = line.split_once('=').filter(|_| in_main) else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "Name" => entry.name = value.to_owned(),
                "GenericName" => entry.generic_name = Some(value.to_owned()),
                "Keywords" => {
                    entry.keywords = value
                        .split(';')
                        .filter(|k| !k.is_empty())
                        .map(str::to_owned)
                        .collect()
                }
                "Exec" => entry.exec = value.to_owned(),
                "Icon" => entry.icon = Some(value.to_owned()),
                "TryExec" => entry.try_exec = Some(value.to_owned()),
                "Categories" => {
                    entry.categories = value
                        .split(';')
                        .filter(|c| !c.is_empty())
                        .map(str::to_owned)
                        .collect()
                }
                "Type" => entry.is_app = value == "Application",
                "NoDisplay" | "Hidden" if value == "true" => entry.hidden = true,
                "Terminal" => entry.in_terminal = value == "true",
                _ => {}
            }
        }
        (!entry.name.is_empty() && !entry.exec.is_empty()).then_some(entry)
    }

    /// Shown in menus, starts on its own, and installed. Apps that run inside a
    /// terminal, like Neovim, need one to be started from Sonar, so they're left out.
    fn usable(&self) -> bool {
        self.is_app
            && !self.hidden
            && !self.in_terminal
            && self.try_exec.as_deref().is_none_or(on_path)
    }

    /// `Exec` without its field codes like `%F`, split into the program and its
    /// arguments.
    fn command(&self) -> Option<Vec<String>> {
        let words = shell_words::split(&self.exec).ok()?;
        let command: Vec<String> = words
            .into_iter()
            .filter(|word| !(word.len() == 2 && word.starts_with('%')))
            .map(|word| word.replace("%%", "%"))
            .collect();
        (!command.is_empty()).then_some(command)
    }
}

fn on_path(program: &str) -> bool {
    if Path::new(program).is_absolute() {
        return Path::new(program).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests that point the data folders somewhere else take turns.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const CODE: &str = "[Desktop Entry]
Name=Visual Studio Code Insiders
Exec=/usr/bin/code-insiders %F
Type=Application
Categories=TextEditor;Development;IDE;

[Desktop Action new-empty-window]
Name=New Empty Window
Exec=/usr/bin/code-insiders --new-window %F
";

    #[test]
    fn reads_the_main_entry_only() {
        let entry = Entry::parse(CODE).unwrap();
        assert_eq!(entry.name, "Visual Studio Code Insiders");
        assert_eq!(entry.command().unwrap(), ["/usr/bin/code-insiders"]);
        assert_eq!(entry.categories, ["TextEditor", "Development", "IDE"]);
        assert!(entry.is_app && !entry.hidden);
    }

    #[test]
    fn keeps_arguments_and_quotes() {
        let entry = Entry::parse(
            "[Desktop Entry]\nName=Zed\nType=Application\nExec=\"/opt/Zed App/zed\" --new %U 100%%\n",
        )
        .unwrap();
        assert_eq!(
            entry.command().unwrap(),
            ["/opt/Zed App/zed", "--new", "100%"]
        );
    }

    #[test]
    fn leaves_out_hidden_terminal_and_missing_apps() {
        let usable = |text: &str| Entry::parse(text).unwrap().usable();
        let base = "[Desktop Entry]\nName=X\nType=Application\nExec=x\n";
        assert!(usable(base));
        assert!(!usable(&format!("{base}NoDisplay=true\n")));
        assert!(!usable(&format!("{base}Terminal=true\n")));
        assert!(!usable(&format!("{base}TryExec=sonar-no-such-program\n")));
        assert!(!usable("[Desktop Entry]\nName=X\nType=Link\nExec=x\n"));
        assert!(
            Entry::parse("[Desktop Entry]\nName=X\n").is_none(),
            "no Exec"
        );
    }

    #[test]
    fn finds_an_apps_icon_in_the_theme_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("share");
        fs::create_dir_all(data.join("applications")).unwrap();
        fs::create_dir_all(data.join("icons/hicolor/scalable/apps")).unwrap();
        fs::write(
            data.join("applications/org.example.Editor.desktop"),
            "[Desktop Entry]\nName=Editor\nType=Application\nExec=editor\nIcon=org.example.Editor\n",
        )
        .unwrap();
        fs::write(
            data.join("icons/hicolor/scalable/apps/org.example.Editor.svg"),
            "<svg/>",
        )
        .unwrap();
        // SAFETY: only these tests in this crate read or set these variables, one at a time.
        let _env = ENV.lock().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data);
            std::env::set_var("XDG_DATA_DIRS", tmp.path().join("none"));
        }
        assert_eq!(
            icon("org.example.Editor.desktop"),
            Some(data.join("icons/hicolor/scalable/apps/org.example.Editor.svg"))
        );
        assert_eq!(icon("missing.desktop"), None);
    }

    #[test]
    fn launches_every_shown_app_through_gio() {
        let tmp = tempfile::tempdir().unwrap();
        let apps = tmp.path().join("share/applications");
        fs::create_dir_all(&apps).unwrap();
        fs::write(
            apps.join("firefox.desktop"),
            "[Desktop Entry]\nName=Firefox\nGenericName=Web Browser\nKeywords=internet;www;\nType=Application\nExec=firefox %u\n",
        )
        .unwrap();
        fs::write(
            apps.join("hidden.desktop"),
            "[Desktop Entry]\nName=Hidden\nType=Application\nExec=x\nNoDisplay=true\n",
        )
        .unwrap();
        let _env = ENV.lock().unwrap();
        // SAFETY: only these tests in this crate read or set these variables, one at a time.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().join("share"));
            std::env::set_var("XDG_DATA_DIRS", tmp.path().join("none"));
        }
        let found = launchable();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Firefox");
        assert_eq!(found[0].other_names, ["internet", "www", "Web Browser"]);
        assert_eq!(
            found[0].open,
            [
                "gio",
                "launch",
                &apps.join("firefox.desktop").to_string_lossy()
            ]
        );
    }

    #[test]
    fn user_entries_hide_system_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("user");
        let system = tmp.path().join("system");
        for (dir, name) in [(&user, "Mine"), (&system, "System's")] {
            fs::create_dir_all(dir.join("applications")).unwrap();
            fs::write(
                dir.join("applications/editor.desktop"),
                format!("[Desktop Entry]\nName={name}\nType=Application\nExec=sh\nCategories=TextEditor;\n"),
            )
            .unwrap();
        }
        // SAFETY: only these tests in this crate read or set these variables, one at a time.
        let _env = ENV.lock().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &user);
            std::env::set_var("XDG_DATA_DIRS", &system);
        }
        let names: Vec<String> = apps(|c| c.iter().any(|c| c == "TextEditor"))
            .into_iter()
            .map(|app| app.name)
            .collect();
        assert_eq!(names, ["Mine"]);
    }
}
