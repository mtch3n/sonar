//! Learning about files in the background, after each scan: embedding them for
//! searching by meaning, and having processor plugins look at them. It downloads
//! the chosen model, and hands the search bar a model of its own for queries.

use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Sender},
    },
    thread,
};

use sonar_core::{Embedder, Index, Paths, ProcessOptions};
use sonar_models::Load;
use sonar_plugins::processor::ProcessorPlugin;
use sonar_settings::Settings;

/// The model the search bar embeds queries with, when searching by meaning is on
/// and its model is ready.
pub type QueryModel = Arc<Mutex<Option<Box<dyn Embedder>>>>;

pub enum Status {
    Idle,
    Downloading {
        name: &'static str,
        mb: u32,
    },
    Embedding {
        done: u64,
        total: u64,
    },
    Processing {
        plugin: String,
        done: u64,
        total: u64,
    },
    Failed(String),
}

#[derive(Clone)]
pub struct Learner {
    wake: Sender<()>,
}

type Model = Box<dyn Embedder>;

impl Learner {
    /// Learns whenever woken, with the settings `settings()` gives at the time and
    /// the processors `processors()` builds, and keeps `query` holding the same
    /// model for the search bar.
    pub fn start(
        paths: Paths,
        settings: impl Fn() -> Settings + Send + 'static,
        processors: impl Fn() -> Vec<ProcessorPlugin> + Send + 'static,
        query: QueryModel,
        on_status: impl Fn(Status) + Send + 'static,
    ) -> Learner {
        let (wake, woken) = mpsc::channel::<()>();
        thread::spawn(move || {
            let mut index = match Index::open(&paths.db) {
                Ok(index) => index,
                Err(err) => return on_status(Status::Failed(format!("{err:#}"))),
            };
            let mut model: Option<Model> = None;
            while woken.recv().is_ok() {
                while woken.try_recv().is_ok() {}
                let now = settings();
                let private = now.private_folders(&paths.home);
                if !now.meaning.enabled {
                    model = None;
                    *lock(&query) = None;
                } else if model.as_ref().is_none_or(|m| m.id() != now.meaning.model) {
                    model = None;
                    *lock(&query) = None;
                    match load(&paths, &now, &on_status) {
                        Ok((background, for_queries)) => {
                            model = Some(background);
                            *lock(&query) = Some(for_queries);
                        }
                        Err(err) => on_status(Status::Failed(err)),
                    }
                }
                let embed = |index: &mut Index, model: &mut Option<Model>| -> Result<(), String> {
                    let Some(model) = model.as_mut() else {
                        return Ok(());
                    };
                    let (names, files) = index
                        .pending_meaning(model.id())
                        .map_err(|err| format!("{err:#}"))?;
                    let total = names + files;
                    let id = model.id().to_owned();
                    index
                        .embed(model.as_mut(), &private, &mut |stats| {
                            if total > 0 {
                                on_status(Status::Embedding {
                                    done: stats.names + stats.files,
                                    total,
                                });
                            }
                            // A change of model or turning it off stops this round.
                            let now = settings().meaning;
                            now.enabled && now.model == id
                        })
                        .map(|_| ())
                        .map_err(|err| format!("{err:#}"))
                };
                if let Err(err) = embed(&mut index, &mut model) {
                    on_status(Status::Failed(err));
                    continue;
                }

                let options = ProcessOptions {
                    root: &paths.home,
                    frames: &paths.frames,
                    private: &private,
                };
                let mut learned = false;
                for mut processor in processors() {
                    let plugin = sonar_core::Processor::id(&processor).to_owned();
                    let processed = index.process(&mut processor, &options, &mut |stats| {
                        let total = stats.done + stats.failed + stats.left;
                        if total > 0 {
                            on_status(Status::Processing {
                                plugin: plugin.clone(),
                                done: stats.done + stats.failed,
                                total,
                            });
                        }
                        // Turning the plugin off stops it.
                        settings().plugin(&plugin).enabled
                    });
                    match processed {
                        Ok(stats) => learned |= stats.done > 0,
                        Err(err) => on_status(Status::Failed(format!("{plugin}: {err:#}"))),
                    }
                }
                // What processors said is searched by meaning too.
                if learned && let Err(err) = embed(&mut index, &mut model) {
                    on_status(Status::Failed(err));
                    continue;
                }
                if let Some(model) = model.as_mut()
                    && let Err(err) = index.learn_labels(model.as_mut(), &now.labels())
                {
                    on_status(Status::Failed(format!("labels: {err:#}")));
                    continue;
                }
                on_status(Status::Idle);
            }
        });
        Learner { wake }
    }

    /// Learns about what the last scan found, or loads another model.
    pub fn wake(&self) {
        let _ = self.wake.send(());
    }
}

/// Two copies of the model: one for embedding in the background, and one the
/// search bar can use while the other is busy.
fn load(
    paths: &Paths,
    settings: &Settings,
    on_status: &impl Fn(Status),
) -> Result<(Model, Model), String> {
    if !settings.meaning_ready(&paths.models)
        && let Some(info) = sonar_models::info(&settings.meaning.model)
    {
        on_status(Status::Downloading {
            name: info.name,
            mb: info.download_mb,
        });
    }
    let background = settings.meaning_model(
        &paths.models,
        Load {
            download: true,
            threads: sonar_models::threads_for_background(),
        },
    )?;
    let for_queries = settings.meaning_model(
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
