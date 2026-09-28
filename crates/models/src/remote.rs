//! Models behind an OpenAI-compatible API: OpenAI, OpenRouter, or a server on this
//! computer like Ollama or LM Studio.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use sonar_core::Embedder;

/// The keychain entry keys are kept under, one per provider.
const KEYCHAIN_SERVICE: &str = "sonar";
/// Answers are small; anything bigger is a mistake.
const MAX_RESPONSE: u64 = 64 * 1024 * 1024;
/// Texts sent per request for embedding.
const EMBED_BATCH: usize = 64;

/// Somewhere models run.
#[derive(Clone, Debug)]
pub struct Provider {
    pub id: String,
    /// Where its API is, like `https://api.openai.com/v1`.
    pub url: String,
    pub key: Option<String>,
}

impl Provider {
    /// Whether it runs on this computer, so what it's sent stays here.
    pub fn is_local(&self) -> bool {
        is_local_url(&self.url)
    }

    fn post(&self, path: &str, body: &Value, timeout: Duration) -> Result<Value> {
        let url = format!("{}/{path}", self.url.trim_end_matches('/'));
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        let mut request = agent
            .post(&url)
            .header("Content-Type", "application/json")
            // OpenRouter lists apps that say who they are.
            .header("HTTP-Referer", "https://github.com/mtch3n/sonar")
            .header("X-Title", "Sonar");
        if let Some(key) = &self.key {
            request = request.header("Authorization", &format!("Bearer {key}"));
        }
        let mut response = request
            .send(body.to_string())
            .with_context(|| format!("couldn't reach {}", self.id))?;
        let status = response.status();
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE)
            .read_to_string()
            .with_context(|| format!("couldn't read {}'s answer", self.id))?;
        let answer: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if !status.is_success() {
            let message = answer["error"]["message"]
                .as_str()
                .or_else(|| answer["error"].as_str())
                .map_or_else(|| text.chars().take(200).collect(), str::to_owned);
            bail!("{} says {}: {message}", self.id, status.as_u16());
        }
        Ok(answer)
    }

    /// Vectors of `texts`, in order.
    pub fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(EMBED_BATCH) {
            let answer = self.post(
                "embeddings",
                &json!({ "model": model, "input": batch }),
                Duration::from_secs(120),
            )?;
            let mut data: Vec<(u64, Vec<f32>)> = answer["data"]
                .as_array()
                .context("the answer has no vectors")?
                .iter()
                .map(|item| {
                    let vector = item["embedding"]
                        .as_array()
                        .context("a vector is missing")?
                        .iter()
                        .map(|x| {
                            x.as_f64()
                                .map(|x| x as f32)
                                .context("a vector isn't numbers")
                        })
                        .collect::<Result<Vec<f32>>>()?;
                    Ok((item["index"].as_u64().unwrap_or(0), vector))
                })
                .collect::<Result<_>>()?;
            if data.len() != batch.len() {
                bail!("asked for {} vectors and got {}", batch.len(), data.len());
            }
            data.sort_by_key(|(index, _)| *index);
            vectors.extend(data.into_iter().map(|(_, v)| unit(v)));
        }
        Ok(vectors)
    }

    /// The model's answer to a conversation, as OpenAI's chat API takes it.
    pub fn chat(&self, model: &str, messages: Value, json_answer: bool) -> Result<String> {
        let mut body = json!({ "model": model, "messages": messages, "temperature": 0 });
        if json_answer {
            body["response_format"] = json!({ "type": "json_object" });
        }
        let answer = self.post("chat/completions", &body, Duration::from_secs(300))?;
        answer["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("{} gave no answer", self.id))
    }
}

fn is_local_url(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    host == "localhost" || host == "::1" || host.starts_with("127.") || host.ends_with(".localhost")
}

