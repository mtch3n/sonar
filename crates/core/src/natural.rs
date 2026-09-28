//! Everyday phrases as filters: "pdfs from last week" searches like `kind:pdf
//! modified:<7d`. Times and sizes are always taken as filters; the name of a kind
//! of file only when a time or size came with it, so `photos` alone still finds
//! `backupPhotos.sh` by its name.

use jiff::{Span, Timestamp, ToSpan, Zoned, tz::TimeZone};

use crate::{Kind, Query, Term};

/// What a phrase asks for.
enum Filter {
    After(i64),
    Between(i64, i64),
    Above(u64),
    Below(u64),
}

/// Words that lead into a time and say nothing of their own.
const INTO_TIME: [&str; 5] = ["from", "in", "since", "during", "within"];
/// Words left out once a query is read as filters.
const FILLER: [&str; 8] = ["my", "the", "all", "files", "file", "of", "from", "in"];

/// Takes everyday times and sizes out of `q`'s words and makes them filters, and
/// then kinds of file too.
pub(crate) fn apply(q: &mut Query, now: i64) {
    let words: Vec<String> = q.terms.iter().map(|t| t.text.to_lowercase()).collect();
    let plain = |i: usize| !q.terms[i].exact && !q.terms[i].name_only;
    let today = Timestamp::from_second(now)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(TimeZone::system());
    let mut used = vec![false; words.len()];
    let mut filters = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if !plain(i) {
            i += 1;
            continue;
        }
        // A time may follow "from" or "in", which go with it.
        let lead = usize::from(INTO_TIME.contains(&words[i].as_str()) && i + 1 < words.len());
        let found = time(&words[i + lead..], &today, lead == 1)
            .map(|(taken, filter)| (lead + taken, filter))
            .or_else(|| size(&words[i..]));
        match found {
            Some((taken, filter)) if (i..i + taken).all(plain) => {
                used[i..i + taken].fill(true);
                filters.push(filter);
                i += taken;
            }
            _ => i += 1,
        }
    }
    if filters.is_empty() {
        return;
    }
    for filter in filters {
        match filter {
            Filter::After(t) => q.modified_after = Some(t),
            Filter::Between(a, b) => {
                q.modified_after = Some(a);
                q.modified_before = Some(b);
            }
            Filter::Above(b) => q.size_above = Some(b),
            Filter::Below(b) => q.size_below = Some(b),
        }
    }
    for (i, word) in words.iter().enumerate() {
        if used[i] || !plain(i) {
            continue;
        }
        if let Some(kind) = kind(word) {
            if !q.kinds.contains(&kind) {
                q.kinds.push(kind);
            }
            used[i] = true;
        } else if FILLER.contains(&word.as_str()) {
            used[i] = true;
        }
    }
    let terms = std::mem::take(&mut q.terms);
    q.terms = terms
        .into_iter()
        .zip(used)
        .filter(|(_, used)| !used)
        .map(|(term, _)| term)
        .collect::<Vec<Term>>();
}

/// A time at the start of `words`, and how many words it took. A bare year counts
/// only after "in" or "from", since `invoice 2024` more likely means the name.
fn time(words: &[String], today: &Zoned, led: bool) -> Option<(usize, Filter)> {
    let start = |day: &Zoned| day.start_of_day().ok().map(|d| d.timestamp().as_second());
    let word = |i: usize| words.get(i).map(String::as_str);
    let day = today.start_of_day().ok()?;
    match (word(0)?, word(1), word(2)) {
        ("today", ..) => Some((1, Filter::After(start(today)?))),
        ("yesterday", ..) => {
            let yesterday = day.checked_sub(1.day()).ok()?;
            Some((1, Filter::Between(start(&yesterday)?, start(today)?)))
        }
        ("this", Some("week"), _) => {
            let back = day.weekday().to_monday_zero_offset();
            let monday = day.checked_sub(i64::from(back).days()).ok()?;
            Some((2, Filter::After(start(&monday)?)))
        }
        ("this", Some("month"), _) => {
            let first = day.first_of_month().ok()?;
            Some((2, Filter::After(start(&first)?)))
        }
        ("this", Some("year"), _) => {
            let first = day.first_of_year().ok()?;
            Some((2, Filter::After(start(&first)?)))
        }
        ("last" | "past", Some("week"), _) => ago(today, 1.week(), 2),
        ("last" | "past", Some("month"), _) => {
            let this = day.first_of_month().ok()?;
            let last = this.checked_sub(1.month()).ok()?;
            Some((2, Filter::Between(start(&last)?, start(&this)?)))
        }
        ("last" | "past", Some("year"), _) => {
            let this = day.first_of_year().ok()?;
            let last = this.checked_sub(1.year()).ok()?;
            Some((2, Filter::Between(start(&last)?, start(&this)?)))
        }
        ("last" | "past", Some(n), Some(unit)) => {
            let n: i64 = n.parse().ok().filter(|n| (1..=1000).contains(n))?;
            let span = match unit.trim_end_matches('s') {
                "hour" => n.hours(),
                "day" => n.days(),
                "week" => n.weeks(),
                "month" => n.months(),
                "year" => n.years(),
                _ => return None,
            };
            ago(today, span, 3)
        }
        (year, ..) if led && year.len() == 4 => {
            let year: i16 = year.parse().ok().filter(|y| (1970..=2100).contains(y))?;
            let first = jiff::civil::date(year, 1, 1)
                .to_zoned(today.time_zone().clone())
                .ok()?;
            let next = first.checked_add(1.year()).ok()?;
            Some((1, Filter::Between(start(&first)?, start(&next)?)))
        }
        _ => None,
    }
}

