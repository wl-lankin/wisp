use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, Mutex};
use wisp_core::clipboard::{ClipboardBackend, SystemClipboard};
use wisp_core::engine::{Event, Node, NodeConfig};

use crate::dto::event_to_ui;
use crate::settings::AppSettings;

pub const DEFAULT_PORT: u16 = 47800;
const PORT_RETRIES: u32 = 5;

/// Where engine events go: the Tauri app in production, a recorder in tests.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, value: Value);
    fn react(&self, event: &Event);
}

pub async fn pump(mut rx: mpsc::UnboundedReceiver<Event>, sink: Arc<dyn EventSink>) {
    while let Some(event) = rx.recv().await {
        sink.emit(event_to_ui(&event));
        sink.react(&event);
    }
}

pub struct Paths {
    pub data_dir: PathBuf,
    pub default_save_dir: PathBuf,
}

pub fn default_device_name() -> String {
    let name = gethostname::gethostname().to_string_lossy().trim().to_string();
    if name.is_empty() {
        "My computer".to_string()
    } else {
        name
    }
}

pub fn node_config(
    settings: &AppSettings,
    paths: &Paths,
    clipboard: Arc<dyn ClipboardBackend>,
    port: u16,
) -> NodeConfig {
    NodeConfig {
        data_dir: paths.data_dir.join("engine"),
        save_dir: settings.save_dir.clone().unwrap_or_else(|| paths.default_save_dir.clone()),
        device_name: settings.device_name.clone().unwrap_or_else(default_device_name),
        bind: SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
        discovery: true,
        clipboard,
        clipboard_poll: Duration::from_millis(250),
        dial_interval: Duration::from_secs(5),
    }
}

pub fn parse_peer_addr(input: &str) -> Result<SocketAddr, String> {
    let t = input.trim();
    if let Ok(addr) = t.parse::<SocketAddr>() {
        return Ok(addr);
    }
    t.parse::<IpAddr>()
        .map(|ip| SocketAddr::new(ip, DEFAULT_PORT))
        .map_err(|_| format!("\"{t}\" is not an IP address"))
}

#[derive(Default)]
pub struct Engine {
    node: Mutex<Option<Node>>,
    clipboard: Mutex<Option<Arc<dyn ClipboardBackend>>>,
}

impl Engine {
    pub async fn start(&self, settings: &AppSettings, paths: &Paths, sink: Arc<dyn EventSink>) -> Result<(), String> {
        let mut slot = self.node.lock().await;
        if let Some(old) = slot.take() {
            old.shutdown();
        }
        let clipboard = self.clipboard().await?;
        let mut attempt = 0;
        let (node, rx) = loop {
            match Node::start(node_config(settings, paths, clipboard.clone(), DEFAULT_PORT)).await {
                Ok(started) => break started,
                Err(e) if attempt < PORT_RETRIES => {
                    attempt += 1;
                    tracing::debug!("port {DEFAULT_PORT} busy ({e}), retrying");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                Err(e) => {
                    tracing::warn!("port {DEFAULT_PORT} unavailable ({e}), using a random port");
                    break Node::start(node_config(settings, paths, clipboard.clone(), 0))
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
        };
        tauri::async_runtime::spawn(pump(rx, sink));
        *slot = Some(node);
        Ok(())
    }

    pub async fn node(&self) -> Result<Node, String> {
        self.node.lock().await.clone().ok_or_else(|| "Wisp is still starting".to_string())
    }

    pub async fn stop(&self) {
        if let Some(node) = self.node.lock().await.take() {
            node.shutdown();
        }
    }

    /// One OS clipboard watcher for the whole app lifetime, shared across engine restarts.
    async fn clipboard(&self) -> Result<Arc<dyn ClipboardBackend>, String> {
        let mut slot = self.clipboard.lock().await;
        if let Some(c) = slot.as_ref() {
            return Ok(c.clone());
        }
        let c: Arc<dyn ClipboardBackend> = Arc::new(SystemClipboard::start().map_err(|e| e.to_string())?);
        *slot = Some(c.clone());
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use uuid::Uuid;
    use wisp_core::clipboard::MemoryClipboard;

    #[test]
    fn peer_addresses_accept_ip_or_ip_and_port() {
        assert_eq!(parse_peer_addr("192.168.1.20").unwrap(), "192.168.1.20:47800".parse().unwrap());
        assert_eq!(parse_peer_addr(" 10.0.0.5:5000 ").unwrap(), "10.0.0.5:5000".parse().unwrap());
        assert_eq!(parse_peer_addr("fd00::5").unwrap(), "[fd00::5]:47800".parse().unwrap());
        assert!(parse_peer_addr("mac.local").is_err());
    }

    #[test]
    fn node_config_uses_settings_with_sensible_fallbacks() {
        let paths = Paths { data_dir: "/data".into(), default_save_dir: "/dl/Wisp".into() };
        let clip: Arc<dyn ClipboardBackend> = Arc::new(MemoryClipboard::new());
        let defaults = node_config(&AppSettings::default(), &paths, clip.clone(), DEFAULT_PORT);
        assert_eq!(defaults.save_dir, PathBuf::from("/dl/Wisp"));
        assert_eq!(defaults.data_dir, PathBuf::from("/data").join("engine"));
        assert_eq!(defaults.bind.port(), DEFAULT_PORT);
        assert!(defaults.discovery);
        assert!(!defaults.device_name.is_empty());

        let custom = AppSettings {
            device_name: Some("Laptop".into()),
            save_dir: Some("/elsewhere".into()),
            ..AppSettings::default()
        };
        let cfg = node_config(&custom, &paths, clip, 0);
        assert_eq!(cfg.device_name, "Laptop");
        assert_eq!(cfg.save_dir, PathBuf::from("/elsewhere"));
    }

    struct Recorder {
        values: StdMutex<Vec<serde_json::Value>>,
        reacted: StdMutex<usize>,
    }

    impl EventSink for Recorder {
        fn emit(&self, value: serde_json::Value) {
            self.values.lock().unwrap().push(value);
        }
        fn react(&self, _event: &Event) {
            *self.reacted.lock().unwrap() += 1;
        }
    }

    #[tokio::test]
    async fn pump_forwards_every_event_as_ui_json() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = Arc::new(Recorder { values: StdMutex::new(vec![]), reacted: StdMutex::new(0) });
        let task = tokio::spawn(pump(rx, sink.clone()));
        let peer = Uuid::new_v4();
        tx.send(Event::PeerOffline { peer }).unwrap();
        tx.send(Event::Error { message: "x".into() }).unwrap();
        drop(tx);
        task.await.unwrap();
        let values = sink.values.lock().unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0]["type"], "peerOffline");
        assert_eq!(values[1]["message"], "x");
        assert_eq!(*sink.reacted.lock().unwrap(), 2);
    }
}
