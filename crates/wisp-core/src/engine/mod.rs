mod files;
mod links;
mod session;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::clipboard::{write_and_mark, ClipboardBackend, EchoGuard};
use crate::discovery::{DiscoveredSet, Discovery};
use crate::fsutil::create_private_dir;
use crate::history::{History, HistoryEntry, HistoryQuery};
use crate::identity::{Identity, Os};
use crate::item::{ClipItem, ItemKind, Preview};
use crate::pairing::PairingSession;
use crate::peers::{DeliveryMode, Peer, PeerStore};
use crate::policy::{offer_for, Inbox, InboxEntry, OfferQueue};
use crate::protocol::Message;
use crate::transfer::{cleanup_partials, expand_paths, RESUME_WINDOW};
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
pub struct NearbyDevice {
    pub device: Uuid,
    pub name: String,
    pub os: Os,
    pub addresses: Vec<SocketAddr>,
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
    save_dir: PathBuf,
    tmp_dir: PathBuf,
    /// Devices seen on the local network, paired or not.
    discovered: Mutex<DiscoveredSet>,
    /// Peers with a dial in flight, so ticks and manual adds do not stack dials.
    dialing: Mutex<HashSet<Uuid>>,
    closed: AtomicBool,
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
        // tmp only holds clipboard spills of a previous run, which may carry secrets.
        let tmp_dir = cfg.data_dir.join("tmp");
        match std::fs::remove_dir_all(&tmp_dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        create_private_dir(&tmp_dir)?;
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
            save_dir: cfg.save_dir,
            tmp_dir,
            discovered: Mutex::new(DiscoveredSet::new()),
            dialing: Mutex::new(HashSet::new()),
            closed: AtomicBool::new(false),
        });
        inner.clone().spawn_accept_loop();
        inner.clone().spawn_clipboard_loop(cfg.clipboard_poll);
        if cfg.discovery {
            inner.start_discovery()?;
        }
        inner.clone().spawn_dialer(cfg.dial_interval);
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
        // Held across both steps, so `register` sees either the peer with its connection or
        // neither.
        let mut peers = self.inner.peers.lock().unwrap();
        peers.remove(peer)?;
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
        // A file-sized item arrives as a file transfer, never as a Push, so it is not tracked.
        let result = if entry.kind == ItemKind::File {
            self.inner.send_to(entry.peer, &Message::Pull { item_id }).await
        } else {
            self.inner.request_pull(entry.peer, item_id).await
        };
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

    pub async fn send_files(&self, peer: Uuid, paths: Vec<PathBuf>) -> Result<Vec<Uuid>> {
        if self.inner.peers.lock().unwrap().get(peer).is_none() {
            return Err(CoreError::UnknownPeer(peer.to_string()));
        }
        let files = expand_paths(&paths)?;
        Ok(files
            .into_iter()
            .map(|(path, name)| self.inner.spawn_send(peer, path, name, false))
            .collect())
    }

    /// Manual "add by IP" for a paired device; dials right away.
    pub fn add_address(&self, peer: Uuid, addr: SocketAddr) -> Result<()> {
        if self.inner.is_closed() {
            return Ok(());
        }
        self.inner.peers.lock().unwrap().remember_address(peer, addr)?;
        self.inner.clone().dial(peer);
        Ok(())
    }

    /// Devices discovered on the local network that are not paired yet.
    pub fn nearby(&self) -> Vec<NearbyDevice> {
        // Lock order everywhere: peers, then conns, then discovered.
        let peers = self.inner.peers.lock().unwrap();
        let discovered = self.inner.discovered.lock().unwrap();
        discovered
            .all()
            .into_iter()
            .filter(|d| peers.get(d.device_id).is_none())
            .map(|d| NearbyDevice { device: d.device_id, name: d.name.clone(), os: d.os, addresses: d.addresses.clone() })
            .collect()
    }

    pub fn shutdown(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
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
    use crate::identity::Fingerprint;
    use crate::item::{ClipPayload, MAX_CLIP_BYTES};

    async fn start_node(dir: &std::path::Path) -> (Node, Arc<MemoryClipboard>, mpsc::UnboundedReceiver<Event>) {
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
        let (node, events) = Node::start(cfg).await.unwrap();
        (node, clip, events)
    }

    async fn node(dir: &std::path::Path) -> (Node, Arc<MemoryClipboard>) {
        let (node, clip, _events) = start_node(dir).await;
        (node, clip)
    }

    async fn wait_for(events: &mut mpsc::UnboundedReceiver<Event>, pred: impl Fn(&Event) -> bool) -> Event {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let e = events.recv().await.expect("event stream ended");
                if pred(&e) {
                    return e;
                }
            }
        })
        .await
        .expect("timed out waiting for an event")
    }

    fn tmp_files(dir: &std::path::Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir.join("data").join("tmp")).unwrap().map(|e| e.unwrap().path()).collect()
    }

    /// A connection from `node` to a fresh device that is not paired with it. The returned
    /// server-side handle keeps the connection open.
    async fn raw_connection(node: &Node) -> (quinn::Connection, quinn::Connection, Identity) {
        let other = Identity::generate("Other").unwrap();
        let transport = Transport::bind(&other, "127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = transport.local_addr().unwrap();
        let accept = tokio::spawn(async move { transport.accept().await });
        let conn = node.inner.transport.connect(addr, None).await.unwrap();
        let server_side = accept.await.unwrap().unwrap().unwrap();
        (conn, server_side, other)
    }

    fn pinned(id: &Identity, fingerprint: Fingerprint) -> Peer {
        Peer {
            device_id: id.device_id,
            name: id.name.clone(),
            os: id.os,
            fingerprint,
            addresses: vec![],
            mode: DeliveryMode::Catch,
        }
    }

    #[tokio::test]
    async fn register_refuses_connections_that_are_not_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let (node, _clip) = node(dir.path()).await;

        // Forgotten while the dial was in flight: no longer in the store.
        let (conn, _keep, other) = raw_connection(&node).await;
        node.inner.register(other.device_id, conn.clone(), true);
        assert!(node.inner.connection(other.device_id).is_none());
        assert!(conn.close_reason().is_some());

        // Re-paired meanwhile with a different certificate.
        let (conn, _keep, other) = raw_connection(&node).await;
        node.inner.peers.lock().unwrap().upsert(pinned(&other, Fingerprint([9; 32]))).unwrap();
        node.inner.register(other.device_id, conn.clone(), true);
        assert!(node.inner.connection(other.device_id).is_none());
        assert!(conn.close_reason().is_some());

        // Pinned with this very certificate.
        let (conn, _keep, other) = raw_connection(&node).await;
        node.inner.peers.lock().unwrap().upsert(pinned(&other, other.fingerprint())).unwrap();
        node.inner.register(other.device_id, conn.clone(), true);
        assert!(node.inner.connection(other.device_id).is_some());
        node.shutdown();
    }

    fn queue_three(node: &Node, peer: Uuid) -> Vec<Uuid> {
        let items: Vec<ClipItem> =
            (0..3).map(|i| ClipItem::new(node.device_id(), ClipPayload::Text(format!("queued {i}")))).collect();
        let mut queue = node.inner.queue.lock().unwrap();
        for item in &items {
            queue.push(peer, item);
        }
        items.iter().map(|i| i.id).collect()
    }

    fn queued_ids(node: &Node, peer: Uuid) -> Vec<Uuid> {
        node.inner
            .queue
            .lock()
            .unwrap()
            .drain(peer)
            .into_iter()
            .map(|m| match m {
                Message::Offer { item_id, .. } => item_id,
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    #[tokio::test]
    async fn queued_offers_stay_queued_unless_delivered_on_the_current_connection() {
        let dir = tempfile::tempdir().unwrap();
        let (node, _clip) = node(dir.path()).await;
        let peer = Uuid::new_v4();
        let (current, _keep_current, _) = raw_connection(&node).await;
        let (replaced, _keep_replaced, _) = raw_connection(&node).await;
        node.inner.conns.lock().unwrap().insert(peer, Conn { conn: current.clone(), initiated_by_me: true });

        // Closed as a duplicate or replaced: it must not take the queue with it.
        let ids = queue_three(&node, peer);
        node.inner.clone().deliver_queued(peer, replaced).await;
        assert_eq!(queued_ids(&node, peer), ids);

        // Still the current connection, but the sends fail: everything goes back in order.
        let ids = queue_three(&node, peer);
        current.close(0u32.into(), b"gone");
        node.inner.clone().deliver_queued(peer, current).await;
        assert_eq!(queued_ids(&node, peer), ids);
        node.shutdown();
    }

    #[tokio::test]
    async fn a_dropped_incoming_transfer_fails_under_the_sender_id() {
        let dir = tempfile::tempdir().unwrap();
        let (node, _clip, mut events) = start_node(dir.path()).await;
        let (peer, transfer_id) = (Uuid::new_v4(), Uuid::new_v4());
        let (mut sender, receiver) = tokio::io::duplex(64 * 1024);
        let start = Message::FileStart { transfer_id, name: "big.bin".into(), size: 100, hash: [0; 32] };
        crate::protocol::write_frame(&mut sender, &start).await.unwrap();
        let receive = node.inner.receive_on(peer, receiver);
        let drop_sender = async move {
            let _ack = crate::protocol::read_frame(&mut sender).await.unwrap();
            drop(sender);
        };
        let (result, ()) = tokio::join!(receive, drop_sender);
        assert!(result.is_err());
        let progress = wait_for(&mut events, |e| matches!(e, Event::TransferProgress { .. })).await;
        assert!(matches!(progress, Event::TransferProgress { transfer_id: id, ref name, done: 0, total: 100, .. }
            if id == transfer_id && name == "big.bin"));
        let failed = wait_for(&mut events, |e| matches!(e, Event::TransferFailed { .. })).await;
        assert!(matches!(failed, Event::TransferFailed { transfer_id: id, peer: p, .. } if id == transfer_id && p == peer));
    }

    #[tokio::test]
    async fn start_removes_leftover_spills() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join("data").join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("stale-clipboard.txt"), b"secret").unwrap();
        let (_node, _clip) = node(dir.path()).await;
        assert!(tmp_files(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn oversized_pulls_spill_to_a_unique_file_per_serve() {
        let dir = tempfile::tempdir().unwrap();
        let (node, _clip) = node(dir.path()).await;
        let big = "x".repeat(MAX_CLIP_BYTES as usize + 1);
        let item = ClipItem::new(node.device_id(), ClipPayload::Text(big));
        node.inner.history.lock().unwrap().insert(&item).unwrap();
        // Nobody is connected, so both sends stay in their retry loop with the spill kept.
        let peer = Uuid::new_v4();
        node.inner.serve_pull(peer, item.id).await.unwrap();
        node.inner.serve_pull(peer, item.id).await.unwrap();
        let spills = tmp_files(dir.path());
        assert_eq!(spills.len(), 2, "{spills:?}");
        for spill in &spills {
            assert_eq!(std::fs::metadata(spill).unwrap().len(), MAX_CLIP_BYTES + 1);
        }
        node.shutdown();
    }

    #[tokio::test]
    async fn catching_a_file_offer_tracks_no_pull() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let (a, _ca, mut a_events) = start_node(dir_a.path()).await;
        let (b, _cb) = node(dir_b.path()).await;
        let code = b.begin_pairing();
        a.pair_with(b.local_addr().unwrap(), &code).await.unwrap();
        wait_for(&mut a_events, |e| matches!(e, Event::PeerOnline { .. })).await;

        let item_id = Uuid::new_v4();
        a.inner.inbox.lock().unwrap().add(InboxEntry {
            peer: b.device_id(),
            item_id,
            kind: ItemKind::File,
            size: MAX_CLIP_BYTES + 1,
            preview: Preview::File { name: "clipboard-1.txt".into() },
            received: Instant::now(),
        });
        a.catch(item_id).await.unwrap();
        assert!(a.inner.pulls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn sends_stop_after_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let (node, _clip, mut events) = start_node(dir.path()).await;
        let spill = dir.path().join("data").join("tmp").join("spill.txt");
        std::fs::write(&spill, b"data").unwrap();
        node.shutdown();
        let id = node.inner.spawn_send(Uuid::new_v4(), spill.clone(), "spill.txt".into(), true);
        let failed = wait_for(&mut events, |e| matches!(e, Event::TransferFailed { .. })).await;
        let Event::TransferFailed { transfer_id, reason, .. } = failed else { unreachable!() };
        assert_eq!((transfer_id, reason.as_str()), (id, "shut down"));
        // The spill is removed right after the event; allow for a busy test runtime.
        let deadline = Instant::now() + Duration::from_secs(2);
        while spill.exists() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!spill.exists());
    }

    #[tokio::test]
    async fn no_dials_after_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let (node, _clip) = node(dir.path()).await;
        node.shutdown();
        let peer = Uuid::new_v4();
        node.inner.clone().dial(peer);
        assert!(node.inner.dialing.lock().unwrap().is_empty());
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
