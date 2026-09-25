use std::{thread, time::Duration};

use anyhow::Result;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::tray::Tray;

const CHECK_EVERY: Duration = Duration::from_secs(12 * 60 * 60);

pub fn watch(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        loop {
            check(&app, false);
            thread::sleep(CHECK_EVERY);
        }
    });
}

pub fn check(app: &AppHandle, install: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(tray) = app.try_state::<Tray>().map(|tray| tray.inner().clone()) else {
            return;
        };
        if install {
            tray.show_update("Checking for updates…", false);
        }
        match run(&app, &tray, install).await {
            Ok(Some(version)) => {
                tray.set_update_ready(true);
                tray.show_update(&format!("Update to v{version}"), true);
            }
            Ok(None) if install => {
                tray.set_update_ready(false);
                let current = &app.package_info().version;
                tray.show_update(&format!("Up to date (v{current})"), true);
            }
            Ok(None) => tray.set_update_ready(false),
            Err(err) => {
                eprintln!("sonar: update failed: {err:#}");
                if install {
                    tray.show_update("Update failed, try again", true);
                }
            }
        }
    });
}

async fn run(app: &AppHandle, tray: &Tray, install: bool) -> Result<Option<String>> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(None);
    };
    if !install {
        return Ok(Some(update.version));
    }
    tray.show_update(&format!("Downloading v{}…", update.version), false);
    update.download_and_install(|_, _| {}, || {}).await?;
    app.restart();
}
