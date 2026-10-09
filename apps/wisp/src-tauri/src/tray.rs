use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::windows::{self, View};
use crate::AppState;

const TRAY_ID: &str = "wisp";

fn icon(pending: bool) -> Image<'static> {
    let bytes: &'static [u8] = match (cfg!(target_os = "macos"), pending) {
        (true, false) => include_bytes!("../icons/tray-mono/32x32.png"),
        (true, true) => include_bytes!("../icons/tray-mono-pending/32x32.png"),
        (false, false) => include_bytes!("../icons/tray-color/32x32.png"),
        (false, true) => include_bytes!("../icons/tray-color-pending/32x32.png"),
    };
    Image::from_bytes(bytes).expect("tray icon is a valid PNG")
}

fn event_rect(event: &TrayIconEvent) -> Option<windows::Rect> {
    let rect = match event {
        TrayIconEvent::Click { rect, .. }
        | TrayIconEvent::DoubleClick { rect, .. }
        | TrayIconEvent::Enter { rect, .. }
        | TrayIconEvent::Move { rect, .. }
        | TrayIconEvent::Leave { rect, .. } => rect,
        _ => return None,
    };
    // Tray rects are reported in physical pixels, so a scale of 1 leaves them unchanged.
    let position = rect.position.to_physical::<i32>(1.0);
    let size = rect.size.to_physical::<u32>(1.0);
    Some(windows::Rect { x: position.x, y: position.y, width: size.width, height: size.height })
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Wisp", true, None::<&str>)?;
    let timeline = MenuItem::with_id(app, "timeline", "Timeline", true, None::<&str>)?;
    let pair = MenuItem::with_id(app, "pair", "Pair a device", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Wisp", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &timeline, &pair, &settings, &separator, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(false))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Wisp")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => windows::show_entry(app, View::Panel),
            "timeline" => windows::show_entry(app, View::Timeline),
            "pair" | "settings" => windows::show_entry(app, View::Settings),
            "quit" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    handle.state::<AppState>().engine.stop().await;
                    handle.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            windows::set_tray_rect(tray.app_handle(), event_rect(&event));
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                windows::toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Tells the tray and every window that the inbox changed, so a caught, dismissed or saved offer
/// disappears everywhere and not only in the window that acted on it.
pub async fn inbox_changed(app: &AppHandle) {
    refresh(app).await;
    let _ = app.emit("wisp://event", serde_json::json!({ "type": "inboxChanged" }));
}

/// Shows the pending dot while uncaught offers wait in the inbox.
async fn refresh(app: &AppHandle) {
    let pending = match app.state::<AppState>().engine.node().await {
        Ok(node) => !node.inbox().is_empty(),
        Err(_) => false,
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon(pending)));
        let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
    }
}
