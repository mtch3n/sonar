//! The apps installed on this computer, from the desktop's own lists: `.desktop`
//! entries on Linux, the Applications folders on macOS and the usual install folders
//! on Windows. Sonar offers editors and terminals from them in Settings.

#[cfg(any(target_os = "macos", windows))]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod desktop;

/// An installed app and the command that starts it. An editor gets the file or folder
/// after the command; a terminal is started in the folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub command: Vec<String>,
}

impl App {
    /// The command as one line, quoted where it needs to be, for `settings.toml`.
    pub fn command_line(&self) -> String {
        shell_words::join(&self.command)
    }
}

/// An app to open from the search bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launchable {
    pub name: String,
    /// What else people call it, like "Web Browser" for Firefox.
    pub other_names: Vec<String>,
    /// A PNG or SVG the search window can show.
    pub icon: Option<std::path::PathBuf>,
    /// The command that opens it.
    pub open: Vec<String>,
}

/// Every app the desktop lists, sorted by name.
pub fn launchable() -> Vec<Launchable> {
    let mut apps = platform::launchable();
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps.dedup_by(|a, b| a.name == b.name && a.open == b.open);
    apps
}

/// Apps that edit text and code, most people's pick first where that's known.
pub fn editors() -> Vec<App> {
    sorted(platform::editors())
}

/// Terminal apps.
pub fn terminals() -> Vec<App> {
    sorted(platform::terminals())
}

/// The icon file of the app whose desktop entry is `id`, like
/// `org.gnome.Ptyxis.desktop`.
#[cfg(target_os = "linux")]
pub fn icon(id: &str) -> Option<std::path::PathBuf> {
    desktop::icon(id)
}

/// The command that opens `terminal` in `dir`. Terminals take their starting folder
/// in different ways; the ones not listed start in the folder they're started from,
/// which the caller sets.
pub fn open_terminal(terminal: &[String], dir: &str) -> Vec<String> {
    let mut command = terminal.to_vec();
    let program = terminal
        .iter()
        .rev()
        .find_map(|word| word.strip_prefix("--command="))
        .or_else(|| terminal.first().map(String::as_str))
        .unwrap_or_default();
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let name = name.trim_end_matches(".exe");
    match name {
        // `open -a Terminal <folder>` on macOS.
        "open" => command.push(dir.to_owned()),
        "gnome-terminal" | "ptyxis" | "kgx" | "xfce4-terminal" | "mate-terminal" | "foot"
        | "ghostty" | "alacritty" => command.push(format!("--working-directory={dir}")),
        "konsole" => command.extend(["--workdir".to_owned(), dir.to_owned()]),
        "kitty" => command.push(format!("--directory={dir}")),
        "wezterm" => command.extend(["start".to_owned(), "--cwd".to_owned(), dir.to_owned()]),
        "tilix" => command.push(format!("--working-directory={dir}")),
        _ => {}
    }
    command
}

