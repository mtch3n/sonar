//! Lock, sleep, restart, shut down, log out and empty the trash from the search bar.

use crate::{Action, Item, Setting};

/// The system plugin's id in `settings.toml`.
pub const ID: &str = "system";

/// Queries shorter than this match too many commands to be worth answering.
const MIN_QUERY: usize = 2;
/// Typed after a command that can't be undone, like `restart now`, to do it.
const CONFIRM: &str = "now";

pub fn settings() -> Vec<Setting> {
    Vec::new()
}

/// Runs the system commands as a plugin, which `sonar-app --plugin system` does.
pub fn serve() {
    crate::serve(|query, _| Ok(answer(query, &commands())));
}

struct Command {
    title: &'static str,
    /// Other words people type for it.
    words: &'static [&'static str],
    icon: &'static str,
    /// Logs out or turns the computer off, so it asks for `now` first.
    drastic: bool,
    run: Vec<String>,
}

fn answer(query: &str, commands: &[Command]) -> Vec<Item> {
    let query = query.trim().to_lowercase();
    let (asked, confirmed) = match query.strip_suffix(CONFIRM) {
        Some(rest) if rest.ends_with(' ') => (rest.trim().to_owned(), true),
        _ => (query.clone(), false),
    };
    if asked.chars().count() < MIN_QUERY {
        return Vec::new();
    }
    commands
        .iter()
        .filter(|command| {
            let title = command.title.to_lowercase();
            std::iter::once(title.as_str())
                .chain(command.words.iter().copied())
                .any(|word| word.starts_with(&asked))
        })
        .map(|command| {
            let name = command.title.to_lowercase();
            if command.drastic && !(confirmed && name.starts_with(&asked)) {
                let mut item = Item::new(command.title, Action::Fill(format!("{name} {CONFIRM}")));
                item.subtitle = Some(format!("Press Enter, then Enter again to {name}"));
                item.icon = Some(command.icon.into());
                item.label = Some("Confirm".into());
                return item;
            }
            let mut item = Item::new(command.title, Action::Run(command.run.clone()));
            item.subtitle = command.drastic.then(|| "Press Enter to do it now".into());
            item.icon = Some(command.icon.into());
            item.label = Some(command.title.into());
            item
        })
        .collect()
}

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

#[cfg(target_os = "linux")]
fn commands() -> Vec<Command> {
    // GNOME asks its own session to log out; elsewhere logind ends the session.
    let on_path = |program: &str| {
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
    };
    let log_out = if on_path("gnome-session-quit") {
        argv(&["gnome-session-quit", "--logout", "--no-prompt"])
    } else {
        let session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
        argv(&["loginctl", "terminate-session", &session])
    };
    vec![
        Command {
            title: "Lock",
            words: &["lock screen"],
            icon: "lock",
            drastic: false,
            run: argv(&["loginctl", "lock-session"]),
        },
        Command {
            title: "Sleep",
            words: &["suspend"],
            icon: "sleep",
            drastic: false,
            run: argv(&["systemctl", "suspend"]),
        },
        Command {
            title: "Restart",
            words: &["reboot"],
            icon: "restart",
            drastic: true,
            run: argv(&["systemctl", "reboot"]),
        },
        Command {
            title: "Shut down",
            words: &["shutdown", "power off", "turn off"],
            icon: "power",
            drastic: true,
            run: argv(&["systemctl", "poweroff"]),
        },
        Command {
            title: "Log out",
            words: &["logout", "sign out"],
            icon: "logout",
            drastic: true,
            run: log_out,
        },
        Command {
            title: "Empty trash",
            words: &["trash"],
            icon: "trash",
            drastic: false,
            run: argv(&["gio", "trash", "--empty"]),
        },
    ]
}

