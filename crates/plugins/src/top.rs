//! Top: what the computer is busy with, `top` style. A line on the whole system,
//! then running programs as a tree under the ones that started them, the busiest
//! first, with their CPU and memory. `top firefox` narrows it to Firefox and its
//! children. Enter ends a program, as `kill` does.

use std::{collections::HashMap, thread, time::Duration};

use sysinfo::{
    CpuRefreshKind, MemoryRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind,
    System, UpdateKind, Users,
};

use crate::{Action, Item, Setting, processes::stop};

pub const ID: &str = "top";
pub const KEYWORD: &str = "top";

/// Rows shown for one query, the system's line included.
const LIMIT: usize = 50;
/// Programs using less than this share of a core and this much memory, with nothing
/// busier under them, are left out, so the tree shows what matters.
const IDLE_CPU: f32 = 0.5;
const SMALL_MEMORY: u64 = 100 * 1024 * 1024;

/// Programs that start and watch services.
const MANAGERS: [&str; 3] = ["systemd", "init", "launchd"];

pub fn settings() -> Vec<Setting> {
    Vec::new()
}

/// A running program as the tree shows it.
#[derive(Clone, Debug, PartialEq)]
struct Running {
    pid: u32,
    parent: Option<u32>,
    name: String,
    /// Its command line, for telling programs of one name apart.
    command: String,
    user: Option<String>,
    /// Percent of one core; a busy program on several cores has more than 100.
    cpu: f32,
    /// Resident memory, in bytes.
    memory: u64,
    /// Seconds since it started.
    running_for: u64,
}

/// The whole system, for the first line.
#[derive(Clone, Debug, PartialEq)]
struct Overall {
    cpu: f32,
    cores: usize,
    used: u64,
    total: u64,
    swap_used: u64,
    load: Option<f64>,
}

/// Runs the Top plugin, which `sonar-app --plugin top` does. CPU use is measured
/// between two reads, so the first answer waits a moment for the second.
pub fn serve() {
    let mut system = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
            .with_memory(MemoryRefreshKind::everything()),
    );
    let users = Users::new_with_refreshed_list();
    let me = std::process::id();
    let what = ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_user(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet);
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, what);
    thread::sleep(Duration::from_millis(300));
    crate::serve(|query, _| {
        system.refresh_cpu_usage();
        system.refresh_memory();
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, what);
        let running: Vec<Running> = system
            .processes()
            .values()
            .filter(|p| p.pid().as_u32() != me && p.thread_kind().is_none())
            .map(|p| Running {
                pid: p.pid().as_u32(),
                parent: p.parent().map(Pid::as_u32),
                name: name(
                    &p.name().to_string_lossy(),
                    p.cmd().first().map(|c| c.to_string_lossy()).as_deref(),
                ),
                command: p
                    .cmd()
                    .iter()
                    .map(|part| part.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" "),
                user: p
                    .user_id()
                    .and_then(|id| users.get_user_by_id(id))
                    .map(|u| u.name().to_owned()),
                cpu: p.cpu_usage(),
                memory: p.memory(),
                running_for: p.run_time(),
            })
            .collect();
        let overall = Overall {
            cpu: system.global_cpu_usage(),
            cores: system.cpus().len(),
            used: system.used_memory(),
            total: system.total_memory(),
            swap_used: system.used_swap(),
            load: (!cfg!(windows)).then(|| System::load_average().one),
        };
        Ok(answer(query, &overall, running))
    });
}

fn answer(query: &str, overall: &Overall, running: Vec<Running>) -> Vec<Item> {
    let mut items = vec![summary(overall)];
    for (prefix, program, tree) in rows(query, running).into_iter().take(LIMIT - 1) {
        let mut item = Item::new(
            format!("{prefix}{}", program.name),
            Action::Run(stop(program.pid, false)),
        );
        let mut details = vec![
            format!("{:.1}% CPU", program.cpu),
            size(program.memory),
            format!("PID {}", program.pid),
        ];
        if let Some(user) = &program.user {
            details.push(user.clone());
        }
        details.push(span(program.running_for));
        if let Some((cpu, memory, count)) = tree {
            details.push(format!(
                "with {count} under it: {cpu:.1}% CPU, {}",
                size(memory)
            ));
        }
        if !program.command.is_empty() && program.command != program.name {
            details.push(program.command.clone());
        }
        item.subtitle = Some(details.join(" · "));
        item.icon = Some("process".into());
        item.alt = Some(Action::Run(stop(program.pid, true)));
        item.label = Some("End".into());
        item.alt_label = Some("Force quit".into());
        items.push(item);
    }
    items
}

