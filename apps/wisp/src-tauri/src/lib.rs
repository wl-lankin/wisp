mod commands;
mod dto;
mod engine;
mod hotkey;
mod settings;
mod tray;
mod windows;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager};
use wisp_core::engine::Event;

use crate::engine::{Engine, EventSink, Paths};
use crate::settings::AppSettings;

pub struct AppState {
    pub engine: Engine,
    pub settings: Mutex<AppSettings>,
    pub paths: Paths,
    pub settings_path: PathBuf,
    /// True while a file drag hovers a Wisp window, so the panel does not hide mid-drag.
    pub dragging: AtomicBool,
    /// Last known tray icon rect in physical pixels, used to place the panel and toast.
    pub tray_rect: Mutex<Option<windows::Rect>>,
    /// When the panel last hid because it lost focus, so the tray click that caused it closes it.
    pub panel_blurred_at: Mutex<Option<Instant>>,
    /// Why the catch shortcut is not registered, shown in the UI until a later registration succeeds.
    pub hotkey_error: Mutex<Option<String>>,
    /// The last clipboard write failure, kept for a toast window that did not exist when it happened.
    pub clipboard_error: Mutex<ClipboardErrorLog>,
    /// True while a shortcut press is being handled, so a held key does not stack catches.
    pub hotkey_busy: AtomicBool,
    /// Held for a whole settings change, so two quick saves cannot interleave and undo each other.
    pub settings_change: tokio::sync::Mutex<()>,
}

/// The newest clipboard write failure with a sequence number, until a later write succeeds.
#[derive(Default)]
pub struct ClipboardErrorLog {
    seq: u64,
    current: Option<String>,
}

impl ClipboardErrorLog {
    /// Notes a failure and returns its sequence number.
    pub fn record(&mut self, message: &str) -> u64 {
        self.seq += 1;
        self.current = Some(message.to_string());
        self.seq
    }

    /// Forgets the failure once a clipboard write succeeded.
    pub fn clear(&mut self) {
        self.current = None;
    }

    pub fn current(&self) -> Option<(u64, String)> {
        self.current.clone().map(|message| (self.seq, message))
    }
}

struct TauriSink {
    app: AppHandle,
}

impl EventSink for TauriSink {
    fn emit(&self, mut value: serde_json::Value) {
        let kind = value.get("type").and_then(|t| t.as_str()).unwrap_or_default().to_string();
        if kind == "clipboardApplied" {
            self.app.state::<AppState>().clipboard_error.lock().unwrap().clear();
        } else if kind == "error" {
            let message = value.get("message").and_then(|m| m.as_str()).unwrap_or_default().to_string();
            if message.contains("clipboard") {
                let seq = self.app.state::<AppState>().clipboard_error.lock().unwrap().record(&message);
                value["seq"] = serde_json::json!(seq);
            }
        }
        let _ = self.app.emit("wisp://event", value);
    }

    fn react(&self, event: &Event) {
        match event {
            Event::OfferReceived(_) => {
                let _ = windows::show(&self.app, windows::View::Toast);
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move { tray::inbox_changed(&app).await });
            }
            Event::ClipboardApplied { .. } => {
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move { tray::inbox_changed(&app).await });
            }
            Event::Error { message } if message.contains("clipboard") => {
                let _ = windows::show(&self.app, windows::View::Toast);
            }
            _ => {}
        }
    }
}

pub(crate) async fn start_engine(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let sink = Arc::new(TauriSink { app: app.clone() });
    if let Err(message) = state.engine.start(&settings, &state.paths, sink).await {
        let _ = app.emit("wisp://event", serde_json::json!({ "type": "error", "message": message }));
    }
}

pub(crate) async fn catch_from_hotkey(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.hotkey_busy.swap(true, Ordering::AcqRel) {
        return;
    }
    catch_newest_or_show_panel(app).await;
    state.hotkey_busy.store(false, Ordering::Release);
}

