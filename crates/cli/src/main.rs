use std::{
    io::IsTerminal,
    path::{MAIN_SEPARATOR, Path},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use sonar_core::{Hit, Index, Paths, Query, Rules};
use sonar_models::Load;
use sonar_settings::Settings;

#[derive(Parser)]
#[command(name = "sonar", version, about = "Find any file in your home folder")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Scan your home folder and update the index")]
    Index,
    #[command(about = "Read and learn files or folders again, then update the index")]
    Reindex {
        #[arg(required = true, help = "Files or folders to read again")]
        paths: Vec<std::path::PathBuf>,
    },
    #[command(about = "Search the index", visible_alias = "s", after_help = SYNTAX)]
    Search {
        #[arg(help = "Words and filters, e.g. `invoice kind:pdf modified:<30d`")]
        query: Vec<String>,
    },
    #[command(about = "Update sonar to the latest release on GitHub")]
    Update,
}

const SYNTAX: &str = "\
Filters:
  kind:pdf,image      project folder app code script key pdf doc sheet
                      slides image video audio archive config other
  ext:sh              file extension
  in:~/Documents      under a path; in:trellis matches any folder named trellis
  name:readme         match the file name only
  modified:<30d       changed in the last 30 days (h d w m y); >1y for older
  after:2026-09-01    changed on or after a date; before: for earlier
  size:>10mb          bigger than; size:<100kb for smaller
  limit:50            number of results (default 20)
  -draft              leave out matches for a word
  '\"tax return\"'      exact words (keep the double quotes from the shell)

Files inside code projects only show up with kind:, ext: or in:.";

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Index => {
            let (paths, mut index) = open()?;
            run_index(&mut index, &paths)
        }
        Command::Reindex { paths: chosen } => {
            let (paths, mut index) = open()?;
            for path in &chosen {
                let path = std::path::absolute(path)?;
                let files = index.forget(&path)?;
                if files == 0 {
                    bail!("{} isn't in the index", path.display());
                }
                println!(
                    "Reading {files} files under {} again",
                    tilde(&path.to_string_lossy(), &paths.home)
                );
            }
            run_index(&mut index, &paths)
        }
        Command::Search { query } => {
            let (paths, mut index) = open()?;
            run_search(&mut index, &paths, &query.join(" "))
        }
        Command::Update => run_update(),
    }
}

fn open() -> Result<(Paths, Index)> {
    let paths = Paths::from_env()?;
    let index = Index::open(&paths.db)?;
    Ok((paths, index))
}

fn run_update() -> Result<()> {
    let target = if cfg!(target_os = "macos") {
        "universal-apple-darwin"
    } else {
        self_update::get_target()
    };
    let status = self_update::backends::github::Update::configure()
        .repo_owner("mtch3n")
        .repo_name("sonar")
        .bin_name("sonar")
        .asset_identifier("sonar-cli")
        .target(target)
        .checksum_from_asset("SHA256SUMS.txt")
        .current_version(self_update::cargo_crate_version!())
        .show_download_progress(true)
        .no_confirm(true)
        .build()?
        .update()?;
    if status.is_updated() {
        println!("Updated sonar to {}", status.version());
    } else {
        println!("sonar {} is the latest version", status.version());
    }
    Ok(())
}

fn run_index(index: &mut Index, paths: &Paths) -> Result<()> {
    let rules = Rules::load(&paths.rules, &paths.home)?;
    let started = Instant::now();
    let settings = Settings::load(&paths.settings).unwrap_or_default();
    let stats = index.scan(&paths.home, &rules, &settings.index.scan_options())?;
    println!(
        "Indexed {} files, {} folders and {} projects in {:.1}s",
        stats.files,
        stats.folders,
        stats.projects,
        started.elapsed().as_secs_f64()
    );
    if stats.removed > 0 {
        println!("Dropped {} entries that no longer exist", stats.removed);
    }
    println!(
        "Skipped paths are listed in {}",
        tilde(&paths.rules.to_string_lossy(), &paths.home)
    );
    if settings.meaning.enabled {
        embed(index, paths, &settings)?;
    }
    Ok(())
}

