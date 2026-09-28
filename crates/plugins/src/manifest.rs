use std::{
    fs,
    path::{Path, PathBuf},
};

use base64::Engine;
use serde::Deserialize;

use crate::setting::{self, Setting};

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
    /// The program that answers searches, and its arguments; empty for a plugin that
    /// only processes files.
    pub command: Vec<String>,
    pub keyword: Option<String>,
    /// The icon as a `data:` URL.
    pub icon: Option<String>,
    pub settings: Vec<Setting>,
    pub position: Position,
    /// Programs the plugin needs, like `python3`, and how to get them.
    pub requires: Vec<Requirement>,
    /// The systems the plugin works on; empty means all of them.
    pub platforms: Vec<Platform>,
    /// How the plugin processes files, if it does.
    pub process: Option<Process>,
}

/// `[process]`: a plugin that looks at files as they're indexed and says what it
/// learned, like a description of a photo or a fingerprint of a video.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Process {
    /// The kinds of file it's given, like `image` and `video`.
    pub kinds: Vec<String>,
    /// The program that processes, and its arguments.
    pub command: Vec<String>,
    /// Changes when the plugin would say something different, so files are looked
    /// at again.
    #[serde(default = "first_version")]
    pub version: String,
    /// Frames of each video it wants, instead of the video itself.
    #[serde(default)]
    pub frames: usize,
    /// The key of the plugin's own setting that names the model it uses, as
    /// `provider:model`; Sonar sends that provider's address and key with each file.
    pub model: Option<String>,
}

fn first_version() -> String {
    "1".into()
}

/// An operating system a plugin can say it works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}

impl Platform {
    /// The system Sonar is running on.
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    fn name(self) -> &'static str {
        match self {
            Platform::Linux => "Linux",
            Platform::Macos => "macOS",
            Platform::Windows => "Windows",
        }
    }
}

/// Where a plugin without a keyword shows its results.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    /// Above the files, like the calculator's answers.
    Top,
    /// Below the files.
    #[default]
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub program: String,
    /// What to do when the program is missing, like where to download it.
    pub help: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    name: String,
    description: Option<String>,
    #[serde(default)]
    command: Vec<String>,
    keyword: Option<String>,
    icon: Option<PathBuf>,
    #[serde(default)]
    settings: Vec<setting::Raw>,
    #[serde(default)]
    position: Position,
    #[serde(default)]
    requires: Vec<Requirement>,
    #[serde(default)]
    platforms: Vec<Platform>,
    process: Option<Process>,
}

/// Every plugin in `dir` that works on this system, one per folder, sorted by id,
/// and a message for each folder whose `plugin.toml` couldn't be used.
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
            Ok(manifest) if manifest.runs_here() => manifests.push(manifest),
            Ok(_) => {}
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
        let runs = |command: &[String]| command.first().is_some_and(|p| !p.is_empty());
        if raw.process.is_none() && !runs(&raw.command) {
            return Err("`command` needs at least the program to run".into());
        }
        if let Some(process) = &raw.process {
            if !runs(&process.command) {
                return Err("`process.command` needs at least the program to run".into());
            }
            if process.kinds.is_empty() {
                return Err("`process.kinds` needs the kinds of file to process".into());
            }
        }
        if let Some(keyword) = &raw.keyword {
            check_keyword(keyword)?;
        }
        let icon = raw.icon.map(|icon| data_url(&dir.join(icon))).transpose()?;
        let settings = Setting::from_raw(raw.settings)?;
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
            settings,
            position: raw.position,
            requires: raw.requires,
            platforms: raw.platforms,
            process: raw.process,
        })
    }

    /// Whether the plugin works on the system Sonar is running on.
    pub fn runs_here(&self) -> bool {
        self.platforms.is_empty() || self.platforms.contains(&Platform::current())
    }

    /// Why the plugin can't be installed here, when it's made for other systems.
    pub fn unsupported(&self) -> Option<String> {
        (!self.runs_here()).then(|| {
            let names: Vec<&str> = self.platforms.iter().map(|p| p.name()).collect();
            format!("{} works on {} only", self.name, names.join(" and "))
        })
    }

    /// Why the plugin can't run here: the help of the first program it needs that
    /// isn't installed.
    pub fn missing(&self) -> Option<String> {
        self.requires
            .iter()
            .find(|requirement| !on_path(&requirement.program))
            .map(|requirement| format!("Needs {}. {}", requirement.program, requirement.help))
    }

    /// Whether the plugin answers searches, rather than only processing files.
    pub fn searches(&self) -> bool {
        !self.command.is_empty()
    }

    /// The program to start for searches.
    pub(crate) fn program(&self) -> PathBuf {
        self.resolve(&self.command[0])
    }

    /// A program named in `plugin.toml`. A path with a folder in it is relative to
    /// the plugin folder; a bare name is looked up on `PATH`.
    pub(crate) fn resolve(&self, program: &str) -> PathBuf {
        let program = Path::new(program);
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

fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: &[&str] = if cfg!(windows) {
        &["exe", "cmd", "bat", "com"]
    } else {
        &[""]
    };
    std::env::split_paths(&path).any(|dir| {
        exts.iter().any(|ext| {
            dir.join(program).with_extension(ext).is_file() || dir.join(program).is_file()
        })
    })
}

