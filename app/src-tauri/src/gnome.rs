//! Installs the GNOME Shell extension that lets the windows plugin see and switch
//! windows, and tells Sonar what's copied for the clipboard plugin. GNOME on Wayland
//! loads a new or updated extension at the next login.

use std::{fs, path::PathBuf, process::Command};

use sonar_plugins::windows::EXTENSION;

const METADATA: &str = include_str!("../../../gnome-extension/metadata.json");
const SCRIPT: &str = include_str!("../../../gnome-extension/extension.js");

/// Writes the extension into the user's extensions folder, turns it on and says to
/// log out and back in.
pub fn install_extension() -> Result<(), String> {
    let dir = dirs_data()?
        .join("gnome-shell")
        .join("extensions")
        .join(EXTENSION);
    fs::create_dir_all(&dir).map_err(|err| format!("creating {}: {err}", dir.display()))?;
    fs::write(dir.join("metadata.json"), METADATA).map_err(|err| err.to_string())?;
    fs::write(dir.join("extension.js"), SCRIPT).map_err(|err| err.to_string())?;
    enable()?;
    notify("Log out and back in to finish setting up window switching");
    Ok(())
}

fn dirs_data() -> Result<PathBuf, String> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| "can't find the data folder".into())
}

/// Adds the extension to GNOME's list of enabled ones. `gnome-extensions enable`
/// refuses an extension the shell hasn't loaded yet, so the list is edited directly.
fn enable() -> Result<(), String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.shell", "enabled-extensions"])
        .output()
        .map_err(|err| format!("running gsettings: {err}"))?;
    let current = String::from_utf8_lossy(&output.stdout);
    let Some(list) = with_extension(current.trim()) else {
        return Ok(());
    };
    let status = Command::new("gsettings")
        .args(["set", "org.gnome.shell", "enabled-extensions", &list])
        .status()
        .map_err(|err| format!("running gsettings: {err}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "gsettings couldn't turn the extension on".into())
}

/// GNOME's list with the extension added, or `None` when it's already there.
fn with_extension(list: &str) -> Option<String> {
    let quoted = format!("'{EXTENSION}'");
    if list.contains(&quoted) {
        return None;
    }
    let inner = list
        .trim_start_matches("@as")
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    Some(if inner.is_empty() {
        format!("[{quoted}]")
    } else {
        format!("[{inner}, {quoted}]")
    })
}

fn notify(text: &str) {
    let _ = Command::new("notify-send").args(["Sonar", text]).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_the_extension_to_gnomes_list_once() {
        assert_eq!(
            with_extension("@as []").as_deref(),
            Some("['sonar-windows@mtch3n.github.io']")
        );
        assert_eq!(
            with_extension("['caffeine@patapon.info', 'Vitals@CoreCoding.com']").as_deref(),
            Some(
                "['caffeine@patapon.info', 'Vitals@CoreCoding.com', 'sonar-windows@mtch3n.github.io']"
            )
        );
        assert_eq!(with_extension("['sonar-windows@mtch3n.github.io']"), None);
    }

    #[test]
    fn the_shipped_extension_is_named_as_the_plugin_expects() {
        assert!(METADATA.contains(&format!("\"uuid\": \"{EXTENSION}\"")));
        assert!(SCRIPT.contains("io.github.mtch3n.Sonar.Windows"));
        assert!(SCRIPT.contains("io.github.mtch3n.Sonar.Clipboard"));
    }
}
