use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

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

const FRAME: Duration = Duration::from_millis(400);

const ICONS: [(&[u8], &[u8]); 5] = [
    (
        include_bytes!("../icons/tray/idle-black.png"),
        include_bytes!("../icons/tray/idle-white.png"),
    ),
    (
        include_bytes!("../icons/tray/indexing-1-black.png"),
        include_bytes!("../icons/tray/indexing-1-white.png"),
    ),
    (
        include_bytes!("../icons/tray/indexing-2-black.png"),
        include_bytes!("../icons/tray/indexing-2-white.png"),
    ),
    (
        include_bytes!("../icons/tray/indexing-3-black.png"),
        include_bytes!("../icons/tray/indexing-3-white.png"),
    ),
    (
        include_bytes!("../icons/tray/update-black.png"),
        include_bytes!("../icons/tray/update-white.png"),
    ),
];

#[derive(Clone, Copy)]
enum Look {
    Idle,
    Indexing(usize),
    Update,
}

impl Look {
    fn icon(self) -> usize {
        match self {
            Look::Idle => 0,
            Look::Indexing(frame) => 1 + frame % 3,
            Look::Update => 4,
        }
    }
}

#[derive(Default)]
struct Flags {
    indexing: bool,
    update_ready: bool,
    frame: usize,
}

#[derive(Clone)]
pub struct Tray {
    icon: TrayIcon,
    status: MenuItem<Wry>,
    update: MenuItem<Wry>,
    flags: Arc<Mutex<Flags>>,
}

pub fn create(app: &AppHandle) -> tauri::Result<Tray> {
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
        .icon(image(Look::Indexing(0))?)
        .icon_as_template(cfg!(target_os = "macos"))
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
    let tray = Tray {
        icon,
        status,
        update,
        flags: Arc::new(Mutex::new(Flags {
            indexing: true,
            ..Flags::default()
        })),
    };
    tray.animate();
    Ok(tray)
}

impl Tray {
    pub fn show_status(&self, status: &Status) {
        let text = match status {
            Status::Indexing => "Indexing…".to_owned(),
            Status::Ready { files, at } => format!("{} files · updated {at}", thousands(*files)),
            Status::Failed(err) => format!("Indexing failed: {err}"),
        };
        let _ = self.status.set_text(&text);
        let _ = self.icon.set_tooltip(Some(&text));
        if let Ok(mut flags) = self.flags.lock() {
            flags.indexing = matches!(status, Status::Indexing);
        }
        self.refresh();
    }

    fn animate(&self) {
        let tray = self.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(FRAME);
                let indexing = match tray.flags.lock() {
                    Ok(mut flags) if flags.indexing => {
                        flags.frame = (flags.frame + 1) % 3;
                        true
                    }
                    _ => false,
                };
                if indexing {
                    tray.refresh();
                }
            }
        });
    }

    pub fn show_update(&self, text: &str, enabled: bool) {
        let _ = self.update.set_text(text);
        let _ = self.update.set_enabled(enabled);
    }

    pub fn set_update_ready(&self, ready: bool) {
        if let Ok(mut flags) = self.flags.lock() {
            flags.update_ready = ready;
        }
        self.refresh();
    }

    pub fn refresh(&self) {
        let look = match self.flags.lock() {
            Ok(flags) if flags.indexing => Look::Indexing(flags.frame),
            Ok(flags) if flags.update_ready => Look::Update,
            _ => Look::Idle,
        };
        if let Ok(image) = image(look) {
            let _ = self.icon.set_icon(Some(image));
            let _ = self.icon.set_icon_as_template(cfg!(target_os = "macos"));
        }
    }
}

fn image(look: Look) -> tauri::Result<Image<'static>> {
    let (black, white) = ICONS[look.icon()];
    Image::from_bytes(if white_glyph() { white } else { black })
}

#[cfg(target_os = "macos")]
fn white_glyph() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn white_glyph() -> bool {
    true
}

#[cfg(windows)]
fn white_glyph() -> bool {
    let light_taskbar = windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        .is_ok_and(|value| value == 1);
    !light_taskbar
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
