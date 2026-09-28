//! Clipboard history: `clip` lists what you copied, newest first, and Enter copies it
//! again. Sonar's tray app keeps the history. It watches the clipboard through X11,
//! which Wayland desktops mirror their clipboard into for XWayland: GNOME offers no
//! Wayland way for a program without focus to watch it.

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

/// Runs the clipboard plugin, which `sonar-app --plugin clipboard` does. The history
/// is read again for every query, so new copies show at once.
pub fn serve() {
    let program = std::env::current_exe().unwrap_or_default();
    let dir = std::env::var_os("SONAR_PLUGIN_DATA")
        .map(PathBuf::from)
        .unwrap_or_default();
    crate::serve(|query, _| {
        let now = jiff::Timestamp::now().as_millisecond();
        Ok(answer(query, history(&dir), &program, now))
    });
}

/// The copies with every word of `query` in them, newest first. Enter copies one
/// again and Ctrl+Enter forgets it.
fn answer(query: &str, copies: Vec<Copy>, program: &Path, now: i64) -> Vec<Item> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
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
        })
        .collect()
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

/// Records each text copied into the history in `dir`, for as long as the X server
/// lasts; `on` says whether to keep them. XFixes says when the clipboard changes
/// hands, then Sonar asks the new owner what it holds, skipping what password
/// managers mark as secret.
#[cfg(target_os = "linux")]
pub fn record(dir: &Path, on: impl Fn() -> bool) -> Result<(), String> {
    use x11rb::{
        connection::Connection,
        protocol::{
            Event,
            xfixes::{ConnectionExt as _, SelectionEventMask},
            xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, WindowClass},
        },
    };

    /// What Sonar last asked the clipboard's owner for.
    enum Asked {
        Nothing,
        Targets(u32),
        Text,
    }

    let failed = |err: &dyn std::fmt::Display| format!("couldn't watch the clipboard: {err}");
    let (conn, screen) = x11rb::connect(None).map_err(|e| failed(&e))?;
    let root = conn.setup().roots[screen].root;
    let window = conn.generate_id().map_err(|e| failed(&e))?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )
    .map_err(|e| failed(&e))?;
    conn.xfixes_query_version(5, 0)
        .map_err(|e| failed(&e))?
        .reply()
        .map_err(|e| failed(&e))?;
    let atom = |name: &[u8]| -> Result<u32, String> {
        Ok(conn
            .intern_atom(false, name)
            .map_err(|e| failed(&e))?
            .reply()
            .map_err(|e| failed(&e))?
            .atom)
    };
    let clipboard = atom(b"CLIPBOARD")?;
    let targets = atom(b"TARGETS")?;
    let utf8 = atom(b"UTF8_STRING")?;
    let secret = atom(b"x-kde-passwordManagerHint")?;
    let incr = atom(b"INCR")?;
    let property = atom(b"SONAR_CLIPBOARD")?;
    conn.xfixes_select_selection_input(window, clipboard, SelectionEventMask::SET_SELECTION_OWNER)
        .map_err(|e| failed(&e))?;
    conn.flush().map_err(|e| failed(&e))?;

    let mut asked = Asked::Nothing;
    loop {
        match conn.wait_for_event().map_err(|e| failed(&e))? {
            Event::XfixesSelectionNotify(e) if e.owner != x11rb::NONE => {
                conn.convert_selection(window, clipboard, targets, property, e.selection_timestamp)
                    .map_err(|e| failed(&e))?;
                conn.flush().map_err(|e| failed(&e))?;
                asked = Asked::Targets(e.selection_timestamp);
            }
            Event::SelectionNotify(e) if e.requestor == window => {
                if e.property == x11rb::NONE {
                    asked = Asked::Nothing;
                    continue;
                }
                let reply = conn
                    .get_property(true, window, property, AtomEnum::ANY, 0, u32::MAX / 4)
                    .map_err(|e| failed(&e))?
                    .reply()
                    .map_err(|e| failed(&e))?;
                asked = match asked {
                    Asked::Targets(time) => {
                        let offered: Vec<u32> =
                            reply.value32().map(Iterator::collect).unwrap_or_default();
                        if offered.contains(&secret) || !offered.contains(&utf8) {
                            Asked::Nothing
                        } else {
                            conn.convert_selection(window, clipboard, utf8, property, time)
                                .map_err(|e| failed(&e))?;
                            conn.flush().map_err(|e| failed(&e))?;
                            Asked::Text
                        }
                    }
                    Asked::Text => {
                        // Text sent in pieces is longer than the history keeps.
                        if reply.type_ != incr && on() {
                            let text = String::from_utf8_lossy(&reply.value);
                            let now = jiff::Timestamp::now().as_millisecond();
                            if let Err(err) = remember(dir, &text, now) {
                                eprintln!("sonar: couldn't keep a copy: {err}");
                            }
                        }
                        Asked::Nothing
                    }
                    Asked::Nothing => Asked::Nothing,
                };
            }
            _ => {}
        }
    }
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
        let all = answer("", copies.clone(), program, now);
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
        let found = answer("INVOICE example", copies, program, now);
        assert_eq!(titles(&found), ["https://example.com/Invoice"]);
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "needs a desktop session with wl-copy"]
    fn records_what_is_copied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_owned();
        std::thread::spawn(move || record(&path, || true));
        std::thread::sleep(std::time::Duration::from_millis(500));
        let text = format!("sonar test {}", std::process::id());
        // wl-copy stays behind to serve the clipboard; it mustn't hold the test's output.
        let mut copy = std::process::Command::new("wl-copy")
            .arg(&text)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        copy.wait().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert_eq!(
            history(dir.path()).first().map(|c| c.text.as_str()),
            Some(text.as_str())
        );
    }
}