fn unit(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// API keys, kept in the system's keychain rather than in the settings file.
pub mod keys {
    use super::KEYCHAIN_SERVICE;

    fn entry(provider: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, provider).map_err(|err| err.to_string())
    }

    pub fn get(provider: &str) -> Option<String> {
        entry(provider).ok()?.get_password().ok()
    }

    pub fn set(provider: &str, key: &str) -> Result<(), String> {
        entry(provider)?
            .set_password(key)
            .map_err(|err| format!("couldn't keep the key in the keychain: {err}"))
    }

    pub fn delete(provider: &str) -> Result<(), String> {
        match entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!("couldn't remove the key from the keychain: {err}")),
        }
    }
}

/// An embedding model behind a provider's API, named `provider:model`.
pub(crate) struct RemoteModel {
    id: String,
    provider: Provider,
    model: String,
    min_score: f32,
}

impl RemoteModel {
    pub(crate) fn new(provider: Provider, model: &str) -> RemoteModel {
        RemoteModel {
            id: format!("{}:{model}", provider.id),
            min_score: remote_min_score(model),
            provider,
            model: model.to_owned(),
        }
    }
}

/// Where related and unrelated texts part, for models whose scores are known;
/// others get a middling cutoff.
fn remote_min_score(model: &str) -> f32 {
    match model.rsplit('/').next().unwrap_or(model) {
        "text-embedding-3-small" | "text-embedding-3-large" => 0.3,
        "nomic-embed-text" | "nomic-embed-text:latest" => 0.5,
        _ => 0.35,
    }
}

impl Embedder for RemoteModel {
    fn id(&self) -> &str {
        &self.id
    }

    fn passages(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.provider.embed(&self.model, texts)
    }

    fn query(&mut self, text: &str) -> Result<Vec<f32>> {
        self.provider
            .embed(&self.model, &[text.to_owned()])?
            .pop()
            .context("no vector came back")
    }

    fn min_score(&self) -> f32 {
        self.min_score
    }

    fn is_local(&self) -> bool {
        self.provider.is_local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_urls() {
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:8080/v1",
        ] {
            assert!(is_local_url(url), "{url}");
        }
        for url in [
            "https://api.openai.com/v1",
            "https://openrouter.ai/api/v1",
            "http://localhost.evil.com/v1",
        ] {
            assert!(!is_local_url(url), "{url}");
        }
    }

    #[test]
    fn vectors_are_made_unit_length() {
        let v = unit(vec![3.0, 4.0]);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
    }

    /// Answers one request with `answer`, and returns its address and what it was asked.
    fn server(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let n = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .and_then(|l| l.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if body.len() >= length {
                        break;
                    }
                }
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{answer}",
                answer.len()
            )
            .unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (url, handle)
    }

    #[test]
    fn embeds_through_an_openai_compatible_api() {
        let (url, asked) =
            server(r#"{"data":[{"index":1,"embedding":[0,2]},{"index":0,"embedding":[3,4]}]}"#);
        let provider = Provider {
            id: "test".into(),
            url,
            key: Some("secret".into()),
        };
        let vectors = provider.embed("tiny", &["a".into(), "b".into()]).unwrap();
        assert_eq!(vectors, [vec![0.6, 0.8], vec![0.0, 1.0]]);
        let request = asked.join().unwrap();
        assert!(request.starts_with("POST /v1/embeddings "), "{request}");
        assert!(request.contains("Bearer secret"), "{request}");
        assert!(request.contains(r#""input":["a","b"]"#), "{request}");
    }

    #[test]
    fn chats_through_an_openai_compatible_api() {
        let (url, asked) =
            server(r#"{"choices":[{"message":{"role":"assistant","content":"{\"tags\":[]}"}}]}"#);
        let provider = Provider {
            id: "test".into(),
            url,
            key: None,
        };
        let answer = provider
            .chat("vision", json!([{"role": "user", "content": "hi"}]), true)
            .unwrap();
        assert_eq!(answer, r#"{"tags":[]}"#);
        assert!(
            asked
                .join()
                .unwrap()
                .contains(r#""response_format":{"type":"json_object"}"#)
        );
    }
}
