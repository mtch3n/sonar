//! Find a running program by name and end it: `kill chrome`.

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::{Action, Item, Setting};

/// The processes plugin's id in `settings.toml`, and its keyword.
pub const ID: &str = "processes";
pub const KEYWORD: &str = "kill";

/// The most processes shown for one query.
const LIMIT: usize = 20;

pub fn settings() -> Vec<Setting> {
    Vec::new()
}

/// A running program as the list shows it.
#[derive(Clone, Debug, PartialEq)]
struct Running {
    pid: u32,
    name: String,
    /// Resident memory, in bytes.
    memory: u64,
}

/// Runs the processes plugin, which `sonar-app --plugin processes` does. The list is
/// read again for every query, so it's never stale.
pub fn serve() {
    let mut system = System::new();
    let me = std::process::id();
    crate::serve(|query, _| {
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );
        let running: Vec<Running> = system
            .processes()
            .values()
            .filter(|p| p.pid().as_u32() != me && p.thread_kind().is_none())
            .map(|p| Running {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                memory: p.memory(),
            })
            .collect();
        Ok(answer(query, running))
    });
}

/// Programs whose name contains every word of `query`, most memory first.
fn answer(query: &str, mut running: Vec<Running>) -> Vec<Item> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    running.retain(|p| {
        let name = p.name.to_lowercase();
        words.iter().all(|w| name.contains(w.as_str()))
    });
    running.sort_by(|a, b| b.memory.cmp(&a.memory).then(a.name.cmp(&b.name)));
    running
        .into_iter()
        .take(LIMIT)
        .map(|p| {
            let mut item = Item::new(p.name.clone(), Action::Run(stop(p.pid, false)));
            item.subtitle = Some(format!("{} · process {}", memory(p.memory), p.pid));
            item.icon = Some("process".into());
            item.alt = Some(Action::Run(stop(p.pid, true)));
            item.label = Some("End".into());
            item.alt_label = Some("Force quit".into());
            item
        })
        .collect()
}

/// The command that asks a process to end, or with `force` makes it.
pub(crate) fn stop(pid: u32, force: bool) -> Vec<String> {
    let pid = pid.to_string();
    let words: Vec<&str> = if cfg!(windows) {
        let mut words = vec!["taskkill.exe", "/PID", &pid];
        if force {
            words.push("/F");
        }
        words
    } else if force {
        vec!["kill", "-KILL", &pid]
    } else {
        vec!["kill", &pid]
    };
    words.into_iter().map(str::to_owned).collect()
}

fn memory(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> Vec<Running> {
        let p = |pid, name: &str, mb: u64| Running {
            pid,
            name: name.into(),
            memory: mb * 1024 * 1024,
        };
        vec![
            p(10, "chrome", 300),
            p(11, "chrome", 1500),
            p(12, "chromedriver", 20),
            p(13, "zsh", 5),
        ]
    }

    #[test]
    fn lists_matching_programs_by_memory() {
        let items = answer("chrom", running());
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["chrome", "chrome", "chromedriver"]);
        assert_eq!(items[0].subtitle.as_deref(), Some("1.5 GB · process 11"));
        assert_eq!(items[1].subtitle.as_deref(), Some("300 MB · process 10"));
        assert_eq!(
            answer("CHROME ZSH", running()).len(),
            0,
            "every word must be in the name"
        );
        assert_eq!(answer("chrome driv", running()).len(), 1);
    }

    #[test]
    fn ends_or_forces_the_process() {
        let item = &answer("zsh", running())[0];
        assert_eq!(item.label.as_deref(), Some("End"));
        assert_eq!(item.alt_label.as_deref(), Some("Force quit"));
        if cfg!(windows) {
            assert_eq!(
                item.action,
                Action::Run(vec!["taskkill.exe".into(), "/PID".into(), "13".into()])
            );
        } else {
            assert_eq!(item.action, Action::Run(vec!["kill".into(), "13".into()]));
            assert_eq!(
                item.alt,
                Some(Action::Run(vec![
                    "kill".into(),
                    "-KILL".into(),
                    "13".into()
                ]))
            );
        }
    }

    #[test]
    fn sees_this_computers_programs() {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        assert!(!system.processes().is_empty());
    }
}
