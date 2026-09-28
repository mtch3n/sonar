//! Times and dates for the calculator: the time in a place, a time moved to another
//! place, and dates moved by days, weeks, months or years.
//!
//! The grammar, word by word and ignoring case:
//!
//! ```text
//! query = "time" ["in" | "at"] place             time in tokyo
//!       | place "time"                           tokyo time
//!       | "now" ("in" | "at") place              now in london
//!       | clock [place] [("in" | "to") place]    3pm tokyo in taipei, 15:30 utc to pst
//!       | "days" ("until" | "since") date        days until 2026-12-25
//!       | date "-" date                          2026-12-25 - today
//!       | date (("+" | "-") number unit)+        today + 90 days
//! clock = H ("am" | "pm") | H:MM ["am" | "pm"] | "noon" | "midnight"
//! date  = "today" | "tomorrow" | "yesterday" | YYYY-MM-DD
//! unit  = "day" | "week" | "month" | "year", or their plurals
//! place = the city of a zone, like "tokyo" for Asia/Tokyo, a name in ALIASES, an
//!         abbreviation in ABBREVIATIONS, or a zone like "asia/tokyo"
//! ```
//!
//! A clock with a place left out is in your time zone, on your today. Anything else
//! is left to the rest of the calculator, so file searches like `tokyo trip photos`
//! or `2026-09-25` get no answer here.
//!
//! Zones come from jiff's database: the system's on Linux and macOS, and on Windows,
//! which has none, a copy built into Sonar by jiff's default `tzdb-bundle-platform`
//! feature.

use jiff::{
    Span, Timestamp, Zoned,
    civil::{Date, Time},
    tz::{self, Offset, TimeZone},
};

use crate::{Action, Item};

/// Abbreviations people type for a zone. Each stands for one place and follows its
/// daylight saving time, so `pst` in summer is shown as PDT. Where an abbreviation
/// is shared, the most common meaning wins: `cst` is US Central, not China, and `ist`
/// is India, not Ireland or Israel.
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("utc", "UTC"),
    ("gmt", "Etc/GMT"),
    ("est", "America/New_York"),
    ("edt", "America/New_York"),
    ("cst", "America/Chicago"),
    ("cdt", "America/Chicago"),
    ("mst", "America/Denver"),
    ("mdt", "America/Denver"),
    ("pst", "America/Los_Angeles"),
    ("pdt", "America/Los_Angeles"),
    ("bst", "Europe/London"),
    ("cet", "Europe/Berlin"),
    ("cest", "Europe/Berlin"),
    ("eet", "Europe/Athens"),
    ("eest", "Europe/Athens"),
    ("ist", "Asia/Kolkata"),
    ("hkt", "Asia/Hong_Kong"),
    ("sgt", "Asia/Singapore"),
    ("kst", "Asia/Seoul"),
    ("jst", "Asia/Tokyo"),
    ("aest", "Australia/Sydney"),
    ("aedt", "Australia/Sydney"),
    ("nzst", "Pacific/Auckland"),
    ("nzdt", "Pacific/Auckland"),
];

/// Places people ask about that aren't the city a zone is named after.
const ALIASES: &[(&str, &str)] = &[
    ("beijing", "Asia/Shanghai"),
    ("china", "Asia/Shanghai"),
    ("shenzhen", "Asia/Shanghai"),
    ("taiwan", "Asia/Taipei"),
    ("japan", "Asia/Tokyo"),
    ("osaka", "Asia/Tokyo"),
    ("korea", "Asia/Seoul"),
    ("india", "Asia/Kolkata"),
    ("delhi", "Asia/Kolkata"),
    ("new delhi", "Asia/Kolkata"),
    ("mumbai", "Asia/Kolkata"),
    ("bangalore", "Asia/Kolkata"),
    ("vietnam", "Asia/Ho_Chi_Minh"),
    ("hanoi", "Asia/Ho_Chi_Minh"),
    ("saigon", "Asia/Ho_Chi_Minh"),
    ("uk", "Europe/London"),
    ("germany", "Europe/Berlin"),
    ("munich", "Europe/Berlin"),
    ("frankfurt", "Europe/Berlin"),
    ("france", "Europe/Paris"),
    ("barcelona", "Europe/Madrid"),
    ("milan", "Europe/Rome"),
    ("geneva", "Europe/Zurich"),
    ("nyc", "America/New_York"),
    ("washington", "America/New_York"),
    ("boston", "America/New_York"),
    ("miami", "America/New_York"),
    ("atlanta", "America/New_York"),
    ("montreal", "America/Toronto"),
    ("dallas", "America/Chicago"),
    ("houston", "America/Chicago"),
    ("austin", "America/Chicago"),
    ("seattle", "America/Los_Angeles"),
    ("san francisco", "America/Los_Angeles"),
    ("sf", "America/Los_Angeles"),
    ("la", "America/Los_Angeles"),
    ("california", "America/Los_Angeles"),
];

