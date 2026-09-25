use std::{
    process::Stdio,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{Notify, oneshot},
    task::JoinHandle,
    time::timeout,
};

use crate::{Action, Item, Manifest};

/// The most items Sonar takes from one answer.
const MAX_ITEMS: usize = 50;

/// Adjusts how plugin programs are started, e.g. to clear environment variables
/// that only make sense inside Sonar's own process.
pub type Prepare = fn(&mut std::process::Command);

/// A plugin that runs as its own program. It starts on the first search and stops
/// when this value is dropped.
pub struct External {
    pub manifest: Manifest,
    prepare: Prepare,
    answer_within: Duration,
    queue: Arc<Queue>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

/// Holds only the newest search, so a plugin never works through queries the user
/// has already typed past.
#[derive(Default)]
struct Queue {
    next: Mutex<Option<Request>>,
    wake: Notify,
}

struct Request {
    query: String,
    reply: oneshot::Sender<Result<Vec<Item>, String>>,
}

impl External {
    pub fn new(manifest: Manifest, prepare: Prepare, answer_within: Duration) -> External {
        External {
            manifest,
            prepare,
            answer_within,
            queue: Arc::default(),
            worker: Mutex::new(None),
        }
    }

    /// Asks the plugin for `query`. Gives `None` when a newer search replaced this
    /// one before the plugin got to it.
    pub async fn search(&self, query: &str) -> Option<Result<Vec<Item>, String>> {
        let (reply, answer) = oneshot::channel();
        *lock(&self.queue.next) = Some(Request {
            query: query.to_owned(),
            reply,
        });
        lock(&self.worker).get_or_insert_with(|| {
            tokio::spawn(work(
                self.manifest.clone(),
                self.prepare,
                self.answer_within,
                self.queue.clone(),
            ))
        });
        self.queue.wake.notify_one();
        answer.await.ok()
    }
}

impl Drop for External {
    fn drop(&mut self) {
        if let Some(worker) = lock(&self.worker).take() {
            worker.abort();
        }
    }
}

async fn work(manifest: Manifest, prepare: Prepare, answer_within: Duration, queue: Arc<Queue>) {
    let mut running: Option<Process> = None;
    loop {
        queue.wake.notified().await;
        let Some(request) = lock(&queue.next).take() else {
            continue;
        };
        let process = match running.take() {
            Some(process) => Ok(process),
            None => Process::start(&manifest, prepare),
        };
        let answer = match process {
            Ok(mut process) => match process.ask(&request.query, answer_within).await {
                Ok(answer) => {
                    running = Some(process);
                    answer
                }
                Err(broken) => Err(broken),
            },
            Err(err) => Err(err),
        };
        let _ = request.reply.send(answer);
    }
}

struct Process {
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    stderr: JoinHandle<()>,
    last_error: Arc<Mutex<String>>,
}

#[derive(Serialize)]
struct Question<'q> {
    query: &'q str,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    items: Vec<Item>,
    error: Option<String>,
}

impl Process {
    fn start(manifest: &Manifest, prepare: Prepare) -> Result<Process, String> {
        let mut command = std::process::Command::new(manifest.program());
        command
            .args(&manifest.command[1..])
            .current_dir(&manifest.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        prepare(&mut command);
        let mut child = Command::from(command)
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| format!("couldn't start `{}`: {err}", manifest.command[0]))?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err("couldn't connect to the plugin".into());
        };

        let last_error = Arc::new(Mutex::new(String::new()));
        let id = manifest.id.clone();
        let tail = last_error.clone();
        let stderr = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("sonar: plugin {id}: {line}");
                if !line.trim().is_empty() {
                    *lock(&tail) = line;
                }
            }
        });
        Ok(Process {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            stderr,
            last_error,
        })
    }

    /// The outer error means the process is unusable and has to be restarted; the
    /// inner one is an error the plugin reported itself.
    async fn ask(
        &mut self,
        query: &str,
        within: Duration,
    ) -> Result<Result<Vec<Item>, String>, String> {
        let mut question =
            serde_json::to_string(&Question { query }).map_err(|err| err.to_string())?;
        question.push('\n');
        let exchange = async {
            self.stdin.write_all(question.as_bytes()).await?;
            self.stdin.flush().await?;
            self.stdout.next_line().await
        };
        let line = match timeout(within, exchange).await {
            Err(_) => {
                return Err(format!(
                    "didn't answer within {} seconds",
                    within.as_secs_f32()
                ));
            }
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None) | Err(_)) => return Err(self.stopped().await),
        };
        let answer: Answer = serde_json::from_str(&line).map_err(|err| {
            format!(
                "answered with something other than one line of JSON ({err}); write logs to stderr"
            )
        })?;
        Ok(match answer.error {
            Some(error) => Err(error),
            None => check(answer.items),
        })
    }

    async fn stopped(&mut self) -> String {
        // Give the last lines of stderr a moment to arrive; they usually say why.
        let _ = timeout(Duration::from_millis(300), &mut self.stderr).await;
        match lock(&self.last_error).as_str() {
            "" => "stopped without answering".to_owned(),
            line => format!("stopped: {line}"),
        }
    }
}

fn check(mut items: Vec<Item>) -> Result<Vec<Item>, String> {
    items.truncate(MAX_ITEMS);
    let empty_run = |action: &Action| matches!(action, Action::Run(argv) if argv.is_empty());
    if items
        .iter()
        .any(|item| empty_run(&item.action) || item.alt.as_ref().is_some_and(empty_run))
    {
        return Err("sent a `run` action without a program".into());
    }
    Ok(items)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
