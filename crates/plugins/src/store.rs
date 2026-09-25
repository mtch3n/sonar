//! Installing plugins from GitHub. A marketplace is a repository with a
//! `marketplace.toml` that lists plugins; a plugin is a folder with a `plugin.toml`,
//! either inside a marketplace repository or at the root of its own.

use std::{
    fmt, fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use crate::manifest::{self, Manifest};

const MARKETPLACE_FILE: &str = "marketplace.toml";
/// Written next to `plugin.toml` in every installed plugin, so it can be updated and
/// uninstalled. Plugins without it were made by hand and are left alone.
const SOURCE_FILE: &str = ".sonar-source.toml";
const MAX_DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;
const MAX_FILES: usize = 5_000;
const USER_AGENT: &str = concat!("sonar/", env!("CARGO_PKG_VERSION"));

/// A GitHub repository.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

impl Repo {
    /// Reads `owner/name`, `github.com/owner/name` or a GitHub URL.
    pub fn parse(text: &str) -> Option<Repo> {
        let text = text.trim();
        let path = text
            .strip_prefix("https://")
            .or_else(|| text.strip_prefix("http://"))
            .unwrap_or(text);
        let path = path
            .strip_prefix("www.")
            .unwrap_or(path)
            .strip_prefix("github.com/")
            .unwrap_or(path);
        let mut parts = path.trim_end_matches('/').split('/');
        let owner = parts.next()?;
        let name = parts.next()?;
        let name = name.strip_suffix(".git").unwrap_or(name);
        let valid = |part: &str| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        // Links to a file or folder in the repo (`/tree/main/...`) still name the repo.
        let rest_is_page = parts
            .next()
            .is_none_or(|page| matches!(page, "tree" | "blob"));
        (valid(owner) && valid(name) && rest_is_page).then(|| Repo {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }

    fn raw(&self, file: &str) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{}/HEAD/{file}",
            self.owner, self.name
        )
    }
}

impl fmt::Display for Repo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// A marketplace's `marketplace.toml`.
#[derive(Clone, Debug)]
pub struct Marketplace {
    pub repo: Repo,
    pub name: String,
    pub plugins: Vec<Listing>,
}

/// A plugin a marketplace offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source: Source,
}