fn summary(o: &Overall) -> Item {
    let mut parts = vec![
        format!("CPU {:.0}% of {} cores", o.cpu, o.cores),
        format!("memory {} of {}", size(o.used), size(o.total)),
    ];
    if o.swap_used > 0 {
        parts.push(format!("swap {}", size(o.swap_used)));
    }
    if let Some(load) = o.load {
        parts.push(format!("load {load:.2}"));
    }
    let text = parts.join(" · ");
    let mut item = Item::new(capitalize(&text), Action::Copy(text));
    item.subtitle =
        Some("Programs below are grouped under the ones that started them, busiest first".into());
    item.icon = Some("process".into());
    item.label = Some("Copy".into());
    item
}

/// A tree row: the lines drawing its place in the tree, the program, and for one
/// with programs under it, their CPU, memory and count together with its own.
type Row = (String, Running, Option<(f32, u64, usize)>);

/// The programs to show, in tree order: each after the one that started it, the
/// busiest subtree first. With a query, only programs whose name or command has
/// every word, the ones that started them and the ones under them.
fn rows(query: &str, running: Vec<Running>) -> Vec<Row> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let by_pid: HashMap<u32, Running> = running.into_iter().map(|p| (p.pid, p)).collect();
    // Service managers start nearly everything, so the programs they started are
    // the top of the tree and they aren't shown.
    let manager = |pid: u32| {
        by_pid
            .get(&pid)
            .is_some_and(|p| MANAGERS.contains(&p.name.as_str()))
    };
    let by_pid: HashMap<u32, Running> = by_pid
        .iter()
        .filter(|(pid, _)| !manager(**pid))
        .map(|(pid, p)| (*pid, p.clone()))
        .collect();
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut roots = Vec::new();
    for program in by_pid.values() {
        match program.parent.filter(|parent| by_pid.contains_key(parent)) {
            Some(parent) => children.entry(parent).or_default().push(program.pid),
            None => roots.push(program.pid),
        }
    }

    // What each subtree uses altogether.
    let mut totals: HashMap<u32, (f32, u64, usize)> = HashMap::new();
    fn total(
        pid: u32,
        by_pid: &HashMap<u32, Running>,
        children: &HashMap<u32, Vec<u32>>,
        totals: &mut HashMap<u32, (f32, u64, usize)>,
    ) -> (f32, u64, usize) {
        let own = &by_pid[&pid];
        let mut sum = (own.cpu, own.memory, 0);
        for &child in children.get(&pid).map_or(&[][..], Vec::as_slice) {
            let (cpu, memory, count) = total(child, by_pid, children, totals);
            sum = (sum.0 + cpu, sum.1 + memory, sum.2 + count + 1);
        }
        totals.insert(pid, sum);
        sum
    }
    for &root in &roots {
        total(root, &by_pid, &children, &mut totals);
    }

    let matches = |p: &Running| {
        let text = format!("{} {}", p.name, p.command).to_lowercase();
        words.iter().all(|w| text.contains(w.as_str()))
            || words.iter().all(|w| p.pid.to_string() == *w)
    };
    // With a query, a program is shown if it matches, is under one that does, or has
    // one under it that does.
    let mut shown: HashMap<u32, bool> = HashMap::new();
    fn mark(
        pid: u32,
        inside: bool,
        by_pid: &HashMap<u32, Running>,
        children: &HashMap<u32, Vec<u32>>,
        matches: &dyn Fn(&Running) -> bool,
        shown: &mut HashMap<u32, bool>,
    ) -> bool {
        let inside = inside || matches(&by_pid[&pid]);
        let mut any = inside;
        for &child in children.get(&pid).map_or(&[][..], Vec::as_slice) {
            any |= mark(child, inside, by_pid, children, matches, shown);
        }
        shown.insert(pid, any);
        any
    }
    let busy = |pid: u32| {
        let (cpu, memory, _) = totals[&pid];
        cpu >= IDLE_CPU || memory >= SMALL_MEMORY
    };
    if words.is_empty() {
        for &pid in by_pid.keys() {
            shown.insert(pid, busy(pid));
        }
    } else {
        for &root in &roots {
            mark(root, false, &by_pid, &children, &matches, &mut shown);
        }
    }

    let order = |pids: &mut Vec<u32>| {
        pids.retain(|pid| shown[pid]);
        pids.sort_by(|a, b| {
            let (ta, tb) = (totals[a], totals[b]);
            tb.0.total_cmp(&ta.0).then(tb.1.cmp(&ta.1)).then(a.cmp(b))
        });
    };
    let tree = Tree {
        by_pid: &by_pid,
        children: &children,
        totals: &totals,
        order: &order,
    };
    let mut out = Vec::new();
    order(&mut roots);
    for root in roots {
        tree.walk(root, "", None, &mut out);
    }
    out
}

/// The programs and how they're related, for drawing the tree.
struct Tree<'a> {
    by_pid: &'a HashMap<u32, Running>,
    children: &'a HashMap<u32, Vec<u32>>,
    totals: &'a HashMap<u32, (f32, u64, usize)>,
    /// Leaves out what isn't shown and puts the busiest first.
    order: &'a dyn Fn(&mut Vec<u32>),
}

