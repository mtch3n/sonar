//! Clipboard history: `clip` lists what you copied, newest first, and Enter copies it
//! again. Sonar's tray app keeps the history; on GNOME its Shell extension tells it
//! about each copy, because GNOME on Wayland keeps other programs from watching the
//! clipboard.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{Action, Item, Setting};

/// The clipboard plugin's id in `settings.toml`, and its keyword.
pub const ID: &str = "clipboard";
pub const KEYWORD: &str = "clip";
/// The file in the plugin's data folder that holds the history.
const FILE: &str = "history.json";
/// How many copies the history keeps.
const KEEP: usize = 200;
/// Longer text isn't kept, so the history stays quick to read and write.
const LONGEST: usize = 256 * 1024;
/// The most copies shown for one query.
const LIMIT: usize = 50;
/// Where the extension announces copies on the session bus.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const PATH: &str = "/io/github/mtch3n/Sonar/Clipboard";
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const INTERFACE: &str = "io.github.mtch3n.Sonar.Clipboard";

pub fn settings() -> Vec<Setting> {
    Vec::new()
}

/// One copy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Copy {
    pub text: String,
    /// When it was copied, in Unix milliseconds; also names it for forgetting.
    pub copied: i64,
}

/// The copies kept in `dir`, newest first.
pub fn history(dir: &Path) -> Vec<Copy> {
    fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(dir: &Path, copies: &[Copy]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| format!("creating {}: {err}", dir.display()))?;
    let text = serde_json::to_string(copies).map_err(|err| err.to_string())?;
    // Written aside and moved into place, so the plugin never reads half a file.
    let partial = dir.join(format!("{FILE}.partial"));
    fs::write(&partial, text).map_err(|err| err.to_string())?;
    fs::rename(&partial, dir.join(FILE)).map_err(|err| err.to_string())
}

/// Puts `text` at the top of the history, moving it there if it was copied before.
pub fn remember(dir: &Path, text: &str, now: i64) -> Result<(), String> {
    if text.trim().is_empty() || text.len() > LONGEST {
        return Ok(());
    }
    let mut copies = history(dir);
    if copies.first().is_some_and(|c| c.text == text) {
        return Ok(());
    }
    copies.retain(|c| c.text != text);
    copies.insert(
        0,
        Copy {
            text: text.to_owned(),
            copied: now,
        },
    );
    copies.truncate(KEEP);
    save(dir, &copies)
}

/// Removes the copy made at `copied`, or all of them when it's `None`.
pub fn forget(dir: &Path, copied: Option<i64>) -> Result<(), String> {
    let mut copies = history(dir);
    match copied {
        Some(copied) => copies.retain(|c| c.copied != copied),
        None => copies.clear(),
    }
    save(dir, &copies)
}

/// Why copies aren't being recorded.
#[derive(Debug, PartialEq)]
enum Unavailable {
    /// Not GNOME, whose extension is the only way Sonar knows so far.
    Desktop,
    /// GNOME, but the extension isn't installed, is too old, or hasn't loaded yet.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Extension,
}

/// Runs the clipboard plugin, which `sonar-app --plugin clipboard` does. The history
/// is read again for every query, so new copies show at once.
pub fn serve() {
    let program = std::env::current_exe().unwrap_or_default();
    let dir = std::env::var_os("SONAR_PLUGIN_DATA")
        .map(PathBuf::from)
        .unwrap_or_default();
    crate::serve(|query, _| {
        let now = jiff::Timestamp::now().as_millisecond();
        answer(query, history(&dir), recording(), &program, now)
    });
}

fn answer(
    query: &str,
    copies: Vec<Copy>,
    recording: Result<(), Unavailable>,
    program: &Path,
    now: i64,
) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    match recording {
        Ok(()) => {}
        Err(Unavailable::Desktop) => {
            return Err("Clipboard history works on GNOME for now".into());
        }
        Err(Unavailable::Extension) => {
            let mut item = Item::new(
                "Install the Sonar extension for GNOME",
                Action::Run(vec![
                    program.to_string_lossy().into_owned(),
                    "--install-gnome-extension".into(),
                ]),
            );
            item.subtitle = Some(
                "GNOME only lets its own extensions see what you copy. Log out and back in afterwards"
                    .into(),
            );
            item.icon = Some("clipboard".into());
            item.label = Some("Install".into());
            items.push(item);
        }
    }
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    items.extend(
        copies
            .into_iter()
            .filter(|c| {
                let text = c.text.to_lowercase();
                words.iter().all(|w| text.contains(w.as_str()))
            })
            .take(LIMIT)
            .map(|c| {
                let forget = vec![
                    program.to_string_lossy().into_owned(),
                    "--forget-copy".into(),
                    c.copied.to_string(),
                ];
                let mut item = Item::new(title(&c.text), Action::Copy(c.text.clone()));
                item.subtitle = Some(subtitle(&c, now));
                item.icon = Some("clipboard".into());
                item.alt = Some(Action::Run(forget));
                item.alt_label = Some("Remove from history".into());
                item
            }),
    );
    Ok(items)
}

