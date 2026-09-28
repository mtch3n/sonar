//! Keeps the calculator's exchange rates current. The rates saved last time are used
//! at once; newer ones are downloaded in the background once they're due, never while
//! someone waits for a result.

use std::{
    path::PathBuf,
    sync::{
        Arc, RwLock,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use sonar_plugins::currency::Rates;

/// Shared with the launcher, which reads it for every calculation.
pub type Shared = Arc<RwLock<Option<Arc<Rates>>>>;

/// How often to look whether new rates are due, besides every time the bar opens.
const LOOK_EVERY: Duration = Duration::from_secs(60 * 60);
/// How long to wait after a failed download before trying again.
const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);

pub struct RateKeeper {
    wake: Sender<()>,
}

impl RateKeeper {
    /// Loads the saved rates into `rates`, then downloads new ones whenever they're
    /// due and `enabled()` says downloading is on.
    pub fn start(
        path: PathBuf,
        rates: Shared,
        enabled: impl Fn() -> bool + Send + 'static,
    ) -> RateKeeper {
        let (wake, woken) = mpsc::channel();
        thread::spawn(move || {
            if let Some(saved) = Rates::load(&path) {
                *rates.write().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(saved));
            }
            let mut failed_at: Option<Instant> = None;
            loop {
                let now = jiff::Timestamp::now().as_second();
                let current = rates.read().unwrap_or_else(|p| p.into_inner()).clone();
                let since_failure = failed_at.map(|at| at.elapsed());
                if should_download(current.as_deref(), now, since_failure) && enabled() {
                    match Rates::download() {
                        Ok(fresh) => {
                            if let Err(err) = fresh.save(&path) {
                                eprintln!("sonar: couldn't save exchange rates: {err}");
                            }
                            *rates.write().unwrap_or_else(|p| p.into_inner()) =
                                Some(Arc::new(fresh));
                            failed_at = None;
                        }
                        Err(err) => {
                            eprintln!("sonar: {err}");
                            failed_at = Some(Instant::now());
                        }
                    }
                }
                match woken.recv_timeout(LOOK_EVERY) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => while woken.try_recv().is_ok() {},
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        RateKeeper { wake }
    }

    /// Looks whether new rates are due now, rather than at the next hourly look.
    pub fn check(&self) {
        let _ = self.wake.send(());
    }
}

/// Whether to download rates: when there are none or new ones are due, unless a
/// download failed a moment ago.
fn should_download(current: Option<&Rates>, now: i64, since_failure: Option<Duration>) -> bool {
    let due = current.is_none_or(|rates| rates.is_due(now));
    let resting = since_failure.is_some_and(|elapsed| elapsed < RETRY_AFTER);
    due && !resting
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_when_due_but_not_right_after_a_failure() {
        let rates = Rates {
            published: 100,
            next: 200,
            per_usd: Default::default(),
        };
        assert!(should_download(None, 0, None), "nothing yet");
        assert!(!should_download(Some(&rates), 199, None), "still current");
        assert!(should_download(Some(&rates), 200, None), "due");
        let moment = Some(Duration::from_secs(60));
        assert!(
            !should_download(Some(&rates), 200, moment),
            "failed a minute ago"
        );
        assert!(!should_download(None, 0, moment));
        assert!(
            should_download(Some(&rates), 200, Some(RETRY_AFTER)),
            "rested long enough"
        );
    }
}
