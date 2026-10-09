use std::sync::Mutex;

use tauri::AppHandle;
#[cfg(target_os = "macos")]
use tauri::{Emitter, Manager};
#[cfg(target_os = "macos")]
use wisp_core::screenshots::{is_screenshot, screenshot_folder, ScreenshotWatcher};

/// Owns the screenshot watcher; dropping the watcher stops it. Only macOS ever holds one.
#[derive(Default)]
pub struct ScreenshotService {
    #[cfg(target_os = "macos")]
    watcher: Mutex<Option<ScreenshotWatcher>>,
    #[cfg(not(target_os = "macos"))]
    watcher: Mutex<Option<()>>,
}

impl ScreenshotService {
    /// Starts or stops watching to match `enabled`. Must run on the Tauri async runtime.
    #[cfg(target_os = "macos")]
    pub fn sync(&self, app: &AppHandle, enabled: bool) {
        let mut slot = self.watcher.lock().unwrap();
        if !enabled {
            *slot = None;
            return;
        }
        if slot.is_some() {
            return;
        }
        let handle = app.clone();
        let on_shot = std::sync::Arc::new(move |payload| {
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Resolved per shot, so an engine restart is transparent. A shot taken while the
                // engine is restarting has no node yet and is dropped.
                if let Ok(node) = handle.state::<crate::AppState>().engine.node().await {
                    node.offer_local(payload).await;
                }
            });
        });
        match ScreenshotWatcher::start(screenshot_folder(), is_screenshot, on_shot) {
            Ok(watcher) => *slot = Some(watcher),
            Err(e) => {
                tracing::warn!("screenshot watcher did not start: {e}");
                let _ = app.emit(
                    "wisp://event",
                    serde_json::json!({ "type": "error", "message": "Wisp can't watch your screenshot folder." }),
                );
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn sync(&self, _app: &AppHandle, _enabled: bool) {
        *self.watcher.lock().unwrap() = None;
    }
}
