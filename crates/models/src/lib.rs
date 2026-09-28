//! The models Sonar can run to search by meaning, and where they come from.
//!
//! Models are downloaded from Hugging Face the first time they're used, into a
//! folder Sonar owns, and run on this computer from then on.

mod onnx;
mod static_model;

use std::path::Path;

use anyhow::{Result, bail};
use sonar_core::Embedder;

pub use onnx::threads_for_background;

/// A model Sonar can search with.
pub struct ModelInfo {
    /// What `semantic.model` in the settings calls it.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Roughly how much is downloaded the first time.
    pub download_mb: u32,
    engine: Engine,
}

enum Engine {
    /// A static model: a vector per token, averaged. Thousands of chunks a second.
    Static {
        repo: &'static str,
        revision: &'static str,
        min_score: f32,
    },
    /// A transformer run with ONNX Runtime. Better at whole sentences, and tens of
    /// chunks a second.
    Onnx {
        model: fastembed::EmbeddingModel,
        query_prefix: &'static str,
        passage_prefix: &'static str,
        min_score: f32,
    },
}

pub const DEFAULT_MODEL: &str = "multilingual";

pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "multilingual",
        name: "Multilingual, fast",
        description: "Over 100 languages, including Chinese. Indexes thousands of pieces of text a second",
        download_mb: 530,
        engine: Engine::Static {
            repo: "minishlab/potion-multilingual-128M",
            revision: "73908c3438cf03b6a01bcb9611d62b23d0726f08",
            min_score: 0.3,
        },
    },
    ModelInfo {
        id: "english",
        name: "English, fast",
        description: "English only. Indexes thousands of pieces of text a second",
        download_mb: 131,
        engine: Engine::Static {
            repo: "minishlab/potion-retrieval-32M",
            revision: "6fc8051fab2a1e0ee76689cf08c853792ac285e7",
            min_score: 0.25,
        },
    },
    ModelInfo {
        id: "bge-small-en",
        name: "English, precise",
        description: "English only. Understands whole sentences better; indexes about 30 pieces of text a second",
        download_mb: 67,
        engine: Engine::Onnx {
            model: fastembed::EmbeddingModel::BGESmallENV15Q,
            query_prefix: "Represent this sentence for searching relevant passages: ",
            passage_prefix: "",
            min_score: 0.52,
        },
    },
    ModelInfo {
        id: "multilingual-e5-small",
        name: "Multilingual, precise",
        description: "About 100 languages. Understands whole sentences better; indexes about 30 pieces of text a second",
        download_mb: 470,
        engine: Engine::Onnx {
            model: fastembed::EmbeddingModel::MultilingualE5Small,
            query_prefix: "query: ",
            passage_prefix: "passage: ",
            min_score: 0.8,
        },
    },
];

pub fn info(id: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == id)
}

/// Whether `id` is in `dir` already, so loading it won't download anything.
pub fn is_downloaded(id: &str, dir: &Path) -> bool {
    match info(id).map(|m| &m.engine) {
        Some(Engine::Static { repo, revision, .. }) => {
            static_model::is_downloaded(dir, repo, revision)
        }
        Some(Engine::Onnx { model, .. }) => onnx::is_downloaded(dir, model),
        None => false,
    }
}

/// How a model is run.
#[derive(Clone, Copy)]
pub struct Load {
    /// Whether to download the model when it isn't in the folder yet.
    pub download: bool,
    /// Threads a transformer may use; static models use one.
    pub threads: usize,
}

/// Loads model `id` from `dir`, downloading it first if `load` allows.
pub fn load(id: &str, dir: &Path, load: Load) -> Result<Box<dyn Embedder>> {
    let Some(model) = info(id) else {
        let known: Vec<&str> = MODELS.iter().map(|m| m.id).collect();
        bail!("unknown model `{id}`; use one of: {}", known.join(", "));
    };
    if !load.download && !is_downloaded(id, dir) {
        bail!("the {} model isn't downloaded", model.name);
    }
    Ok(match &model.engine {
        Engine::Static {
            repo,
            revision,
            min_score,
        } => Box::new(static_model::StaticModel::load(
            model.id, dir, repo, revision, *min_score,
        )?),
        Engine::Onnx {
            model: which,
            query_prefix,
            passage_prefix,
            min_score,
        } => Box::new(onnx::OnnxModel::load(
            model.id,
            dir,
            which.clone(),
            query_prefix,
            passage_prefix,
            *min_score,
            load.threads,
        )?),
    })
}