/// Pictures on results are drawn this many pixels wide at most; larger ones are
/// scaled down, so a 1024-pixel app icon doesn't cost a megabyte per row.
const PICTURE_PIXELS: u32 = 64;
/// Raster pictures bigger than this aren't worth decoding for a small icon.
const MAX_PICTURE_BYTES: u64 = 8 * 1024 * 1024;

/// A result's picture as a `data:` URL the search window can show. SVGs are used as
/// they are; PNG, JPEG and WebP pictures are scaled down to fit the row.
pub fn image_url(path: &Path) -> Result<String, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
    if ext.as_deref() == Some("svg") {
        return data_url(path);
    }
    let size = fs::metadata(path)
        .map_err(|err| format!("{}: {err}", path.display()))?
        .len();
    if size > MAX_PICTURE_BYTES {
        return Err(format!("{} is over 8 MB", path.display()));
    }
    let picture = image::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let picture = if picture.width() > PICTURE_PIXELS || picture.height() > PICTURE_PIXELS {
        picture.thumbnail(PICTURE_PIXELS, PICTURE_PIXELS)
    } else {
        picture
    };
    let mut png = std::io::Cursor::new(Vec::new());
    picture
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
    Ok(format!("data:image/png;base64,{encoded}"))
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
        let elsewhere = if cfg!(windows) { "linux" } else { "windows" };
        plugin(
            root,
            "elsewhere",
            &format!("name = \"E\"\ncommand = [\"x\"]\nplatforms = [\"{elsewhere}\"]"),
        );

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
    fn says_which_systems_a_plugin_is_for() {
        let tmp = tempfile::tempdir().unwrap();
        plugin(
            tmp.path(),
            "tray",
            "name = \"Tray\"\ncommand = [\"x\"]\nplatforms = [\"linux\", \"macos\"]",
        );
        let tray = Manifest::read(&tmp.path().join("tray")).unwrap();
        assert_eq!(tray.platforms, [Platform::Linux, Platform::Macos]);
        assert_eq!(tray.runs_here(), !cfg!(windows));
        if cfg!(windows) {
            assert_eq!(
                tray.unsupported().as_deref(),
                Some("Tray works on Linux and macOS only")
            );
        }
        plugin(
            tmp.path(),
            "bad",
            "name = \"Bad\"\ncommand = [\"x\"]\nplatforms = [\"beos\"]",
        );
        assert!(Manifest::read(&tmp.path().join("bad")).is_err());
    }

    #[test]
    fn the_bundled_plugins_read() {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let (manifests, problems) = discover(&bundled);
        assert!(problems.is_empty(), "{problems:?}");
        let web = manifests.iter().find(|m| m.id == "web-search").unwrap();
        assert_eq!(web.settings[0].key, "first");
    }

    #[test]
    fn reads_position_and_requirements() {
        let tmp = tempfile::tempdir().unwrap();
        plugin(
            tmp.path(),
            "py",
            "name = \"Py\"\ncommand = [\"x\"]\nposition = \"top\"\n\n[[requires]]\nprogram = \"sonar-no-such-program\"\nhelp = \"Get it from example.com.\"\n",
        );
        let manifest = Manifest::read(&tmp.path().join("py")).unwrap();
        assert_eq!(manifest.position, Position::Top);
        assert_eq!(
            manifest.missing().as_deref(),
            Some("Needs sonar-no-such-program. Get it from example.com.")
        );
        plugin(
            tmp.path(),
            "sh",
            "name = \"Sh\"\ncommand = [\"x\"]\n\n[[requires]]\nprogram = \"sh\"\nhelp = \"-\"\n",
        );
        let manifest = Manifest::read(&tmp.path().join("sh")).unwrap();
        assert_eq!(manifest.position, Position::Bottom);
        #[cfg(unix)]
        assert_eq!(manifest.missing(), None);
    }

    #[test]
    fn scales_big_pictures_down_for_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let big = tmp.path().join("big.png");
        image::RgbaImage::from_pixel(1024, 1024, image::Rgba([255, 90, 31, 255]))
            .save(&big)
            .unwrap();
        let url = image_url(&big).unwrap();
        let png = base64::engine::general_purpose::STANDARD
            .decode(url.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        let small = image::load_from_memory(&png).unwrap();
        assert_eq!((small.width(), small.height()), (64, 64));
        assert!(png.len() < 8 * 1024, "{} bytes", png.len());

        let svg = tmp.path().join("icon.svg");
        fs::write(&svg, "<svg/>").unwrap();
        assert!(
            image_url(&svg)
                .unwrap()
                .starts_with("data:image/svg+xml;base64,")
        );
        fs::write(tmp.path().join("broken.png"), "not a png").unwrap();
        assert!(image_url(&tmp.path().join("broken.png")).is_err());
    }

    #[test]
    fn missing_folder_has_no_plugins() {
        let (manifests, problems) = discover(Path::new("/nonexistent/sonar/plugins"));
        assert!(manifests.is_empty() && problems.is_empty());
    }
}
