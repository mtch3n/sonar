//! Manage systemd services: `svc docker` lists them, and a verb first, like
//! `svc restart docker` or `svc logs nginx`, does that. systemd asks polkit before
//! changing a system service, so GNOME shows its password dialog when it's needed;
//! the user's own services need none.

use std::collections::BTreeMap;

use crate::{Action, Item, Setting};

/// The services plugin's id in `settings.toml`, and its keyword.
pub const ID: &str = "services";
pub const KEYWORD: &str = "svc";

/// The most services shown for one query.
const LIMIT: usize = 50;

pub fn settings() -> Vec<Setting> {
    Vec::new()
}

/// A service, as systemd describes it.
#[derive(Clone, Debug, PartialEq)]
struct Service {
    /// The unit's name, like `docker.service`.
    unit: String,
    description: String,
    /// Running, or starting, reloading or stopping.
    active: bool,
    /// The unit file's state, like `enabled`, `disabled` or `static`.
    file: String,
    /// One of the user's own services, rather than the system's.
    user: bool,
}

/// What a query asks to do to a service.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Verb {
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
    Logs,
}

const VERBS: [(&str, Verb); 6] = [
    ("start", Verb::Start),
    ("stop", Verb::Stop),
    ("restart", Verb::Restart),
    ("enable", Verb::Enable),
    ("disable", Verb::Disable),
    ("logs", Verb::Logs),
];

impl Verb {
    fn word(self) -> &'static str {
        VERBS
            .iter()
            .find(|(_, v)| *v == self)
            .map_or("", |(w, _)| w)
    }

    fn label(self) -> &'static str {
        match self {
            Verb::Start => "Start",
            Verb::Stop => "Stop",
            Verb::Restart => "Restart",
            Verb::Enable => "Enable",
            Verb::Disable => "Disable",
            Verb::Logs => "Show logs",
        }
    }

    /// Whether it makes sense for `service` as it is now.
    fn fits(self, service: &Service) -> bool {
        match self {
            Verb::Start => !service.active,
            Verb::Stop | Verb::Restart => service.active,
            Verb::Enable => service.file == "disabled",
            Verb::Disable => service.file == "enabled",
            Verb::Logs => true,
        }
    }

    /// What it runs for `service`: `systemctl`, or `journalctl` in a terminal.
    fn action(self, service: &Service) -> Action {
        let scope = service.user.then_some("--user");
        if self == Verb::Logs {
            let unit = if service.user {
                "--user-unit"
            } else {
                "--unit"
            };
            return Action::Terminal(
                ["journalctl", "--follow", "--lines=200", unit, &service.unit]
                    .map(str::to_owned)
                    .to_vec(),
            );
        }
        Action::Run(
            ["systemctl"]
                .into_iter()
                .chain(scope)
                .chain([self.word(), service.unit.as_str()])
                .map(str::to_owned)
                .collect(),
        )
    }
}

/// Runs the services plugin, which `sonar-app --plugin services` does. The services
/// are listed again for every query, so their states are current.
pub fn serve() {
    crate::serve(|query, _| Ok(answer(query, list()?)));
}

/// The services matching `query`, running ones first. With a verb first, only the
/// ones it fits, and Enter does it; otherwise Enter starts or stops one and
/// Ctrl+Enter shows its logs.
fn answer(query: &str, mut services: Vec<Service>) -> Vec<Item> {
    let mut words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let verb = words
        .first()
        .and_then(|first| VERBS.iter().find(|(w, _)| w == first))
        .map(|(_, verb)| *verb);
    if verb.is_some() {
        words.remove(0);
    }
    services.retain(|s| {
        let text = format!("{} {}", s.unit, s.description).to_lowercase();
        words.iter().all(|w| text.contains(w.as_str())) && verb.is_none_or(|v| v.fits(s))
    });
    services.sort_by(|a, b| b.active.cmp(&a.active).then(a.unit.cmp(&b.unit)));
    services
        .into_iter()
        .take(LIMIT)
        .map(|s| {
            let (enter, alt) = match verb {
                Some(verb) => (verb, None),
                None if s.active => (Verb::Stop, Some(Verb::Logs)),
                None => (Verb::Start, Some(Verb::Logs)),
            };
            let name = s.unit.trim_end_matches(".service");
            let mut item = Item::new(name, enter.action(&s));
            item.subtitle = Some(subtitle(&s));
            item.icon = Some("service".into());
            item.label = Some(enter.label().into());
            if let Some(alt) = alt {
                item.alt = Some(alt.action(&s));
                item.alt_label = Some(alt.label().into());
            }
            item
        })
        .collect()
}

fn subtitle(service: &Service) -> String {
    let mut parts = vec![if service.active { "Running" } else { "Stopped" }];
    if !service.file.is_empty() {
        parts.push(&service.file);
    }
    if service.user {
        parts.push("yours");
    }
    if !service.description.is_empty() {
        parts.push(&service.description);
    }
    parts.join(" · ")
}

/// The services on the system bus and the user's own, both the loaded ones and
/// those only installed.
#[cfg(target_os = "linux")]
fn list() -> Result<Vec<Service>, String> {
    let system = zbus::blocking::Connection::system()
        .map_err(|err| format!("couldn't reach systemd: {err}"))?;
    let mut services = on(&system, false)?;
    // A desktop session has a user manager; a bare one may not.
    if let Ok(session) = zbus::blocking::Connection::session() {
        services.extend(on(&session, true).unwrap_or_default());
    }
    Ok(services)
}

#[cfg(not(target_os = "linux"))]
fn list() -> Result<Vec<Service>, String> {
    Err("Services are systemd's, on Linux".into())
}