/// The regions IANA zones are named under, like `Asia` in `Asia/Tokyo`.
const REGIONS: &[&str] = &[
    "Africa",
    "America",
    "Antarctica",
    "Asia",
    "Atlantic",
    "Australia",
    "Europe",
    "Indian",
    "Pacific",
];

/// The answer to `query` as a time or date question, as of `now`, or `None` when it
/// isn't one.
pub fn answer(query: &str, now: &Zoned) -> Option<Item> {
    // `today+90 days` reads like `today + 90 days`. A `-` can't be split off the same
    // way, since dates have them.
    let query = query.to_lowercase().replace('+', " + ");
    let words: Vec<&str> = query.split_whitespace().collect();
    time_in_place(&words, now)
        .or_else(|| convert(&words, now))
        .or_else(|| date_math(&words, now.date()))
}

/// A place's zone, and what to call it.
struct Place {
    zone: TimeZone,
    /// `None` for an abbreviation, which is shown as the zone's own at the time, so
    /// `pst` in summer shows PDT.
    name: Option<String>,
}

impl Place {
    fn label(&self, at: Timestamp) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => self.zone.to_offset_info(at).abbreviation().to_owned(),
        }
    }
}

fn place(words: &[&str]) -> Option<Place> {
    let name = words.join(" ");
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '_' | '/'))
    {
        return None;
    }
    if let Some((_, zone)) = ABBREVIATIONS.iter().find(|(short, _)| *short == name) {
        let zone = tz::db().get(zone).ok()?;
        return Some(Place { zone, name: None });
    }
    if let Some((alias, zone)) = ALIASES.iter().find(|(alias, _)| *alias == name) {
        let zone = tz::db().get(zone).ok()?;
        let name = if alias.len() <= 3 {
            alias.to_uppercase()
        } else {
            title_case(alias)
        };
        return Some(Place {
            zone,
            name: Some(name),
        });
    }
    let city = name.replace(' ', "_");
    let zone = if city.contains('/') {
        tz::db().get(&city).ok()?
    } else {
        REGIONS
            .iter()
            .find_map(|region| tz::db().get(&format!("{region}/{city}")).ok())?
    };
    // The zone's spelling, so `NEW YORK` is shown as New York.
    let name = zone.iana_name()?.rsplit('/').next()?.replace('_', " ");
    Some(Place {
        zone,
        name: Some(name),
    })
}

fn title_case(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `time in tokyo`, `tokyo time`, `now in london`.
fn time_in_place(words: &[&str], now: &Zoned) -> Option<Item> {
    let place = match words {
        ["time" | "now", "in" | "at", rest @ ..] => place(rest)?,
        ["time", rest @ ..] | [rest @ .., "time"] => place(rest)?,
        _ => return None,
    };
    let there = now.with_time_zone(place.zone.clone());
    Some(show(&there, Some(&place), now.time_zone(), "you"))
}

/// `3pm tokyo in taipei`, `15:30 utc to pst`, `9am new york`, `3pm in tokyo`.
fn convert(words: &[&str], now: &Zoned) -> Option<Item> {
    let (time, rest) = clock(words)?;
    let (from, to) = match rest.iter().rposition(|w| matches!(*w, "in" | "to")) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &[][..]),
    };
    let from = if from.is_empty() {
        None
    } else {
        Some(place(from)?)
    };
    let to = if to.is_empty() {
        None
    } else {
        Some(place(to)?)
    };
    if from.is_none() && to.is_none() {
        return None;
    }
    let local = now.time_zone();
    let start = now
        .date()
        .to_datetime(time)
        .to_zoned(from.as_ref().map_or(local, |p| &p.zone).clone())
        .ok()?;
    let end = start.with_time_zone(to.as_ref().map_or(local, |p| &p.zone).clone());
    let who = from
        .as_ref()
        .map_or_else(|| "you".to_owned(), |p| p.label(start.timestamp()));
    Some(show(&end, to.as_ref(), start.time_zone(), &who))
}

