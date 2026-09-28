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

/// Sonar's own plugins are this same program, so they keep its environment, which
/// inside an AppImage points at the libraries it needs.
pub fn prepare_bundled(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
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

/// The desktop's accent color, from the XDG settings portal that GNOME and KDE
/// both answer, as `#rrggbb`.
#[cfg(target_os = "linux")]
pub fn system_accent() -> Option<String> {
    use zbus::zvariant::OwnedValue;
    let bus = zbus::blocking::Connection::session().ok()?;
    let reply = bus
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            "ReadOne",
            &("org.freedesktop.appearance", "accent-color"),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    let rgb: (f64, f64, f64) = value.try_into().ok()?;
    hex(rgb)
}

#[cfg(not(target_os = "linux"))]
pub fn system_accent() -> Option<String> {
    None
}

/// The portal's color, whose channels run from 0 to 1; outside that there is none.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn hex((r, g, b): (f64, f64, f64)) -> Option<String> {
    let channel = |c: f64| (0.0..=1.0).contains(&c).then(|| (c * 255.0).round() as u8);
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        channel(r)?,
        channel(g)?,
        channel(b)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_portals_accent_as_hex() {
        assert_eq!(hex((0.0, 0.5, 1.0)).as_deref(), Some("#0080ff"));
        assert_eq!(hex((-1.0, -1.0, -1.0)), None);
    }

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
