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
    })
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

fn save_settings(app: &AppHandle, state: &AppState, next: AppSettings) -> Cmd<AppSettings> {
    next.save(&state.settings_path)?;
    *state.settings.lock().unwrap() = next.clone();
    let _ = app.emit("wisp://settings", &next);
    Ok(next)
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, state: State<'_, AppState>, next: AppSettings) -> Cmd<AppSettings> {
    let previous = state.settings.lock().unwrap().clone();
    let saved = save_settings(&app, &state, next)?;
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
    save_settings(&app, &state, next)?;
    crate::start_engine(&app).await;
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
pub async fn catch(state: State<'_, AppState>, item_id: Uuid) -> Cmd<()> {
    state.engine.node().await?.catch(item_id).await.map_err(err)
}

#[tauri::command]
pub async fn catch_latest(state: State<'_, AppState>) -> Cmd<Option<Uuid>> {
    state.engine.node().await?.catch_latest().await.map_err(err)
}

#[tauri::command]
pub async fn dismiss(state: State<'_, AppState>, item_id: Uuid) -> Cmd<()> {
    state.engine.node().await?.dismiss(item_id);
    Ok(())
}

fn save_dir(state: &AppState) -> PathBuf {
    let settings = state.settings.lock().unwrap();
    settings.save_dir.clone().unwrap_or_else(|| state.paths.default_save_dir.clone())
}

/// Catches an offer, waits until it is in the timeline, writes it to the save folder and reveals it.
#[tauri::command]
pub async fn save_offer(app: AppHandle, state: State<'_, AppState>, item_id: Uuid) -> Cmd<String> {
    let node = state.engine.node().await?;
    node.catch(item_id).await.map_err(err)?;
    let dir = save_dir(&state);
    for _ in 0..50 {
        if let Ok(path) = node.export_item(item_id, &dir).await {
            let _ = app.opener().reveal_item_in_dir(&path);
            return Ok(path.to_string_lossy().into_owned());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("The item did not arrive in time. Try again.".into())
}

#[tauri::command]
pub async fn history(
    state: State<'_, AppState>,
    text: Option<String>,
    source: Option<Uuid>,
    kind: Option<ItemKind>,
    limit: Option<usize>,
) -> Cmd<Vec<Value>> {
    let q = HistoryQuery { text, source, kind, limit: limit.unwrap_or(200) };
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
