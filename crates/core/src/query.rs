use std::{
    fmt,
    path::{MAIN_SEPARATOR, MAIN_SEPARATOR_STR, Path},
};

use crate::Kind;

pub const DEFAULT_LIMIT: usize = 20;

#[derive(Debug, PartialEq)]
pub struct Query {
    pub terms: Vec<Term>,
    pub exclude: Vec<String>,
    pub kinds: Vec<Kind>,
    pub exts: Vec<String>,
    pub within: Vec<Within>,
    pub modified_after: Option<i64>,
    pub modified_before: Option<i64>,
    pub size_above: Option<u64>,
    pub size_below: Option<u64>,
    pub limit: usize,
}

#[derive(Debug, PartialEq)]
pub struct Term {
    pub text: String,
    pub exact: bool,
    pub name_only: bool,
}

#[derive(Debug, PartialEq)]
pub enum Within {
    Path(String),
    Folder(String),
}

#[derive(Debug, PartialEq)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Copy)]
enum Key {
    Kind,
    Ext,
    In,
    Name,
    Modified,
    After,
    Before,
    Size,
    Limit,
}

const KEYS: [(&str, Key); 9] = [
    ("kind", Key::Kind),
    ("ext", Key::Ext),
    ("in", Key::In),
    ("name", Key::Name),
    ("modified", Key::Modified),
    ("after", Key::After),
    ("before", Key::Before),
    ("size", Key::Size),
    ("limit", Key::Limit),
];

impl Query {
    pub fn parse(input: &str, home: &Path) -> Result<Query, ParseError> {
        Query::parse_at(input, home, jiff::Timestamp::now().as_second())
    }

    fn parse_at(input: &str, home: &Path, now: i64) -> Result<Query, ParseError> {
        let mut q = Query {
            terms: Vec::new(),
            exclude: Vec::new(),
            kinds: Vec::new(),
            exts: Vec::new(),
            within: Vec::new(),
            modified_after: None,
            modified_before: None,
            size_above: None,
            size_below: None,
            limit: DEFAULT_LIMIT,
        };
        for token in tokens(input) {
            if let Some(word) = token.strip_prefix('-').filter(|w| !w.is_empty()) {
                q.exclude.push(unquote(word));
            } else if let Some((key, value)) = filter(token) {
                q.apply(key, value, home, now)?;
            } else {
                q.terms.push(Term {
                    text: unquote(token),
                    exact: token.contains('"'),
                    name_only: false,
                });
            }
        }
        Ok(q)
    }

    fn apply(&mut self, key: Key, raw: &str, home: &Path, now: i64) -> Result<(), ParseError> {
        let value = unquote(raw);
        if value.is_empty() {
            return Ok(());
        }
        match key {
            Key::Kind => {
                for name in list(&value) {
                    let kind = Kind::from_name(name).ok_or_else(|| {
                        let known: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
                        ParseError(format!(
                            "unknown kind `{name}`; use one of: {}",
                            known.join(", ")
                        ))
                    })?;
                    self.kinds.push(kind);
                }
            }
            Key::Ext => self
                .exts
                .extend(list(&value).map(|e| e.trim_start_matches('.').to_lowercase())),
            Key::In => self.within.push(within(&value, home)),
            Key::Name => self.terms.push(Term {
                text: value,
                exact: raw.contains('"'),
                name_only: true,
            }),
            Key::Modified => match split_op(&value) {
                (Some('>'), span) => self.modified_before = Some(now - duration(span)?),
                (_, span) => self.modified_after = Some(now - duration(span)?),
            },
            Key::After => self.modified_after = Some(date(&value)?),
            Key::Before => self.modified_before = Some(date(&value)?),
            Key::Size => match split_op(&value) {
                (Some('<'), amount) => self.size_below = Some(size(amount)?),
                (_, amount) => self.size_above = Some(size(amount)?),
            },
            Key::Limit => {
                self.limit = value.parse().ok().filter(|n| *n > 0).ok_or_else(|| {
                    ParseError(format!("`limit:` needs a positive number, not `{value}`"))
                })?;
            }
        }
        Ok(())
    }
}

fn tokens(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (i, c) in input.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if c.is_whitespace() && !quoted {
            if let Some(s) = start.take() {
                out.push(&input[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&input[s..]);
    }
    out
}

fn filter(token: &str) -> Option<(Key, &str)> {
    let (key, value) = token.split_once(':')?;
    KEYS.iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, k)| (*k, value))
}

fn unquote(s: &str) -> String {
    s.replace('"', "")
}

fn list(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty())
}

fn split_op(value: &str) -> (Option<char>, &str) {
    match value.chars().next() {
        Some(op @ ('<' | '>')) => (Some(op), &value[1..]),
        _ => (None, value),
    }
}