/// Where a plugin's files come from: a folder in a repository, `""` for its root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    #[serde(with = "repo_text")]
    pub repo: Repo,
    #[serde(default)]
    pub path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMarketplace {
    name: String,
    #[serde(default)]
    plugins: Vec<RawListing>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawListing {
    id: String,
    name: String,
    description: Option<String>,
    /// A folder in the marketplace's own repository…
    path: Option<String>,
    /// …or a repository of its own, with `plugin.toml` at its root.
    repo: Option<String>,
}

/// What a repository turned out to hold.
pub enum Found {
    Marketplace(Marketplace),
    Plugin,
}

/// What an installed plugin folder holds besides the plugin.
#[derive(Serialize, Deserialize)]
struct Installed {
    #[serde(flatten)]
    source: Source,
    commit: String,
}

/// Downloads and reads a marketplace's listing.
pub fn marketplace(repo: &Repo) -> Result<Marketplace, String> {
    match fetch_text(&repo.raw(MARKETPLACE_FILE))? {
        Some(text) => parse_marketplace(repo, &text),
        None => Err(format!("{repo} has no {MARKETPLACE_FILE}")),
    }
}

/// Whether `repo` is a marketplace or a single plugin.
pub fn probe(repo: &Repo) -> Result<Found, String> {
    if let Some(text) = fetch_text(&repo.raw(MARKETPLACE_FILE))? {
        return parse_marketplace(repo, &text).map(Found::Marketplace);
    }
    match fetch_text(&repo.raw(manifest::FILE))? {
        Some(_) => Ok(Found::Plugin),
        None => Err(format!(
            "{repo} has neither a {MARKETPLACE_FILE} nor a {} at its root",
            manifest::FILE
        )),
    }
}

fn parse_marketplace(repo: &Repo, text: &str) -> Result<Marketplace, String> {
    let raw: RawMarketplace = toml::from_str(text)
        .map_err(|err| format!("{repo}/{MARKETPLACE_FILE}: {}", err.message()))?;
    let plugins = raw
        .plugins
        .into_iter()
        .map(|listing| {
            check_id(&listing.id)?;
            let source = match (listing.repo, listing.path) {
                (Some(other), None) => Source {
                    repo: Repo::parse(&other)
                        .ok_or_else(|| format!("`{other}` isn't a GitHub repository"))?,
                    path: String::new(),
                },
                (None, Some(path)) => Source {
                    repo: repo.clone(),
                    path: path.trim_matches('/').to_owned(),
                },
                _ => {
                    return Err(format!(
                        "plugin `{}` needs either `path` or `repo`",
                        listing.id
                    ));
                }
            };
            Ok(Listing {
                id: listing.id,
                name: listing.name,
                description: listing.description,
                source,
            })
        })
        .collect::<Result<_, String>>()
        .map_err(|err| format!("{repo}/{MARKETPLACE_FILE}: {err}"))?;
    Ok(Marketplace {
        repo: repo.clone(),
        name: raw.name,
        plugins,
    })
}

/// The id a plugin installed straight from its own repository gets.
pub fn id_for(repo: &Repo) -> String {
    let name = repo.name.to_lowercase();
    name.strip_prefix("sonar-").unwrap_or(&name).to_owned()
}

/// Where an installed plugin came from, or `None` for a plugin made by hand.
pub fn source_of(plugin_dir: &Path) -> Option<Source> {
    let text = fs::read_to_string(plugin_dir.join(SOURCE_FILE)).ok()?;
    toml::from_str::<Installed>(&text).ok().map(|i| i.source)
}

/// Downloads the newest version of `source` into `plugins/<id>`, replacing what was
/// there. Nothing in the download runs until the plugin is first searched.
pub fn install(plugins: &Path, id: &str, source: &Source) -> Result<Manifest, String> {
    check_id(id)?;
    let target = plugins.join(id);
    if target.exists() && source_of(&target).as_ref() != Some(source) {
        return Err(format!(
            "a different plugin is already installed as `{id}`; remove {} first",
            target.display()
        ));
    }
    let commit = latest_commit(&source.repo)?;
    let url = format!(
        "https://codeload.github.com/{}/{}/tar.gz/{commit}",
        source.repo.owner, source.repo.name
    );
    let staging = plugins.join(format!(".installing-{id}"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|err| err.to_string())?;
    let unpacked = fetch(&url, MAX_DOWNLOAD_BYTES)
        .and_then(|download| unpack(GzDecoder::new(download), Path::new(&source.path), &staging));
    let checked = unpacked.and_then(|()| {
        if !staging.join(manifest::FILE).is_file() {
            return Err(match source.path.as_str() {
                "" => format!("{} has no {} at its root", source.repo, manifest::FILE),
                path => format!("{}/{path} has no {}", source.repo, manifest::FILE),
            });
        }
        Manifest::read(&staging)?;
        let installed = Installed {
            source: source.clone(),
            commit,
        };
        let text = toml::to_string(&installed).map_err(|err| err.to_string())?;
        fs::write(staging.join(SOURCE_FILE), text).map_err(|err| err.to_string())
    });
    if let Err(err) = checked {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }
    replace(&staging, &target)?;
    Manifest::read(&target)
}

/// Deletes an installed plugin. Plugins made by hand are never deleted.
pub fn uninstall(plugins: &Path, id: &str) -> Result<(), String> {
    check_id(id)?;
    let target = plugins.join(id);
    if source_of(&target).is_none() {
        return Err(format!("`{id}` wasn't installed by Sonar, so it stays"));
    }
    fs::remove_dir_all(&target).map_err(|err| format!("removing {}: {err}", target.display()))
}

fn replace(staging: &Path, target: &Path) -> Result<(), String> {
    let old = target.with_file_name(format!(
        ".removing-{}",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = fs::remove_dir_all(&old);
    if target.exists() {
        fs::rename(target, &old).map_err(|err| err.to_string())?;
    }
    fs::rename(staging, target).map_err(|err| err.to_string())?;
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

/// Extracts the files under `folder` in a GitHub tarball into `dest`. GitHub wraps
/// everything in one top folder, which is dropped. Links and anything that would
/// land outside `dest` are skipped.
fn unpack(tarball: impl Read, folder: &Path, dest: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(tarball);
    let mut files = 0;
    for entry in archive.entries().map_err(|err| err.to_string())? {
        let mut entry = entry.map_err(|err| format!("reading the download: {err}"))?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            continue;
        }
        let path = entry.path().map_err(|err| err.to_string())?.into_owned();
        let Some(relative) = inside(&path, folder) else {
            continue;
        };
        files += 1;
        if files > MAX_FILES {
            return Err(format!("the plugin has more than {MAX_FILES} files"));
        }
        let out = dest.join(relative);
        if kind.is_dir() {
            fs::create_dir_all(&out).map_err(|err| err.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            }
            entry
                .unpack(&out)
                .map_err(|err| format!("writing {}: {err}", out.display()))?;
        }
    }
    Ok(())
}

/// The part of `path` under `folder`, once the tarball's top folder is dropped.
fn inside(path: &Path, folder: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    components.next()?;
    let rest = components.as_path().strip_prefix(folder).ok()?;
    let safe = rest.components().all(|c| matches!(c, Component::Normal(_)));
    (safe && rest.components().next().is_some()).then(|| rest.to_owned())
}

fn check_id(id: &str) -> Result<(), String> {
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "plugin id `{id}` may only use lowercase letters, digits, - and _"
        ))
    }
}

fn latest_commit(repo: &Repo) -> Result<String, String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/commits/HEAD",
        repo.owner, repo.name
    );
    let response = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github.sha")
        .call()
        .map_err(|err| http_error(repo, err))?;
    let sha = response
        .into_body()
        .with_config()
        .limit(1024)
        .read_to_string()
        .map_err(|err| err.to_string())?;
    let sha = sha.trim();
    if sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(sha.to_owned())
    } else {
        Err(format!("GitHub didn't say which commit {repo} is at"))
    }
}

