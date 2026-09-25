//! Starting other programs: plugins, `run` actions and helpers like gsettings.

use std::process::Command;

/// Variables the AppImage sets so Sonar finds its bundled libraries. Programs Sonar
/// starts need the system's own.
const APPIMAGE_ONLY: [&str; 3] = [
    "LD_LIBRARY_PATH",
    "GSETTINGS_SCHEMA_DIR",
    "GIO_EXTRA_MODULES",
];

/// Gives a program the environment it would get if the user had started it.
pub fn clean(command: &mut Command) {
    if std::env::var_os("APPIMAGE").is_some() {
        for var in APPIMAGE_ONLY {
            command.env_remove(var);
        }
    }
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
