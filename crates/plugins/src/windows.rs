//! Switch to an open window: `w mail`. On GNOME, Sonar's own Shell extension lists
//! the windows, because GNOME on Wayland keeps other programs from seeing them.

use std::path::Path;

use serde::Deserialize;

use crate::{Action, Item, Setting};

/// The windows plugin's id in `settings.toml`, and its keyword.
pub const ID: &str = "windows";
pub const KEYWORD: &str = "w";
/// The GNOME Shell extension that lists windows for Sonar.
pub const EXTENSION: &str = "sonar-windows@mtch3n.github.io";
/// Where the extension answers on the session bus.
const DESTINATION: &str = "org.gnome.Shell";
const PATH: &str = "/io/github/mtch3n/Sonar/Windows";
const INTERFACE: &str = "io.github.mtch3n.Sonar.Windows";

pub fn settings() -> Vec<Setting> {
    Vec::new()
}

/// An open window, as the extension describes it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
struct Window {
    id: u64,
    title: String,
    app: String,
    /// The app's desktop entry, like `org.gnome.Ptyxis.desktop`.
    app_id: String,
    workspace: i32,
    minimized: bool,
}

/// Why the windows can't be listed.
#[derive(Debug, PartialEq)]
enum Unavailable {
    /// Not GNOME, whose extension is the only way Sonar knows so far.
    Desktop,
    /// GNOME, but the extension isn't installed or hasn't loaded yet.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Extension,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Failed(String),
}

/// Runs the windows plugin, which `sonar-app --plugin windows` does. The windows are
/// listed again for every query, in the order Alt+Tab shows them.
pub fn serve() {
    let program = std::env::current_exe().unwrap_or_default();
    crate::serve(|query, _| answer(query, list(), &program));
}

fn answer(
    query: &str,
    windows: Result<Vec<Window>, Unavailable>,
    program: &Path,
) -> Result<Vec<Item>, String> {
    let windows = match windows {
        Ok(windows) => windows,
        Err(Unavailable::Desktop) => {
            return Err("Switching windows works on GNOME for now".into());
        }
        Err(Unavailable::Extension) => {
            let install = vec![
                program.to_string_lossy().into_owned(),
                "--install-gnome-extension".into(),
            ];
            let mut item = Item::new(
                "Install the Sonar extension for GNOME",
                Action::Run(install),
            );
            item.subtitle = Some(
                "GNOME only shows windows to its own extensions. Log out and back in afterwards"
                    .into(),
            );
            item.icon = Some("window".into());
            item.label = Some("Install".into());
            return Ok(vec![item]);
        }
        Err(Unavailable::Failed(err)) => return Err(err),
    };
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let workspaces = windows
        .iter()
        .map(|w| w.workspace)
        .collect::<std::collections::HashSet<_>>()
        .len();
    Ok(windows
        .into_iter()
        .filter(|w| {
            let text = format!("{} {}", w.title, w.app).to_lowercase();
            words.iter().all(|word| text.contains(word.as_str()))
        })
        .map(|w| {
            let title = if w.title.trim().is_empty() {
                w.app.clone()
            } else {
                w.title.clone()
            };
            let mut subtitle = vec![w.app.clone()];
            if workspaces > 1 && w.workspace >= 0 {
                subtitle.push(format!("workspace {}", w.workspace + 1));
            }
            if w.minimized {
                subtitle.push("minimized".into());
            }
            let mut item = Item::new(title, Action::Run(call("Activate", w.id)));
            item.subtitle = Some(subtitle.join(" · "));
            item.image = icon(&w.app_id);
            // The glyph stands in if the picture can't be shown.
            item.icon = Some("window".into());
            item.alt = Some(Action::Run(call("Close", w.id)));
            item.label = Some("Switch".into());
            item.alt_label = Some("Close window".into());
            item
        })
        .collect())
}

/// The command that asks the extension to do `method` to a window.
fn call(method: &str, id: u64) -> Vec<String> {
    [
        "gdbus",
        "call",
        "--session",
        "--dest",
        DESTINATION,
        "--object-path",
        PATH,
        "--method",
    ]
    .iter()
    .map(|w| w.to_string())
    .chain([format!("{INTERFACE}.{method}"), id.to_string()])
    .collect()
}