/// A text file, or `None` when it doesn't exist.
fn fetch_text(url: &str) -> Result<Option<String>, String> {
    let response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|err| format!("couldn't reach GitHub: {err}"))?;
    match response.status().as_u16() {
        200 => response
            .into_body()
            .with_config()
            .limit(1024 * 1024)
            .read_to_string()
            .map(Some)
            .map_err(|err| err.to_string()),
        404 => Ok(None),
        status => Err(format!("GitHub answered {status} for {url}")),
    }
}

fn fetch(url: &str, limit: u64) -> Result<impl Read, String> {
    let response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|err| format!("downloading {url}: {err}"))?;
    Ok(response
        .into_body()
        .into_with_config()
        .limit(limit)
        .reader())
}

fn http_error(repo: &Repo, err: ureq::Error) -> String {
    match err {
        ureq::Error::StatusCode(404) => format!("{repo} isn't a public GitHub repository"),
        ureq::Error::StatusCode(403 | 429) => {
            "GitHub's hourly limit for downloads is used up; try again later".to_owned()
        }
        err => format!("couldn't reach GitHub: {err}"),
    }
}

mod repo_text {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use super::Repo;

    pub fn serialize<S: Serializer>(repo: &Repo, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(repo)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Repo, D::Error> {
        let text = String::deserialize(d)?;
        Repo::parse(&text).ok_or_else(|| D::Error::custom(format!("`{text}` isn't owner/name")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(text: &str) -> Option<String> {
        Repo::parse(text).map(|r| r.to_string())
    }

    #[test]
    fn reads_repos_in_the_forms_people_paste() {
        for text in [
            "alice/sonar-emoji",
            "github.com/alice/sonar-emoji",
            "https://github.com/alice/sonar-emoji",
            "https://github.com/alice/sonar-emoji.git",
            "https://github.com/alice/sonar-emoji/",
            "https://github.com/alice/sonar-emoji/tree/main/src",
        ] {
            assert_eq!(repo(text).as_deref(), Some("alice/sonar-emoji"), "{text}");
        }
        for text in [
            "alice",
            "a b/c",
            "alice/../x",
            "https://example.com/a/b/c",
            "~/Documents",
        ] {
            assert_eq!(repo(text), None, "{text}");
        }
        assert_eq!(id_for(&Repo::parse("alice/Sonar-Emoji").unwrap()), "emoji");
    }

    #[test]
    fn reads_marketplaces() {
        let home = Repo::parse("mtch3n/sonar").unwrap();
        let market = parse_marketplace(
            &home,
            r#"
            name = "Sonar"
            [[plugins]]
            id = "web-search"
            name = "Web search"
            path = "/plugins/web-search/"
            [[plugins]]
            id = "emoji"
            name = "Emoji"
            description = "Find emoji by name"
            repo = "https://github.com/alice/sonar-emoji"
            "#,
        )
        .unwrap();
        assert_eq!(market.plugins[0].source.path, "plugins/web-search");
        assert_eq!(market.plugins[0].source.repo, home);
        assert_eq!(
            market.plugins[1].source.repo.to_string(),
            "alice/sonar-emoji"
        );

        let bad = |text: &str| parse_marketplace(&home, text).unwrap_err();
        assert!(bad("name = \"x\"\n[[plugins]]\nid = \"a\"\nname = \"A\"").contains("path"));
        assert!(
            bad("name = \"x\"\n[[plugins]]\nid = \"Bad Id\"\nname = \"A\"\npath = \"a\"")
                .contains("lowercase")
        );
    }

    #[test]
    fn sonars_own_marketplace_is_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = fs::read_to_string(root.join(MARKETPLACE_FILE)).unwrap();
        let market = parse_marketplace(&Repo::parse("mtch3n/sonar").unwrap(), &text).unwrap();
        assert!(!market.plugins.is_empty());
        for listing in market.plugins {
            let manifest = Manifest::read(&root.join(&listing.source.path)).unwrap();
            assert_eq!(manifest.name, listing.name);
        }
    }

    #[test]
    fn unpacks_one_folder_of_a_github_tarball() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut add = |path: &str, body: &[u8], mode: u32| {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            builder.append_data(&mut header, path, body).unwrap();
        };
        add("sonar-abc123/README.md", b"readme", 0o644);
        add(
            "sonar-abc123/plugins/web/plugin.toml",
            b"name = \"Web\"",
            0o644,
        );
        add("sonar-abc123/plugins/web/bin/web", b"#!/bin/sh", 0o755);
        add("sonar-abc123/plugins/webby/plugin.toml", b"other", 0o644);
        let tarball = builder.into_inner().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        unpack(tarball.as_slice(), Path::new("plugins/web"), tmp.path()).unwrap();
        let mut files: Vec<String> = walk(tmp.path());
        files.sort();
        assert_eq!(files, ["bin/web", "plugin.toml"]);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(tmp.path().join("bin/web"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "keeps the executable bit");
        }
    }

    #[test]
    fn never_writes_outside_the_plugin_folder() {
        assert_eq!(
            inside(Path::new("top/a/../../etc/passwd"), Path::new("")),
            None
        );
        assert_eq!(inside(Path::new("top"), Path::new("")), None);
        assert_eq!(
            inside(Path::new("top/src/main.py"), Path::new("")),
            Some(PathBuf::from("src/main.py"))
        );
    }

    #[test]
    fn only_removes_what_sonar_installed() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("mine")).unwrap();
        assert!(uninstall(tmp.path(), "mine").is_err());
        assert!(tmp.path().join("mine").exists());

        let installed = tmp.path().join("web");
        fs::create_dir_all(&installed).unwrap();
        let record = Installed {
            source: Source {
                repo: Repo::parse("mtch3n/sonar").unwrap(),
                path: "plugins/web".into(),
            },
            commit: "0".repeat(40),
        };
        fs::write(
            installed.join(SOURCE_FILE),
            toml::to_string(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(source_of(&installed), Some(record.source));
        uninstall(tmp.path(), "web").unwrap();
        assert!(!installed.exists());
    }

    fn walk(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(
                    walk(&path)
                        .into_iter()
                        .map(|p| format!("{}/{p}", entry.file_name().to_string_lossy())),
                );
            } else {
                out.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        out
    }
}
