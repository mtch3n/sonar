use anyhow::Result;
use tauri::App;

#[cfg(target_os = "macos")]
pub const LABEL: &str = "⌥Space";
#[cfg(target_os = "windows")]
pub const LABEL: &str = "Alt+Space";
#[cfg(target_os = "linux")]
pub const LABEL: &str = "Ctrl+Alt+Space";

#[cfg(not(target_os = "linux"))]
pub fn setup(app: &App) -> Result<()> {
    use tauri_plugin_global_shortcut::ShortcutState;

    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_shortcuts(["alt+space"])?
            .with_handler(|app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    crate::window::toggle(app);
                }
            })
            .build(),
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn setup(_app: &App) -> Result<()> {
    let exe = std::env::current_exe()?;
    let command = format!("\"{}\" {}", exe.display(), crate::TOGGLE);
    let on_gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("GNOME"));
    if on_gnome {
        gnome::ensure_shortcut(&command)
    } else {
        eprintln!("sonar: bind a keyboard shortcut in your desktop settings to run: {command}");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod gnome {
    use std::process::Command;

    use anyhow::{Context, Result, bail};

    const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/sonar/";
    const BINDING: &str = "<Control><Alt>space";

    pub fn ensure_shortcut(command: &str) -> Result<()> {
        let entry = format!("{SCHEMA}.custom-keybinding:{PATH}");
        let listed = gsettings(&["get", SCHEMA, "custom-keybindings"])?;
        let mut paths: Vec<&str> = listed.split('\'').skip(1).step_by(2).collect();
        if !paths.contains(&PATH) {
            gsettings(&["set", &entry, "name", "'Sonar'"])?;
            gsettings(&["set", &entry, "binding", &format!("'{BINDING}'")])?;
            paths.push(PATH);
            let list: Vec<String> = paths.iter().map(|p| format!("'{p}'")).collect();
            gsettings(&[
                "set",
                SCHEMA,
                "custom-keybindings",
                &format!("[{}]", list.join(", ")),
            ])?;
        }
        gsettings(&["set", &entry, "command", &format!("'{command}'")])?;
        Ok(())
    }

    fn gsettings(args: &[&str]) -> Result<String> {
        let output = Command::new("gsettings")
            .args(args)
            .output()
            .context("running gsettings")?;
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
