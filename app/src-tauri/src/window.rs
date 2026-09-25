use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

const MAIN: &str = "main";

pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        present(&window);
    }
}

pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        present(&window);
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.hide();
    }
}

fn present(window: &WebviewWindow) {
    let _ = place(window);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("sonar://shown", ());
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