impl Tree<'_> {
    /// Adds program `pid` and those under it. `lines` are the lines drawn for the
    /// programs above it, and `last` says whether it's the last of its parent's,
    /// or `None` at the top.
    fn walk(&self, pid: u32, lines: &str, last: Option<bool>, out: &mut Vec<Row>) {
        if out.len() >= LIMIT {
            return;
        }
        let prefix = match last {
            None => String::new(),
            Some(true) => format!("{lines}└ "),
            Some(false) => format!("{lines}├ "),
        };
        let mut kids = self.children.get(&pid).cloned().unwrap_or_default();
        (self.order)(&mut kids);
        let totals = self.totals[&pid];
        out.push((
            prefix,
            self.by_pid[&pid].clone(),
            (totals.2 > 0).then_some(totals),
        ));
        let below = match last {
            None => String::new(),
            Some(true) => format!("{lines}   "),
            Some(false) => format!("{lines}│  "),
        };
        let count = kids.len();
        for (i, kid) in kids.into_iter().enumerate() {
            self.walk(kid, &below, Some(i + 1 == count), out);
        }
    }
}

/// A program's name. Linux cuts names at 15 characters, so a name that long is
/// taken whole from the program's path when that starts with it.
fn name(short: &str, program: Option<&str>) -> String {
    let whole = program
        .and_then(|p| p.rsplit(['/', '\\']).next())
        .filter(|whole| short.len() == 15 && whole.starts_with(short));
    whole.unwrap_or(short).to_owned()
}

fn size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

/// How long a program has run, like "3d 4h" or "12m".
fn span(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60);
    match (days, hours, minutes) {
        (0, 0, 0) => format!("{seconds}s"),
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(pid: u32, parent: Option<u32>, name: &str, cpu: f32, mb: u64) -> Running {
        Running {
            pid,
            parent,
            name: name.into(),
            command: format!("/usr/bin/{name}"),
            user: Some("me".into()),
            cpu,
            memory: mb * 1024 * 1024,
            running_for: 3700,
        }
    }

    fn running() -> Vec<Running> {
        vec![
            program(1, None, "systemd", 0.0, 10),
            program(2, Some(1), "gnome-shell", 4.0, 400),
            program(3, Some(2), "firefox", 20.0, 900),
            program(4, Some(3), "Web Content", 30.0, 300),
            program(5, Some(3), "Isolated Web", 0.1, 5),
            program(6, Some(1), "sleepy", 0.0, 2),
            program(7, Some(1), "cargo", 90.0, 200),
        ]
    }

    fn titles(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|(prefix, p, _)| format!("{prefix}{}", p.name))
            .collect()
    }

    #[test]
    fn busiest_trees_first_and_idle_ones_left_out() {
        let rows = rows("", running());
        assert_eq!(
            titles(&rows),
            ["cargo", "gnome-shell", "└ firefox", "   └ Web Content"]
        );
        let (_, _, firefox) = &rows[2];
        let (cpu, mb, count) = firefox.unwrap();
        assert_eq!((cpu, mb / 1024 / 1024, count), (50.1, 1205, 2));
    }

    #[test]
    fn a_query_keeps_matches_their_parents_and_children() {
        let found = rows("firefox", running());
        assert_eq!(
            titles(&found),
            [
                "gnome-shell",
                "└ firefox",
                "   ├ Web Content",
                "   └ Isolated Web",
            ]
        );
        assert_eq!(titles(&rows("7", running())), ["cargo"]);
        assert!(rows("nothing-like-it", running()).is_empty());
    }

    #[test]
    fn the_first_line_sums_up_the_system() {
        let overall = Overall {
            cpu: 23.4,
            cores: 16,
            used: 9 * 1024 * 1024 * 1024,
            total: 32 * 1024 * 1024 * 1024,
            swap_used: 0,
            load: Some(1.25),
        };
        let items = answer("", &overall, running());
        assert_eq!(
            items[0].title,
            "CPU 23% of 16 cores · memory 9.0 GB of 32.0 GB · load 1.25"
        );
        let cargo = &items[1];
        assert_eq!(cargo.title, "cargo");
        let subtitle = cargo.subtitle.as_deref().unwrap();
        assert!(
            subtitle.starts_with("90.0% CPU · 200 MB · PID 7 · me · 1h 1m"),
            "{subtitle}"
        );
        assert_eq!(span(90_000), "1d 1h");
        assert_eq!(
            name("WebKitWebProces", Some("/usr/lib/WebKitWebProcess")),
            "WebKitWebProcess"
        );
        assert_eq!(name("bash", Some("/bin/zsh")), "bash");
    }
}