#[cfg(target_os = "linux")]
fn on(bus: &zbus::blocking::Connection, user: bool) -> Result<Vec<Service>, String> {
    type Unit = (
        String,
        String,
        String,
        String,
        String,
        String,
        zbus::zvariant::OwnedObjectPath,
        u32,
        String,
        zbus::zvariant::OwnedObjectPath,
    );
    let call = |method: &str| {
        bus.call_method(
            Some("org.freedesktop.systemd1"),
            "/org/freedesktop/systemd1",
            Some("org.freedesktop.systemd1.Manager"),
            method,
            &(Vec::<&str>::new(), vec!["*.service"]),
        )
        .map_err(|err| format!("couldn't list services: {err}"))
    };
    let units: Vec<Unit> = call("ListUnitsByPatterns")?
        .body()
        .deserialize()
        .map_err(|err| err.to_string())?;
    let files: Vec<(String, String)> = call("ListUnitFilesByPatterns")?
        .body()
        .deserialize()
        .map_err(|err| err.to_string())?;
    let loaded = units
        .into_iter()
        .filter(|u| u.2 != "not-found")
        .map(|u| (u.0, (u.1, u.3)));
    let files = files.into_iter().filter_map(|(path, state)| {
        let unit = path.rsplit('/').next()?.to_owned();
        Some((unit, state))
    });
    Ok(merge(loaded, files, user))
}

/// Joins the loaded units, by name with their description and active state, with
/// the unit files, by name with their state. Templates like `getty@.service` can't
/// run by themselves, so they're left out.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn merge(
    loaded: impl Iterator<Item = (String, (String, String))>,
    files: impl Iterator<Item = (String, String)>,
    user: bool,
) -> Vec<Service> {
    let mut services: BTreeMap<String, Service> = BTreeMap::new();
    let blank = |unit: &str| Service {
        unit: unit.to_owned(),
        description: String::new(),
        active: false,
        file: String::new(),
        user,
    };
    for (unit, (description, active)) in loaded {
        let service = services.entry(unit.clone()).or_insert_with(|| blank(&unit));
        service.description = description;
        service.active = matches!(
            active.as_str(),
            "active" | "activating" | "reloading" | "deactivating"
        );
    }
    for (unit, state) in files {
        services
            .entry(unit.clone())
            .or_insert_with(|| blank(&unit))
            .file = state;
    }
    services.retain(|unit, _| !unit.ends_with("@.service"));
    services.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn services() -> Vec<Service> {
        let s = |unit: &str, active, file: &str, user| Service {
            unit: unit.into(),
            description: format!("The {unit}"),
            active,
            file: file.into(),
            user,
        };
        vec![
            s("sshd.service", false, "disabled", false),
            s("docker.service", true, "enabled", false),
            s("syncthing.service", true, "enabled", true),
        ]
    }

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn lists_running_services_first_and_toggles_them() {
        let items = answer("", services());
        assert_eq!(titles(&items), ["docker", "syncthing", "sshd"]);
        assert_eq!(
            items[0].subtitle.as_deref(),
            Some("Running · enabled · The docker.service")
        );
        assert_eq!(
            items[0].action,
            Action::Run(vec![
                "systemctl".into(),
                "stop".into(),
                "docker.service".into()
            ])
        );
        assert_eq!(items[0].label.as_deref(), Some("Stop"));
        assert_eq!(
            items[1].action,
            Action::Run(vec![
                "systemctl".into(),
                "--user".into(),
                "stop".into(),
                "syncthing.service".into()
            ])
        );
        assert_eq!(items[2].label.as_deref(), Some("Start"));
        assert_eq!(
            items[2].alt,
            Some(Action::Terminal(vec![
                "journalctl".into(),
                "--follow".into(),
                "--lines=200".into(),
                "--unit".into(),
                "sshd.service".into()
            ]))
        );
    }

    #[test]
    fn a_verb_offers_the_services_it_fits() {
        assert_eq!(titles(&answer("enable", services())), ["sshd"]);
        assert_eq!(titles(&answer("restart dock", services())), ["docker"]);
        assert!(answer("start dock", services()).is_empty());
        let logs = answer("logs sync", services());
        assert_eq!(logs[0].label.as_deref(), Some("Show logs"));
        assert_eq!(logs[0].alt, None);
        let Action::Terminal(argv) = &logs[0].action else {
            panic!("{:?}", logs[0].action)
        };
        assert!(argv.contains(&"--user-unit".to_owned()));
    }

    #[test]
    fn joins_loaded_units_with_their_files() {
        let loaded = [
            (
                "docker.service".to_owned(),
                ("Docker".to_owned(), "active".to_owned()),
            ),
            (
                "getty@tty1.service".to_owned(),
                ("Getty".to_owned(), "active".to_owned()),
            ),
        ];
        let files = [
            ("docker.service".to_owned(), "enabled".to_owned()),
            ("getty@.service".to_owned(), "enabled".to_owned()),
            ("sshd.service".to_owned(), "disabled".to_owned()),
        ];
        let merged = merge(loaded.into_iter(), files.into_iter(), false);
        let units: Vec<&str> = merged.iter().map(|s| s.unit.as_str()).collect();
        assert_eq!(
            units,
            ["docker.service", "getty@tty1.service", "sshd.service"]
        );
        assert!(merged[0].active && merged[0].file == "enabled");
        assert!(!merged[2].active && merged[2].description.is_empty());
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "needs systemd"]
    fn lists_this_computers_services() {
        let services = list().unwrap();
        assert!(services.iter().any(|s| s.active), "{services:?}");
    }
}
