use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

use crate::AppState;

/// Logical gap between the toast and the edges of the work area.
const MARGIN: f64 = 16.0;
/// Logical gap between the panel and the tray icon or the work area edges.
const PANEL_MARGIN: f64 = 8.0;
const TOAST_WIDTH: f64 = 360.0;
/// A tray click right after the panel hid on blur is the click that blurred it, so it closes the panel.
const BLUR_CLICK_WINDOW: Duration = Duration::from_millis(300);

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

impl Rect {
    fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }
}

/// Top-left corner of the toast inside `work`, in the same units as `work` and `size` (logical pixels
/// when called from `place_toast`, which scales the result to physical pixels).
pub fn toast_origin(work: Rect, size: (u32, u32), macos: bool) -> (i32, i32) {
    let margin = MARGIN as i32;
    let x = work.right() - size.0 as i32 - margin;
    let y = if macos { work.y + margin } else { work.bottom() - size.1 as i32 - margin };
    (x, y)
}

/// Top-left corner of the panel in physical pixels. Centered on the tray icon, above it on Windows and
/// below the menu bar icon on macOS, always kept inside `work`. Without a tray rect it takes the work
/// area corner where the tray usually lives. `margin` is in physical pixels.
pub fn panel_origin(tray: Option<Rect>, size: (u32, u32), work: Rect, macos: bool, margin: i32) -> (i32, i32) {
    let (w, h) = (size.0 as i32, size.1 as i32);
    let (x, y) = match tray {
        Some(t) => {
            let x = t.x + t.width as i32 / 2 - w / 2;
            let y = if macos { t.bottom() + margin } else { t.y - margin - h };
            (x, y)
        }
        None => {
            let y = if macos { work.y + margin } else { work.bottom() - h - margin };
            (work.right() - w - margin, y)
        }
    };
    let x = x.min(work.right() - w - margin).max(work.x + margin);
    let y = y.min(work.bottom() - h - margin).max(work.y + margin);
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
            .inner_size(TOAST_WIDTH, 160.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .focused(false)
            // Never activates when shown or clicked, so the toast cannot steal focus.
            .focusable(false)
            .shadow(false),
        View::Onboarding => b
            .title("Welcome to Wisp")
            .inner_size(520.0, 600.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .skip_taskbar(true)
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
        const ACRYLIC: (u8, u8, u8, u8) = (11, 13, 20, 200);
        // Mica renders flat while inactive, so the transient panel and toast use acrylic.
        let applied = view == View::Onboarding && window_vibrancy::apply_mica(window, None).is_ok();
        if !applied {
            let _ = window_vibrancy::apply_acrylic(window, Some(ACRYLIC));
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

/// Remembers where the tray icon is, so the panel and toast open on the right monitor and spot.
pub fn set_tray_rect(app: &AppHandle, rect: Option<Rect>) {
    if rect.is_some() {
        *app.state::<AppState>().tray_rect.lock().unwrap() = rect;
    }
}

fn tray_rect(window: &WebviewWindow) -> Option<Rect> {
    *window.app_handle().state::<AppState>().tray_rect.lock().unwrap()
}

/// The monitor holding the tray icon when known, else the primary monitor.
fn monitor_for(window: &WebviewWindow, tray: Option<Rect>) -> Option<Monitor> {
    let at_tray = tray.and_then(|t| {
        let (cx, cy) = (t.x as f64 + t.width as f64 / 2.0, t.y as f64 + t.height as f64 / 2.0);
        window.monitor_from_point(cx, cy).ok().flatten()
    });
    at_tray.or_else(|| window.primary_monitor().ok().flatten())
}

fn work_rect(monitor: &Monitor) -> Rect {
    let area = monitor.work_area();
    Rect { x: area.position.x, y: area.position.y, width: area.size.width, height: area.size.height }
}

fn place(window: &WebviewWindow, view: View) {
    match view {
        View::Panel => place_panel(window),
        View::Toast => place_toast(window),
        _ => {}
    }
}

fn place_panel(window: &WebviewWindow) {
    let tray = tray_rect(window);
    let (Some(monitor), Ok(size)) = (monitor_for(window, tray), window.outer_size()) else {
        return;
    };
    let margin = (PANEL_MARGIN * monitor.scale_factor()).round() as i32;
    let (x, y) = panel_origin(tray, (size.width, size.height), work_rect(&monitor), cfg!(target_os = "macos"), margin);
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

fn place_toast(window: &WebviewWindow) {
    let (Some(monitor), Ok(size)) = (monitor_for(window, tray_rect(window)), window.outer_size()) else {
        return;
    };
    let scale = monitor.scale_factor();
    let logical = |physical: u32| (physical as f64 / scale).round() as u32;
    let work = work_rect(&monitor);
    let local = Rect { x: 0, y: 0, width: logical(work.width), height: logical(work.height) };
    let (x, y) = toast_origin(local, (logical(size.width), logical(size.height)), cfg!(target_os = "macos"));
    let to_physical = |v: i32| (v as f64 * scale).round() as i32;
    let _ = window.set_position(PhysicalPosition::new(work.x + to_physical(x), work.y + to_physical(y)));
}

fn hide_by_blur(app: &AppHandle) {
    *app.state::<AppState>().panel_blurred_at.lock().unwrap() = Some(Instant::now());
    hide(app, View::Panel);
}

#[cfg(target_os = "windows")]
fn left_button_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    // SAFETY: GetAsyncKeyState only reads global input state; the high bit is set while the key is down.
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 }
}

/// Windows: an Explorer drag starts with a mouse press that blurs the panel before any drag event
/// arrives. Wait for the release, then hide only if no drag reached the panel and it did not regain focus.
#[cfg(target_os = "windows")]
fn hide_after_release(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let mut drag_seen = false;
        for _ in 0..100 {
            drag_seen |= state.dragging.load(Ordering::SeqCst);
            if !left_button_down() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        drag_seen |= state.dragging.load(Ordering::SeqCst);
        let focused = app
            .get_webview_window(View::Panel.label())
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
        if !drag_seen && !focused {
            hide_by_blur(&app);
        }
    });
}

fn on_panel_blur(app: &AppHandle) {
    if app.state::<AppState>().dragging.load(Ordering::SeqCst) {
        return;
    }
    #[cfg(target_os = "windows")]
    if left_button_down() {
        hide_after_release(app.clone());
        return;
    }
    hide_by_blur(app);
}

/// A pairing code belongs to the window showing it, so closing that window ends the code.
fn ends_pairing(view: View) -> bool {
    matches!(view, View::Settings | View::Onboarding)
}

fn cancel_pairing(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // While the engine is still starting there is no code to cancel.
        if let Ok(node) = app.state::<AppState>().engine.node().await {
            node.cancel_pairing();
        }
    });
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
                    WindowEvent::Focused(true) => {
                        handle.state::<AppState>().dragging.store(false, Ordering::SeqCst);
                    }
                    WindowEvent::Focused(false) => on_panel_blur(&handle),
                    WindowEvent::DragDrop(drag) => {
                        let active = matches!(drag, tauri::DragDropEvent::Enter { .. } | tauri::DragDropEvent::Over { .. });
                        handle.state::<AppState>().dragging.store(active, Ordering::SeqCst);
                    }
                    _ => {}
                });
            }
            if ends_pairing(view) {
                let handle = app.clone();
                w.on_window_event(move |event| {
                    if matches!(event, WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed) {
                        cancel_pairing(&handle);
                    }
                });
            }
            w
        }
    };
    if view == View::Panel {
        app.state::<AppState>().dragging.store(false, Ordering::SeqCst);
    }
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
    if ends_pairing(view) {
        cancel_pairing(app);
    }
}

