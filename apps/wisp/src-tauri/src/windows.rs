use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::AppState;

const MARGIN: f64 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Panel,
    Toast,
    Timeline,
    Settings,
    Onboarding,
}

impl View {
    pub fn label(&self) -> &'static str {
        match self {
            View::Panel => "panel",
            View::Toast => "toast",
            View::Timeline => "timeline",
            View::Settings => "settings",
            View::Onboarding => "onboarding",
        }
    }

    pub fn url(&self) -> String {
        format!("index.html?view={}", self.label())
    }

    pub fn from_label(label: &str) -> Option<View> {
        [View::Panel, View::Toast, View::Timeline, View::Settings, View::Onboarding]
            .into_iter()
            .find(|v| v.label() == label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Top-left corner of the toast inside `work`, in the same units as `work` and `size` (logical pixels
/// when called from `place_toast`, which scales the result to physical pixels).
pub fn toast_origin(work: Rect, size: (u32, u32), macos: bool) -> (i32, i32) {
    let margin = MARGIN as i32;
    let x = work.x + work.width as i32 - size.0 as i32 - margin;
    let y = if macos { work.y + margin } else { work.y + work.height as i32 - size.1 as i32 - margin };
    (x, y)
}

fn builder<'a>(app: &'a AppHandle, view: View) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let b = WebviewWindowBuilder::new(app, view.label(), WebviewUrl::App(view.url().into())).visible(false);
    match view {
        View::Panel => b
            .title("Wisp")
            .inner_size(360.0, 480.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true),
        View::Toast => b
            .title("Wisp")
            .inner_size(360.0, 160.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .focused(false)
            .shadow(false),
        View::Onboarding => b
            .title("Welcome to Wisp")
            .inner_size(520.0, 600.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .center(),
        View::Timeline => b.title("Wisp timeline").inner_size(1040.0, 720.0).min_inner_size(640.0, 480.0).center(),
        View::Settings => b.title("Wisp settings").inner_size(560.0, 640.0).resizable(false).center(),
    }
}

fn apply_material(window: &WebviewWindow, view: View) {
    if !matches!(view, View::Panel | View::Toast | View::Onboarding) {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        let applied = window_vibrancy::apply_mica(window, None).is_ok();
        if !applied {
            let _ = window_vibrancy::apply_acrylic(window, Some((11, 13, 20, 200)));
        }
    }
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState};
        let _ = window_vibrancy::apply_vibrancy(
            window,
            NSVisualEffectMaterial::HudWindow,
            Some(NSVisualEffectState::Active),
            Some(16.0),
        );
    }
}

fn place(window: &WebviewWindow, view: View) {
    match view {
        View::Panel => {
            #[cfg(target_os = "macos")]
            let _ = window.move_window(Position::TrayCenter);
            #[cfg(not(target_os = "macos"))]
            let _ = window.move_window(Position::TrayBottomCenter);
        }
        View::Toast => place_toast(window),
        _ => {}
    }
}

fn place_toast(window: &WebviewWindow) {
    let (Ok(Some(monitor)), Ok(size)) = (window.primary_monitor(), window.outer_size()) else {
        return;
    };
    let scale = monitor.scale_factor();
    let logical = |physical: u32| (physical as f64 / scale).round() as u32;
    // Leave room for the taskbar on Windows and the menu bar on macOS.
    let (top_inset, bottom_inset) = if cfg!(target_os = "macos") { (28, 0) } else { (0, 48) };
    let work = Rect {
        x: 0,
        y: top_inset as i32,
        width: logical(monitor.size().width),
        height: logical(monitor.size().height).saturating_sub(top_inset + bottom_inset),
    };
    let (x, y) = toast_origin(work, (logical(size.width), logical(size.height)), cfg!(target_os = "macos"));
    let origin = monitor.position();
    let to_physical = |v: i32| (v as f64 * scale).round() as i32;
    let _ = window.set_position(PhysicalPosition::new(origin.x + to_physical(x), origin.y + to_physical(y)));
}

pub fn show(app: &AppHandle, view: View) -> tauri::Result<WebviewWindow> {
    let window = match app.get_webview_window(view.label()) {
        Some(w) => w,
        None => {
            let w = builder(app, view).build()?;
            apply_material(&w, view);
            if view == View::Panel {
                let handle = app.clone();
                w.on_window_event(move |event| match event {
                    WindowEvent::Focused(false) => {
                        let dragging = handle.state::<AppState>().dragging.load(Ordering::SeqCst);
                        if !dragging {
                            hide(&handle, View::Panel);
                        }
                    }
                    WindowEvent::DragDrop(drag) => {
                        let active = matches!(drag, tauri::DragDropEvent::Enter { .. } | tauri::DragDropEvent::Over { .. });
                        handle.state::<AppState>().dragging.store(active, Ordering::SeqCst);
                    }
                    _ => {}
                });
            }
            w
        }
    };
    place(&window, view);
    window.show()?;
    if view != View::Toast {
        window.set_focus()?;
    }
    Ok(window)
}

pub fn hide(app: &AppHandle, view: View) {
    if let Some(w) = app.get_webview_window(view.label()) {
        let _ = w.hide();
    }
}

pub fn toggle_panel(app: &AppHandle) {
    match app.get_webview_window(View::Panel.label()) {
        Some(w) if w.is_visible().unwrap_or(false) => {
            let _ = w.hide();
        }
        _ => {
            let _ = show(app, View::Panel);
        }
    }
}

pub fn fit_toast(app: &AppHandle, logical_height: f64) {
    if let Some(w) = app.get_webview_window(View::Toast.label()) {
        let scale = w.scale_factor().unwrap_or(1.0);
        let height = (logical_height.clamp(80.0, 520.0) * scale) as u32;
        let width = (360.0 * scale) as u32;
        let _ = w.set_size(PhysicalSize::new(width, height));
        place_toast(&w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_round_trip_through_labels() {
        for v in [View::Panel, View::Toast, View::Timeline, View::Settings, View::Onboarding] {
            assert_eq!(View::from_label(v.label()), Some(v));
            assert_eq!(v.url(), format!("index.html?view={}", v.label()));
        }
        assert_eq!(View::from_label("nope"), None);
    }

    #[test]
    fn toast_sits_bottom_right_on_windows_and_top_right_on_macos() {
        let work = Rect { x: 0, y: 0, width: 1920, height: 1040 };
        assert_eq!(toast_origin(work, (360, 200), false), (1920 - 360 - 16, 1040 - 200 - 16));
        assert_eq!(toast_origin(work, (360, 200), true), (1920 - 360 - 16, 16));
        let second = Rect { x: 1920, y: 0, width: 2560, height: 1400 };
        assert_eq!(toast_origin(second, (360, 200), false), (1920 + 2560 - 360 - 16, 1400 - 200 - 16));
    }
}