fn within(value: &str, home: &Path) -> Within {
    let value = value
        .replace('/', MAIN_SEPARATOR_STR)
        .trim_end_matches(MAIN_SEPARATOR)
        .to_owned();
    if let Some(rest) = value.strip_prefix('~') {
        Within::Path(format!("{}{rest}", home.display()))
    } else if Path::new(&value).is_absolute() {
        Within::Path(value)
    } else {
        Within::Folder(value)
    }
}

fn duration(span: &str) -> Result<i64, ParseError> {
    let err = || {
        ParseError(format!(
            "`{span}` isn't a time span; use a number and h, d, w, m or y, like 30d"
        ))
    };
    let split = span.find(|c: char| !c.is_ascii_digit()).ok_or_else(err)?;
    let (number, unit) = span.split_at(split);
    let n: i64 = number.parse().map_err(|_| err())?;
    let unit_secs = match unit.to_lowercase().as_str() {
        "h" => 3600,
        "d" => 86_400,
        "w" => 7 * 86_400,
        "m" | "mo" => 30 * 86_400,
        "y" => 365 * 86_400,
        _ => return Err(err()),
    };
    Ok(n * unit_secs)
}

fn date(value: &str) -> Result<i64, ParseError> {
    let day: jiff::civil::Date = value
        .parse()
        .map_err(|_| ParseError(format!("`{value}` isn't a date; use YYYY-MM-DD")))?;
    let zoned = day
        .to_zoned(jiff::tz::TimeZone::system())
        .map_err(|e| ParseError(e.to_string()))?;
    Ok(zoned.timestamp().as_second())
}

fn size(amount: &str) -> Result<u64, ParseError> {
    let err = || {
        ParseError(format!(
            "`{amount}` isn't a size; use a number and kb, mb or gb, like 10mb"
        ))
    };
    let split = amount
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(amount.len());
    let (number, unit) = amount.split_at(split);
    let n: f64 = number.parse().map_err(|_| err())?;
    let multiplier: f64 = match unit.to_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1024.0,
        "m" | "mb" => 1024.0 * 1024.0,
        "g" | "gb" => 1024.0 * 1024.0 * 1024.0,
        _ => return Err(err()),
    };
    Ok((n * multiplier) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    fn parse(input: &str) -> Result<Query, ParseError> {
        Query::parse_at(input, Path::new("/home/me"), NOW)
    }

    fn term(text: &str, exact: bool, name_only: bool) -> Term {
        Term {
            text: text.to_owned(),
            exact,
            name_only,
        }
    }

    #[test]
    fn words_and_filters() {
        let q = parse(
            r#"invoice "tax return" kind:pdf,Images ext:.SH in:~/Documents in:trellis modified:<30d size:>10mb limit:5 -draft name:readme"#,
        )
        .unwrap();
        assert_eq!(
            q.terms,
            [
                term("invoice", false, false),
                term("tax return", true, false),
                term("readme", false, true)
            ]
        );
        assert_eq!(q.exclude, ["draft"]);
        assert_eq!(q.kinds, [Kind::Pdf, Kind::Image]);
        assert_eq!(q.exts, ["sh"]);
        assert_eq!(
            q.within,
            [
                Within::Path(format!("/home/me{MAIN_SEPARATOR}Documents")),
                Within::Folder("trellis".into())
            ]
        );
        assert_eq!(q.modified_after, Some(NOW - 30 * DAY));
        assert_eq!(q.size_above, Some(10 * 1024 * 1024));
        assert_eq!(q.limit, 5);
    }

    #[test]
    fn older_than_and_smaller_than() {
        let q = parse("modified:>1y size:<1.5kb").unwrap();
        assert_eq!(q.modified_before, Some(NOW - 365 * DAY));
        assert_eq!(q.size_below, Some(1536));
    }

    #[test]
    fn quoted_paths_keep_spaces() {
        let q = parse(r#"in:"~/My Docs""#).unwrap();
        assert_eq!(
            q.within,
            [Within::Path(format!("/home/me{MAIN_SEPARATOR}My Docs"))]
        );
    }

    #[test]
    fn unknown_keys_are_search_words() {
        let q = parse("https://example.com todo:later").unwrap();
        assert_eq!(
            q.terms,
            [
                term("https://example.com", false, false),
                term("todo:later", false, false)
            ]
        );
    }

    #[test]
    fn empty_filter_is_ignored_while_typing() {
        let q = parse("kind:").unwrap();
        assert!(q.kinds.is_empty() && q.terms.is_empty());
    }

    #[test]
    fn bad_values_are_errors() {
        for input in [
            "kind:banana",
            "size:big",
            "modified:soon",
            "limit:0",
            "after:yesterday",
        ] {
            assert!(parse(input).is_err(), "{input} should fail");
        }
    }
}
