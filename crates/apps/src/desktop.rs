//! Freedesktop `.desktop` entries: how Linux desktops list installed apps.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::App;

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

#[derive(Debug, Default, PartialEq)]
struct Entry {
    name: String,
    exec: String,
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
                "Exec" => entry.exec = value.to_owned(),
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
        // SAFETY: only this test in this crate reads or sets these variables.
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
