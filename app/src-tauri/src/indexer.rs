use std::{
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread,
    time::Duration,
};

use sonar_core::{Index, Paths, Rules};

use crate::{settings, watcher::Watcher};

pub enum Status {
    Indexing,
    Ready { files: u64, at: String },
    Failed(String),
}

pub struct Indexer {
    wake: Sender<()>,
}

impl Indexer {
    /// Scans now, then again soon after files change, and every `rescan_minutes` in
    /// case a change went unseen. `settings()` is asked for each scan and wait, so a
    /// changed setting applies from the next one.
    pub fn start(
        paths: Paths,
        settings: impl Fn() -> settings::Index + Send + 'static,
        on_status: impl Fn(Status) + Send + 'static,
    ) -> Indexer {
        let (wake, woken) = mpsc::channel();
        let rescan = wake.clone();
        thread::spawn(move || {
            let mut index = match Index::open(&paths.db) {
                Ok(index) => index,
                Err(err) => return on_status(Status::Failed(format!("{err:#}"))),
            };
            let data = paths.db.parent().unwrap_or(&paths.db).to_owned();
            let watcher = Watcher::start(paths.home.clone(), data, move || {
                let _ = rescan.send(());
            });
            // Watch what the last run indexed while this first scan runs.
            watcher.follow(index.folders().unwrap_or_default());
            loop {
                on_status(Status::Indexing);
                let scanned = Rules::load(&paths.rules, &paths.home).and_then(|rules| {
                    let text_limit = settings().text_kb as usize * 1024;
                    index.scan(&paths.home, &rules, text_limit)
                });
                on_status(match scanned {
                    Ok(stats) => Status::Ready {
                        files: stats.files,
                        at: jiff::Zoned::now().strftime("%H:%M").to_string(),
                    },
                    Err(err) => Status::Failed(format!("{err:#}")),
                });
                if let Ok(folders) = index.folders() {
                    watcher.follow(folders);
                }
                let interval = Duration::from_secs(settings().rescan_minutes * 60);
                match woken.recv_timeout(interval) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => while woken.try_recv().is_ok() {},
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Indexer { wake }
    }

    pub fn reindex(&self) {
        let _ = self.wake.send(());
    }
}
