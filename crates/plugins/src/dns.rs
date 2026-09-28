//! Look up a domain's DNS records: `dns example.com`, `dns example.com mx` or
//! `dns example.com @1.1.1.1`, and an address's name with `dns 1.1.1.1`.

use std::{net::IpAddr, time::Duration};

use hickory_resolver::{
    Resolver,
    config::{NameServerConfig, ResolverConfig},
    net::runtime::TokioRuntimeProvider,
    proto::rr::{Name, RecordType},
};
use serde_json::Value;

use crate::{Action, Field, Item, Setting};

/// The DNS plugin's id in `settings.toml`, and its keyword.
pub const ID: &str = "dns";
pub const KEYWORD: &str = "dns";
/// The server to ask instead of the system's.
const SERVER: &str = "server";
/// How long one server may take to answer.
const TIMEOUT: Duration = Duration::from_secs(2);
/// The record types a query can name.
const TYPES: [RecordType; 10] = [
    RecordType::A,
    RecordType::AAAA,
    RecordType::CNAME,
    RecordType::MX,
    RecordType::TXT,
    RecordType::NS,
    RecordType::SOA,
    RecordType::SRV,
    RecordType::CAA,
    RecordType::PTR,
];

pub fn settings() -> Vec<Setting> {
    vec![Setting {
        key: SERVER.into(),
        title: "DNS server".into(),
        description: Some(
            "The address of the server to ask, like 1.1.1.1; empty asks your system's. @1.1.1.1 in a search asks another".into(),
        ),
        field: Field::Text {
            default: String::new(),
            placeholder: Some("Your system's".into()),
        },
    }]
}

/// One lookup, as a query asks for it.
#[derive(Debug, PartialEq)]
struct Ask {
    name: String,
    types: Vec<RecordType>,
    /// `None` asks the system's servers.
    server: Option<IpAddr>,
}

/// Reads `query`; `None` when it doesn't name a domain yet.
fn ask(query: &str, server: &str) -> Result<Option<Ask>, String> {
    let mut name = None;
    let mut types = Vec::new();
    let mut server = server.trim();
    for word in query.split_whitespace() {
        if let Some(at) = word.strip_prefix('@') {
            server = at;
        } else if let Some(kind) = TYPES
            .iter()
            .find(|t| t.to_string().eq_ignore_ascii_case(word))
        {
            types.push(*kind);
        } else if name.is_none() {
            name = Some(word.trim_end_matches('.').to_owned());
        } else {
            return Err(format!("`{word}` isn't a record type or @server"));
        }
    }
    let server = match server {
        "" => None,
        text => Some(
            text.parse::<IpAddr>()
                .map_err(|_| format!("the DNS server `{text}` isn't an IP address"))?,
        ),
    };
    let Some(mut name) = name else {
        return Ok(None);
    };
    // An address is looked up by its reverse name.
    if let Ok(ip) = name.parse::<IpAddr>() {
        name = Name::from(ip).to_string();
        types = vec![RecordType::PTR];
    }
    if types.is_empty() {
        types = vec![RecordType::A, RecordType::AAAA];
    }
    Ok(Some(Ask {
        name,
        types,
        server,
    }))
}

/// One record found.
#[derive(Debug, PartialEq)]
struct Found {
    kind: RecordType,
    name: String,
    value: String,
    ttl: u32,
}

/// Runs the DNS plugin, which `sonar-app --plugin dns` does.
pub fn serve() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("starting the DNS plugin's runtime");
    crate::serve(|query, settings| {
        let server = settings
            .get(SERVER)
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(ask) = ask(query, server)? else {
            return Ok(Vec::new());
        };
        let found = runtime.block_on(look_up(&ask))?;
        answer(&ask, found)
    });
}