/// A time of day at the start of `words`, and the words after it.
fn clock<'w>(words: &'w [&'w str]) -> Option<(Time, &'w [&'w str])> {
    let (first, rest) = words.split_first()?;
    match *first {
        "noon" => return Some((Time::constant(12, 0, 0, 0), rest)),
        "midnight" => return Some((Time::midnight(), rest)),
        _ => {}
    }
    // `3 pm`
    if let [meridiem @ ("am" | "pm"), after @ ..] = rest {
        return Some((time_of_day(first, Some(meridiem))?, after));
    }
    let (digits, meridiem) = match (first.strip_suffix("am"), first.strip_suffix("pm")) {
        (Some(digits), _) => (digits, Some("am")),
        (_, Some(digits)) => (digits, Some("pm")),
        _ => (*first, None),
    };
    Some((time_of_day(digits, meridiem)?, rest))
}

/// `3` with `pm`, `3:30` with `pm`, or `15:30` with nothing. A bare `15` is a
/// number, not a time.
fn time_of_day(text: &str, meridiem: Option<&str>) -> Option<Time> {
    let (hour, minute) = text.split_once(':').unwrap_or((text, ""));
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if (meridiem.is_none() && minute.is_empty())
        || !(1..=2).contains(&hour.len())
        || !(minute.is_empty() || minute.len() == 2)
        || !digits(hour)
        || !digits(minute)
    {
        return None;
    }
    let hour: i8 = hour.parse().ok()?;
    let minute: i8 = if minute.is_empty() {
        0
    } else {
        minute.parse().ok()?
    };
    let hour = match meridiem {
        Some(_) if !(1..=12).contains(&hour) => return None,
        Some("pm") => hour % 12 + 12,
        Some(_) => hour % 12,
        None => hour,
    };
    Time::new(hour, minute, 0, 0).ok()
}

/// `15:30 Tue 29 Sep · Taipei`, with the offset and how far it is from `other`.
fn show(time: &Zoned, place: Option<&Place>, other: &TimeZone, who: &str) -> Item {
    let mut title = time.strftime("%H:%M %a %-d %b").to_string();
    if let Some(place) = place {
        title = format!("{title} · {}", place.label(time.timestamp()));
    }
    let apart = time.offset().seconds() - other.to_offset(time.timestamp()).seconds();
    let apart = match apart {
        0 => format!("same time as {who}"),
        ..0 => format!("{} behind {who}", hours(-apart)),
        _ => format!("{} ahead of {who}", hours(apart)),
    };
    Item {
        title: title.clone(),
        subtitle: Some(format!("{} · {apart}", utc(time.offset()))),
        action: Action::Copy(title),
        alt: Some(Action::Copy(time.strftime("%Y-%m-%dT%H:%M%:z").to_string())),
        icon: Some("clock".into()),
        image: None,
        label: None,
        alt_label: None,
    }
}

/// `UTC`, `UTC+8`, `UTC-7` or `UTC+5:30`.
fn utc(offset: Offset) -> String {
    let seconds = offset.seconds();
    if seconds == 0 {
        return "UTC".to_owned();
    }
    let sign = if seconds < 0 { '-' } else { '+' };
    let (hours, minutes) = (seconds.abs() / 3600, seconds.abs() % 3600 / 60);
    if minutes == 0 {
        format!("UTC{sign}{hours}")
    } else {
        format!("UTC{sign}{hours}:{minutes:02}")
    }
}

/// `1 hour`, `8 hours` or `2.5 hours`.
fn hours(seconds: i32) -> String {
    let hours = f64::from(seconds) / 3600.0;
    if hours == 1.0 {
        "1 hour".to_owned()
    } else {
        format!("{hours} hours")
    }
}

/// `today + 90 days`, `days until 2026-12-25`, `2026-12-25 - today`.
fn date_math(words: &[&str], today: Date) -> Option<Item> {
    match words {
        ["days", "until" | "till", date] => return between(today, day(date, today)?),
        ["days", "since", date] => return between(day(date, today)?, today),
        [a, "-", b] if day(a, today).is_some() && day(b, today).is_some() => {
            return between(day(b, today)?, day(a, today)?);
        }
        _ => {}
    }
    let (first, mut rest) = words.split_first()?;
    let mut date = day(first, today)?;
    if rest.is_empty() {
        return None;
    }
    while let [sign @ ("+" | "-"), number, unit, after @ ..] = rest {
        if !number.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let number: i64 = number.parse().ok()?;
        let number = if *sign == "-" { -number } else { number };
        let span = match unit.strip_suffix('s').unwrap_or(unit) {
            "day" => Span::new().try_days(number),
            "week" => Span::new().try_weeks(number),
            "month" => Span::new().try_months(number),
            "year" => Span::new().try_years(number),
            _ => return None,
        }
        .ok()?;
        date = date.checked_add(span).ok()?;
        rest = after;
    }
    if !rest.is_empty() {
        return None;
    }
    let from_today = match today.until(date).ok()?.get_days() {
        0 => "today".to_owned(),
        1 => "tomorrow".to_owned(),
        -1 => "yesterday".to_owned(),
        days @ 2.. => format!("in {days} days"),
        days => format!("{} days ago", -days),
    };
    let title = long_date(date);
    Some(Item {
        title: title.clone(),
        subtitle: Some(from_today),
        action: Action::Copy(title),
        alt: Some(Action::Copy(date.to_string())),
        icon: Some("clock".into()),
        image: None,
        label: None,
        alt_label: None,
    })
}

fn day(word: &str, today: Date) -> Option<Date> {
    match word {
        "today" => Some(today),
        "tomorrow" => today.tomorrow().ok(),
        "yesterday" => today.yesterday().ok(),
        // Only `YYYY-MM-DD`, not the other forms jiff reads, like `20261225`.
        _ if word.len() == 10 && word.as_bytes()[4] == b'-' && word.as_bytes()[7] == b'-' => {
            word.parse().ok()
        }
        _ => None,
    }
}

/// How many days from `from` to `to`.
fn between(from: Date, to: Date) -> Option<Item> {
    let days = from.until(to).ok()?.get_days();
    let title = format!("{days} {}", if days.abs() == 1 { "day" } else { "days" });
    let mut subtitle = format!("{} to {}", long_date(from), long_date(to));
    let (weeks, rest) = (days.abs() / 7, days.abs() % 7);
    if weeks > 0 {
        let weeks = format!("{weeks} {}", if weeks == 1 { "week" } else { "weeks" });
        subtitle = match rest {
            0 => format!("{subtitle} · {weeks}"),
            1 => format!("{subtitle} · {weeks} and 1 day"),
            _ => format!("{subtitle} · {weeks} and {rest} days"),
        };
    }
    Some(Item {
        title: title.clone(),
        subtitle: Some(subtitle),
        action: Action::Copy(title),
        alt: Some(Action::Copy(days.to_string())),
        icon: Some("clock".into()),
        image: None,
        label: None,
        alt_label: None,
    })
}

/// `Sun 27 Dec 2026`.
fn long_date(date: Date) -> String {
    date.strftime("%a %-d %b %Y").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 28 September 2026, 10:00 in Taipei.
    fn now() -> Zoned {
        "2026-09-28T10:00[Asia/Taipei]".parse().unwrap()
    }

    fn answered(query: &str) -> Option<(String, String)> {
        answer(query, &now()).map(|item| (item.title, item.subtitle.unwrap()))
    }

    fn check(cases: &[(&str, &str, &str)]) {
        for (query, title, subtitle) in cases {
            assert_eq!(
                answered(query),
                Some(((*title).to_owned(), (*subtitle).to_owned())),
                "{query}"
            );
        }
    }

    #[test]
    fn tells_the_time_in_a_place() {
        check(&[
            (
                "time in tokyo",
                "11:00 Mon 28 Sep · Tokyo",
                "UTC+9 · 1 hour ahead of you",
            ),
            (
                "Tokyo time",
                "11:00 Mon 28 Sep · Tokyo",
                "UTC+9 · 1 hour ahead of you",
            ),
            (
                "now in london",
                "03:00 Mon 28 Sep · London",
                "UTC+1 · 7 hours behind you",
            ),
            (
                "time in New York",
                "22:00 Sun 27 Sep · New York",
                "UTC-4 · 12 hours behind you",
            ),
            (
                "time at sao paulo",
                "23:00 Sun 27 Sep · Sao Paulo",
                "UTC-3 · 11 hours behind you",
            ),
            (
                "taipei time",
                "10:00 Mon 28 Sep · Taipei",
                "UTC+8 · same time as you",
            ),
            (
                "time in asia/kolkata",
                "07:30 Mon 28 Sep · Kolkata",
                "UTC+5:30 · 2.5 hours behind you",
            ),
        ]);
    }

    #[test]
    fn knows_aliases_and_abbreviations() {
        check(&[
            (
                "time in nyc",
                "22:00 Sun 27 Sep · NYC",
                "UTC-4 · 12 hours behind you",
            ),
            (
                "san francisco time",
                "19:00 Sun 27 Sep · San Francisco",
                "UTC-7 · 15 hours behind you",
            ),
            (
                "time in delhi",
                "07:30 Mon 28 Sep · Delhi",
                "UTC+5:30 · 2.5 hours behind you",
            ),
            (
                "beijing time",
                "10:00 Mon 28 Sep · Beijing",
                "UTC+8 · same time as you",
            ),
            (
                "time in utc",
                "02:00 Mon 28 Sep · UTC",
                "UTC · 8 hours behind you",
            ),
            // Abbreviations follow daylight saving time.
            (
                "time in pst",
                "19:00 Sun 27 Sep · PDT",
                "UTC-7 · 15 hours behind you",
            ),
            (
                "EST time",
                "22:00 Sun 27 Sep · EDT",
                "UTC-4 · 12 hours behind you",
            ),
            (
                "time in cet",
                "04:00 Mon 28 Sep · CEST",
                "UTC+2 · 6 hours behind you",
            ),
            (
                "time in ist",
                "07:30 Mon 28 Sep · IST",
                "UTC+5:30 · 2.5 hours behind you",
            ),
        ]);
    }

    #[test]
    fn converts_times_between_places() {
        check(&[
            (
                "3pm tokyo in taipei",
                "14:00 Mon 28 Sep · Taipei",
                "UTC+8 · 1 hour behind Tokyo",
            ),
            (
                "15:30 utc to pst",
                "08:30 Mon 28 Sep · PDT",
                "UTC-7 · 7 hours behind UTC",
            ),
            (
                "9am new york to berlin",
                "15:00 Mon 28 Sep · Berlin",
                "UTC+2 · 6 hours ahead of New York",
            ),
            (
                "11pm new york to tokyo",
                "12:00 Tue 29 Sep · Tokyo",
                "UTC+9 · 13 hours ahead of New York",
            ),
            (
                "3 pm london to tokyo",
                "23:00 Mon 28 Sep · Tokyo",
                "UTC+9 · 8 hours ahead of London",
            ),
            (
                "noon utc in tokyo",
                "21:00 Mon 28 Sep · Tokyo",
                "UTC+9 · 9 hours ahead of UTC",
            ),
            (
                "12am utc to taipei",
                "08:00 Mon 28 Sep · Taipei",
                "UTC+8 · 8 hours ahead of UTC",
            ),
            (
                "3:30pm jst to delhi",
                "12:00 Mon 28 Sep · Delhi",
                "UTC+5:30 · 3.5 hours behind JST",
            ),
        ]);
    }

    #[test]
    fn a_missing_place_is_yours() {
        check(&[
            (
                "3pm tokyo",
                "14:00 Mon 28 Sep",
                "UTC+8 · 1 hour behind Tokyo",
            ),
            (
                "3pm in tokyo",
                "16:00 Mon 28 Sep · Tokyo",
                "UTC+9 · 1 hour ahead of you",
            ),
            (
                "9:00 london",
                "16:00 Mon 28 Sep",
                "UTC+8 · 7 hours ahead of London",
            ),
        ]);
    }

    #[test]
    fn moves_dates() {
        check(&[
            ("today + 90 days", "Sun 27 Dec 2026", "in 90 days"),
            ("today+90 days", "Sun 27 Dec 2026", "in 90 days"),
            ("today + 3 weeks", "Mon 19 Oct 2026", "in 21 days"),
            ("2026-10-01 + 45 days", "Sun 15 Nov 2026", "in 48 days"),
            ("today - 1 month", "Fri 28 Aug 2026", "31 days ago"),
            ("tomorrow + 1 year", "Wed 29 Sep 2027", "in 366 days"),
            ("today + 1 week + 2 days", "Wed 7 Oct 2026", "in 9 days"),
            ("yesterday + 2 days", "Tue 29 Sep 2026", "tomorrow"),
            ("today + 0 days", "Mon 28 Sep 2026", "today"),
        ]);
        let item = answer("today + 90 days", &now()).unwrap();
        assert_eq!(item.action, Action::Copy("Sun 27 Dec 2026".into()));
        assert_eq!(item.alt, Some(Action::Copy("2026-12-27".into())));
    }

    #[test]
    fn counts_days() {
        check(&[
            (
                "days until 2026-12-25",
                "88 days",
                "Mon 28 Sep 2026 to Fri 25 Dec 2026 · 12 weeks and 4 days",
            ),
            (
                "2026-12-25 - today",
                "88 days",
                "Mon 28 Sep 2026 to Fri 25 Dec 2026 · 12 weeks and 4 days",
            ),
            (
                "days since 2026-01-01",
                "270 days",
                "Thu 1 Jan 2026 to Mon 28 Sep 2026 · 38 weeks and 4 days",
            ),
            (
                "today - 2026-12-25",
                "-88 days",
                "Fri 25 Dec 2026 to Mon 28 Sep 2026 · 12 weeks and 4 days",
            ),
            (
                "days until tomorrow",
                "1 day",
                "Mon 28 Sep 2026 to Tue 29 Sep 2026",
            ),
            (
                "days until 2026-10-05",
                "7 days",
                "Mon 28 Sep 2026 to Mon 5 Oct 2026 · 1 week",
            ),
        ]);
        let item = answer("days until 2026-12-25", &now()).unwrap();
        assert_eq!(item.alt, Some(Action::Copy("88".into())));
    }

    #[test]
    fn copies_times_with_their_offset() {
        let item = answer("3pm tokyo in taipei", &now()).unwrap();
        assert_eq!(
            item.action,
            Action::Copy("14:00 Mon 28 Sep · Taipei".into())
        );
        assert_eq!(
            item.alt,
            Some(Action::Copy("2026-09-28T14:00+08:00".into()))
        );
    }

    #[test]
    fn leaves_file_searches_alone() {
        for query in [
            "tokyo trip photos",
            "tokyo",
            "new york",
            "london.pdf",
            "time",
            "time in",
            "time machine",
            "screen time",
            "2026-09-25",
            "2026-09-25 notes",
            "report 2024",
            "today",
            "today notes",
            "days",
            "3 days",
            "3pm",
            "15:30",
            "15 tokyo",
            "3pm meeting notes",
            "9am standup",
            "25:00 utc to tokyo",
            "13pm tokyo",
            "today + 3 apples",
            "today + 3",
            "c++ notes",
            "days until christmas",
            "time in ../../etc/passwd",
        ] {
            assert_eq!(answered(query), None, "{query}");
        }
    }

    #[test]
    fn reads_times_of_day() {
        let t = |h, m| Some(Time::constant(h, m, 0, 0));
        assert_eq!(time_of_day("12", Some("am")), t(0, 0));
        assert_eq!(time_of_day("12", Some("pm")), t(12, 0));
        assert_eq!(time_of_day("9:05", Some("am")), t(9, 5));
        assert_eq!(time_of_day("23:59", None), t(23, 59));
        assert_eq!(time_of_day("0", Some("am")), None);
        assert_eq!(time_of_day("9:5", None), None);
        assert_eq!(time_of_day("24:00", None), None);
    }
}
