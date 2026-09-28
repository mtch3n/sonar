//! Settings a plugin declares in `plugin.toml`. Sonar draws a form for them in the
//! Settings window, keeps the values in `settings.toml` and sends them to the plugin
//! with every query.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Keys `settings.toml` already uses under `[plugins.<id>]`.
const RESERVED: [&str; 2] = ["enabled", "keyword"];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    pub key: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(flatten)]
    pub field: Field,
}

/// What kind of value a setting holds, and how the form shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Field {
    Text {
        default: String,
        placeholder: Option<String>,
    },
    Number {
        default: f64,
        min: Option<f64>,
        max: Option<f64>,
    },
    Toggle {
        default: bool,
    },
    Choice {
        default: String,
        options: Vec<Choice>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub value: String,
    pub title: String,
}

/// A setting as `plugin.toml` writes it, before it is checked.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Raw {
    key: String,
    title: String,
    description: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    default: Option<toml::Value>,
    placeholder: Option<String>,
    min: Option<f64>,
    max: Option<f64>,
    options: Option<Vec<Choice>>,
}

impl Setting {
    /// Checks the settings a plugin declares, in order.
    pub(crate) fn from_raw(raw: Vec<Raw>) -> Result<Vec<Setting>, String> {
        let mut settings: Vec<Setting> = Vec::with_capacity(raw.len());
        for raw in raw {
            let setting = Setting::check_raw(raw)?;
            if settings.iter().any(|s| s.key == setting.key) {
                return Err(format!("setting `{}` is declared twice", setting.key));
            }
            settings.push(setting);
        }
        Ok(settings)
    }

    fn check_raw(raw: Raw) -> Result<Setting, String> {
        let key = raw.key;
        let valid_key = !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !valid_key {
            return Err(format!(
                "setting key `{key}` may only use lowercase letters, digits, `-` and `_`"
            ));
        }
        if RESERVED.contains(&key.as_str()) {
            return Err(format!("setting key `{key}` is used by Sonar itself"));
        }
        let unused = |name: &str, present: bool| {
            if present {
                Err(format!(
                    "setting `{key}`: `{name}` doesn't apply to type `{}`",
                    raw.kind
                ))
            } else {
                Ok(())
            }
        };
        let wrong_default = || format!("setting `{key}`: `default` doesn't match its type");
        let field = match raw.kind.as_str() {
            "text" => {
                unused("min", raw.min.is_some())?;
                unused("max", raw.max.is_some())?;
                unused("options", raw.options.is_some())?;
                let default = match raw.default {
                    None => String::new(),
                    Some(toml::Value::String(text)) => text,
                    Some(_) => return Err(wrong_default()),
                };
                Field::Text {
                    default,
                    placeholder: raw.placeholder,
                }
            }
            "number" => {
                unused("placeholder", raw.placeholder.is_some())?;
                unused("options", raw.options.is_some())?;
                let default = match raw.default {
                    None => raw.min.unwrap_or(0.0),
                    Some(toml::Value::Integer(n)) => n as f64,
                    Some(toml::Value::Float(n)) => n,
                    Some(_) => return Err(wrong_default()),
                };
                if let (Some(min), Some(max)) = (raw.min, raw.max)
                    && min > max
                {
                    return Err(format!("setting `{key}`: `min` is above `max`"));
                }
                let field = Field::Number {
                    default,
                    min: raw.min,
                    max: raw.max,
                };
                field
                    .check(&field.default())
                    .map_err(|err| format!("setting `{key}`: the default {err}"))?;
                field
            }
            "toggle" => {
                unused("placeholder", raw.placeholder.is_some())?;
                unused("min", raw.min.is_some())?;
                unused("max", raw.max.is_some())?;
                unused("options", raw.options.is_some())?;
                let default = match raw.default {
                    None => false,
                    Some(toml::Value::Boolean(on)) => on,
                    Some(_) => return Err(wrong_default()),
                };
                Field::Toggle { default }
            }
            "choice" => {
                unused("placeholder", raw.placeholder.is_some())?;
                unused("min", raw.min.is_some())?;
                unused("max", raw.max.is_some())?;
                let options = raw
                    .options
                    .filter(|options| !options.is_empty())
                    .ok_or_else(|| format!("setting `{key}` needs `options`"))?;
                let default = match raw.default {
                    None => options[0].value.clone(),
                    Some(toml::Value::String(value)) => value,
                    Some(_) => return Err(wrong_default()),
                };
                if !options.iter().any(|o| o.value == default) {
                    return Err(format!(
                        "setting `{key}`: the default `{default}` isn't one of its options"
                    ));
                }
                Field::Choice { default, options }
            }
            other => {
                return Err(format!(
                    "setting `{key}` has type `{other}`; use text, number, toggle or choice"
                ));
            }
        };
        Ok(Setting {
            key,
            title: raw.title,
            description: raw.description,
            field,
        })
    }
}

impl Field {
    pub fn default(&self) -> Value {
        match self {
            Field::Text { default, .. } | Field::Choice { default, .. } => {
                Value::String(default.clone())
            }
            Field::Number { default, .. } => number(*default),
            Field::Toggle { default } => Value::Bool(*default),
        }
    }

