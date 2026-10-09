use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;
use wisp_core::engine::{NearbyDevice, PeerStatus};
use wisp_core::history::HistoryQuery;
use wisp_core::item::ItemKind;
use wisp_core::peers::DeliveryMode;

use crate::dto::with_ui_preview;
use crate::engine::parse_peer_addr;
use crate::settings::AppSettings;
use crate::windows::View;
use crate::AppState;

type Cmd<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    device_id: Uuid,
    device_name: String,
    platform: &'static str,
    port: u16,
    peers: Vec<PeerStatus>,
    nearby: Vec<NearbyDevice>,
    inbox: Vec<Value>,
    hotkey_error: Option<String>,
}

#[tauri::command]
pub async fn snapshot(state: State<'_, AppState>) -> Cmd<Snapshot> {
    let node = state.engine.node().await?;
    Ok(Snapshot {
        device_id: node.device_id(),
        device_name: node.device_name(),
        platform: std::env::consts::OS,
        port: node.local_addr().map_err(err)?.port(),
        peers: node.peers(),
        nearby: node.nearby(),
        inbox: node.inbox().iter().map(|o| with_ui_preview(o, &o.preview)).collect(),
        hotkey_error: state.hotkey_error.lock().unwrap().clone(),
    })
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

pub(crate) fn save_settings(app: &AppHandle, state: &AppState, next: AppSettings) -> Cmd<AppSettings> {
    {
        let mut current = state.settings.lock().unwrap();
        next.save(&state.settings_path)?;
        *current = next.clone();
    }
    let _ = app.emit("wisp://settings", &next);
    Ok(next)
}

fn roll_back_hotkey(app: &AppHandle, registered: bool, previous: &AppSettings, next: &AppSettings) {
    if registered && previous.hotkey != next.hotkey {
        let _ = crate::hotkey::apply(app, Some(&next.hotkey), &previous.hotkey);
    }
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, state: State<'_, AppState>, mut next: AppSettings) -> Cmd<AppSettings> {
    if let Some(name) = next.device_name.take() {
        let name = name.trim();
        if name.is_empty() {
            return Err("Please give this device a name".into());
        }
        next.device_name = Some(name.chars().take(64).collect());
    }
    let previous = state.settings.lock().unwrap().clone();
    crate::hotkey::parse(&next.hotkey)?;
    // Re-saving an unchanged shortcut re-registers it when something else released it.
    let register_hotkey = previous.onboarded;
    if register_hotkey {
        match crate::hotkey::apply(&app, Some(&previous.hotkey), &next.hotkey) {
            Ok(()) => crate::set_hotkey_error(&app, None),
            Err(e) if next.hotkey != previous.hotkey => return Err(e),
            // An unchanged shortcut that is still taken must not block unrelated settings.
            Err(_) => {}
        }
    }
    let autostart_changed = next.autostart != previous.autostart;
    if autostart_changed {
        if let Err(e) = crate::apply_autostart(&app, next.autostart) {
            roll_back_hotkey(&app, register_hotkey, &previous, &next);
            return Err(e);
        }
    }
    let saved = match save_settings(&app, &state, next.clone()) {
        Ok(saved) => saved,
        Err(e) => {
            roll_back_hotkey(&app, register_hotkey, &previous, &next);
            if autostart_changed {
                let _ = crate::apply_autostart(&app, previous.autostart);
            }
            return Err(e);
        }
    };
    if saved.onboarded && saved.save_dir != previous.save_dir {
        crate::start_engine(&app).await;
    }
    Ok(saved)
}

#[tauri::command]
pub async fn complete_onboarding(app: AppHandle, state: State<'_, AppState>, device_name: String) -> Cmd<()> {
    let name = device_name.trim();
    if name.is_empty() {
        return Err("Please give this device a name".into());
    }
    let mut next = state.settings.lock().unwrap().clone();
    next.device_name = Some(name.chars().take(64).collect());
    next.onboarded = true;
    let saved = save_settings(&app, &state, next)?;
    crate::start_engine(&app).await;
    crate::register_saved_hotkey(&app, &saved);
    if let Err(message) = crate::apply_autostart(&app, saved.autostart) {
        let _ = app.emit("wisp://event", serde_json::json!({ "type": "error", "message": message }));
    }
    crate::windows::hide(&app, View::Onboarding);
    let _ = crate::windows::show(&app, View::Panel);
    Ok(())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Cmd<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
        .await
        .map_err(err)?;
    Ok(picked.map(|p| p.to_string()))
}

#[tauri::command]
pub async fn begin_pairing(state: State<'_, AppState>) -> Cmd<String> {
    Ok(state.engine.node().await?.begin_pairing())
}

#[tauri::command]
pub async fn cancel_pairing(state: State<'_, AppState>) -> Cmd<()> {
    state.engine.node().await?.cancel_pairing();
    Ok(())
}

#[tauri::command]
pub async fn pair_with(state: State<'_, AppState>, addr: String, code: String) -> Cmd<Value> {
    let addr = parse_peer_addr(&addr)?;
    let peer = state.engine.node().await?.pair_with(addr, code.trim()).await.map_err(err)?;
    serde_json::to_value(peer).map_err(err)
}

#[tauri::command]
pub async fn set_mode(state: State<'_, AppState>, peer: Uuid, mode: DeliveryMode) -> Cmd<()> {
    state.engine.node().await?.set_mode(peer, mode).map_err(err)
}

#[tauri::command]
pub async fn forget(state: State<'_, AppState>, peer: Uuid) -> Cmd<()> {
    state.engine.node().await?.forget(peer).map_err(err)
}

#[tauri::command]
pub async fn catch(app: AppHandle, state: State<'_, AppState>, item_id: Uuid) -> Cmd<()> {
    let result = state.engine.node().await?.catch(item_id).await.map_err(err);
    crate::tray::refresh(&app).await;
    result
}

#[tauri::command]
pub async fn catch_latest(app: AppHandle, state: State<'_, AppState>) -> Cmd<Option<Uuid>> {
    let result = state.engine.node().await?.catch_latest().await.map_err(err);
    crate::tray::refresh(&app).await;
    result
}

#[tauri::command]
pub async fn dismiss(app: AppHandle, state: State<'_, AppState>, item_id: Uuid) -> Cmd<()> {
    state.engine.node().await?.dismiss(item_id);
    crate::tray::refresh(&app).await;
    Ok(())
}

fn save_dir(state: &AppState) -> PathBuf {
    let settings = state.settings.lock().unwrap();
    settings.save_dir.clone().unwrap_or_else(|| state.paths.default_save_dir.clone())
}

/// Catches an offer and saves it to the save folder. Files arrive straight in the folder, which is
/// opened; clipboard items are written from the timeline once they have landed there.
#[tauri::command]
pub async fn save_offer(app: AppHandle, state: State<'_, AppState>, item_id: Uuid) -> Cmd<String> {
    let node = state.engine.node().await?;
    let kind = node.inbox().iter().find(|o| o.item_id == item_id).map(|o| o.kind);
    let kind = kind.ok_or_else(|| "This offer is no longer available".to_string())?;
    let caught = node.catch(item_id).await.map_err(err);
    crate::tray::refresh(&app).await;
    caught?;
    let dir = save_dir(&state);
    if kind == ItemKind::File {
        tokio::fs::create_dir_all(&dir).await.map_err(err)?;
        let _ = app.opener().open_path(dir.to_string_lossy(), None::<&str>);
        return Ok(dir.to_string_lossy().into_owned());
    }
    for _ in 0..50 {
        match node.export_item(item_id, &dir).await {
            Ok(path) => {
                let _ = app.opener().reveal_item_in_dir(&path);
                return Ok(path.to_string_lossy().into_owned());
            }
            Err(e) if e.to_string().contains("no longer in the timeline") => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(err(e)),
        }
    }
    Err("The item did not arrive in time. It may still appear in the Timeline.".into())
}

#[tauri::command]
pub async fn history(
    state: State<'_, AppState>,
    text: Option<String>,
    source: Option<Uuid>,
    kind: Option<ItemKind>,
    limit: Option<usize>,
) -> Cmd<Vec<Value>> {
    let q = HistoryQuery { text, source, kind, limit: limit.unwrap_or(200).min(500) };
    let entries = state.engine.node().await?.history(&q).map_err(err)?;
    Ok(entries.iter().map(|e| with_ui_preview(e, &e.preview)).collect())
}

#[tauri::command]
pub async fn copy_from_history(state: State<'_, AppState>, id: Uuid) -> Cmd<()> {
    state.engine.node().await?.copy_from_history(id).await.map_err(err)
}

#[tauri::command]
pub async fn export_item(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> Cmd<String> {
    let dir = save_dir(&state);
    let path = state.engine.node().await?.export_item(id, &dir).await.map_err(err)?;
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn send_clipboard_to(state: State<'_, AppState>, peer: Uuid) -> Cmd<()> {
    state.engine.node().await?.send_clipboard_to(peer).await.map_err(err)
}

#[tauri::command]
pub async fn send_files(state: State<'_, AppState>, peer: Uuid, paths: Vec<String>) -> Cmd<Vec<Uuid>> {
    let paths = paths.into_iter().map(PathBuf::from).collect();
    state.engine.node().await?.send_files(peer, paths).await.map_err(err)
}

#[tauri::command]
pub async fn add_address(state: State<'_, AppState>, peer: Uuid, addr: String) -> Cmd<()> {
    let addr = parse_peer_addr(&addr)?;
    state.engine.node().await?.add_address(peer, addr).map_err(err)
}

/// Async on purpose: creating a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_window(app: AppHandle, name: String) -> Cmd<()> {
    let view = View::from_label(&name).ok_or_else(|| format!("unknown window {name}"))?;
    crate::windows::show(&app, view).map(|_| ()).map_err(err)
}

#[tauri::command]
pub fn hide_window(app: AppHandle, name: String) -> Cmd<()> {
    let view = View::from_label(&name).ok_or_else(|| format!("unknown window {name}"))?;
    crate::windows::hide(&app, view);
    Ok(())
}

#[tauri::command]
pub fn fit_toast(app: AppHandle, height: f64) {
    crate::windows::fit_toast(&app, height);
}
