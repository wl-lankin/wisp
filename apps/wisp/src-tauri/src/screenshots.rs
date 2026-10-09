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
        if !enabled {
            *self.watcher.lock().unwrap() = None;
            return;
        }
        if self.watcher.lock().unwrap().is_some() {
            return;
        }
        // Looking the folder up runs a subprocess, so it happens before the lock is taken again.
        let folder = screenshot_folder();
        let mut slot = self.watcher.lock().unwrap();
        if slot.is_some() {
            return;
        }
        if folder.fell_back {
            notice(app, "Your screenshot folder was not found. Wisp is watching the Desktop instead.");
        }
        let handle = app.clone();
        let on_shot = std::sync::Arc::new(move |payload| {
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Resolved per shot, so an engine restart is transparent. Only a failed engine
                // start leaves no node, and a shot taken then is dropped.
                if let Ok(node) = handle.state::<crate::AppState>().engine.node().await {
                    node.offer_local(payload).await;
                }
            });
        });
        let denied_handle = app.clone();
        let on_denied = std::sync::Arc::new(move || {
            notice(
                &denied_handle,
                "Wisp can't read your screenshot folder. Allow access in System Settings > Privacy & Security > Files and Folders.",
            );
        });
        match ScreenshotWatcher::start(folder.path, is_screenshot, on_shot, on_denied) {
            Ok(watcher) => *slot = Some(watcher),
            Err(e) => {
                tracing::warn!("screenshot watcher did not start: {e}");
                notice(app, "Wisp can't watch your screenshot folder.");
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn sync(&self, _app: &AppHandle, _enabled: bool) {
        *self.watcher.lock().unwrap() = None;
    }
}

/// Shows an error notice in the app.
#[cfg(target_os = "macos")]
fn notice(app: &AppHandle, message: &str) {
    let _ = app.emit("wisp://event", serde_json::json!({ "type": "error", "message": message }));
}