    /// Why `value` can't be used for this field, completing "`key` ...".
    pub fn check(&self, value: &Value) -> Result<(), String> {
        match (self, value) {
            (Field::Text { .. }, Value::String(_)) | (Field::Toggle { .. }, Value::Bool(_)) => {
                Ok(())
            }
            (Field::Number { min, max, .. }, Value::Number(n)) => {
                let n = n.as_f64().unwrap_or(f64::NAN);
                let low = min.is_some_and(|min| n < min);
                let high = max.is_some_and(|max| n > max);
                match (min, max) {
                    (Some(min), Some(max)) if low || high => {
                        Err(format!("is {n}; use {min} to {max}"))
                    }
                    (Some(min), _) if low => Err(format!("is {n}; use {min} or more")),
                    (_, Some(max)) if high => Err(format!("is {n}; use {max} or less")),
                    _ => Ok(()),
                }
            }
            (Field::Choice { options, .. }, Value::String(value)) => {
                if options.iter().any(|o| &o.value == value) {
                    Ok(())
                } else {
                    let values: Vec<&str> = options.iter().map(|o| o.value.as_str()).collect();
                    Err(format!("is `{value}`; use one of {}", values.join(", ")))
                }
            }
            (Field::Text { .. } | Field::Choice { .. }, _) => Err("must be text".into()),
            (Field::Number { .. }, _) => Err("must be a number".into()),
            (Field::Toggle { .. }, _) => Err("must be true or false".into()),
        }
    }
}

/// Every setting's value: the one in `values` when it is usable, otherwise the
/// default. Values that can't be used, and values for settings the plugin doesn't
/// declare, are described in the problems.
pub fn resolve(
    settings: &[Setting],
    values: &Map<String, Value>,
) -> (Map<String, Value>, Vec<String>) {
    let mut resolved = Map::new();
    let mut problems = Vec::new();
    for setting in settings {
        let value = match values.get(&setting.key) {
            Some(value) => match setting.field.check(value) {
                Ok(()) => value.clone(),
                Err(err) => {
                    problems.push(format!("`{}` {err}", setting.key));
                    setting.field.default()
                }
            },
            None => setting.field.default(),
        };
        resolved.insert(setting.key.clone(), value);
    }
    for key in values.keys() {
        if !settings.iter().any(|s| &s.key == key) {
            problems.push(format!("there's no setting `{key}`"));
        }
    }
    (resolved, problems)
}

/// Whole numbers stay integers, so plugins and `settings.toml` see `5`, not `5.0`.
fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 2f64.powi(53) {
        Value::from(n as i64)
    } else {
        Value::from(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(text: &str) -> Result<Vec<Setting>, String> {
        #[derive(Deserialize)]
        struct File {
            settings: Vec<Raw>,
        }
        let file: File = toml::from_str(text).map_err(|err| err.message().to_owned())?;
        Setting::from_raw(file.settings)
    }

    const DECLARED: &str = r#"
        [[settings]]
        key = "engine"
        title = "Search engine"
        type = "choice"
        options = [{ value = "google", title = "Google" }, { value = "ddg", title = "DuckDuckGo" }]

        [[settings]]
        key = "results"
        title = "Results"
        type = "number"
        default = 5
        min = 1
        max = 20

        [[settings]]
        key = "safe"
        title = "Safe search"
        type = "toggle"
        default = true

        [[settings]]
        key = "region"
        title = "Region"
        type = "text"
    "#;

    #[test]
    fn declares_and_resolves_values() {
        let settings = parse(DECLARED).unwrap();
        let values = json!({"engine": "ddg", "results": 50, "extra": 1});
        let (resolved, problems) = resolve(&settings, values.as_object().unwrap());
        assert_eq!(
            Value::Object(resolved),
            json!({"engine": "ddg", "results": 5, "safe": true, "region": ""})
        );
        assert_eq!(
            problems,
            ["`results` is 50; use 1 to 20", "there's no setting `extra`"]
        );
    }

    #[test]
    fn serializes_for_the_settings_window() {
        let settings = parse(DECLARED).unwrap();
        assert_eq!(
            serde_json::to_value(&settings[1]).unwrap(),
            json!({"key": "results", "title": "Results", "description": null, "type": "number", "default": 5.0, "min": 1.0, "max": 20.0})
        );
    }

    #[test]
    fn rejects_bad_declarations() {
        for (text, says) in [
            (
                "key = \"enabled\"\ntitle = \"x\"\ntype = \"toggle\"",
                "used by Sonar",
            ),
            ("key = \"A\"\ntitle = \"x\"\ntype = \"toggle\"", "lowercase"),
            (
                "key = \"a\"\ntitle = \"x\"\ntype = \"color\"",
                "type `color`",
            ),
            (
                "key = \"a\"\ntitle = \"x\"\ntype = \"toggle\"\ndefault = \"yes\"",
                "match its type",
            ),
            (
                "key = \"a\"\ntitle = \"x\"\ntype = \"choice\"",
                "needs `options`",
            ),
            (
                "key = \"a\"\ntitle = \"x\"\ntype = \"choice\"\ndefault = \"c\"\noptions = [{ value = \"b\", title = \"B\" }]",
                "isn't one of",
            ),
            (
                "key = \"a\"\ntitle = \"x\"\ntype = \"number\"\ndefault = 0\nmin = 1",
                "the default is 0",
            ),
            (
                "key = \"a\"\ntitle = \"x\"\ntype = \"text\"\nmin = 1",
                "doesn't apply",
            ),
        ] {
            let err = parse(&format!("[[settings]]\n{text}")).unwrap_err();
            assert!(err.contains(says), "{text}: {err}");
        }
        let twice = "[[settings]]\nkey = \"a\"\ntitle = \"x\"\ntype = \"toggle\"\n";
        assert!(parse(&twice.repeat(2)).unwrap_err().contains("twice"));
    }
}
