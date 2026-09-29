//! The models Sonar can run to search by meaning, and where they come from.
//!
//! Models are downloaded from Hugging Face the first time they're used, into a
//! folder Sonar owns, and run on this computer from then on.

pub mod remote;
mod static_model;

use std::path::Path;

use anyhow::{Result, bail};
use sonar_core::Embedder;

pub use remote::{Provider, keys};

/// A model Sonar can search with. Sonar runs static models itself, in pure Rust on
/// every system it builds for; transformer models are reached through a provider,
/// like Ollama on this computer.
pub struct ModelInfo {
    /// What `meaning.model` in the settings calls it.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Roughly how much is downloaded the first time.
    pub download_mb: u32,
    /// Its Hugging Face repository, at a pinned revision.
    repo: &'static str,
    revision: &'static str,
    /// Scores below this mean unrelated.
    min_score: f32,
}

pub const DEFAULT_MODEL: &str = "multilingual";

pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "multilingual",
        name: "Multilingual",
        description: "Over 100 languages, including Chinese. Indexes thousands of pieces of text a second",
        download_mb: 530,
        repo: "minishlab/potion-multilingual-128M",
        revision: "73908c3438cf03b6a01bcb9611d62b23d0726f08",
        min_score: 0.3,
    },
    ModelInfo {
        id: "english",
        name: "English",
        description: "English only, and a smaller download. Indexes thousands of pieces of text a second",
        download_mb: 131,
        repo: "minishlab/potion-retrieval-32M",
        revision: "6fc8051fab2a1e0ee76689cf08c853792ac285e7",
        min_score: 0.25,
    },
];

pub fn info(id: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == id)
}

/// Whether `id` is in `dir` already, so loading it won't download anything.
pub fn is_downloaded(id: &str, dir: &Path) -> bool {
    info(id).is_some_and(|m| static_model::is_downloaded(dir, m.repo, m.revision))
}

/// Loads model `id` from `dir`, downloading it first if `download` allows.
pub fn load(id: &str, dir: &Path, download: bool) -> Result<Box<dyn Embedder>> {
    let Some(model) = info(id) else {
        let known: Vec<&str> = MODELS.iter().map(|m| m.id).collect();
        bail!("unknown model `{id}`; use one of: {}", known.join(", "));
    };
    if !download && !is_downloaded(id, dir) {
        bail!("the {} model isn't downloaded", model.name);
    }
    Ok(Box::new(static_model::StaticModel::load(
        model.id,
        dir,
        model.repo,
        model.revision,
        model.min_score,
    )?))
}

/// An embedding model behind `provider`'s API, like OpenAI's
/// `text-embedding-3-small` or a model served by Ollama.
pub fn remote(provider: Provider, model: &str) -> Box<dyn Embedder> {
    Box::new(remote::RemoteModel::new(provider, model))
}