/// Embeds what the scan found for searching by meaning, downloading the model the
/// first time.
fn embed(index: &mut Index, paths: &Paths, settings: &Settings) -> Result<()> {
    if !settings.meaning_ready(&paths.models)
        && let Some(info) = sonar_models::info(&settings.meaning.model)
    {
        println!(
            "Downloading the {} model ({} MB)…",
            info.name, info.download_mb
        );
    }
    let load = Load {
        download: true,
        threads: std::thread::available_parallelism().map_or(4, |n| n.get()),
    };
    let mut embedder = settings
        .meaning_model(&paths.models, load)
        .map_err(anyhow::Error::msg)?;
    let (names, files) = index.pending_meaning(embedder.id())?;
    let started = Instant::now();
    let tty = std::io::stderr().is_terminal();
    let private = settings.private_folders(&paths.home);
    let stats = index.embed(embedder.as_mut(), &private, &mut |stats| {
        if tty {
            eprint!(
                "\rLearning meaning: {} of {}",
                stats.names + stats.files,
                names + files
            );
        }
        true
    })?;
    if tty && names + files > 0 {
        eprintln!();
    }
    println!(
        "Learned the meaning of {} names and {} files ({} pieces of text) in {:.1}s",
        stats.names,
        stats.files,
        stats.chunks,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn run_search(index: &mut Index, paths: &Paths, input: &str) -> Result<()> {
    if index.is_empty()? {
        bail!("the index is empty; run `sonar index` first");
    }
    let query = Query::parse(input, &paths.home)?;
    let settings = Settings::load(&paths.settings).unwrap_or_default();
    let load = Load {
        download: false,
        threads: 2,
    };
    let embedder = settings
        .meaning
        .enabled
        .then(|| settings.meaning_model(&paths.models, load).ok())
        .flatten();
    let hits = match embedder {
        Some(mut embedder) => index.search_with(&query, embedder.as_mut())?,
        None => index.search(&query)?,
    };
    if hits.is_empty() {
        println!("No matches");
        return Ok(());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let color = std::io::stdout().is_terminal();
    for hit in &hits {
        println!("{}", format_hit(hit, &paths.home, now, color));
    }
    Ok(())
}

fn format_hit(hit: &Hit, home: &Path, now: i64, color: bool) -> String {
    let (dim, bold, reset) = if color {
        ("\x1b[2m", "\x1b[1m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    let folder = Path::new(&hit.path)
        .parent()
        .map(|p| tilde(&p.to_string_lossy(), home))
        .unwrap_or_default();
    let mut details = vec![age(now - hit.mtime)];
    if let Some(size) = hit.size {
        details.push(human_size(size));
    }
    let mut out = format!(
        "{dim}{:<8}{reset}{bold}{}{reset}  {dim}{folder}  ·  {}{reset}",
        hit.kind.as_str(),
        hit.name,
        details.join(" · ")
    );
    if let Some(line) = &hit.line {
        out.push_str(&format!("\n        {dim}{line}{reset}"));
    }
    out
}

fn tilde(path: &str, home: &Path) -> String {
    match Path::new(path).strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
        Err(_) => path.to_owned(),
    }
}

fn age(secs: i64) -> String {
    const HOUR: i64 = 3600;
    const DAY: i64 = 24 * HOUR;
    match secs.max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < HOUR => format!("{}m ago", s / 60),
        s if s < DAY => format!("{}h ago", s / HOUR),
        s if s < 30 * DAY => format!("{}d ago", s / DAY),
        s if s < 365 * DAY => format!("{}mo ago", s / (30 * DAY)),
        s => format!("{}y ago", s / (365 * DAY)),
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}
