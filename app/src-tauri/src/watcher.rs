use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant},
};

use notify::{
    ErrorKind, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _, event::ModifyKind,
};

/// How long changes must stop before a rescan, so a burst of writes costs one.
const QUIET: Duration = Duration::from_secs(1);
/// The longest a rescan waits, so a folder that never stops changing is still indexed.
const MAX_WAIT: Duration = Duration::from_secs(10);

enum Message {
    Event(notify::Result<Event>),
    Folders(Vec<PathBuf>),
}

/// Watches the folders the last scan went into and calls `rescan` shortly after
/// something in them changes.
pub struct Watcher {
    messages: Sender<Message>,
}

impl Watcher {
    pub fn start(home: PathBuf, data: PathBuf, rescan: impl Fn() + Send + 'static) -> Watcher {
        let (messages, received) = mpsc::channel();
        let events = messages.clone();
        let watcher = notify::recommended_watcher(move |event| {
            let _ = events.send(Message::Event(event));
        });
        thread::spawn(move || {
            let mut watch = match watcher {
                Ok(watcher) => Watch::new(watcher, &home),
                Err(err) => return eprintln!("sonar: can't watch for changes: {err}"),
            };
            let mut folders = HashSet::new();
            let mut settle = Settle::default();
            loop {
                let message = match settle.due() {
                    Some(at) => received.recv_timeout(at.saturating_duration_since(Instant::now())),
                    None => received.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };
                match message {
                    Ok(Message::Event(Ok(event))) => {
                        if matters(&event, &folders) {
                            settle.change(Instant::now());
                        }
                    }
                    Ok(Message::Event(Err(err))) => watch.failed(err),
                    Ok(Message::Folders(scanned)) => {
                        let scanned = watched(scanned, &home, &data);
                        // A folder that appeared since the last scan may have had
                        // files added before it was watched, so look once more.
                        if !folders.is_empty() && !scanned.is_subset(&folders) {
                            settle.change(Instant::now());
                        }
                        watch.follow(&scanned);
                        folders = scanned;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        settle = Settle::default();
                        rescan();
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Watcher { messages }
    }

    /// Watches `folders`, the ones the last scan went into, from now on.
    pub fn follow(&self, folders: Vec<PathBuf>) {
        let _ = self.messages.send(Message::Folders(folders));
    }
}

/// The OS watch. Linux's inotify needs a watch per folder, so it watches only the
/// scanned ones rather than every folder under home; the others watch home whole.
struct Watch {
    watcher: Option<RecommendedWatcher>,
    watching: HashSet<PathBuf>,
}

impl Watch {
    fn new(mut watcher: RecommendedWatcher, home: &Path) -> Watch {
        let mut watch = Watch {
            watcher: None,
            watching: HashSet::new(),
        };
        if !cfg!(target_os = "linux")
            && let Err(err) = watcher.watch(home, RecursiveMode::Recursive)
        {
            eprintln!("sonar: can't watch for changes: {err}");
        } else {
            watch.watcher = Some(watcher);
        }
        watch
    }

    fn follow(&mut self, folders: &HashSet<PathBuf>) {
        if !cfg!(target_os = "linux") {
            return;
        }
        let Some(watcher) = &mut self.watcher else {
            return;
        };
        for gone in self.watching.difference(folders) {
            let _ = watcher.unwatch(gone);
        }
        self.watching.retain(|f| folders.contains(f));
        for folder in folders {
            if self.watching.contains(folder) {
                continue;
            }
            match watcher.watch(folder, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    self.watching.insert(folder.clone());
                }
                Err(err) if matches!(err.kind, ErrorKind::MaxFilesWatch) => {
                    return self.failed(err);
                }
                // Gone or unreadable since the scan: the next scan settles it.
                Err(_) => {}
            }
        }
    }

    /// Stops watching when the OS runs out of watches, freeing the ones taken for
    /// other apps; the periodic rescan still runs.
    fn failed(&mut self, err: notify::Error) {
        if matches!(err.kind, ErrorKind::MaxFilesWatch) && self.watcher.take().is_some() {
            self.watching.clear();
            eprintln!(
                "sonar: too many folders to watch for changes (see fs.inotify.max_user_watches); \
                 rescanning on a timer instead"
            );
        }
    }
}

/// The folders to watch: the scanned ones and home, but never Sonar's own data
/// folder, or writing the index would set off another scan.
fn watched(scanned: Vec<PathBuf>, home: &Path, data: &Path) -> HashSet<PathBuf> {
    scanned
        .into_iter()
        .chain([home.to_owned()])
        .filter(|f| !f.starts_with(data))
        .collect()
}

/// Whether `event` may change what the index holds: something created, removed,
/// written or renamed directly inside a watched folder. Changes under folders the
/// scan skips (excluded, ignored, inside bundles) don't count, nor do reads, or
/// metadata changes, which reading a file can cause.
fn matters(event: &Event, folders: &HashSet<PathBuf>) -> bool {
    if event.need_rescan() {
        return true;
    }
    let changes = match event.kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Modify(_) => true,
        EventKind::Any | EventKind::Access(_) | EventKind::Other => false,
    };
    changes
        && event
            .paths
            .iter()
            .any(|p| p.parent().is_some_and(|d| folders.contains(d)))
}

/// When to rescan after changes: once they've been quiet for `QUIET`, or `MAX_WAIT`
/// after the first, whichever comes sooner.
#[derive(Default)]
struct Settle {
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Settle {
    fn change(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    fn due(&self) -> Option<Instant> {
        Some((self.last? + QUIET).min(self.first? + MAX_WAIT))
    }
}
