//! Everything that answers a search besides the file index: the built-in calculator,
//! external plugins that speak Sonar's JSON-lines protocol (see `docs/plugins.md`),
//! and installing plugins from GitHub marketplaces.

mod calculator;
mod external;
mod manifest;
pub mod store;

use serde::Deserialize;

pub use calculator::calculate;
pub use external::{External, Prepare};
pub use manifest::{Manifest, check_keyword, discover};

/// One result from a plugin.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Item {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    pub action: Action,
    /// What Ctrl+Enter (⌘+Enter on macOS) does.
    #[serde(default)]
    pub alt: Option<Action>,
}

/// What happens when an item is chosen. Sonar carries these out, so plugins need no
/// platform code to open a link or copy text.
#[derive(Clone, Debug, PartialEq, Deserialize)]
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
}
