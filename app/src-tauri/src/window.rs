use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow};

use crate::launcher::Launcher;

const MAIN: &str = "main";

pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        hide(app);
    } else {
        present(&window);
    }
}

pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        present(&window);
    }
}

/// Opens the search bar with `text` already typed.
pub fn show_with(app: &AppHandle, text: &str) {
    show(app);
    let _ = app.emit_to(MAIN, "sonar://fill", text);
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.hide();
    }
    if let Some(launcher) = app.try_state::<Launcher>() {
        launcher.stop_plugins();
    }
}

fn present(window: &WebviewWindow) {
    let app = window.app_handle();
    crate::reload(app);
    if let Some(launcher) = app.try_state::<Launcher>() {
        let _ = fit_width(window, launcher.current_settings().appearance.width);
    }
    let _ = place(window);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("sonar://shown", ());
}

/// The search window sets its own height; the width comes from the settings and has to
/// be right before the window is centered.
fn fit_width(window: &WebviewWindow, width: u32) -> tauri::Result<()> {
    let height = window
        .inner_size()?
        .to_logical::<f64>(window.scale_factor()?)
        .height;
    window.set_size(LogicalSize::new(f64::from(width), height))
}

fn place(window: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) else {
        return Ok(());
    };
    let (area, origin) = (monitor.size(), monitor.position());
    let size = window.outer_size()?;
    let x = origin.x + (area.width as i32 - size.width as i32) / 2;
    let y = origin.y + area.height as i32 / 5;
    window.set_position(PhysicalPosition::new(x, y))
}
