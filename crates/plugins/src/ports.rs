//! Which program listens on which port: `port 3000` or `port node`, and end it.

use std::{collections::BTreeMap, net::SocketAddr};

use listeners::{Protocol, SocketState};

use crate::{Action, Item, Setting, processes};

/// The ports plugin's id in `settings.toml`, and its keyword.
pub const ID: &str = "ports";
pub const KEYWORD: &str = "port";

/// The most ports shown for one query.
const LIMIT: usize = 50;

pub fn settings() -> Vec<Setting> {
    Vec::new()
}

/// A port a program listens on, on one or more addresses.
#[derive(Clone, Debug, PartialEq)]
struct Open {
    port: u16,
    protocol: &'static str,
    pid: u32,
    name: String,
    addresses: Vec<String>,
}

/// Runs the ports plugin, which `sonar-app --plugin ports` does. The ports are read
/// again for every query.
pub fn serve() {
    crate::serve(|query, _| {
        let all = listeners::get_all().map_err(|err| format!("couldn't list ports: {err}"))?;
        let sockets = all.into_iter().filter_map(|l| {
            let protocol = match (l.protocol, l.state) {
                (Protocol::TCP, SocketState::Listen) => "TCP",
                // UDP has no listening state; a bound port stands in for it.
                (Protocol::UDP, _) if l.socket.port() != 0 => "UDP",
                _ => return None,
            };
            Some((l.process.pid, l.process.name, protocol, l.socket))
        });
        Ok(answer(query, open(sockets)))
    });
}

/// Merges the sockets a program has on one port, like its IPv4 and IPv6 ones.
fn open(sockets: impl Iterator<Item = (u32, String, &'static str, SocketAddr)>) -> Vec<Open> {
    let mut ports: BTreeMap<(u16, &str, u32), Open> = BTreeMap::new();
    for (pid, name, protocol, socket) in sockets {
        let open = ports
            .entry((socket.port(), protocol, pid))
            .or_insert_with(|| Open {
                port: socket.port(),
                protocol,
                pid,
                name,
                addresses: Vec::new(),
            });
        let address = socket.ip().to_string();
        if !open.addresses.contains(&address) {
            open.addresses.push(address);
            open.addresses.sort();
        }
    }
    ports.into_values().collect()
}

/// Ports whose number starts with a number in `query` and whose program's name has
/// every other word, lowest port first.
fn answer(query: &str, open: Vec<Open>) -> Vec<Item> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    open.into_iter()
        .filter(|o| {
            let (port, name) = (o.port.to_string(), o.name.to_lowercase());
            words.iter().all(|w| {
                if w.chars().all(|c| c.is_ascii_digit()) {
                    port.starts_with(w.as_str())
                } else {
                    name.contains(w.as_str()) || o.protocol.eq_ignore_ascii_case(w)
                }
            })
        })
        .take(LIMIT)
        .map(|o| {
            let mut item = Item::new(
                format!("{} · {}", o.port, o.name),
                Action::Run(processes::stop(o.pid, false)),
            );
            item.subtitle = Some(format!(
                "{} on {} · process {}",
                o.protocol,
                reach(&o.addresses),
                o.pid
            ));
            item.icon = Some("port".into());
            item.alt = Some(Action::Run(processes::stop(o.pid, true)));
            item.label = Some("End".into());
            item.alt_label = Some("Force quit".into());
            item
        })
        .collect()
}

/// Who can reach the port, in words where the addresses say it.
fn reach(addresses: &[String]) -> String {
    if addresses.iter().any(|a| a == "0.0.0.0" || a == "::") {
        "every network".into()
    } else if addresses.iter().all(|a| a == "127.0.0.1" || a == "::1") {
        "this computer only".into()
    } else {
        addresses.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sockets() -> Vec<Open> {
        let s = |pid, name: &str, protocol, addr: &str| {
            (
                pid,
                name.to_owned(),
                protocol,
                addr.parse::<SocketAddr>().unwrap(),
            )
        };
        open(
            [
                s(20, "node", "TCP", "127.0.0.1:3000"),
                s(20, "node", "TCP", "[::1]:3000"),
                s(30, "postgres", "TCP", "0.0.0.0:5432"),
                s(30, "postgres", "TCP", "[::]:5432"),
                s(40, "avahi-daemon", "UDP", "0.0.0.0:5353"),
            ]
            .into_iter(),
        )
    }

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn lists_each_programs_ports_once() {
        let items = answer("", sockets());
        assert_eq!(
            titles(&items),
            ["3000 · node", "5353 · avahi-daemon", "5432 · postgres"]
        );
        assert_eq!(
            items[0].subtitle.as_deref(),
            Some("TCP on this computer only · process 20")
        );
        assert_eq!(
            items[2].subtitle.as_deref(),
            Some("TCP on every network · process 30")
        );
    }

    #[test]
    fn finds_ports_by_number_program_or_protocol() {
        assert_eq!(titles(&answer("30", sockets())), ["3000 · node"]);
        assert_eq!(titles(&answer("5", sockets())).len(), 2);
        assert_eq!(titles(&answer("POST", sockets())), ["5432 · postgres"]);
        assert_eq!(titles(&answer("udp", sockets())), ["5353 · avahi-daemon"]);
        assert!(answer("3000 postgres", sockets()).is_empty());
    }

    #[test]
    fn ends_the_program_on_the_port() {
        let item = &answer("3000", sockets())[0];
        assert_eq!(item.action, Action::Run(processes::stop(20, false)));
        assert_eq!(item.alt, Some(Action::Run(processes::stop(20, true))));
    }
}
