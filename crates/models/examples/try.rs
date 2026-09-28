//! Embeds an index with a model and runs searches against it:
//! `cargo run --release -p sonar-models --example try -- <index.db> <model> <query>...`

use std::{path::Path, time::Instant};

use sonar_core::{Index, Query};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [db, model, queries @ ..] = &args[..] else {
        anyhow::bail!("usage: try <index.db> <model> <query>...");
    };
    let dir = Path::new(db).with_file_name("models");
    let load = sonar_models::Load {
        download: true,
        threads: sonar_models::threads_for_background(),
    };
    let started = Instant::now();
    let mut embedder = sonar_models::load(model, &dir, load)?;
    println!("loaded {model} in {:.1?}", started.elapsed());
    let mut index = Index::open(Path::new(db))?;
    let started = Instant::now();
    let stats = index.embed(embedder.as_mut(), &mut |_| true)?;
    println!("embedded {stats:?} in {:.1?}", started.elapsed());
    let home = dirs_home();
    for q in queries {
        let started = Instant::now();
        let hits = index.search_with(&Query::parse(q, &home)?, embedder.as_mut())?;
        println!("\n== {q}  ({:.1?})", started.elapsed());
        for hit in hits.iter().take(6) {
            println!(
                "  {:7} {}  | {}",
                hit.kind.as_str(),
                hit.path.replace(home.to_str().unwrap(), "~"),
                hit.line.as_deref().unwrap_or("")
            );
        }
    }
    Ok(())
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap()
}
