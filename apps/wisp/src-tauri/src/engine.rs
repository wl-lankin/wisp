use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, Mutex};
use wisp_core::clipboard::{ClipboardBackend, SystemClipboard};
use wisp_core::engine::{Event, Node, NodeConfig};
use wisp_core::history::HISTORY_CAP;
use wisp_core::CoreError;

use crate::dto::event_to_ui;
use crate::settings::AppSettings;

pub const DEFAULT_PORT: u16 = 47800;
const PORT_BUDGET: Duration = Duration::from_secs(3);
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(3);

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
        history_cap: HISTORY_CAP,
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

/// Forwards events only while its generation is the newest, so a replaced node's pump goes quiet.
struct GenerationSink {
    generation: u64,
    current: Arc<AtomicU64>,
    inner: Arc<dyn EventSink>,
}

impl GenerationSink {
    fn is_current(&self) -> bool {
        self.current.load(Ordering::SeqCst) == self.generation
    }
}

impl EventSink for GenerationSink {
    fn emit(&self, value: Value) {
        if self.is_current() {
            self.inner.emit(value);
        }
    }

    fn react(&self, event: &Event) {
        if self.is_current() {
            self.inner.react(event);
        }
    }
}

/// Why the preferred port could not be bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortProblem {
    /// Taken right now, possibly by an older Wisp still letting go; worth a short retry.
    Busy,
    /// Never usable here (for example inside a Windows excluded port range); retrying cannot help.
    Unavailable,
}

fn port_problem(e: &CoreError) -> Option<PortProblem> {
    use std::io::ErrorKind;
    match e {
        CoreError::Io(io) => match io.kind() {
            ErrorKind::AddrInUse => Some(PortProblem::Busy),
            ErrorKind::PermissionDenied | ErrorKind::AddrNotAvailable => Some(PortProblem::Unavailable),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Default)]
pub struct Engine {
    node: Mutex<Option<Node>>,
    clipboard: Mutex<Option<Arc<dyn ClipboardBackend>>>,
    generation: Arc<AtomicU64>,
}

impl Engine {
    #[cfg(test)]
    fn with_clipboard(clipboard: Arc<dyn ClipboardBackend>) -> Self {
        Self { clipboard: Mutex::new(Some(clipboard)), ..Self::default() }
    }

    pub async fn start(&self, settings: &AppSettings, paths: &Paths, sink: Arc<dyn EventSink>) -> Result<(), String> {
        self.start_on(settings, paths, sink, DEFAULT_PORT).await
    }

    async fn start_on(
        &self,
        settings: &AppSettings,
        paths: &Paths,
        sink: Arc<dyn EventSink>,
        preferred_port: u16,
    ) -> Result<(), String> {
        let mut slot = self.node.lock().await;
        // Silence the old pump first; its node may still emit while it winds down.
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(old) = slot.take() {
            old.shutdown_and_wait(SHUTDOWN_BUDGET).await;
        }
        let clipboard = self.clipboard().await?;
        let sink: Arc<dyn EventSink> =
            Arc::new(GenerationSink { generation, current: self.generation.clone(), inner: sink });
        let deadline = tokio::time::Instant::now() + PORT_BUDGET;
        let (node, rx) = loop {
            let error = match Node::start(node_config(settings, paths, clipboard.clone(), preferred_port)).await {
                Ok(started) => break started,
                Err(e) => e,
            };
            match port_problem(&error) {
                Some(PortProblem::Busy) if tokio::time::Instant::now() < deadline => {
                    tracing::debug!("port {preferred_port} busy, retrying");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Some(_) => {
                    tracing::warn!("port {preferred_port} unavailable, using a random port");
                    let started = Node::start(node_config(settings, paths, clipboard.clone(), 0))
                        .await
                        .map_err(|e| e.to_string())?;
                    let port = started.0.local_addr().map(|a| a.port()).unwrap_or_default();
                    sink.emit(serde_json::json!({
                        "type": "error",
                        "message": format!("Port {preferred_port} was busy, Wisp is using port {port} for now"),
                    }));
                    break started;
                }
                None => return Err(error.to_string()),
            }
        };
        tauri::async_runtime::spawn(pump(rx, sink.clone()));
        *slot = Some(node);
        sink.emit(serde_json::json!({ "type": "engineRestarted" }));
        Ok(())
    }

    pub async fn node(&self) -> Result<Node, String> {
        self.node.lock().await.clone().ok_or_else(|| "Wisp is still starting".to_string())
    }

    pub async fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(node) = self.node.lock().await.take() {
            node.shutdown_and_wait(SHUTDOWN_BUDGET).await;
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

    #[tokio::test]
    async fn stale_generation_forwards_nothing() {
        let current = Arc::new(AtomicU64::new(1));
        let recorder = Arc::new(Recorder { values: StdMutex::new(vec![]), reacted: StdMutex::new(0) });
        let sink: Arc<dyn EventSink> =
            Arc::new(GenerationSink { generation: 1, current: current.clone(), inner: recorder.clone() });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(pump(rx, sink));
        tx.send(Event::Error { message: "old".into() }).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        current.store(2, Ordering::SeqCst);
        tx.send(Event::Error { message: "stale".into() }).unwrap();
        drop(tx);
        task.await.unwrap();
        let values = recorder.values.lock().unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0]["message"], "old");
        assert_eq!(*recorder.reacted.lock().unwrap(), 1);
    }

    #[test]
    fn port_errors_are_sorted_into_busy_and_unavailable() {
        use std::io::{Error, ErrorKind};
        let io = |kind: ErrorKind| CoreError::Io(Error::from(kind));
        assert_eq!(port_problem(&io(ErrorKind::AddrInUse)), Some(PortProblem::Busy));
        // A port inside a Windows excluded range (Hyper-V, WSL) reports PermissionDenied.
        assert_eq!(port_problem(&io(ErrorKind::PermissionDenied)), Some(PortProblem::Unavailable));
        assert_eq!(port_problem(&io(ErrorKind::AddrNotAvailable)), Some(PortProblem::Unavailable));
        assert_eq!(port_problem(&io(ErrorKind::NotFound)), None);
    }

    #[tokio::test]
    async fn restart_reuses_the_port_and_announces_itself() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths { data_dir: dir.path().join("data"), default_save_dir: dir.path().join("save") };
        let port = std::net::UdpSocket::bind(("0.0.0.0", 0)).unwrap().local_addr().unwrap().port();
        let engine = Engine::with_clipboard(Arc::new(MemoryClipboard::new()));
        let recorder = Arc::new(Recorder { values: StdMutex::new(vec![]), reacted: StdMutex::new(0) });
        let settings = AppSettings::default();

        engine.start_on(&settings, &paths, recorder.clone(), port).await.unwrap();
        assert_eq!(engine.node().await.unwrap().local_addr().unwrap().port(), port);
        engine.start_on(&settings, &paths, recorder.clone(), port).await.unwrap();
        assert_eq!(engine.node().await.unwrap().local_addr().unwrap().port(), port);

        engine.stop().await;
        let values = recorder.values.lock().unwrap();
        let restarts = values.iter().filter(|v| v["type"] == "engineRestarted").count();
        assert_eq!(restarts, 2);
        assert!(values.iter().all(|v| v["type"] != "error"), "no port fallback expected: {values:?}");
    }
}
