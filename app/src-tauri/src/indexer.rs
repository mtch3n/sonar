use std::{
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread,
    time::Duration,
};

use sonar_core::{Index, Paths, Rules};

pub enum Status {
    Indexing,
    Ready { files: u64, at: String },
    Failed(String),
}

pub struct Indexer {
    wake: Sender<()>,
}

impl Indexer {
    /// Scans now, then again every `interval()`, which is asked after each scan so a
    /// changed setting applies from the next wait.
    pub fn start(
        paths: Paths,
        interval: impl Fn() -> Duration + Send + 'static,
        on_status: impl Fn(Status) + Send + 'static,
    ) -> Indexer {
        let (wake, woken) = mpsc::channel();
        thread::spawn(move || {
            let mut index = match Index::open(&paths.db) {
                Ok(index) => index,
                Err(err) => return on_status(Status::Failed(format!("{err:#}"))),
            };
            loop {
                on_status(Status::Indexing);
                let scanned = Rules::load(&paths.rules, &paths.home)
                    .and_then(|rules| index.scan(&paths.home, &rules));
                on_status(match scanned {
                    Ok(stats) => Status::Ready {
                        files: stats.files,
                        at: jiff::Zoned::now().strftime("%H:%M").to_string(),
                    },
                    Err(err) => Status::Failed(format!("{err:#}")),
                });
                match woken.recv_timeout(interval()) {
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
