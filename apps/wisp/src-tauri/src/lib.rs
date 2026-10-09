mod commands;
mod dto;
mod engine;
mod settings;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

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
}

struct TauriSink {
    app: AppHandle,
}

impl EventSink for TauriSink {
    fn emit(&self, value: serde_json::Value) {
        let _ = self.app.emit("wisp://event", value);
    }

    fn react(&self, _event: &Event) {}
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
            });
            if onboarded {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { start_engine(&handle).await });
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
