//! Starting other programs: plugins, `run` actions, opening files and helpers like
//! gsettings.

use std::{env, process::Command};

/// Set by the AppImage's start script without pointing into the AppImage, so they
/// can't be recognized by their value.
const APPIMAGE_SETTINGS: [&str; 6] = [
    "GDK_BACKEND",
    "GTK_THEME",
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
];

/// Gives a program the environment it would get if the user had started it. Inside
/// an AppImage, Sonar's environment points Python, GTK, GIO and the library loader at
/// the AppImage's bundled copies, which break other programs: a Python plugin can't
/// find its standard library and Text Editor loads the wrong GTK.
pub fn clean(command: &mut Command) {
    let Some(appdir) = env::var_os("APPDIR") else {
        return;
    };
    for (key, value) in outside_appimage(env::vars(), &appdir.to_string_lossy()) {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
}

/// The variables to change, and their new values, `None` to remove them. Lists like
/// `PATH` keep their entries that don't point into the AppImage.
fn outside_appimage(
    vars: impl Iterator<Item = (String, String)>,
    appdir: &str,
) -> Vec<(String, Option<String>)> {
    vars.filter_map(|(key, value)| {
        if APPIMAGE_SETTINGS.contains(&key.as_str()) {
            return Some((key, None));
        }
        if !value.contains(appdir) {
            return None;
        }
        let kept: Vec<&str> = value
            .split(':')
            .filter(|entry| !entry.is_empty() && !entry.contains(appdir))
            .collect();
        Some((key, (!kept.is_empty()).then(|| kept.join(":"))))
    })
    .collect()
}

/// Plugins run in the background, so on Windows they get no console window.
pub fn prepare_plugin(command: &mut Command) {
    clean(command);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

/// Opens a file, folder or URL in its default app.
pub fn open(app: &tauri::AppHandle, target: &str) -> Result<(), String> {
    // The opener plugin starts xdg-open with Sonar's own environment, which is the
    // AppImage's on Linux; start it with the user's instead.
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        let mut command = Command::new("xdg-open");
        command.arg(target);
        clean(&mut command);
        let mut child = command
            .spawn()
            .map_err(|err| format!("couldn't run xdg-open: {err}"))?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_opener::OpenerExt;
        let opener = app.opener();
        let opened = if target.contains("://") || target.starts_with("mailto:") {
            opener.open_url(target, None::<&str>)
        } else {
            opener.open_path(target, None::<&str>)
        };
        opened.map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_what_points_into_the_appimage() {
        let dir = "/tmp/.mount_sonar.pbiFEK";
        let vars = [
            ("PYTHONHOME", "/tmp/.mount_sonar.pbiFEK/usr/"),
            (
                "PYTHONPATH",
                "/tmp/.mount_sonar.pbiFEK/usr/share/pyshared/:",
            ),
            (
                "PATH",
                "/tmp/.mount_sonar.pbiFEK/usr/bin/:/usr/local/bin:/usr/bin",
            ),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_sonar.pbiFEK/usr/share/:/usr/share",
            ),
            ("GDK_BACKEND", "x11"),
            ("HOME", "/home/me"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()));
        let changes = outside_appimage(vars.into_iter(), dir);
        let get = |key: &str| {
            changes
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("PYTHONHOME"), Some(None));
        assert_eq!(get("PYTHONPATH"), Some(None));
        assert_eq!(get("PATH"), Some(Some("/usr/local/bin:/usr/bin".into())));
        assert_eq!(get("XDG_DATA_DIRS"), Some(Some("/usr/share".into())));
        assert_eq!(get("GDK_BACKEND"), Some(None));
        assert_eq!(get("HOME"), None);
    }
}