/// Sorted by name, each command once.
fn sorted(mut apps: Vec<App>) -> Vec<App> {
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps.dedup_by(|a, b| a.command == b.command);
    apps
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{App, Launchable, desktop};

    pub fn launchable() -> Vec<Launchable> {
        desktop::launchable()
    }

    pub fn editors() -> Vec<App> {
        desktop::apps(|categories| categories.iter().any(|c| c == "TextEditor" || c == "IDE"))
    }

    pub fn terminals() -> Vec<App> {
        desktop::apps(|categories| categories.iter().any(|c| c == "TerminalEmulator"))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::PathBuf;

    use super::{App, Launchable, installed_bundle};

    /// The `.app` bundles in the Applications folders, opened with `open -a`. Their
    /// icons are `.icns`, which the search window can't show, so none are given.
    pub fn launchable() -> Vec<Launchable> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let folders = [
            Some(PathBuf::from("/Applications")),
            Some(PathBuf::from("/Applications/Utilities")),
            Some(PathBuf::from("/System/Applications")),
            Some(PathBuf::from("/System/Applications/Utilities")),
            home.map(|h| h.join("Applications")),
        ];
        folders
            .into_iter()
            .flatten()
            .filter_map(|folder| std::fs::read_dir(folder).ok())
            .flat_map(|entries| entries.flatten().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
            .map(|path| Launchable {
                name: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                other_names: Vec::new(),
                icon: None,
                open: vec![
                    "open".into(),
                    "-a".into(),
                    path.to_string_lossy().into_owned(),
                ],
            })
            .collect()
    }

    const EDITORS: &[&str] = &[
        "Visual Studio Code",
        "Visual Studio Code - Insiders",
        "VSCodium",
        "Cursor",
        "Windsurf",
        "Zed",
        "Zed Preview",
        "Sublime Text",
        "Nova",
        "BBEdit",
        "CotEditor",
        "TextMate",
        "Xcode",
        "IntelliJ IDEA",
        "IntelliJ IDEA CE",
        "PyCharm",
        "PyCharm CE",
        "WebStorm",
        "RustRover",
        "GoLand",
        "CLion",
        "PhpStorm",
        "Rider",
        "RubyMine",
        "DataGrip",
        "Fleet",
        "Android Studio",
        "MacVim",
        "Emacs",
        "TextEdit",
    ];
    const TERMINALS: &[&str] = &[
        "Terminal",
        "iTerm",
        "Warp",
        "Ghostty",
        "kitty",
        "Alacritty",
        "WezTerm",
        "Hyper",
        "Tabby",
    ];

    fn found(names: &[&str]) -> Vec<App> {
        names
            .iter()
            .filter(|name| installed_bundle(name))
            .map(|name| App {
                name: name.to_string(),
                command: vec!["open".into(), "-a".into(), name.to_string()],
            })
            .collect()
    }

    pub fn editors() -> Vec<App> {
        found(EDITORS)
    }

    pub fn terminals() -> Vec<App> {
        found(TERMINALS)
    }
}

#[cfg(target_os = "macos")]
fn installed_bundle(name: &str) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let folders = [
        Some(PathBuf::from("/Applications")),
        Some(PathBuf::from("/System/Applications")),
        Some(PathBuf::from("/System/Applications/Utilities")),
        home.map(|h| h.join("Applications")),
    ];
    folders
        .into_iter()
        .flatten()
        .any(|folder| folder.join(format!("{name}.app")).exists())
}

#[cfg(windows)]
mod platform {
    use std::path::{Path, PathBuf};

    use super::{App, Launchable, first_existing};

    /// The shortcuts in the Start menu, opened the way the Start menu opens them.
    pub fn launchable() -> Vec<Launchable> {
        let roots = [std::env::var_os("APPDATA"), std::env::var_os("ProgramData")];
        let mut found = Vec::new();
        for root in roots.into_iter().flatten() {
            collect(
                &PathBuf::from(root).join(r"Microsoft\Windows\Start Menu\Programs"),
                &mut found,
            );
        }
        found
    }