#[cfg(target_os = "macos")]
fn commands() -> Vec<Command> {
    let events = |what: &str| {
        argv(&[
            "osascript",
            "-e",
            &format!("tell application \"System Events\" to {what}"),
        ])
    };
    vec![
        Command {
            title: "Lock",
            words: &["lock screen"],
            icon: "lock",
            drastic: false,
            run: argv(&["pmset", "displaysleepnow"]),
        },
        Command {
            title: "Sleep",
            words: &["suspend"],
            icon: "sleep",
            drastic: false,
            run: argv(&["pmset", "sleepnow"]),
        },
        Command {
            title: "Restart",
            words: &["reboot"],
            icon: "restart",
            drastic: true,
            run: events("restart"),
        },
        Command {
            title: "Shut down",
            words: &["shutdown", "power off", "turn off"],
            icon: "power",
            drastic: true,
            run: events("shut down"),
        },
        Command {
            title: "Log out",
            words: &["logout", "sign out"],
            icon: "logout",
            drastic: true,
            run: events("log out"),
        },
        Command {
            title: "Empty trash",
            words: &["trash"],
            icon: "trash",
            drastic: false,
            run: argv(&[
                "osascript",
                "-e",
                "tell application \"Finder\" to empty trash",
            ]),
        },
    ]
}

#[cfg(windows)]
fn commands() -> Vec<Command> {
    vec![
        Command {
            title: "Lock",
            words: &["lock screen"],
            icon: "lock",
            drastic: false,
            run: argv(&["rundll32.exe", "user32.dll,LockWorkStation"]),
        },
        Command {
            title: "Sleep",
            words: &["suspend"],
            icon: "sleep",
            drastic: false,
            run: argv(&["rundll32.exe", "powrprof.dll,SetSuspendState", "0,1,0"]),
        },
        Command {
            title: "Restart",
            words: &["reboot"],
            icon: "restart",
            drastic: true,
            run: argv(&["shutdown.exe", "/r", "/t", "0"]),
        },
        Command {
            title: "Shut down",
            words: &["shutdown", "power off", "turn off"],
            icon: "power",
            drastic: true,
            run: argv(&["shutdown.exe", "/s", "/t", "0"]),
        },
        Command {
            title: "Log out",
            words: &["logout", "sign out"],
            icon: "logout",
            drastic: true,
            run: argv(&["shutdown.exe", "/l"]),
        },
        Command {
            title: "Empty recycle bin",
            words: &["empty trash", "trash", "recycle bin"],
            icon: "trash",
            drastic: false,
            run: argv(&[
                "powershell.exe",
                "-NoProfile",
                "-Command",
                "Clear-RecycleBin -Force",
            ]),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Command> {
        vec![
            Command {
                title: "Lock",
                words: &["lock screen"],
                icon: "lock",
                drastic: false,
                run: argv(&["lock"]),
            },
            Command {
                title: "Restart",
                words: &["reboot"],
                icon: "restart",
                drastic: true,
                run: argv(&["reboot"]),
            },
            Command {
                title: "Shut down",
                words: &["shutdown"],
                icon: "power",
                drastic: true,
                run: argv(&["poweroff"]),
            },
        ]
    }

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn matches_the_start_of_a_command_or_another_word_for_it() {
        assert_eq!(titles(&answer("lo", &sample())), ["Lock"]);
        assert_eq!(titles(&answer("reb", &sample())), ["Restart"]);
        assert_eq!(titles(&answer("SHUT", &sample())), ["Shut down"]);
        assert!(answer("l", &sample()).is_empty(), "too short");
        assert!(answer("invoice", &sample()).is_empty());
    }

    #[test]
    fn harmless_commands_run_at_once() {
        let lock = &answer("lock", &sample())[0];
        assert_eq!(lock.action, Action::Run(argv(&["lock"])));
        assert_eq!(lock.icon.as_deref(), Some("lock"));
    }

    #[test]
    fn drastic_commands_ask_for_now_first() {
        let first = &answer("restart", &sample())[0];
        assert_eq!(first.action, Action::Fill("restart now".into()));
        assert_eq!(first.label.as_deref(), Some("Confirm"));
        let second = &answer("restart now", &sample())[0];
        assert_eq!(second.action, Action::Run(argv(&["reboot"])));
        let other = &answer("reboot now", &sample())[0];
        assert_eq!(
            other.action,
            Action::Fill("restart now".into()),
            "confirm with the command's own name"
        );
    }

    #[test]
    fn every_platform_has_every_command() {
        let names: Vec<&str> = commands().iter().map(|c| c.title).collect();
        assert_eq!(names.len(), 6, "{names:?}");
        assert!(commands().iter().all(|c| !c.run.is_empty()));
    }
}