pub fn toggle_panel(app: &AppHandle) {
    let blurred_at = app.state::<AppState>().panel_blurred_at.lock().unwrap().take();
    if blurred_at.is_some_and(|at| at.elapsed() < BLUR_CLICK_WINDOW) {
        // The press on the tray icon already hid the panel; this click was meant to close it.
        return;
    }
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
        let width = (TOAST_WIDTH * scale) as u32;
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

    const PANEL: (u32, u32) = (360, 480);

    #[test]
    fn panel_sits_above_a_bottom_taskbar_tray_icon_on_windows() {
        let work = Rect { x: 0, y: 0, width: 1920, height: 1032 };
        let tray = Rect { x: 1600, y: 1035, width: 40, height: 40 };
        // Centered on the icon; the clamp keeps the margin above the taskbar edge of the work area.
        assert_eq!(panel_origin(Some(tray), PANEL, work, false, 8), (1620 - 180, 1032 - 8 - 480));
    }

    #[test]
    fn panel_stays_inside_the_work_area_with_a_top_taskbar() {
        let work = Rect { x: 0, y: 40, width: 1920, height: 1040 };
        let tray = Rect { x: 1600, y: 4, width: 40, height: 32 };
        assert_eq!(panel_origin(Some(tray), PANEL, work, false, 8), (1440, 48));
    }

    #[test]
    fn panel_hangs_below_the_menu_bar_icon_on_macos() {
        let work = Rect { x: 0, y: 25, width: 1440, height: 875 };
        let tray = Rect { x: 1000, y: 0, width: 30, height: 25 };
        assert_eq!(panel_origin(Some(tray), PANEL, work, true, 8), (1015 - 180, 25 + 8));
    }

    #[test]
    fn panel_without_a_tray_rect_uses_the_work_area_corner() {
        let work = Rect { x: 1920, y: 0, width: 2560, height: 1400 };
        assert_eq!(panel_origin(None, PANEL, work, false, 8), (1920 + 2560 - 360 - 8, 1400 - 480 - 8));
        assert_eq!(panel_origin(None, PANEL, work, true, 8), (1920 + 2560 - 360 - 8, 8));
    }

    #[test]
    fn panel_is_clamped_at_the_right_edge() {
        let work = Rect { x: 0, y: 0, width: 1920, height: 1032 };
        let tray = Rect { x: 1900, y: 1035, width: 20, height: 40 };
        assert_eq!(panel_origin(Some(tray), PANEL, work, false, 8), (1920 - 360 - 8, 1032 - 8 - 480));
    }
}
