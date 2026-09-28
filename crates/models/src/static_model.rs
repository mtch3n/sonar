//! Static models, as Model2Vec makes them: one vector per token, and a text's
//! vector is the average of its tokens'. The table of vectors is memory-mapped, so
//! only the rows of tokens that come up take memory.

use std::{fs::File, path::Path};

use anyhow::{Context, Result, bail};
use hf_hub::{Cache, Repo, RepoType, api::sync::ApiBuilder};
use memmap2::Mmap;
use serde::Deserialize;
use sonar_core::Embedder;
use tokenizers::Tokenizer;

const FILES: [&str; 2] = ["tokenizer.json", "model.safetensors"];
/// Tokens of a text that count; the rest of a long text is left out.
const MAX_TOKENS: usize = 512;
/// A text is cut to this many bytes before it's split into tokens, as a text this
/// long has more than `MAX_TOKENS` tokens anyway.
const MAX_BYTES: usize = MAX_TOKENS * 8;

pub(crate) struct StaticModel {
    id: String,
    tokenizer: Tokenizer,
    table: Mmap,
    /// Where the vectors start in `table`.
    offset: usize,
    rows: usize,
    dims: usize,
    unknown: Option<u32>,
    min_score: f32,
}

#[derive(Deserialize)]
struct Tensor {
    dtype: String,
    shape: Vec<usize>,
    data_offsets: [usize; 2],
}

fn repo(repo: &str, revision: &str) -> Repo {
    Repo::with_revision(repo.to_owned(), RepoType::Model, revision.to_owned())
}

pub(crate) fn is_downloaded(dir: &Path, name: &str, revision: &str) -> bool {
    let cache = Cache::new(dir.to_owned()).repo(repo(name, revision));
    FILES.iter().all(|file| cache.get(file).is_some())
}

impl StaticModel {
    pub(crate) fn load(
        id: &str,
        dir: &Path,
        name: &str,
        revision: &str,
        min_score: f32,
    ) -> Result<StaticModel> {
        let api = ApiBuilder::from_cache(Cache::new(dir.to_owned()))
            .with_progress(false)
            .build()?
            .repo(repo(name, revision));
        let [tokenizer, weights] = FILES.map(|file| {
            api.get(file)
                .with_context(|| format!("downloading {file} of {name}"))
        });
        let tokenizer = Tokenizer::from_file(tokenizer?).map_err(anyhow::Error::msg)?;
        let unknown = tokenizer
            .token_to_id("[UNK]")
            .or_else(|| tokenizer.token_to_id("<unk>"));

        // A safetensors file: the length of a JSON header, the header, the data.
        let file = File::open(weights?)?;
        // SAFETY: the file is Sonar's own download, which nothing else writes to.
        let table = unsafe { Mmap::map(&file)? };
        let header_len = u64::from_le_bytes(
            table
                .get(..8)
                .context("the model file is empty")?
                .try_into()?,
        ) as usize;
        let header: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(table.get(8..8 + header_len).context("broken model file")?)?;
        let tensor: Tensor = serde_json::from_value(
            header
                .get("embeddings")
                .context("the model file has no embeddings")?
                .clone(),
        )?;
        let [rows, dims] = tensor.shape[..] else {
            bail!("the embeddings aren't a table");
        };
        if tensor.dtype != "F32"
            || tensor.data_offsets[1] - tensor.data_offsets[0] != rows * dims * 4
        {
            bail!("the embeddings aren't a table of 32-bit floats");
        }
        let offset = 8 + header_len + tensor.data_offsets[0];
        if table.len() < offset + rows * dims * 4 {
            bail!("the model file is cut short");
        }
        Ok(StaticModel {
            id: id.to_owned(),
            tokenizer,
            table,
            offset,
            rows,
            dims,
            unknown,
            min_score,
        })
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut cut = text.len().min(MAX_BYTES);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let encoding = self
            .tokenizer
            .encode(&text[..cut], false)
            .map_err(anyhow::Error::msg)?;
        let mut sum = vec![0f32; self.dims];
        let mut count = 0;
        for &id in encoding.get_ids().iter().take(MAX_TOKENS) {
            if Some(id) == self.unknown || id as usize >= self.rows {
                continue;
            }
            let start = self.offset + id as usize * self.dims * 4;
            let row = &self.table[start..start + self.dims * 4];
            for (total, bytes) in sum.iter_mut().zip(row.chunks_exact(4)) {
                *total += f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            }
            count += 1;
        }
        if count > 0 {
            for x in &mut sum {
                *x /= count as f32;
            }
        }
        // Scores are dot products, so vectors always have unit length.
        let norm = sum.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut sum {
                *x /= norm;
            }
        }
        Ok(sum)
    }
}

impl Embedder for StaticModel {
    fn id(&self) -> &str {
        &self.id
    }

    fn passages(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    fn query(&mut self, text: &str) -> Result<Vec<f32>> {
        self.embed(text)
    }

    fn min_score(&self) -> f32 {
        self.min_score
    }
}
