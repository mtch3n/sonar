//! Searching by meaning in the background: downloads the chosen model, embeds what
//! each scan found, and hands the search bar a model of its own for queries.

use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Sender},
    },
    thread,
};

use sonar_core::{Embedder, Index, Paths};
use sonar_models::Load;

/// The model the search bar embeds queries with, when searching by meaning is on
/// and its model is ready.
pub type QueryModel = Arc<Mutex<Option<Box<dyn Embedder>>>>;

pub enum Status {
    Off,
    Downloading { name: &'static str, mb: u32 },
    Embedding { done: u64, total: u64 },
    Ready,
    Failed(String),
}

#[derive(Clone)]
pub struct Meaning {
    wake: Sender<()>,
}

impl Meaning {
    /// Embeds whenever woken, with the model `settings()` names at the time, and
    /// keeps `query` holding the same model for the search bar.
    pub fn start(
        paths: Paths,
        settings: impl Fn() -> sonar_settings::Meaning + Send + 'static,
        query: QueryModel,
        on_status: impl Fn(Status) + Send + 'static,
    ) -> Meaning {
        let (wake, woken) = mpsc::channel::<()>();
        thread::spawn(move || {
            let mut index = match Index::open(&paths.db) {
                Ok(index) => index,
                Err(err) => return on_status(Status::Failed(format!("{err:#}"))),
            };
            let mut model: Option<Box<dyn Embedder>> = None;
            while woken.recv().is_ok() {
                while woken.try_recv().is_ok() {}
                let wanted = settings();
                if !wanted.enabled {
                    model = None;
                    *lock(&query) = None;
                    on_status(Status::Off);
                    continue;
                }
                if model.as_ref().is_none_or(|m| m.id() != wanted.model) {
                    model = None;
                    *lock(&query) = None;
                    match load(&paths, &wanted.model, &on_status) {
                        Ok((background, for_queries)) => {
                            model = Some(background);
                            *lock(&query) = Some(for_queries);
                        }
                        Err(err) => {
                            on_status(Status::Failed(format!("{err:#}")));
                            continue;
                        }
                    }
                }
                let Some(model) = model.as_mut() else {
                    continue;
                };
                let total = match index.pending_meaning(model.id()) {
                    Ok((names, files)) => names + files,
                    Err(err) => {
                        on_status(Status::Failed(format!("{err:#}")));
                        continue;
                    }
                };
                if total > 0 {
                    on_status(Status::Embedding { done: 0, total });
                }
                let id = model.id().to_owned();
                let embedded = index.embed(model.as_mut(), &mut |stats| {
                    on_status(Status::Embedding {
                        done: stats.names + stats.files,
                        total,
                    });
                    // A change of model or turning it off stops this round.
                    let now = settings();
                    now.enabled && now.model == id
                });
                on_status(match embedded {
                    Ok(_) => Status::Ready,
                    Err(err) => Status::Failed(format!("{err:#}")),
                });
            }
        });
        Meaning { wake }
    }

    /// Embeds what the last scan found, or loads another model.
    pub fn wake(&self) {
        let _ = self.wake.send(());
    }
}

/// Two copies of the model: one for embedding in the background, and one the
/// search bar can use while the other is busy.
fn load(
    paths: &Paths,
    id: &str,
    on_status: &impl Fn(Status),
) -> anyhow::Result<(Box<dyn Embedder>, Box<dyn Embedder>)> {
    if !sonar_models::is_downloaded(id, &paths.models)
        && let Some(info) = sonar_models::info(id)
    {
        on_status(Status::Downloading {
            name: info.name,
            mb: info.download_mb,
        });
    }
    let background = sonar_models::load(
        id,
        &paths.models,
        Load {
            download: true,
            threads: sonar_models::threads_for_background(),
        },
    )?;
    let for_queries = sonar_models::load(
        id,
        &paths.models,
        Load {
            download: false,
            threads: 2,
        },
    )?;
    Ok((background, for_queries))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
