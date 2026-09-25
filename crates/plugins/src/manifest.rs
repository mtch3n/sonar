use std::{
    fs,
    path::{Path, PathBuf},
};

use base64::Engine;
use serde::Deserialize;

pub const FILE: &str = "plugin.toml";

/// Icons are inlined into the search window, so they stay small.
const MAX_ICON_BYTES: u64 = 256 * 1024;

/// A plugin folder's `plugin.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    /// The folder name. Settings refer to the plugin by it.
    pub id: String,
    pub dir: PathBuf,
    pub name: String,
    pub description: Option<String>,
    pub command: Vec<String>,
    pub keyword: Option<String>,
    /// The icon as a `data:` URL.
    pub icon: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    name: String,
    description: Option<String>,
    command: Vec<String>,
    keyword: Option<String>,
    icon: Option<PathBuf>,
}

/// Every plugin in `dir`, one per folder, sorted by id, and a message for each
/// folder whose `plugin.toml` couldn't be used.
pub fn discover(dir: &Path) -> (Vec<Manifest>, Vec<String>) {
    let mut manifests = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return (manifests, problems);
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        let file = folder.join(FILE);
        if !file.is_file() {
            continue;
        }
        match Manifest::read(&folder) {
            Ok(manifest) => manifests.push(manifest),
            Err(err) => problems.push(format!("{}: {err}", file.display())),
        }
    }
    manifests.sort_by(|a, b| a.id.cmp(&b.id));
    (manifests, problems)
}

impl Manifest {
    pub fn read(dir: &Path) -> Result<Manifest, String> {
        let text = fs::read_to_string(dir.join(FILE)).map_err(|err| err.to_string())?;
        let raw: Raw = toml::from_str(&text).map_err(|err| err.message().to_owned())?;
        if raw.command.first().is_none_or(|program| program.is_empty()) {
            return Err("`command` needs at least the program to run".into());
        }
        if let Some(keyword) = &raw.keyword {
            check_keyword(keyword)?;
        }
        let icon = raw.icon.map(|icon| data_url(&dir.join(icon))).transpose()?;
        Ok(Manifest {
            id: dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dir: dir.to_owned(),
            name: raw.name,
            description: raw.description,
            command: raw.command,
            keyword: raw.keyword,
            icon,
        })
    }

    /// The program to start. A path with a folder in it is relative to the plugin
    /// folder; a bare name is looked up on `PATH`.
    pub(crate) fn program(&self) -> PathBuf {
        let program = Path::new(&self.command[0]);
        if program.components().count() > 1 {
            self.dir.join(program)
        } else {
            program.to_owned()
        }
    }
}

pub fn check_keyword(keyword: &str) -> Result<(), String> {
    if keyword.is_empty() || keyword.contains(char::is_whitespace) {
        Err(format!("keyword `{keyword}` must be one word"))
    } else {
        Ok(())
    }
}

fn data_url(path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase());
    let mime = match ext.as_deref() {
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => {
            return Err(format!(
                "icon {} must be PNG, SVG, JPEG or WebP",
                path.display()
            ));
        }
    };
    let size = fs::metadata(path)
        .map_err(|err| format!("icon {}: {err}", path.display()))?
        .len();
    if size > MAX_ICON_BYTES {
        return Err(format!("icon {} is over 256 KB", path.display()));
    }
    let bytes = fs::read(path).map_err(|err| format!("icon {}: {err}", path.display()))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(root: &Path, id: &str, manifest: &str) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FILE), manifest).unwrap();
    }

    #[test]
    fn discovers_plugins_and_reports_broken_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        plugin(
            root,
            "web",
            "name = \"Web search\"\ncommand = [\"python3\", \"web.py\"]\nkeyword = \"g\"\nicon = \"icon.svg\"",
        );
        fs::write(root.join("web").join("icon.svg"), "<svg/>").unwrap();
        plugin(root, "clock", "name = \"Clock\"\ncommand = [\"bin/clock\"]");
        plugin(root, "typo", "name = \"Typo\"\ncomand = [\"x\"]");
        plugin(
            root,
            "spaced",
            "name = \"S\"\ncommand = [\"x\"]\nkeyword = \"a b\"",
        );
        fs::create_dir_all(root.join("notes")).unwrap();

        let (manifests, problems) = discover(root);
        let ids: Vec<&str> = manifests.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["clock", "web"]);
        assert_eq!(problems.len(), 2, "{problems:?}");

        let web = &manifests[1];
        assert_eq!(web.keyword.as_deref(), Some("g"));
        assert!(
            web.icon
                .as_deref()
                .unwrap()
                .starts_with("data:image/svg+xml;base64,")
        );
        assert_eq!(web.program(), PathBuf::from("python3"));
        assert_eq!(manifests[0].program(), root.join("clock").join("bin/clock"));
    }

    #[test]
    fn missing_folder_has_no_plugins() {
        let (manifests, problems) = discover(Path::new("/nonexistent/sonar/plugins"));
        assert!(manifests.is_empty() && problems.is_empty());
    }
}
