//! Exchange rates for the calculator, and the currency people think in.

use std::{collections::HashMap, fs, path::Path, str::FromStr};

use iso_currency::{Currency, IntoEnumIterator};
use serde::{Deserialize, Serialize};

/// Who publishes the rates. Their terms ask for credit wherever rates are shown.
pub const SOURCE: &str = "ExchangeRate-API";
const URL: &str = "https://open.er-api.com/v6/latest/USD";
const USER_AGENT: &str = concat!("sonar/", env!("CARGO_PKG_VERSION"));

/// One day's exchange rates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rates {
    /// When these rates were published, in Unix seconds.
    pub published: i64,
    /// When the next ones are due.
    pub next: i64,
    /// How much of each currency, by ISO code, one US dollar buys.
    pub per_usd: HashMap<String, f64>,
}

#[derive(Deserialize)]
struct Answer {
    result: String,
    #[serde(default, rename = "error-type")]
    error: Option<String>,
    #[serde(default)]
    time_last_update_unix: i64,
    #[serde(default)]
    time_next_update_unix: i64,
    #[serde(default)]
    rates: HashMap<String, f64>,
}

impl Rates {
    /// The latest rates. Blocks while they download, so call it off the search path.
    pub fn download() -> Result<Rates, String> {
        let text = ureq::get(URL)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|err| format!("couldn't download exchange rates: {err}"))?
            .into_body()
            .with_config()
            .limit(256 * 1024)
            .read_to_string()
            .map_err(|err| format!("couldn't download exchange rates: {err}"))?;
        Rates::parse(&text)
    }

    fn parse(text: &str) -> Result<Rates, String> {
        let answer: Answer = serde_json::from_str(text)
            .map_err(|err| format!("{SOURCE} sent rates Sonar can't read: {err}"))?;
        if answer.result != "success" || answer.rates.is_empty() {
            let why = answer.error.unwrap_or(answer.result);
            return Err(format!("{SOURCE} sent no rates ({why})"));
        }
        Ok(Rates {
            published: answer.time_last_update_unix,
            next: answer.time_next_update_unix,
            per_usd: answer.rates,
        })
    }

    /// The rates saved by [`Rates::save`], if there are any.
    pub fn load(path: &Path) -> Option<Rates> {
        serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        }
        let text = serde_json::to_string(self).map_err(|err| err.to_string())?;
        // Written aside and moved into place, so a crash never leaves half a file.
        let partial = path.with_extension("partial");
        fs::write(&partial, text).map_err(|err| err.to_string())?;
        fs::rename(&partial, path).map_err(|err| err.to_string())
    }

    /// Whether newer rates should be out by `now`.
    pub fn is_due(&self, now: i64) -> bool {
        now >= self.next
    }
}

/// Every currency in use, as `(code, name)`, sorted by name.
pub fn currencies() -> Vec<(&'static str, String)> {
    let mut all: Vec<(&'static str, String)> = Currency::iter()
        .filter(|c| !c.is_fund() && !c.is_special() && c.is_superseded().is_none())
        .map(|c| (c.code(), c.name().to_owned()))
        .collect();
    all.sort_by(|a, b| a.1.cmp(&b.1));
    all
}

/// The currency of the country in the system's locale, like `TWD` for `zh-TW`, or
/// `USD` when the locale names no country.
pub fn local_currency() -> String {
    sys_locale::get_locale()
        .and_then(|locale| currency_of_locale(&locale))
        .unwrap_or_else(|| "USD".to_owned())
}

fn currency_of_locale(locale: &str) -> Option<String> {
    // `en-US`, `en_US.UTF-8`, `zh-Hant-TW`, `sr_RS@latin`: the region is the part
    // with two capital letters.
    let locale = locale.split(['.', '@']).next()?;
    let region = locale
        .split(['-', '_'])
        .skip(1)
        .find(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_uppercase()))?;
    let country = iso_country::Country::from_str(region).ok()?;
    Currency::iter()
        .filter(|c| !c.is_fund() && !c.is_special() && c.is_superseded().is_none())
        .find(|c| c.used_by().contains(&country))
        .map(|c| c.code().to_owned())
}

/// How many decimals amounts of `code` are written with: 2 for most, 0 for yen.
pub fn decimals(code: &str) -> usize {
    Currency::from_code(code)
        .and_then(|c| c.exponent())
        .map_or(2, usize::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_published_rates() {
        let rates = Rates::parse(
            r#"{"result":"success","time_last_update_unix":10,"time_next_update_unix":20,"base_code":"USD","rates":{"USD":1,"TWD":31.7654}}"#,
        )
        .unwrap();
        assert_eq!(rates.per_usd["TWD"], 31.7654);
        assert!(!rates.is_due(19) && rates.is_due(20));
        let err = Rates::parse(r#"{"result":"error","error-type":"unsupported-code"}"#);
        assert!(err.unwrap_err().contains("unsupported-code"));
    }

    /// Needs the network: `cargo test -p sonar-plugins -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_todays_rates() {
        let rates = Rates::download().unwrap();
        assert!(rates.per_usd["TWD"] > 1.0);
        assert!(rates.next > rates.published);
    }

    #[test]
    fn saves_and_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sonar").join("rates.json");
        assert_eq!(Rates::load(&path), None);
        let rates = Rates {
            published: 1,
            next: 2,
            per_usd: HashMap::from([("EUR".to_owned(), 0.9)]),
        };
        rates.save(&path).unwrap();
        assert_eq!(Rates::load(&path), Some(rates));
    }

    #[test]
    fn finds_the_currency_of_a_locale() {
        for (locale, code) in [
            ("zh-TW", "TWD"),
            ("en_US.UTF-8", "USD"),
            ("de-DE", "EUR"),
            ("zh-Hant-HK", "HKD"),
            ("ja_JP", "JPY"),
        ] {
            assert_eq!(
                currency_of_locale(locale).as_deref(),
                Some(code),
                "{locale}"
            );
        }
        assert_eq!(currency_of_locale("en"), None);
        assert_eq!(currency_of_locale("C"), None);
    }

    #[test]
    fn lists_currencies_in_use() {
        let all = currencies();
        assert!(all.iter().any(|(code, _)| *code == "TWD"));
        assert!(
            !all.iter().any(|(code, _)| *code == "USN"),
            "funds are left out"
        );
        assert_eq!(decimals("JPY"), 0);
        assert_eq!(decimals("USD"), 2);
        assert_eq!(decimals("euros"), 2);
    }
}
