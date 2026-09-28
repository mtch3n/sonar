//! Describe: a few tags and a line about each image and video, from a vision model
//! on this computer, like one Ollama serves, or from a provider elsewhere. What it
//! says is searched like the file's own text.

use std::{
    collections::BTreeMap,
    io::{BufRead, Cursor, Write},
    path::{Path, PathBuf},
};

use base64::Engine as _;
use image::{ImageFormat, imageops::FilterType};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::setting::{Field, Setting};

pub const ID: &str = "describe";
/// Changes with the prompt, so files are described again.
pub const VERSION: &str = "1";
/// Frames of a video the model sees.
pub const FRAMES: usize = 4;
/// Longest side of a picture sent to the model: enough to read a receipt.
const LARGEST_SIDE: u32 = 1024;

pub fn settings() -> Vec<Setting> {
    vec![
        Setting {
            key: "model".into(),
            title: "Model".into(),
            description: Some(
                "A vision model as provider:model, like ollama:qwen2.5vl:3b or openrouter:google/gemini-2.5-flash".into(),
            ),
            field: Field::Text {
                default: String::new(),
                placeholder: Some("ollama:qwen2.5vl:3b".into()),
            },
        },
        Setting {
            key: "language".into(),
            title: "Language".into(),
            description: Some("What descriptions and tags are written in".into()),
            field: Field::Text {
                default: "English".into(),
                placeholder: None,
            },
        },
    ]
}

#[derive(Deserialize)]
struct Request {
    path: PathBuf,
    kind: String,
    #[serde(default)]
    frames: Vec<PathBuf>,
    #[serde(default)]
    settings: BTreeMap<String, Value>,
    model: Option<Model>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Model {
    url: String,
    key: Option<String>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct Described {
    #[serde(default)]
    description: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    labels: Vec<String>,
}

/// Answers each file on stdin with what the model says about it.
pub fn serve() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let answer = match serde_json::from_str::<Request>(&line) {
            Ok(request) => match describe(&request) {
                Ok(described) => json!({
                    "text": described.description,
                    "tags": described.tags,
                    "labels": described.labels,
                }),
                Err(err) => json!({ "error": err }),
            },
            Err(err) => json!({ "error": format!("couldn't read the request: {err}") }),
        };
        if writeln!(stdout, "{answer}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            return;
        }
    }
}

fn describe(request: &Request) -> Result<Described, String> {
    let model = request
        .model
        .as_ref()
        .ok_or("choose a model for Describe in Settings")?;
    let pictures = if request.kind == "video" {
        request
            .frames
            .iter()
            .map(|f| encode(f))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![encode(&request.path)?]
    };
    if pictures.is_empty() {
        return Err("there's nothing to look at".into());
    }
    let language = request
        .settings
        .get("language")
        .and_then(Value::as_str)
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("English");
    let mut content = vec![json!({ "type": "text", "text": prompt(request, language) })];
    content.extend(
        pictures
            .into_iter()
            .map(|url| json!({ "type": "image_url", "image_url": { "url": url } })),
    );
    let provider = sonar_models::Provider {
        id: "the model".into(),
        url: model.url.clone(),
        key: model.key.clone(),
    };
    let answer = provider
        .chat(
            &model.name,
            json!([{ "role": "user", "content": content }]),
            true,
        )
        .map_err(|err| format!("{err:#}"))?;
    let mut described = parse(&answer)?;
    // Only labels Sonar asked about count.
    described.labels.retain(|l| request.labels.contains_key(l));
    described.tags.truncate(8);
    Ok(described)
}

fn prompt(request: &Request, language: &str) -> String {
    let what = if request.kind == "video" {
        "These are frames from across one video, in order. Describe the video as a whole."
    } else {
        "Describe this picture."
    };
    let labels = if request.labels.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = request
            .labels
            .iter()
            .map(|(name, meaning)| format!("- {name}: {meaning}"))
            .collect();
        format!(
            "\n\"labels\": those of these that clearly apply, by name, or none:\n{}",
            list.join("\n")
        )
    };
    format!(
        "{what} It will be found later by searching, so name what's in it: things, places, \
         kinds of document, and any text that stands out, like a shop's name or a total.\n\
         Answer with JSON only, in {language}:\n\
         \"description\": one sentence of at most 20 words,\n\
         \"tags\": up to 8 short lowercase tags,{labels}"
    )
}

/// The model's answer, which some models wrap in a code block.
fn parse(answer: &str) -> Result<Described, String> {
    let json = answer
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(json).map_err(|_| {
        let start: String = answer.chars().take(120).collect();
        format!("the model didn't answer in JSON: {start}")
    })
}

/// A picture as a `data:` URL, scaled down to what the model needs.
fn encode(path: &Path) -> Result<String, String> {
    let picture = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|err| err.to_string())?
        .decode()
        .map_err(|err| format!("couldn't read the picture: {err}"))?;
    let picture = if picture.width().max(picture.height()) > LARGEST_SIDE {
        picture.resize(LARGEST_SIDE, LARGEST_SIDE, FilterType::Triangle)
    } else {
        picture
    };
    let mut jpeg = Vec::new();
    picture
        .to_rgb8()
        .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .map_err(|err| err.to_string())?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_answers_in_and_out_of_code_blocks() {
        let d = parse("```json\n{\"description\":\"A receipt\",\"tags\":[\"ikea\"]}\n```").unwrap();
        assert_eq!(d.description, "A receipt");
        assert_eq!(d.tags, ["ikea"]);
        assert!(parse("It's a receipt").is_err());
    }

    #[test]
    fn asks_about_the_labels_it_was_given() {
        let mut request = Request {
            path: "a.jpg".into(),
            kind: "image".into(),
            frames: Vec::new(),
            settings: BTreeMap::new(),
            model: None,
            labels: BTreeMap::new(),
        };
        assert!(!prompt(&request, "English").contains("labels"));
        request
            .labels
            .insert("receipt".into(), "proof of purchase".into());
        let prompt = prompt(&request, "Traditional Chinese");
        assert!(prompt.contains("- receipt: proof of purchase"), "{prompt}");
        assert!(prompt.contains("in Traditional Chinese"), "{prompt}");
        assert_eq!(
            describe(&request).unwrap_err(),
            "choose a model for Describe in Settings"
        );
    }
}