async fn look_up(ask: &Ask) -> Result<Vec<Found>, String> {
    let provider = TokioRuntimeProvider::default();
    let mut builder = match ask.server {
        Some(ip) => Resolver::builder_with_config(
            ResolverConfig::from_parts(None, Vec::new(), vec![NameServerConfig::udp_and_tcp(ip)]),
            provider,
        ),
        None => Resolver::builder(provider)
            .map_err(|err| format!("couldn't read your system's DNS settings: {err}"))?,
    };
    let options = builder.options_mut();
    options.timeout = TIMEOUT;
    options.attempts = 1;
    let resolver = builder.build().map_err(|err| err.to_string())?;
    let lookups = ask.types.iter().map(|kind| {
        let resolver = resolver.clone();
        let (name, kind) = (ask.name.clone(), *kind);
        tokio::spawn(async move { (kind, resolver.lookup(name, kind).await) })
    });
    let mut found = Vec::new();
    for lookup in lookups.collect::<Vec<_>>() {
        let (kind, lookup) = lookup.await.map_err(|err| err.to_string())?;
        match lookup {
            Ok(lookup) => found.extend(lookup.answers().iter().map(|record| Found {
                kind: record.record_type(),
                name: record.name.to_string().trim_end_matches('.').to_owned(),
                value: record.data.to_string(),
                ttl: record.ttl,
            })),
            Err(err) if err.is_no_records_found() => {}
            Err(err) => return Err(format!("couldn't look up {kind}: {err}")),
        }
    }
    // A and AAAA lookups both follow the same CNAMEs.
    let mut seen = Vec::new();
    found.retain(|f| {
        let key = (f.kind, f.name.clone(), f.value.clone());
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    Ok(found)
}

fn answer(ask: &Ask, found: Vec<Found>) -> Result<Vec<Item>, String> {
    let from = ask
        .server
        .map_or_else(|| "your DNS".to_owned(), |ip| ip.to_string());
    if found.is_empty() {
        let types: Vec<String> = ask.types.iter().map(ToString::to_string).collect();
        return Err(format!(
            "{from} has no {} records for {}",
            types.join(" or "),
            ask.name
        ));
    }
    Ok(found
        .into_iter()
        .map(|f| {
            let mut item = Item::new(f.value.clone(), Action::Copy(f.value));
            let mut subtitle = vec![f.kind.to_string()];
            if !f.name.eq_ignore_ascii_case(ask.name.trim_end_matches('.')) {
                subtitle.push(f.name);
            }
            subtitle.push(format!("TTL {}", ttl(f.ttl)));
            subtitle.push(format!("from {from}"));
            item.subtitle = Some(subtitle.join(" · "));
            item.icon = Some("globe".into());
            item
        })
        .collect())
}

fn ttl(seconds: u32) -> String {
    match seconds {
        0..120 => format!("{seconds} s"),
        120..7200 => format!("{} min", seconds / 60),
        7200..172_800 => format!("{} h", seconds / 3600),
        _ => format!("{} d", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_domain_types_and_server() {
        assert_eq!(ask("", "").unwrap(), None);
        assert_eq!(
            ask("example.com", "").unwrap(),
            Some(Ask {
                name: "example.com".into(),
                types: vec![RecordType::A, RecordType::AAAA],
                server: None,
            })
        );
        assert_eq!(
            ask("example.com. MX txt @9.9.9.9", "1.1.1.1").unwrap(),
            Some(Ask {
                name: "example.com".into(),
                types: vec![RecordType::MX, RecordType::TXT],
                server: Some("9.9.9.9".parse().unwrap()),
            })
        );
        let reverse = ask("1.1.1.1", "").unwrap().unwrap();
        assert_eq!(reverse.name, "1.1.1.1.in-addr.arpa.");
        assert_eq!(reverse.types, [RecordType::PTR]);
        assert!(ask("example.com @nope", "").is_err());
        assert!(ask("example.com mxx", "").is_err());
    }

    #[test]
    fn copies_each_record() {
        let ask = ask("www.example.com", "1.1.1.1").unwrap().unwrap();
        let found = vec![
            Found {
                kind: RecordType::CNAME,
                name: "www.example.com".into(),
                value: "example.com.".into(),
                ttl: 3600,
            },
            Found {
                kind: RecordType::A,
                name: "example.com".into(),
                value: "93.184.215.14".into(),
                ttl: 60,
            },
        ];
        let items = answer(&ask, found).unwrap();
        assert_eq!(
            items[0].subtitle.as_deref(),
            Some("CNAME · TTL 60 min · from 1.1.1.1")
        );
        assert_eq!(
            items[1].subtitle.as_deref(),
            Some("A · example.com · TTL 60 s · from 1.1.1.1")
        );
        assert_eq!(items[1].action, Action::Copy("93.184.215.14".into()));
        assert_eq!(
            answer(&ask, Vec::new()).unwrap_err(),
            "1.1.1.1 has no A or AAAA records for www.example.com"
        );
    }

    #[test]
    #[ignore = "needs the network"]
    fn asks_a_real_server() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let ask = ask("one.one.one.one", "1.1.1.1").unwrap().unwrap();
        let found = runtime.block_on(look_up(&ask)).unwrap();
        assert!(found.iter().any(|f| f.value == "1.1.1.1"), "{found:?}");
    }
}