    fn collect(dir: &Path, found: &mut Vec<Launchable>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                collect(&path, found);
                continue;
            }
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let lower = name.to_lowercase();
            let is_shortcut = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
            if is_shortcut && !lower.contains("uninstall") {
                found.push(Launchable {
                    name,
                    other_names: Vec::new(),
                    icon: None,
                    open: vec!["explorer.exe".into(), path.to_string_lossy().into_owned()],
                });
            }
        }
    }

    /// Where each editor installs, relative to the per-user and machine-wide
    /// program folders.
    const EDITORS: &[(&str, &[&str])] = &[
        (
            "Visual Studio Code",
            &[
                r"Programs\Microsoft VS Code\Code.exe",
                r"Microsoft VS Code\Code.exe",
            ],
        ),
        (
            "Visual Studio Code - Insiders",
            &[
                r"Programs\Microsoft VS Code Insiders\Code - Insiders.exe",
                r"Microsoft VS Code Insiders\Code - Insiders.exe",
            ],
        ),
        (
            "VSCodium",
            &[r"Programs\VSCodium\VSCodium.exe", r"VSCodium\VSCodium.exe"],
        ),
        ("Cursor", &[r"Programs\cursor\Cursor.exe"]),
        ("Windsurf", &[r"Programs\Windsurf\Windsurf.exe"]),
        ("Zed", &[r"Programs\Zed\Zed.exe", r"Zed\Zed.exe"]),
        (
            "Sublime Text",
            &[
                r"Sublime Text\sublime_text.exe",
                r"Sublime Text 3\sublime_text.exe",
            ],
        ),
        ("Notepad++", &[r"Notepad++\notepad++.exe"]),
    ];

    fn roots() -> Vec<PathBuf> {
        ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect()
    }

    pub fn editors() -> Vec<App> {
        let roots = roots();
        let mut found: Vec<App> = EDITORS
            .iter()
            .filter_map(|(name, paths)| {
                first_existing(&roots, paths).map(|exe| App {
                    name: name.to_string(),
                    command: vec![exe.to_string_lossy().into_owned()],
                })
            })
            .collect();
        found.push(App {
            name: "Notepad".into(),
            command: vec!["notepad.exe".into()],
        });
        found
    }

    pub fn terminals() -> Vec<App> {
        let mut found = Vec::new();
        let apps = std::env::var_os("LOCALAPPDATA")
            .map(|l| PathBuf::from(l).join(r"Microsoft\WindowsApps\wt.exe"));
        if apps.is_some_and(|wt| wt.exists()) {
            found.push(App {
                name: "Windows Terminal".into(),
                command: vec!["wt.exe".into(), "-d".into(), ".".into()],
            });
        }
        let pwsh = first_existing(&roots(), &[r"PowerShell\7\pwsh.exe"]);
        if let Some(pwsh) = pwsh {
            found.push(App {
                name: "PowerShell 7".into(),
                command: vec![pwsh.to_string_lossy().into_owned()],
            });
        }
        found.push(App {
            name: "Windows PowerShell".into(),
            command: vec!["powershell.exe".into()],
        });
        found.push(App {
            name: "Command Prompt".into(),
            command: vec!["cmd.exe".into()],
        });
        found
    }
}

#[cfg(windows)]
fn first_existing(roots: &[PathBuf], paths: &[&str]) -> Option<PathBuf> {
    roots
        .iter()
        .flat_map(|root| paths.iter().map(move |path| root.join(path)))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_quoted_for_settings() {
        let app = App {
            name: "Visual Studio Code".into(),
            command: vec!["open".into(), "-a".into(), "Visual Studio Code".into()],
        };
        assert_eq!(app.command_line(), "open -a 'Visual Studio Code'");
    }

    #[test]
    fn terminals_open_in_the_folder() {
        let open = |command: &[&str]| {
            let command: Vec<String> = command.iter().map(|w| w.to_string()).collect();
            open_terminal(&command, "/home/me/sonar")
        };
        assert_eq!(
            open(&["ptyxis"]),
            ["ptyxis", "--working-directory=/home/me/sonar"]
        );
        assert_eq!(
            open(&[
                "/usr/bin/flatpak",
                "run",
                "--command=ptyxis",
                "app.devsuite.Ptyxis"
            ]),
            [
                "/usr/bin/flatpak",
                "run",
                "--command=ptyxis",
                "app.devsuite.Ptyxis",
                "--working-directory=/home/me/sonar"
            ]
        );
        assert_eq!(
            open(&["konsole"]),
            ["konsole", "--workdir", "/home/me/sonar"]
        );
        assert_eq!(
            open(&["open", "-a", "iTerm"]),
            ["open", "-a", "iTerm", "/home/me/sonar"]
        );
        assert_eq!(
            open(&["wt.exe", "-d", "."]),
            ["wt.exe", "-d", "."],
            "starts in the folder"
        );
    }

    #[test]
    fn lists_are_sorted_and_without_repeats() {
        let app = |name: &str, command: &str| App {
            name: name.into(),
            command: vec![command.into()],
        };
        let apps = sorted(vec![
            app("zed", "zeditor"),
            app("Code", "code"),
            app("Code", "code"),
        ]);
        assert_eq!(apps, [app("Code", "code"), app("zed", "zeditor")]);
    }
}