/// The first line with text on it, shortened to fit one row.
fn title(text: &str) -> String {
    const WIDTH: usize = 200;
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    if line.chars().count() > WIDTH {
        format!("{}…", line.chars().take(WIDTH).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// How long ago it was copied, and how many lines it has when there are several.
fn subtitle(copy: &Copy, now: i64) -> String {
    let minutes = (now - copy.copied).max(0) / 60_000;
    let ago = match minutes {
        0 => "just now".to_owned(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => format!("{} d ago", minutes / 1440),
    };
    let lines = copy.text.trim().lines().count();
    if lines > 1 {
        format!("{ago} · {lines} lines")
    } else {
        ago
    }
}

/// Whether copies are reaching Sonar: on GNOME, whether its extension announces them.
#[cfg(target_os = "linux")]
fn recording() -> Result<(), Unavailable> {
    if !gnome() {
        return Err(Unavailable::Desktop);
    }
    let known = zbus::blocking::Connection::session()
        .and_then(|bus| {
            bus.call_method(
                Some("org.gnome.Shell"),
                PATH,
                Some("org.freedesktop.DBus.Introspectable"),
                "Introspect",
                &(),
            )
        })
        .and_then(|reply| reply.body().deserialize::<String>())
        .is_ok_and(|xml| xml.contains(INTERFACE));
    if known {
        Ok(())
    } else {
        Err(Unavailable::Extension)
    }
}

#[cfg(not(target_os = "linux"))]
fn recording() -> Result<(), Unavailable> {
    Err(Unavailable::Desktop)
}

#[cfg(target_os = "linux")]
fn gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("GNOME"))
}

/// Records each copy GNOME Shell announces into the history in `dir`, for as long as
/// the session lasts; `on` says whether to keep them. Returns at once off GNOME.
#[cfg(target_os = "linux")]
pub fn record(dir: &Path, on: impl Fn() -> bool) -> Result<(), String> {
    if !gnome() {
        return Ok(());
    }
    let bus = zbus::blocking::Connection::session().map_err(|err| err.to_string())?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.gnome.Shell")
        .and_then(|b| b.path(PATH))
        .and_then(|b| b.interface(INTERFACE))
        .and_then(|b| b.member("Copied"))
        .map_err(|err| err.to_string())?
        .build();
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &bus, None)
        .map_err(|err| err.to_string())?;
    for signal in signals {
        let Ok(signal) = signal else { continue };
        let Ok(text) = signal.body().deserialize::<String>() else {
            continue;
        };
        if on() {
            let now = jiff::Timestamp::now().as_millisecond();
            if let Err(err) = remember(dir, &text, now) {
                eprintln!("sonar: couldn't keep a copy: {err}");
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn record(_: &Path, _: impl Fn() -> bool) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000;

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn keeps_copies_newest_first_without_repeats() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        remember(dir, "one", 1).unwrap();
        remember(dir, "two", 2).unwrap();
        remember(dir, "   ", 3).unwrap();
        remember(dir, "one", 4).unwrap();
        let texts: Vec<String> = history(dir).into_iter().map(|c| c.text).collect();
        assert_eq!(texts, ["one", "two"]);
        assert_eq!(history(dir)[0].copied, 4);
        forget(dir, Some(4)).unwrap();
        assert_eq!(history(dir).len(), 1);
        forget(dir, None).unwrap();
        assert!(history(dir).is_empty());
    }

    #[test]
    fn keeps_a_bounded_history() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..KEEP + 5 {
            remember(dir.path(), &n.to_string(), n as i64).unwrap();
        }
        let copies = history(dir.path());
        assert_eq!(copies.len(), KEEP);
        assert_eq!(copies[0].text, (KEEP + 4).to_string());
        remember(dir.path(), &"x".repeat(LONGEST + 1), 0).unwrap();
        assert_eq!(history(dir.path()).len(), KEEP);
    }

    #[test]
    fn lists_and_filters_copies() {
        let now = 1_000 * MINUTE;
        let copies = vec![
            Copy {
                text: "\n  fn main() {\n}\n".into(),
                copied: now - 3 * MINUTE,
            },
            Copy {
                text: "https://example.com/Invoice".into(),
                copied: now - 120 * MINUTE,
            },
        ];
        let program = Path::new("/opt/sonar-app");
        let all = answer("", copies.clone(), Ok(()), program, now).unwrap();
        assert_eq!(titles(&all), ["fn main() {", "https://example.com/Invoice"]);
        assert_eq!(all[0].subtitle.as_deref(), Some("3 min ago · 2 lines"));
        assert_eq!(all[1].subtitle.as_deref(), Some("2 h ago"));
        assert_eq!(all[0].action, Action::Copy("\n  fn main() {\n}\n".into()));
        assert_eq!(
            all[1].alt,
            Some(Action::Run(vec![
                "/opt/sonar-app".into(),
                "--forget-copy".into(),
                (now - 120 * MINUTE).to_string()
            ]))
        );
        let found = answer("INVOICE example", copies, Ok(()), program, now).unwrap();
        assert_eq!(titles(&found), ["https://example.com/Invoice"]);
    }

    #[test]
    fn offers_the_extension_or_explains_the_desktop() {
        let program = Path::new("/opt/sonar-app");
        let offer = answer("", Vec::new(), Err(Unavailable::Extension), program, 0).unwrap();
        assert_eq!(
            offer[0].action,
            Action::Run(vec![
                "/opt/sonar-app".into(),
                "--install-gnome-extension".into()
            ])
        );
        assert_eq!(
            answer("", Vec::new(), Err(Unavailable::Desktop), program, 0).unwrap_err(),
            "Clipboard history works on GNOME for now"
        );
    }
}
