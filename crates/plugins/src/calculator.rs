use std::time::{Duration, Instant};

use crate::{Action, Item};

/// How long one calculation may run before it is abandoned.
const BUDGET: Duration = Duration::from_millis(50);

/// The result of `query` as arithmetic or a unit conversion, like `2^10` or
/// `5 km to miles`. Queries that are most likely a file search give `None`.
pub fn calculate(query: &str) -> Option<Item> {
    let query = query.trim();
    if !query.contains(|c: char| c.is_ascii_digit()) || is_date(query) {
        return None;
    }
    let deadline = Deadline(Instant::now() + BUDGET);
    let result =
        fend_core::evaluate_preview_with_interrupt(query, &fend_core::Context::new(), &deadline);
    let value = result.get_main_result().trim();
    if value.is_empty() || squash(value) == squash(query) {
        return None;
    }
    Some(Item {
        title: value.to_owned(),
        subtitle: None,
        action: Action::Copy(value.to_owned()),
        alt: None,
    })
}

struct Deadline(Instant);

impl fend_core::Interrupt for Deadline {
    fn should_interrupt(&self) -> bool {
        Instant::now() >= self.0
    }
}

/// `2026-09-25` is a file name more often than a subtraction.
fn is_date(query: &str) -> bool {
    let parts: Vec<&str> = query.split('-').collect();
    matches!(parts.as_slice(), [y, m, d]
        if y.len() == 4 && (1..=2).contains(&m.len()) && (1..=2).contains(&d.len())
            && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())))
}

/// Text without spaces or case, so `10 mb` and `10 MB` count as the same.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(query: &str) -> Option<String> {
        calculate(query).map(|item| item.title)
    }

    #[test]
    fn calculates() {
        assert_eq!(value("2^10").as_deref(), Some("1024"));
        assert_eq!(value(" 6 * 7 ").as_deref(), Some("42"));
        assert!(value("5 km to miles").is_some_and(|v| v.ends_with("miles")));
        let item = calculate("1/4").unwrap();
        assert_eq!(item.action, Action::Copy("0.25".into()));
    }

    #[test]
    fn leaves_file_searches_alone() {
        for query in [
            "report",
            "pi",
            "2024",
            "invoice 2024",
            "IMG_2031",
            "2026-09-25",
            "10 mb",
            "3 days",
            "kind:pdf",
            "",
        ] {
            assert_eq!(value(query), None, "{query}");
        }
    }
}
