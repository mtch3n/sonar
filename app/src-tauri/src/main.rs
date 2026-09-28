#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bundled;
mod editor;
#[cfg(target_os = "linux")]
mod gnome;
mod host;
mod hotkey;
mod indexer;
mod launcher;
mod settings;
mod tray;
mod updater;
mod watcher;
mod window;

use sonar_core::Paths;
use sonar_plugins::clipboard;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::{
    hotkey::Hotkey,
    indexer::{Indexer, Status},
    launcher::Launcher,
    tray::Tray,
};

const TOGGLE: &str = "--toggle";
const BACKGROUND: &str = "--background";
const SETTINGS: &str = "--settings";
const QUERY: &str = "--query";

fn main() {
    // Sonar's own plugins run as their own processes of this same program.
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--plugin") => {
            let id = args.next().unwrap_or_default();
            if !bundled::serve(&id) {
                eprintln!("sonar: there's no plugin `{id}` in Sonar");
                std::process::exit(2);
            }
            return;
        }
        #[cfg(target_os = "linux")]
        Some("--install-gnome-extension") => {
            if let Err(err) = gnome::install_extension() {
                eprintln!("sonar: couldn't install the GNOME extension: {err}");
                std::process::exit(1);
            }
            return;
        }
        Some("--forget-copy") => {
            let copied = args.next().and_then(|c| c.parse().ok());
            let forgot = Paths::from_env()
                .map_err(|err| err.to_string())
                .and_then(|paths| clipboard::forget(&clipboard_dir(&paths), copied));
            if let Err(err) = forgot {
                eprintln!("sonar: couldn't remove the copy: {err}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(text) = query_arg(&args) {
                window::show_with(app, &text);
            } else if args.iter().any(|a| a == TOGGLE) {
                window::toggle(app);
            } else if args.iter().any(|a| a == SETTINGS) {
                editor::open(app);
            } else {
                window::show(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // GTK keeps a window that can't be resized at the web view's natural
            // height, so the search bar couldn't shrink to fit what it shows.
            #[cfg(target_os = "linux")]
            if let Some(window) = app.get_webview_window("main") {
                window.set_resizable(true)?;
            }
            if let Some(window) = app.get_webview_window("main") {
                window::plain_scrolling(&window);
            }

            let paths = Paths::from_env()?;
            let launcher = Launcher::open(paths.clone())?;
            let settings = launcher.settings();
            app.manage(launcher);
            app.manage(Hotkey::default());
            if let Err(err) = hotkey::setup(app) {
                eprintln!("sonar: couldn't set up the shortcut: {err:#}");
            }
            let tray = tray::create(app.handle())?;
            app.manage(tray.clone());

            #[cfg(target_os = "linux")]
            record_copies(&paths, &settings);
            let handle = app.handle().clone();
            let index_settings =
                move || settings.read().map(|s| s.index.clone()).unwrap_or_default();
            app.manage(Indexer::start(paths, index_settings, move |status| {
                if let Some(launcher) = handle.try_state::<Launcher>() {
                    launcher.set_indexing(matches!(status, Status::Indexing));
                }
                tray.show_status(&status);
                let _ = handle.emit("sonar://view", ());
            }));
            updater::watch(app.handle());
            reload(app.handle());

            let args: Vec<String> = std::env::args().collect();
            if let Some(text) = query_arg(&args) {
                window::show_with(app.handle(), &text);
            } else if args.iter().any(|a| a == SETTINGS) {
                editor::open(app.handle());
            } else if !std::env::args().any(|a| a == BACKGROUND) {
                window::show(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) if window.label() == "main" => {
                window::hide(window.app_handle())
            }
            WindowEvent::ThemeChanged(_) => {
                if let Some(tray) = window.try_state::<Tray>() {
                    tray.refresh();
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            launcher::search,
            launcher::activate,
            launcher::view,
            editor::settings_get,
            editor::settings_save,
            editor::settings_open_file,
            editor::rates_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sonar");
}

/// The text after `--query`, which opens the bar with it typed, like `--query "w "`
/// for a window switcher bound to Alt+Tab.
fn query_arg(args: &[String]) -> Option<String> {
    let at = args.iter().position(|a| a == QUERY)?;
    args.get(at + 1).cloned()
}

fn clipboard_dir(paths: &Paths) -> std::path::PathBuf {
    paths.plugin_data.join(clipboard::ID)
}

/// Keeps what's copied for the clipboard plugin, while it's turned on. XWayland can
/// stop and start again with the desktop, so the watch starts again when it ends.
#[cfg(target_os = "linux")]
fn record_copies(paths: &Paths, settings: &std::sync::Arc<std::sync::RwLock<settings::Settings>>) {
    let dir = clipboard_dir(paths);
    let settings = settings.clone();
    std::thread::spawn(move || {
        let on = || {
            settings
                .read()
                .is_ok_and(|s| s.plugin(clipboard::ID).enabled)
        };
        let mut said = None;
        loop {
            if let Err(err) = clipboard::record(&dir, on)
                && said.as_ref() != Some(&err)
            {
                eprintln!("sonar: {err}");
                said = Some(err);
            }
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });
}

/// Applies `settings.toml` and the plugin folders: runs every time the bar opens.
fn reload(app: &AppHandle) {
    let Some(launcher) = app.try_state::<Launcher>() else {
        return;
    };
    let text_kb = launcher.current_settings().index.text_kb;
    launcher.reload();
    // Files are read again at once, rather than at the next rescan.
    if launcher.current_settings().index.text_kb != text_kb
        && let Some(indexer) = app.try_state::<Indexer>()
    {
        indexer.reindex();
    }
    let shortcut = launcher.current_settings().shortcut();
    if let Err(err) = hotkey::apply(app, &shortcut) {
        launcher.add_notice(err);
    }
    if let Some(tray) = app.try_state::<Tray>() {
        tray.show_shortcut(&shortcut.label());
    }
}