fn ago(today: &Zoned, span: Span, taken: usize) -> Option<(usize, Filter)> {
    let then = today.checked_sub(span).ok()?;
    Some((taken, Filter::After(then.timestamp().as_second())))
}

/// A size at the start of `words`: "bigger than 100mb", "over 2 gb", "under 10kb".
fn size(words: &[String]) -> Option<(usize, Filter)> {
    let above = match words.first()?.as_str() {
        "bigger" | "larger" | "over" | "above" => true,
        "smaller" | "under" | "below" => false,
        _ => return None,
    };
    let mut taken = 1;
    if words.get(taken).is_some_and(|w| w == "than") {
        taken += 1;
    }
    let first = words.get(taken)?;
    let (amount, used) = if first.ends_with(|c: char| c.is_ascii_alphabetic()) {
        (first.clone(), 1)
    } else {
        (format!("{first}{}", words.get(taken + 1)?), 2)
    };
    let bytes = bytes(&amount)?;
    let filter = if above {
        Filter::Above(bytes)
    } else {
        Filter::Below(bytes)
    };
    Some((taken + used, filter))
}

fn bytes(amount: &str) -> Option<u64> {
    let split = amount.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (number, unit) = amount.split_at(split);
    let n: f64 = number.parse().ok()?;
    let multiplier = match unit {
        "kb" | "k" => 1024.0,
        "mb" | "m" => 1024.0 * 1024.0,
        "gb" | "g" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((n * multiplier) as u64)
}

fn kind(word: &str) -> Option<Kind> {
    Some(match word {
        "pdf" | "pdfs" => Kind::Pdf,
        "photo" | "photos" | "picture" | "pictures" | "image" | "images" => Kind::Image,
        "video" | "videos" | "movie" | "movies" => Kind::Video,
        "spreadsheet" | "spreadsheets" | "sheet" | "sheets" => Kind::Sheet,
        "slides" | "presentation" | "presentations" | "deck" | "decks" => Kind::Slides,
        "document" | "documents" | "doc" | "docs" | "note" | "notes" => Kind::Doc,
        "script" | "scripts" => Kind::Script,
        "song" | "songs" | "music" | "audio" => Kind::Audio,
        "archive" | "archives" | "zip" | "zips" => Kind::Archive,
        "folder" | "folders" => Kind::Folder,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Parses `input` on Wednesday 2026-09-16 at noon, in UTC.
    fn parse(input: &str) -> Query {
        let now = "2026-09-16T12:00:00Z"
            .parse::<Timestamp>()
            .unwrap()
            .as_second();
        Query::parse_at_for_tests(input, Path::new("/home/me"), now)
    }

    fn day(date: &str) -> i64 {
        jiff::civil::Date::strptime("%Y-%m-%d", date)
            .unwrap()
            .to_zoned(TimeZone::system())
            .unwrap()
            .timestamp()
            .as_second()
    }

    fn words(q: &Query) -> Vec<&str> {
        q.terms.iter().map(|t| t.text.as_str()).collect()
    }

    #[test]
    fn kinds_and_times() {
        let q = parse("pdfs from last week");
        assert_eq!(q.kinds, [Kind::Pdf]);
        assert!(words(&q).is_empty());
        let week_ago = "2026-09-09T12:00:00Z"
            .parse::<Timestamp>()
            .unwrap()
            .as_second();
        assert_eq!(q.modified_after, Some(week_ago));

        let q = parse("invoices from last month");
        assert_eq!(words(&q), ["invoices"], "not a kind, so a word to find");
        assert_eq!(q.modified_after, Some(day("2026-08-01")));
        assert_eq!(q.modified_before, Some(day("2026-09-01")));

        let q = parse("photos in 2024");
        assert_eq!(q.kinds, [Kind::Image]);
        assert_eq!(
            (q.modified_after, q.modified_before),
            (Some(day("2024-01-01")), Some(day("2025-01-01")))
        );

        let q = parse("notes this week");
        assert_eq!(q.modified_after, Some(day("2026-09-14")), "since Monday");
        let q = parse("screenshots yesterday");
        assert_eq!(q.modified_before, Some(day("2026-09-16")));
        let q = parse("past 3 days");
        let three_days_ago = "2026-09-13T12:00:00Z".parse::<Timestamp>().unwrap();
        assert_eq!(q.modified_after, Some(three_days_ago.as_second()));
    }

    #[test]
    fn sizes() {
        let q = parse("videos bigger than 1 gb");
        assert_eq!(q.kinds, [Kind::Video]);
        assert_eq!(q.size_above, Some(1024 * 1024 * 1024));
        let q = parse("scans under 500kb");
        assert_eq!(words(&q), ["scans"]);
        assert_eq!(q.size_below, Some(500 * 1024));
    }

    #[test]
    fn plain_words_stay_words() {
        for input in [
            "photos",
            "invoice 2024",
            "last",
            "\"from last week\"",
            "report today.txt",
        ] {
            let q = parse(input);
            assert!(q.kinds.is_empty(), "{input}");
        }
        assert!(parse("invoice 2024").modified_after.is_none());
        assert_eq!(words(&parse("photos")), ["photos"]);
        assert!(parse("\"from last week\"").modified_after.is_none());
    }
}
