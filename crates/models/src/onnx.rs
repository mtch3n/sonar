//! Transformer models run with ONNX Runtime, through fastembed.

use std::path::Path;

use anyhow::{Result, anyhow};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use hf_hub::Cache;
use sonar_core::Embedder;

/// Tokens of a chunk the model reads. Chunks are about a paragraph, so this rarely
/// cuts one short, and it keeps the model several times faster than its maximum.
const MAX_TOKENS: usize = 256;

pub(crate) struct OnnxModel {
    id: String,
    model: TextEmbedding,
    query_prefix: &'static str,
    passage_prefix: &'static str,
    min_score: f32,
}

/// Threads for embedding in the background: half the cores, so the computer stays
/// responsive while a first index is built.
pub fn threads_for_background() -> usize {
    std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).max(1))
}

pub(crate) fn is_downloaded(dir: &Path, model: &EmbeddingModel) -> bool {
    let Ok(info) = TextEmbedding::get_model_info(model) else {
        return false;
    };
    let repo = Cache::new(dir.to_owned()).model(info.model_code.clone());
    [
        info.model_file.as_str(),
        "tokenizer.json",
        "config.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
    ]
    .into_iter()
    .chain(info.additional_files.iter().map(String::as_str))
    .all(|file| repo.get(file).is_some())
}

impl OnnxModel {
    pub(crate) fn load(
        id: &str,
        dir: &Path,
        model: EmbeddingModel,
        query_prefix: &'static str,
        passage_prefix: &'static str,
        min_score: f32,
        threads: usize,
    ) -> Result<OnnxModel> {
        let options = TextInitOptions::new(model)
            .with_cache_dir(dir.to_owned())
            .with_show_download_progress(false)
            .with_max_length(MAX_TOKENS)
            .with_intra_threads(threads.max(1));
        Ok(OnnxModel {
            id: id.to_owned(),
            model: TextEmbedding::try_new(options).map_err(|err| anyhow!("{err}"))?,
            query_prefix,
            passage_prefix,
            min_score,
        })
    }
}

impl Embedder for OnnxModel {
    fn id(&self) -> &str {
        &self.id
    }

    fn passages(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let texts: Vec<String> = texts
            .iter()
            .map(|t| format!("{}{t}", self.passage_prefix))
            .collect();
        self.model
            .embed(texts, Some(16))
            .map_err(|err| anyhow!("{err}"))
    }

    fn query(&mut self, text: &str) -> Result<Vec<f32>> {
        self.model
            .embed(vec![format!("{}{text}", self.query_prefix)], None)
            .map_err(|err| anyhow!("{err}"))?
            .pop()
            .ok_or_else(|| anyhow!("the model returned nothing"))
    }

    fn min_score(&self) -> f32 {
        self.min_score
    }
}
