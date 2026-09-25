#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod hotkey;
mod indexer;
mod tray;
mod updater;
mod window;

use sonar_core::Paths;
use tauri::{Manager, WindowEvent};

use crate::{commands::Searcher, indexer::Indexer, tray::Tray};

const TOGGLE: &str = "--toggle";
const BACKGROUND: &str = "--background";

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if args.iter().any(|a| a == TOGGLE) {
                window::toggle(app);
            } else {
                window::show(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let paths = Paths::from_env()?;
            app.manage(Searcher::open(&paths)?);
            if let Err(err) = hotkey::setup(app) {
                eprintln!("sonar: couldn't set up the hotkey: {err:#}");
            }
            let tray = tray::create(app.handle())?;
            app.manage(tray.clone());
            app.manage(Indexer::start(paths, move |status| {
                tray.show_status(&status)
            }));
            updater::watch(app.handle());

            if !std::env::args().any(|a| a == BACKGROUND) {
                window::show(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) => {
                let _ = window.hide();
            }
            WindowEvent::ThemeChanged(_) => {
                if let Some(tray) = window.try_state::<Tray>() {
                    tray.refresh();
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::search,
            commands::open,
            commands::reveal
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sonar");
}
