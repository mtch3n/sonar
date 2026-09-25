use tauri::{
    AppHandle, Manager, Wry,
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
};

use crate::{
    hotkey,
    indexer::{Indexer, Status},
    updater, window,
};

pub const CHECK_FOR_UPDATES: &str = "Check for updates";

#[derive(Clone)]
pub struct Tray {
    icon: TrayIcon,
    status: MenuItem<Wry>,
    update: MenuItem<Wry>,
    idle: Image<'static>,
    busy: Image<'static>,
}

pub fn create(app: &AppHandle) -> tauri::Result<Tray> {
    let idle = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    let busy = Image::from_bytes(include_bytes!("../icons/tray-busy.png"))?;
    let status = MenuItem::with_id(app, "status", "Starting…", false, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", CHECK_FOR_UPDATES, true, None::<&str>)?;
    let open_label = format!("Open Sonar    {}", hotkey::LABEL);
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open", open_label, true, None::<&str>)?,
            &MenuItem::with_id(app, "reindex", "Reindex now", true, None::<&str>)?,
            &update,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit Sonar", true, None::<&str>)?,
        ],
    )?;
    let icon = TrayIconBuilder::with_id("sonar")
        .icon(busy.clone())
        .tooltip("Sonar")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => window::show(app),
            "reindex" => {
                if let Some(indexer) = app.try_state::<Indexer>() {
                    indexer.reindex();
                }
            }
            "update" => updater::check(app, true),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::toggle(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(Tray {
        icon,
        status,
        update,
        idle,
        busy,
    })
}

impl Tray {
    pub fn show_status(&self, status: &Status) {
        let (text, busy) = match status {
            Status::Indexing => ("Indexing…".to_owned(), true),
            Status::Ready { files, at } => {
                (format!("{} files · updated {at}", thousands(*files)), false)
            }
            Status::Failed(err) => (format!("Indexing failed: {err}"), false),
        };
        let _ = self.status.set_text(&text);
        let _ = self.icon.set_tooltip(Some(&text));
        let image = if busy { &self.busy } else { &self.idle };
        let _ = self.icon.set_icon(Some(image.clone()));
    }

    pub fn show_update(&self, text: &str, enabled: bool) {
        let _ = self.update.set_text(text);
        let _ = self.update.set_enabled(enabled);
    }
}

fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
