mod session;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::clipboard::{write_and_mark, ClipboardBackend, EchoGuard};
use crate::discovery::Discovery;
use crate::history::{History, HistoryEntry, HistoryQuery};
use crate::identity::{Identity, Os};
use crate::item::{ClipItem, ItemKind, Preview};
use crate::pairing::PairingSession;
use crate::peers::{DeliveryMode, Peer, PeerStore};
use crate::policy::{offer_for, Inbox, InboxEntry, OfferQueue};
use crate::transfer::{cleanup_partials, RESUME_WINDOW};
use crate::transport::Transport;
use crate::{CoreError, Result};

pub struct NodeConfig {
    pub data_dir: PathBuf,
    pub save_dir: PathBuf,
    pub device_name: String,
    pub bind: SocketAddr,
    pub discovery: bool,
    pub clipboard: Arc<dyn ClipboardBackend>,
    pub clipboard_poll: Duration,
    pub dial_interval: Duration,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferInfo {
    pub peer: Uuid,
    pub item_id: Uuid,
    pub kind: ItemKind,
    pub size: u64,
    pub preview: Preview,
    pub age_ms: u64,
}

impl From<&InboxEntry> for OfferInfo {
    fn from(e: &InboxEntry) -> Self {
        Self {
            peer: e.peer,
            item_id: e.item_id,
            kind: e.kind,
            size: e.size,
            preview: e.preview.clone(),
            age_ms: e.received.elapsed().as_millis() as u64,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerStatus {
    pub peer: Peer,
    pub online: bool,
    pub addr: Option<SocketAddr>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    PeerOnline { peer: Uuid, name: String, addr: SocketAddr },
    PeerOffline { peer: Uuid },
    Discovered { device: Uuid, name: String, os: Os, addresses: Vec<SocketAddr> },
    IdentityChanged { peer: Uuid },
    Paired { peer: Uuid, name: String },
    PairingFailed { reason: String },
    LocalItem { item_id: Uuid },
    OfferReceived(OfferInfo),
    ClipboardApplied { item_id: Uuid, from: Uuid },
    TransferProgress { transfer_id: Uuid, peer: Uuid, name: String, done: u64, total: u64, outgoing: bool },
    TransferPaused { transfer_id: Uuid, peer: Uuid },
    TransferDone { transfer_id: Uuid, peer: Uuid, path: Option<PathBuf> },
    TransferFailed { transfer_id: Uuid, peer: Uuid, reason: String },
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
enum StreamKind {
    Pair,
    File,
}

struct Conn {
    conn: quinn::Connection,
    initiated_by_me: bool,
}

struct Inner {
    identity: Identity,
    transport: Transport,
    clipboard: Arc<dyn ClipboardBackend>,
    events: mpsc::UnboundedSender<Event>,
    peers: Mutex<PeerStore>,
    conns: Mutex<HashMap<Uuid, Conn>>,
    history: Mutex<History>,
    inbox: Mutex<Inbox>,
    queue: Mutex<OfferQueue>,
    /// Items this device asked for, mapped to the peer that was asked.
    pulls: Mutex<HashMap<Uuid, Uuid>>,
    echo: Arc<Mutex<EchoGuard>>,
    pairing: Mutex<Option<PairingSession>>,
    discovery: Mutex<Option<Discovery>>,
}

#[derive(Clone)]
pub struct Node {
    inner: Arc<Inner>,
}

impl Node {
    pub async fn start(cfg: NodeConfig) -> Result<(Node, mpsc::UnboundedReceiver<Event>)> {
        let identity = Identity::load_or_create(&cfg.data_dir.join("identity.json"), &cfg.device_name)?;
        let transport = Transport::bind(&identity, cfg.bind)?;
        let peers = PeerStore::open(&cfg.data_dir.join("peers.json"))?;
        let history = History::open(&cfg.data_dir.join("history"))?;
        std::fs::create_dir_all(&cfg.save_dir)?;
        std::fs::create_dir_all(cfg.data_dir.join("tmp"))?;
        cleanup_partials(&cfg.save_dir, RESUME_WINDOW)?;

        let (events, rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            identity,
            transport,
            clipboard: cfg.clipboard,
            events,
            peers: Mutex::new(peers),
            conns: Mutex::new(HashMap::new()),
            history: Mutex::new(history),
            inbox: Mutex::new(Inbox::new()),
            queue: Mutex::new(OfferQueue::new()),
            pulls: Mutex::new(HashMap::new()),
            echo: Arc::new(Mutex::new(EchoGuard::new())),
            pairing: Mutex::new(None),
            discovery: Mutex::new(None),
        });
        inner.clone().spawn_accept_loop();
        inner.clone().spawn_clipboard_loop(cfg.clipboard_poll);
        Ok((Node { inner }, rx))
    }

    pub fn device_id(&self) -> Uuid {
        self.inner.identity.device_id
    }

    pub fn device_name(&self) -> String {
        self.inner.identity.name.clone()
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.inner.transport.local_addr()
    }

    /// Opens a pairing session on this device and returns the code to show.
    pub fn begin_pairing(&self) -> String {
        let session = PairingSession::new();
        let code = session.code().to_string();
        *self.inner.pairing.lock().unwrap() = Some(session);
        code
    }

    pub fn cancel_pairing(&self) {
        *self.inner.pairing.lock().unwrap() = None;
    }

    /// Pairs with the device at `addr` using the code shown on that device.
    pub async fn pair_with(&self, addr: SocketAddr, code: &str) -> Result<Peer> {
        self.inner.pair_with(addr, code).await
    }

    pub fn peers(&self) -> Vec<PeerStatus> {
        // Lock order everywhere: peers, then conns.
        let peers = self.inner.peers.lock().unwrap();
        let conns = self.inner.conns.lock().unwrap();
        peers
            .all()
            .iter()
            .map(|p| {
                let conn = conns.get(&p.device_id);
                PeerStatus { peer: p.clone(), online: conn.is_some(), addr: conn.map(|c| c.conn.remote_address()) }
            })
            .collect()
    }

    pub fn set_mode(&self, peer: Uuid, mode: DeliveryMode) -> Result<()> {
        self.inner.peers.lock().unwrap().set_mode(peer, mode)
    }

    pub fn forget(&self, peer: Uuid) -> Result<()> {
        self.inner.peers.lock().unwrap().remove(peer)?;
        if let Some(c) = self.inner.conns.lock().unwrap().remove(&peer) {
            c.conn.close(0u32.into(), b"forgotten");
        }
        Ok(())
    }

    pub fn inbox(&self) -> Vec<OfferInfo> {
        self.inner.inbox.lock().unwrap().all().into_iter().map(OfferInfo::from).collect()
    }

    pub async fn catch(&self, item_id: Uuid) -> Result<()> {
        let entry = self
            .inner
            .inbox
            .lock()
            .unwrap()
            .take(item_id)
            .ok_or_else(|| CoreError::Transfer("this offer is no longer available".into()))?;
        let result = self.inner.request_pull(entry.peer, item_id).await;
        if result.is_err() {
            self.inner.inbox.lock().unwrap().add(entry);
        }
        result
    }

    pub async fn catch_latest(&self) -> Result<Option<Uuid>> {
        let latest = self.inner.inbox.lock().unwrap().latest().map(|e| e.item_id);
        match latest {
            Some(id) => {
                self.catch(id).await?;
                Ok(Some(id))
            }
            None => Ok(None),
        }
    }

    pub fn dismiss(&self, item_id: Uuid) {
        self.inner.inbox.lock().unwrap().dismiss(item_id);
    }

    pub fn history(&self, q: &HistoryQuery) -> Result<Vec<HistoryEntry>> {
        self.inner.history.lock().unwrap().query(q)
    }

    /// Puts a timeline item back on this device's clipboard without re-offering it.
    pub async fn copy_from_history(&self, id: Uuid) -> Result<()> {
        let item = self.inner.history.lock().unwrap().load(id)?;
        let item = item.ok_or_else(|| CoreError::Transfer("item is no longer in the timeline".into()))?;
        write_and_mark(self.inner.clipboard.as_ref(), &self.inner.echo, &item.payload).await
    }

    pub async fn send_clipboard_to(&self, peer: Uuid) -> Result<()> {
        let payload = self
            .inner
            .clipboard
            .read()?
            .ok_or_else(|| CoreError::Clipboard("the clipboard is empty".into()))?;
        let item = ClipItem::new(self.device_id(), payload);
        self.inner.history.lock().unwrap().insert(&item)?;
        self.inner.send_to(peer, &offer_for(&item, true)).await
    }

    pub fn shutdown(&self) {
        if let Some(d) = self.inner.discovery.lock().unwrap().take() {
            d.stop();
        }
        self.inner.transport.close();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::clipboard::MemoryClipboard;
    use crate::item::ClipPayload;

    async fn node(dir: &std::path::Path) -> (Node, Arc<MemoryClipboard>) {
        let clip = Arc::new(MemoryClipboard::new());
        let cfg = NodeConfig {
            data_dir: dir.join("data"),
            save_dir: dir.join("save"),
            device_name: "Test".into(),
            bind: "127.0.0.1:0".parse().unwrap(),
            discovery: false,
            clipboard: clip.clone(),
            clipboard_poll: Duration::from_millis(20),
            dial_interval: Duration::from_millis(200),
        };
        let (node, _events) = Node::start(cfg).await.unwrap();
        (node, clip)
    }

    #[tokio::test]
    async fn push_from_a_peer_that_was_not_asked_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let (node, clip) = node(dir.path()).await;
        let (asked, other, item_id) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        node.inner.pulls.lock().unwrap().insert(item_id, asked);

        node.inner.apply_remote(other, item_id, ClipPayload::Text("forged".into())).await;
        assert_eq!(clip.current(), None);
        assert_eq!(node.inner.pulls.lock().unwrap().get(&item_id), Some(&asked));

        node.inner.apply_remote(asked, item_id, ClipPayload::Text("real".into())).await;
        assert_eq!(clip.current(), Some(ClipPayload::Text("real".into())));
        assert!(node.inner.pulls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_pull_request_leaves_no_pending_pull() {
        let dir = tempfile::tempdir().unwrap();
        let (node, _clip) = node(dir.path()).await;
        assert!(node.inner.request_pull(Uuid::new_v4(), Uuid::new_v4()).await.is_err());
        assert!(node.inner.pulls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_catch_keeps_the_offer_in_the_inbox() {
        let dir = tempfile::tempdir().unwrap();
        let (node, _clip) = node(dir.path()).await;
        let item_id = Uuid::new_v4();
        node.inner.inbox.lock().unwrap().add(InboxEntry {
            peer: Uuid::new_v4(),
            item_id,
            kind: ItemKind::Text,
            size: 2,
            preview: Preview::Text("hi".into()),
            received: Instant::now(),
        });
        assert!(node.catch(item_id).await.is_err());
        assert_eq!(node.inbox().len(), 1);
    }

    #[tokio::test]
    async fn only_own_items_are_served_on_pull() {
        let dir = tempfile::tempdir().unwrap();
        let (node, _clip) = node(dir.path()).await;
        let foreign = ClipItem::new(Uuid::new_v4(), ClipPayload::Text("not mine".into()));
        node.inner.history.lock().unwrap().insert(&foreign).unwrap();
        let err = node.inner.serve_pull(Uuid::new_v4(), foreign.id).await.unwrap_err();
        assert!(err.to_string().contains("not from this device"), "{err}");
    }
}
