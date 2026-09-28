//! Everything that answers a search besides the file index: the built-in calculator,
//! external plugins that speak Sonar's JSON-lines protocol (see `docs/plugins.md`),
//! the settings plugins declare, and installing plugins from GitHub marketplaces.

pub mod browser;
pub mod calculator;
mod clock;
pub mod currency;
mod external;
mod manifest;
pub mod processes;
mod setting;
pub mod store;
pub mod system;

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use calculator::Calculator;
pub use external::{External, Prepare};
pub use manifest::{FILE as MANIFEST, Manifest, Position, check_keyword, discover, image_url};
pub use setting::{Choice, Field, Setting, resolve};

/// One result from a plugin.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Item {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// A glyph Sonar draws, like `bookmark` or `window`; see docs/plugins.md.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A picture of its own instead, like an app's icon: a PNG, SVG, JPEG or WebP
    /// file, relative to the plugin folder or absolute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub action: Action,
    /// What Ctrl+Enter (⌘+Enter on macOS) does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<Action>,
    /// What the footer calls Enter and Ctrl+Enter, when "Open" or "Copy" says too
    /// little, like "Copy number".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_label: Option<String>,
}

impl Item {
    /// A result with only a title and what Enter does.
    pub fn new(title: impl Into<String>, action: Action) -> Item {
        Item {
            title: title.into(),
            subtitle: None,
            icon: None,
            image: None,
            action,
            alt: None,
            label: None,
            alt_label: None,
        }
    }
}

/// What happens when an item is chosen. Sonar carries these out, so plugins need no
/// platform code to open a link or copy text.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Open a URL, or a file or folder in its default app.
    Open(String),
    /// Show a file or folder in the file manager.
    Reveal(String),
    /// Put text on the clipboard.
    Copy(String),
    /// Start a program. The first element is the program, the rest are its arguments.
    Run(Vec<String>),
    /// Replace the search text, e.g. to complete a keyword.
    Fill(String),
}

/// Runs a plugin written in Rust: answers each query Sonar writes to stdin with one
/// line on stdout, as docs/plugins.md describes, until Sonar closes stdin.
pub fn serve(mut answer: impl FnMut(&str, &Map<String, Value>) -> Result<Vec<Item>, String>) {
    #[derive(Deserialize)]
    struct Question {
        query: String,
        #[serde(default)]
        settings: Map<String, Value>,
    }
    let stdout = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let reply = match serde_json::from_str::<Question>(&line) {
            Ok(question) => match answer(&question.query, &question.settings) {
                Ok(items) => serde_json::json!({ "items": items }),
                Err(error) => serde_json::json!({ "error": error }),
            },
            Err(err) => serde_json::json!({ "error": format!("Sonar sent {err}") }),
        };
        let mut out = stdout.lock();
        if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
            break;
        }
    }
}

/// The text after `keyword` when `query` starts with the keyword and a space.
pub fn strip_keyword<'q>(query: &'q str, keyword: &str) -> Option<&'q str> {
    let rest = query.strip_prefix(keyword)?;
    rest.starts_with(char::is_whitespace)
        .then(|| rest.trim_start())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_needs_a_space_after_it() {
        assert_eq!(strip_keyword("g rust traits", "g"), Some("rust traits"));
        assert_eq!(strip_keyword("g ", "g"), Some(""));
        assert_eq!(strip_keyword("g", "g"), None);
        assert_eq!(strip_keyword("gist", "g"), None);
        assert_eq!(strip_keyword("report", "g"), None);
    }

    #[test]
    fn items_parse_from_json() {
        let item: Item = serde_json::from_str(
            r#"{"title": "Sonar", "action": {"open": "https://example.com"}, "alt": {"copy": "x"}, "later": 1}"#,
        )
        .unwrap();
        assert_eq!(item.action, Action::Open("https://example.com".into()));
        assert_eq!(item.alt, Some(Action::Copy("x".into())));
        assert_eq!(item.subtitle, None);
    }

    #[test]
    fn items_write_the_json_they_read() {
        let mut item = Item::new("42", Action::Copy("42".into()));
        item.alt_label = Some("Copy number".into());
        let json = serde_json::to_string(&item).unwrap();
        assert_eq!(
            json,
            r#"{"title":"42","action":{"copy":"42"},"alt_label":"Copy number"}"#
        );
        assert_eq!(serde_json::from_str::<Item>(&json).unwrap(), item);
    }
}
