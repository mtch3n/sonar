use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    Action, Choice, Field, Item, Setting,
    currency::{self, Rates},
};

/// How long one calculation may run before it is abandoned.
const BUDGET: Duration = Duration::from_millis(50);

/// The calculator's id in `settings.toml`, like a plugin's folder name.
pub const ID: &str = "calculator";

/// The calculator's settings keys.
pub const CURRENCY: &str = "currency";
pub const DOWNLOAD_RATES: &str = "rates";

/// Arithmetic, units and currencies, answered as you type.
pub struct Calculator {
    /// What amounts of money are converted to when the query doesn't say, by ISO code.
    pub home: String,
    /// `None` until rates have been downloaded, or when downloading is off.
    pub rates: Option<Arc<Rates>>,
}

/// The settings the calculator declares, like a plugin does in `plugin.toml`.
pub fn settings() -> Vec<Setting> {
    let local = currency::local_currency();
    vec![
        Setting {
            key: CURRENCY.into(),
            title: "Your currency".into(),
            description: Some("Amounts like 100 usd are converted to it".into()),
            field: Field::Choice {
                default: local,
                options: currency::currencies()
                    .into_iter()
                    .map(|(code, name)| Choice {
                        value: code.to_owned(),
                        title: format!("{name} ({code})"),
                    })
                    .collect(),
            },
        },
        Setting {
            key: DOWNLOAD_RATES.into(),
            title: "Download exchange rates".into(),
            description: Some(format!("Once a day, from {}", currency::SOURCE)),
            field: Field::Toggle { default: true },
        },
    ]
}

impl Calculator {
    /// The result of `query` as arithmetic, a unit conversion or a currency
    /// conversion, like `2^10`, `5 km to miles` or `100 usd to eur`. Queries that are
    /// most likely a file search give `None`.
    pub fn calculate(&self, query: &str) -> Option<Item> {
        let query = query.trim();
        if !query.contains(|c: char| c.is_ascii_digit()) || is_date(query) {
            return None;
        }
        let asked_for_rates = Arc::new(AtomicBool::new(false));
        let mut context = fend_core::Context::new();
        context.set_exchange_rate_handler_v2(Lookup {
            rates: self.rates.clone(),
            asked: asked_for_rates.clone(),
        });
        let mut value = evaluate(query, &context);
        let money = asked_for_rates.load(Ordering::Relaxed);
        // Like Spotlight, money with nowhere to go is shown in the home currency.
        if money && !names_a_target(query) {
            let home = evaluate(&format!("({query}) to {}", self.home), &context);
            if !home.is_empty() {
                value = home;
            }
        }
        if value.is_empty() || squash(&value) == squash(query) {
            return None;
        }
        if !money {
            return Some(Item {
                title: value.clone(),
                subtitle: None,
                action: Action::Copy(value),
                alt: None,
            });
        }
        let rates = self.rates.as_ref()?;
        let (amount, unit) = split_amount(&value)?;
        let decimals = currency::decimals(&unit.to_uppercase());
        let title = format!("{} {unit}", grouped(amount, decimals));
        let published = jiff::Timestamp::from_second(rates.published)
            .map(|t| t.to_zoned(jiff::tz::TimeZone::system()).date().to_string())
            .unwrap_or_default();
        Some(Item {
            title: title.clone(),
            subtitle: Some(format!("{} rates of {published}", currency::SOURCE)),
            action: Action::Copy(title),
            alt: Some(Action::Copy(format!("{amount:.decimals$}"))),
        })
    }
}

fn evaluate(query: &str, context: &fend_core::Context) -> String {
    let deadline = Deadline(Instant::now() + BUDGET);
    let result = fend_core::evaluate_preview_with_interrupt(query, context, &deadline);
    result.get_main_result().trim().to_owned()
}

/// Answers fend's questions about exchange rates from memory, never the network, and
/// notes that it was asked: then the query is about money.
struct Lookup {
    rates: Option<Arc<Rates>>,
    asked: Arc<AtomicBool>,
}

