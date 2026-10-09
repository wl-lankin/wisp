mod commands;
mod dto;
mod engine;
mod settings;
mod tray;
mod windows;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
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
}

struct TauriSink {
    app: AppHandle,
}

impl EventSink for TauriSink {
    fn emit(&self, value: serde_json::Value) {
        let _ = self.app.emit("wisp://event", value);
    }

    fn react(&self, event: &Event) {
        match event {
            Event::OfferReceived(_) => {
                let _ = windows::show(&self.app, windows::View::Toast);
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move { tray::refresh(&app).await });
            }
            Event::ClipboardApplied { .. } => {
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move { tray::refresh(&app).await });
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

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let default_save_dir = app.path().download_dir()?.join("Wisp");
            let settings_path = data_dir.join("settings.json");
            let settings = AppSettings::load(&settings_path);
            let onboarded = settings.onboarded;
            app.manage(AppState {
                engine: Engine::default(),
                settings: Mutex::new(settings),
                paths: Paths { data_dir, default_save_dir },
                settings_path,
                dragging: AtomicBool::new(false),
                tray_rect: Mutex::new(None),
                panel_blurred_at: Mutex::new(None),
            });
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::install(app.handle())?;
            if onboarded {
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
