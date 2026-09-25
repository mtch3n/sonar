//! The keyboard shortcut that opens the search bar, as set in `settings.toml`.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::settings::Shortcut;

/// The shortcut Sonar last set up, so it is only set up again when it changes.
#[derive(Default)]
pub struct Hotkey(Mutex<Option<Shortcut>>);

/// Sets up `shortcut` unless it already is.
pub fn apply(app: &AppHandle, shortcut: &Shortcut) -> Result<(), String> {
    let state = app.state::<Hotkey>();
    let mut current = state.0.lock().unwrap_or_else(|p| p.into_inner());
    if current.as_ref() == Some(shortcut) {
        return Ok(());
    }
    // Remember it even when it fails, so a broken shortcut is reported once per change.
    *current = Some(shortcut.clone());
    register(app, shortcut)
}

#[cfg(not(target_os = "linux"))]
pub fn setup(app: &tauri::App) -> anyhow::Result<()> {
    use tauri_plugin_global_shortcut::ShortcutState;

    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    crate::window::toggle(app);
                }
            })
            .build(),
    )?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn register(app: &AppHandle, shortcut: &Shortcut) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let manager = app.global_shortcut();
    manager.unregister_all().map_err(|err| err.to_string())?;
    manager
        .register(shortcut.accelerator().as_str())
        .map_err(|err| format!("Couldn't use the shortcut {}: {err}", shortcut.label()))
}

#[cfg(target_os = "linux")]
pub fn setup(_app: &tauri::App) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn register(_app: &AppHandle, shortcut: &Shortcut) -> Result<(), String> {
    let exe = match std::env::var_os("APPIMAGE") {
        Some(appimage) => std::path::PathBuf::from(appimage),
        None => std::env::current_exe().map_err(|err| err.to_string())?,
    };
    let command = format!("\"{}\" {}", exe.display(), crate::TOGGLE);
    let on_gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("GNOME"));
    if on_gnome {
        gnome::set_shortcut(&command, &shortcut.gnome())
            .map_err(|err| format!("Couldn't set the shortcut in GNOME: {err:#}"))
    } else {
        eprintln!(
            "sonar: bind {} in your desktop's keyboard settings to run: {command}",
            shortcut.label()
        );
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod gnome {
    use std::process::Command;

    use anyhow::{Context, Result, bail};

    const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/sonar/";

    pub fn set_shortcut(command: &str, binding: &str) -> Result<()> {
        let entry = format!("{SCHEMA}.custom-keybinding:{PATH}");
        let listed = gsettings(&["get", SCHEMA, "custom-keybindings"])?;
        let mut paths: Vec<&str> = listed.split('\'').skip(1).step_by(2).collect();
        gsettings(&["set", &entry, "name", "'Sonar'"])?;
        gsettings(&["set", &entry, "binding", &format!("'{binding}'")])?;
        gsettings(&["set", &entry, "command", &format!("'{command}'")])?;
        if !paths.contains(&PATH) {
            paths.push(PATH);
            let list: Vec<String> = paths.iter().map(|p| format!("'{p}'")).collect();
            gsettings(&[
                "set",
                SCHEMA,
                "custom-keybindings",
                &format!("[{}]", list.join(", ")),
            ])?;
        }
        Ok(())
    }

    fn gsettings(args: &[&str]) -> Result<String> {
        let mut command = Command::new("gsettings");
        command.args(args);
        crate::host::clean(&mut command);
        let output = command.output().context("running gsettings")?;
        if !output.status.success() {
            bail!(
                "gsettings {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