impl fend_core::ExchangeRateFnV2 for Lookup {
    fn relative_to_base_currency(
        &self,
        currency: &str,
        _: &fend_core::ExchangeRateFnV2Options,
    ) -> Result<f64, Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.asked.store(true, Ordering::Relaxed);
        self.rates
            .as_ref()
            .and_then(|rates| rates.per_usd.get(currency).copied())
            .ok_or_else(|| format!("no exchange rate for {currency}").into())
    }
}

/// Whether the query says what to convert to, like `100 usd to eur`.
fn names_a_target(query: &str) -> bool {
    query.contains("->")
        || query
            .split_whitespace()
            .any(|word| matches!(word.to_lowercase().as_str(), "to" | "in" | "as"))
}

/// `approx. 87.8356 EUR` as `(87.8356, "EUR")`.
fn split_amount(value: &str) -> Option<(f64, &str)> {
    let value = value.strip_prefix("approx. ").unwrap_or(value);
    let (amount, unit) = value.split_once(' ')?;
    Some((amount.parse().ok()?, unit))
}

/// `3176.5432` with 2 decimals as `3,176.54`.
fn grouped(amount: f64, decimals: usize) -> String {
    let fixed = format!("{:.decimals$}", amount.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut out = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(fraction);
    }
    if amount < 0.0 && out.chars().any(|c| c != '0' && c != '.' && c != ',') {
        out.insert(0, '-');
    }
    out
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

    fn calculator(home: &str) -> Calculator {
        let per_usd = [
            ("USD", 1.0),
            ("EUR", 0.878356),
            ("TWD", 31.7654),
            ("JPY", 149.9),
        ];
        Calculator {
            home: home.to_owned(),
            rates: Some(Arc::new(Rates {
                published: 1_790_553_751,
                next: 1_790_640_541,
                per_usd: per_usd.map(|(c, r)| (c.to_owned(), r)).into(),
            })),
        }
    }

    fn value(query: &str) -> Option<String> {
        calculator("TWD").calculate(query).map(|item| item.title)
    }

    #[test]
    fn calculates() {
        assert_eq!(value("2^10").as_deref(), Some("1024"));
        assert_eq!(value(" 6 * 7 ").as_deref(), Some("42"));
        assert!(value("5 km to miles").is_some_and(|v| v.ends_with("miles")));
        let item = calculator("TWD").calculate("1/4").unwrap();
        assert_eq!(item.action, Action::Copy("0.25".into()));
        assert_eq!(item.subtitle, None);
    }

    #[test]
    fn converts_currencies() {
        assert_eq!(value("100 usd to eur").as_deref(), Some("87.84 EUR"));
        assert_eq!(value("$100 in twd").as_deref(), Some("3,176.54 TWD"));
        assert_eq!(value("1000 twd to jpy").as_deref(), Some("4,719 JPY"));
        assert_eq!(
            value("100 dollars to euros").as_deref(),
            Some("87.84 euros")
        );
        let item = calculator("TWD").calculate("100 usd to eur").unwrap();
        assert!(
            item.subtitle
                .unwrap()
                .starts_with("ExchangeRate-API rates of 2026-09-")
        );
        assert_eq!(item.alt, Some(Action::Copy("87.84".into())));
    }

    #[test]
    fn money_without_a_target_is_shown_in_the_home_currency() {
        assert_eq!(value("100 usd").as_deref(), Some("3,176.54 TWD"));
        assert_eq!(value("10 eur + 5 usd").as_deref(), Some("520.47 TWD"));
        assert_eq!(value("100 twd"), None);
        assert_eq!(calculator("USD").calculate("100 usd"), None);
    }

    #[test]
    fn money_needs_rates() {
        let offline = Calculator {
            home: "TWD".into(),
            rates: None,
        };
        assert_eq!(offline.calculate("100 usd to eur"), None);
        assert!(offline.calculate("5 km to miles").is_some());
    }

    #[test]
    fn groups_thousands() {
        assert_eq!(grouped(3176.5432, 2), "3,176.54");
        assert_eq!(grouped(1_234_567.0, 0), "1,234,567");
        assert_eq!(grouped(999.999, 2), "1,000.00");
        assert_eq!(grouped(-1234.5, 2), "-1,234.50");
        assert_eq!(grouped(-0.001, 2), "0.00");
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
