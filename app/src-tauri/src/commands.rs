use std::{
    path::{MAIN_SEPARATOR, Path, PathBuf},
    sync::Mutex,
};

use serde::Serialize;
use sonar_core::{Hit, Index, Paths, Query};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::window;

pub struct Searcher {
    index: Mutex<Index>,
    home: PathBuf,
}

impl Searcher {
    pub fn open(paths: &Paths) -> anyhow::Result<Searcher> {
        Ok(Searcher {
            index: Mutex::new(Index::open(&paths.db)?),
            home: paths.home.clone(),
        })
    }
}

#[derive(Serialize)]
pub struct Results {
    hits: Vec<Row>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    path: String,
    name: String,
    folder: String,
    kind: &'static str,
    size: Option<u64>,
    modified: i64,
}

#[tauri::command]
pub async fn search(searcher: State<'_, Searcher>, query: String) -> Result<Results, String> {
    let query = match Query::parse(&query, &searcher.home) {
        Ok(query) => query,
        Err(err) => {
            return Ok(Results {
                hits: Vec::new(),
                error: Some(err.to_string()),
            });
        }
    };
    let hits = searcher
        .index
        .lock()
        .map_err(|err| err.to_string())?
        .search(&query)
        .map_err(|err| format!("{err:#}"))?;
    Ok(Results {
        hits: hits
            .into_iter()
            .map(|hit| row(hit, &searcher.home))
            .collect(),
        error: None,
    })
}

#[tauri::command]
pub fn open(app: AppHandle, searcher: State<'_, Searcher>, path: String) -> Result<(), String> {
    check_under_home(&path, &searcher.home)?;
    app.opener()
        .open_path(&path, None::<&str>)
        .map_err(|err| err.to_string())?;
    window::hide(&app);
    Ok(())
}

#[tauri::command]
pub fn reveal(app: AppHandle, searcher: State<'_, Searcher>, path: String) -> Result<(), String> {
    check_under_home(&path, &searcher.home)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|err| err.to_string())?;
    window::hide(&app);
    Ok(())
}

fn check_under_home(path: &str, home: &Path) -> Result<(), String> {
    if Path::new(path).starts_with(home) {
        Ok(())
    } else {
        Err(format!("{path} is outside the indexed folder"))
    }
}

fn row(hit: Hit, home: &Path) -> Row {
    let folder = Path::new(&hit.path)
        .parent()
        .map(|parent| match parent.strip_prefix(home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Ok(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
            Err(_) => parent.display().to_string(),
        })
        .unwrap_or_default();
    Row {
        folder,
        kind: hit.kind.as_str(),
        size: hit.size,
        modified: hit.mtime,
        name: hit.name,
        path: hit.path,
    }
}
