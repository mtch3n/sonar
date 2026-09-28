//! Plugins that process files as they're indexed. Sonar writes one JSON object per
//! file to the program's stdin and reads one line of JSON back; see
//! `docs/plugins.md`.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sonar_core::{Fingerprint, Job, Kind, Output, Processor};

use crate::{Manifest, Prepare};

/// How long a processor may take over one file: a model describing a video on a
/// computer without a graphics card can be slow.
const PER_FILE: Duration = Duration::from_secs(300);

/// The model a processor uses, as the plugin's settings name it.
#[derive(Clone, Debug, Serialize)]
pub struct Model {
    /// Where the provider's OpenAI-compatible API is.
    pub url: String,
    pub key: Option<String>,
    /// The model's name at the provider, like `qwen2.5vl:3b`.
    pub name: String,
    /// Whether the provider runs on this computer.
    pub local: bool,
}

/// A processor plugin, started when it's first given a file.
pub struct ProcessorPlugin {
    manifest: Manifest,
    kinds: Vec<Kind>,
    version: String,
    data: PathBuf,
    settings: Map<String, Value>,
    model: Option<Model>,
    labels: BTreeMap<String, String>,
    prepare: Prepare,
    running: Option<Running>,
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<std::io::Result<String>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Serialize)]
struct Request<'a> {
    path: &'a std::path::Path,
    kind: &'a str,
    hash: &'a str,
    frames: &'a [PathBuf],
    duration: Option<f64>,
    settings: &'a Map<String, Value>,
    model: Option<&'a Model>,
    /// The labels files may be given, with what each means.
    labels: &'a BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Answer {
    text: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    labels: Vec<String>,
    fingerprint: Option<AnswerFingerprint>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct AnswerFingerprint {
    algo: String,
    /// 64 bits, as 16 hex digits.
    bits: String,
}

impl ProcessorPlugin {
    /// `settings` are the values of the plugin's settings, and `model` the model
    /// its settings name, if it uses one. Gives `None` for a plugin that doesn't
    /// process files.
    pub fn new(
        manifest: Manifest,
        data: PathBuf,
        settings: Map<String, Value>,
        model: Option<Model>,
        labels: BTreeMap<String, String>,
        prepare: Prepare,
    ) -> Option<ProcessorPlugin> {
        let process = manifest.process.as_ref()?;
        let kinds = process
            .kinds
            .iter()
            .filter_map(|k| Kind::from_name(k))
            .collect();
        // A different model says different things about the same file.
        let version = match &model {
            Some(model) => format!("{}/{}", process.version, model.name),
            None => process.version.clone(),
        };
        Some(ProcessorPlugin {
            manifest,
            kinds,
            version,
            data,
            settings,
            model,
            labels,
            prepare,
            running: None,
        })
    }

    fn start(&self) -> Result<Running> {
        let process = self.manifest.process.as_ref().context("not a processor")?;
        let _ = std::fs::create_dir_all(&self.data);
        let mut command = Command::new(self.manifest.resolve(&process.command[0]));
        command
            .args(&process.command[1..])
            .current_dir(&self.manifest.dir)
            .env("SONAR_PLUGIN_DATA", &self.data)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        (self.prepare)(&mut command);
        let mut child = command
            .spawn()
            .with_context(|| format!("couldn't start `{}`", process.command[0]))?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            bail!("couldn't connect to the plugin");
        };
        let (send, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if send.send(line).is_err() {
                    return;
                }
            }
        });
        let id = self.manifest.id.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("sonar: plugin {id}: {line}");
            }
        });
        Ok(Running {
            child,
            stdin,
            lines,
        })
    }

    fn ask(&mut self, request: &Request) -> Result<Answer> {
        if self.running.is_none() {
            self.running = Some(self.start()?);
        }
        let running = self.running.as_mut().context("the plugin isn't running")?;
        let line = serde_json::to_string(request)?;
        let answer = writeln!(running.stdin, "{line}")
            .and_then(|()| running.stdin.flush())
            .map_err(anyhow::Error::from)
            .and_then(|()| match running.lines.recv_timeout(PER_FILE) {
                Ok(Ok(line)) => serde_json::from_str::<Answer>(&line)
                    .map_err(|err| anyhow!("its answer isn't JSON Sonar understands: {err}")),
                Ok(Err(err)) => Err(err.into()),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    Err(anyhow!("it took longer than {} s", PER_FILE.as_secs()))
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!("it stopped")),
            });
        if answer.is_err() {
            // Started afresh for the next file.
            self.running = None;
        }
        answer
    }
}