async fn catch_newest_or_show_panel(app: &AppHandle) {
    let Ok(node) = app.state::<AppState>().engine.node().await else {
        let _ = windows::show(app, windows::View::Panel);
        return;
    };
    match node.catch_latest().await {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _ = windows::show(app, windows::View::Panel);
        }
        Err(e) => {
            let _ = app.emit("wisp://event", serde_json::json!({ "type": "error", "message": e.to_string() }));
        }
    }
    tray::inbox_changed(app).await;
}

pub(crate) fn set_hotkey_error(app: &AppHandle, problem: Option<String>) {
    *app.state::<AppState>().hotkey_error.lock().unwrap() = problem;
}

/// Registers the saved shortcut. A conflict is kept in the state because no window can show an event this early.
pub(crate) fn register_saved_hotkey(app: &AppHandle, settings: &AppSettings) {
    match hotkey::apply(app, None, &settings.hotkey) {
        Ok(()) => set_hotkey_error(app, None),
        Err(message) => {
            let problem = format!("{message}. Change it in Settings.");
            let _ = app.emit("wisp://event", serde_json::json!({ "type": "error", "message": problem }));
            set_hotkey_error(app, Some(problem));
        }
    }
}

/// Turns start at login on or off in the system.
pub(crate) fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    result.map_err(|e| format!("Could not change start at login ({e})"))
}

/// Follows a change the user made in the system (for example in Task Manager) instead of overriding it.
fn adopt_system_autostart(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let Ok(enabled) = app.autolaunch().is_enabled() else { return };
    let state = app.state::<AppState>();
    let mut next = state.settings.lock().unwrap().clone();
    if next.autostart == enabled {
        return;
    }
    next.autostart = enabled;
    if let Err(e) = commands::save_settings(app, &state, next) {
        tracing::warn!("could not remember the start at login choice: {e}");
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_entry(app, windows::View::Panel);
        }))
        .plugin(hotkey::plugin())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let default_save_dir = app.path().download_dir()?.join("Wisp");
            let settings_path = data_dir.join("settings.json");
            let settings = AppSettings::load(&settings_path);
            let onboarded = settings.onboarded;
            let startup_settings = settings.clone();
            app.manage(AppState {
                engine: Engine::default(),
                settings: Mutex::new(settings),
                paths: Paths { data_dir, default_save_dir },
                settings_path,
                dragging: AtomicBool::new(false),
                tray_rect: Mutex::new(None),
                panel_blurred_at: Mutex::new(None),
                hotkey_error: Mutex::new(None),
                clipboard_error: Mutex::new(ClipboardErrorLog::default()),
                hotkey_busy: AtomicBool::new(false),
                settings_change: tokio::sync::Mutex::new(()),
            });
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::install(app.handle())?;
            if onboarded {
                register_saved_hotkey(app.handle(), &startup_settings);
                adopt_system_autostart(app.handle());
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { start_engine(&handle).await });
            } else {
                windows::show(app.handle(), windows::View::Onboarding)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::get_settings,
            commands::update_settings,
            commands::complete_onboarding,
            commands::default_device_name,
            commands::pick_folder,
            commands::begin_pairing,
            commands::cancel_pairing,
            commands::pair_with,
            commands::set_mode,
            commands::forget,
            commands::catch,
            commands::catch_latest,
            commands::dismiss,
            commands::save_offer,
            commands::history,
            commands::copy_from_history,
            commands::export_item,
            commands::send_clipboard_to,
            commands::send_files,
            commands::add_address,
            commands::open_window,
            commands::hide_window,
            commands::fit_toast,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the Wisp app")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::ClipboardErrorLog;

    #[test]
    fn clipboard_error_log_numbers_failures_and_clears_on_success() {
        let mut log = ClipboardErrorLog::default();
        assert_eq!(log.current(), None);
        assert_eq!(log.record("first"), 1);
        assert_eq!(log.current(), Some((1, "first".to_string())));
        assert_eq!(log.record("second"), 2);
        assert_eq!(log.current(), Some((2, "second".to_string())));
        log.clear();
        assert_eq!(log.current(), None);
        assert_eq!(log.record("third"), 3);
    }
}