#[cfg(target_os = "linux")]
fn icon(app_id: &str) -> Option<String> {
    sonar_apps::icon(app_id).map(|path| path.to_string_lossy().into_owned())
}

#[cfg(not(target_os = "linux"))]
fn icon(_: &str) -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn list() -> Result<Vec<Window>, Unavailable> {
    let gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("GNOME"));
    if !gnome {
        return Err(Unavailable::Desktop);
    }
    let failed =
        |err: zbus::Error| Unavailable::Failed(format!("couldn't reach GNOME Shell: {err}"));
    let bus = zbus::blocking::Connection::session().map_err(failed)?;
    let reply = bus.call_method(Some(DESTINATION), PATH, Some(INTERFACE), "List", &());
    let reply = match reply {
        Ok(reply) => reply,
        // GNOME Shell answers, but nothing is at the extension's path.
        Err(zbus::Error::MethodError(name, ..))
            if name.contains("UnknownObject")
                || name.contains("UnknownMethod")
                || name.contains("UnknownInterface") =>
        {
            return Err(Unavailable::Extension);
        }
        Err(err) => return Err(failed(err)),
    };
    let json: String = reply.body().deserialize().map_err(failed)?;
    serde_json::from_str(&json)
        .map_err(|err| Unavailable::Failed(format!("the extension sent {err}")))
}

#[cfg(not(target_os = "linux"))]
fn list() -> Result<Vec<Window>, Unavailable> {
    Err(Unavailable::Desktop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows() -> Vec<Window> {
        let w = |id, title: &str, app: &str, workspace| Window {
            id,
            title: title.into(),
            app: app.into(),
            app_id: format!("{app}.desktop"),
            workspace,
            minimized: id == 3,
        };
        vec![
            w(1, "Inbox - Gmail", "Google Chrome", 0),
            w(2, "~/sonar", "Ptyxis", 0),
            w(3, "", "Files", 1),
        ]
    }

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn lists_windows_in_alt_tab_order_and_filters_them() {
        let program = Path::new("/opt/sonar-app");
        let all = answer("", Ok(windows()), program).unwrap();
        assert_eq!(titles(&all), ["Inbox - Gmail", "~/sonar", "Files"]);
        assert_eq!(
            all[0].subtitle.as_deref(),
            Some("Google Chrome · workspace 1")
        );
        assert_eq!(
            all[2].subtitle.as_deref(),
            Some("Files · workspace 2 · minimized")
        );
        assert_eq!(
            titles(&answer("chrome gmail", Ok(windows()), program).unwrap()),
            ["Inbox - Gmail"]
        );
        assert_eq!(
            titles(&answer("PTY", Ok(windows()), program).unwrap()),
            ["~/sonar"]
        );
    }

    #[test]
    fn switches_to_or_closes_a_window_through_the_extension() {
        let item = &answer("ptyxis", Ok(windows()), Path::new("x")).unwrap()[0];
        let Action::Run(argv) = &item.action else {
            panic!("{:?}", item.action)
        };
        assert_eq!(argv.last().unwrap(), "2");
        assert!(argv.contains(&"io.github.mtch3n.Sonar.Windows.Activate".to_owned()));
        let Some(Action::Run(close)) = &item.alt else {
            panic!()
        };
        assert!(close.contains(&"io.github.mtch3n.Sonar.Windows.Close".to_owned()));
        assert_eq!(item.label.as_deref(), Some("Switch"));
    }

    #[test]
    fn offers_the_extension_or_explains_the_desktop() {
        let program = Path::new("/opt/sonar-app");
        let offer = answer("", Err(Unavailable::Extension), program).unwrap();
        assert_eq!(
            offer[0].action,
            Action::Run(vec![
                "/opt/sonar-app".into(),
                "--install-gnome-extension".into()
            ])
        );
        assert_eq!(
            answer("", Err(Unavailable::Desktop), program).unwrap_err(),
            "Switching windows works on GNOME for now"
        );
    }
}