impl Processor for ProcessorPlugin {
    fn id(&self) -> &str {
        &self.manifest.id
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn kinds(&self) -> &[Kind] {
        &self.kinds
    }

    fn frames(&self) -> usize {
        self.manifest.process.as_ref().map_or(0, |p| p.frames)
    }

    fn is_local(&self) -> bool {
        self.model.as_ref().is_none_or(|m| m.local)
    }

    fn process(&mut self, job: &Job) -> Result<Output> {
        let settings = std::mem::take(&mut self.settings);
        let labels = std::mem::take(&mut self.labels);
        let model = self.model.take();
        let answer = self.ask(&Request {
            path: job.path,
            kind: job.kind.as_str(),
            hash: &job.hash,
            frames: &job.frames,
            duration: job.duration,
            settings: &settings,
            model: model.as_ref(),
            labels: &labels,
        });
        self.settings = settings;
        self.labels = labels;
        self.model = model;
        let answer = answer.with_context(|| format!("plugin {}", self.manifest.id))?;
        if let Some(error) = answer.error {
            bail!("{error}");
        }
        let fingerprint = answer
            .fingerprint
            .map(|f| {
                u64::from_str_radix(&f.bits, 16)
                    .map(|bits| Fingerprint { algo: f.algo, bits })
                    .map_err(|_| anyhow!("the fingerprint isn't 16 hex digits"))
            })
            .transpose()?;
        Ok(Output {
            text: answer.text.filter(|t| !t.trim().is_empty()),
            tags: clean(answer.tags),
            labels: clean(answer.labels),
            fingerprint,
        })
    }
}

/// Tags and labels without blanks or repeats, lowercase, at most 20.
fn clean(words: Vec<String>) -> Vec<String> {
    let mut clean: Vec<String> = Vec::new();
    for word in words {
        let word = word.trim().to_lowercase();
        if !word.is_empty() && !clean.contains(&word) {
            clean.push(word);
        }
    }
    clean.truncate(20);
    clean
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;

    fn plugin(dir: &Path, script: &str) -> Manifest {
        fs::write(dir.join("process.sh"), script).unwrap();
        fs::write(
            dir.join("plugin.toml"),
            "name = \"Test\"\n\n[process]\nkinds = [\"image\"]\ncommand = [\"sh\", \"process.sh\"]\nversion = \"3\"\nmodel = \"model\"\n",
        )
        .unwrap();
        Manifest::read(dir).unwrap()
    }

    fn job(path: &Path) -> Job<'_> {
        Job {
            path,
            kind: Kind::Image,
            hash: "ab".into(),
            frames: Vec::new(),
            duration: None,
        }
    }

    #[test]
    fn processes_through_the_plugin_program() {
        let dir = tempfile::tempdir().unwrap();
        // Answers each line with tags, and the model's name it was sent.
        let manifest = plugin(
            dir.path(),
            r#"while read -r line; do
                 name=$(printf '%s' "$line" | sed 's/.*"name":"\([^"]*\)".*/\1/')
                 printf '{"text":"a lighthouse","tags":["Sea","sea"," "],"fingerprint":{"algo":"t","bits":"00000000000000ff"},"labels":["%s"]}\n' "$name"
               done"#,
        );
        assert!(!manifest.searches());
        let model = Model {
            url: "http://localhost:11434/v1".into(),
            key: None,
            name: "tiny".into(),
            local: true,
        };
        let mut processor = ProcessorPlugin::new(
            manifest,
            dir.path().join("data"),
            Map::new(),
            Some(model),
            BTreeMap::new(),
            |_| {},
        )
        .unwrap();
        assert_eq!(processor.version(), "3/tiny");
        assert_eq!(processor.kinds(), [Kind::Image]);
        assert!(processor.is_local());
        let output = processor.process(&job(Path::new("/tmp/a.jpg"))).unwrap();
        assert_eq!(output.text.as_deref(), Some("a lighthouse"));
        assert_eq!(output.tags, ["sea"]);
        assert_eq!(output.labels, ["tiny"]);
        assert_eq!(output.fingerprint.unwrap().bits, 255);
        // The program keeps running between files.
        assert!(processor.process(&job(Path::new("/tmp/b.jpg"))).is_ok());
    }

    #[test]
    fn errors_and_broken_answers_fail_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = plugin(
            dir.path(),
            r#"read -r line; echo '{"error":"no model"}'; read -r line; echo 'not json'"#,
        );
        let mut processor = ProcessorPlugin::new(
            manifest,
            dir.path().join("data"),
            Map::new(),
            None,
            BTreeMap::new(),
            |_| {},
        )
        .unwrap();
        let err = processor
            .process(&job(Path::new("/tmp/a.jpg")))
            .unwrap_err();
        assert!(format!("{err:#}").contains("no model"), "{err:#}");
        let err = processor
            .process(&job(Path::new("/tmp/a.jpg")))
            .unwrap_err();
        assert!(format!("{err:#}").contains("isn't JSON"), "{err:#}");
    }
}
