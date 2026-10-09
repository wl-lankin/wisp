use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedReceiver;
use wisp_core::clipboard::{ClipboardBackend, MemoryClipboard};
use wisp_core::engine::{Event, Node, NodeConfig};
use wisp_core::history::HistoryQuery;
use wisp_core::identity::Identity;
use wisp_core::item::{ClipPayload, Preview};
use wisp_core::pairing::pair;
use wisp_core::peers::DeliveryMode;
use wisp_core::protocol::write_value;
use wisp_core::transport::Transport;

struct TestNode {
    node: Node,
    events: UnboundedReceiver<Event>,
    clip: Arc<MemoryClipboard>,
    dir: TempDir,
}

impl TestNode {
    fn addr(&self) -> SocketAddr {
        self.node.local_addr().unwrap()
    }
}

async fn start_in(dir: TempDir, name: &str) -> TestNode {
    let clip = Arc::new(MemoryClipboard::new());
    let cfg = config(dir.path(), name, clip.clone());
    let (node, events) = Node::start(cfg).await.unwrap();
    TestNode { node, events, clip, dir }
}

fn config(dir: &Path, name: &str, clip: Arc<dyn ClipboardBackend>) -> NodeConfig {
    NodeConfig {
        data_dir: dir.join("data"),
        save_dir: dir.join("save"),
        device_name: name.into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        discovery: false,
        clipboard: clip,
        clipboard_poll: Duration::from_millis(20),
        dial_interval: Duration::from_millis(200),
        history_cap: wisp_core::history::HISTORY_CAP,
    }
}

async fn start(name: &str) -> TestNode {
    start_in(tempfile::tempdir().unwrap(), name).await
}

async fn wait_for(events: &mut UnboundedReceiver<Event>, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let e = events.recv().await.expect("event stream ended");
            if pred(&e) {
                return e;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

async fn paired() -> (TestNode, TestNode) {
    let mut a = start("Mac").await;
    let mut b = start("PC").await;
    let code = b.node.begin_pairing();
    a.node.pair_with(b.addr(), &code).await.unwrap();
    wait_for(&mut a.events, "a online", |e| matches!(e, Event::PeerOnline { .. })).await;
    wait_for(&mut b.events, "b online", |e| matches!(e, Event::PeerOnline { .. })).await;
    (a, b)
}

#[tokio::test]
async fn pairing_connects_both_sides() {
    let (a, b) = paired().await;
    assert!(a.node.peers().iter().any(|p| p.peer.device_id == b.node.device_id() && p.online));
    assert!(b.node.peers().iter().any(|p| p.peer.name == "Mac" && p.online));
    for side in [&a, &b] {
        assert!(side.dir.path().join("data").join("peers.json").exists(), "paired peer was not persisted");
    }
}

#[tokio::test]
async fn three_wrong_codes_close_the_pairing_session() {
    let a = start("Mac").await;
    let mut b = start("PC").await;
    let code = b.node.begin_pairing();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..3 {
        assert!(a.node.pair_with(b.addr(), wrong).await.is_err());
    }
    wait_for(&mut b.events, "session closed", |e| {
        matches!(e, Event::PairingFailed { reason } if reason.contains("Too many"))
    })
    .await;
    assert!(a.node.pair_with(b.addr(), &code).await.is_err());
    assert!(b.node.peers().is_empty());
}

/// Mirrors the engine's private bidirectional stream header on the wire.
#[derive(Serialize)]
enum StreamKind {
    Pair,
}

/// Opens a pairing stream to `addr` and sends its header, but does not start the exchange.
async fn open_pairing_stream(
    transport: &Transport,
    addr: SocketAddr,
) -> (quinn::Connection, tokio::io::Join<quinn::RecvStream, quinn::SendStream>) {
    let conn = transport.connect(addr, None).await.unwrap();
    let (send, recv) = conn.open_bi().await.unwrap();
    let mut stream = tokio::io::join(recv, send);
    write_value(&mut stream, &StreamKind::Pair).await.unwrap();
    (conn, stream)
}

#[tokio::test]
async fn connections_held_open_cannot_exceed_the_attempt_limit() {
    let mut b = start("PC").await;
    let code = b.node.begin_pairing();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let attacker = Identity::generate("Attacker").unwrap();
    let transport = Transport::bind(&attacker, "127.0.0.1:0".parse().unwrap()).unwrap();

    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(open_pairing_stream(&transport, b.addr()).await);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_last_conn, mut last) = held.pop().unwrap();

    let guesses: Vec<_> = held
        .into_iter()
        .map(|(conn, mut stream)| {
            let attacker = attacker.clone();
            tokio::spawn(async move {
                let result = pair(&mut stream, wrong, &attacker, None).await;
                drop(conn);
                result
            })
        })
        .collect();
    for guess in guesses {
        assert!(guess.await.unwrap().is_err());
    }
    wait_for(&mut b.events, "session closed", |e| {
        matches!(e, Event::PairingFailed { reason } if reason.contains("Too many"))
    })
    .await;

    assert!(pair(&mut last, &code, &attacker, None).await.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(b.node.peers().is_empty());
}

#[tokio::test]
async fn catch_mode_waits_for_the_user() {
    let (a, mut b) = paired().await;
    a.clip.set_external(ClipPayload::Text("hello from mac".into()));
    let Event::OfferReceived(info) =
        wait_for(&mut b.events, "offer", |e| matches!(e, Event::OfferReceived(_))).await
    else {
        unreachable!()
    };
    assert_eq!(info.preview, Preview::Text("hello from mac".into()));
    assert_eq!(b.clip.current(), None);

    assert_eq!(b.node.catch_latest().await.unwrap(), Some(info.item_id));
    wait_for(&mut b.events, "applied", |e| matches!(e, Event::ClipboardApplied { .. })).await;
    assert_eq!(b.clip.current(), Some(ClipPayload::Text("hello from mac".into())));
    assert!(b.node.inbox().is_empty());
}

#[tokio::test]
async fn screenshot_image_round_trips() {
    let (a, mut b) = paired().await;
    let shot = ClipPayload::Image { png: vec![42u8; 300_000], width: 200, height: 100 };
    a.clip.set_external(shot.clone());
    wait_for(&mut b.events, "offer", |e| matches!(e, Event::OfferReceived(_))).await;
    b.node.catch_latest().await.unwrap();
    wait_for(&mut b.events, "applied", |e| matches!(e, Event::ClipboardApplied { .. })).await;
    assert_eq!(b.clip.current(), Some(shot));
}

#[tokio::test]
async fn auto_sync_applies_immediately_without_echo() {
    let (mut a, mut b) = paired().await;
    a.node.set_mode(b.node.device_id(), DeliveryMode::AutoSync).unwrap();
    b.node.set_mode(a.node.device_id(), DeliveryMode::AutoSync).unwrap();

    a.clip.set_external(ClipPayload::Text("sync me".into()));
    wait_for(&mut b.events, "applied", |e| matches!(e, Event::ClipboardApplied { .. })).await;
    assert_eq!(b.clip.current(), Some(ClipPayload::Text("sync me".into())));

    tokio::time::sleep(Duration::from_millis(500)).await;
    while let Ok(e) = a.events.try_recv() {
        assert!(
            !matches!(e, Event::OfferReceived(_) | Event::ClipboardApplied { .. }),
            "item bounced back: {e:?}"
        );
    }
    assert_eq!(b.node.history(&HistoryQuery::default()).unwrap().len(), 1);
}

#[tokio::test]
async fn files_arrive_in_the_save_folder() {
    let (mut a, mut b) = paired().await;
    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("report.pdf"), vec![7u8; 700_000]).unwrap();
    let ids = a.node.send_files(b.node.device_id(), vec![src.path().join("report.pdf")]).await.unwrap();
    assert_eq!(ids.len(), 1);

    let Event::TransferProgress { transfer_id, name, outgoing: false, .. } =
        wait_for(&mut b.events, "incoming progress", |e| matches!(e, Event::TransferProgress { outgoing: false, .. })).await
    else {
        unreachable!()
    };
    assert_eq!((transfer_id, name.as_str()), (ids[0], "report.pdf"), "the receiver must use the sender's header");
    let Event::TransferDone { transfer_id, path: Some(path), .. } =
        wait_for(&mut b.events, "file saved", |e| matches!(e, Event::TransferDone { path: Some(_), .. })).await
    else {
        unreachable!()
    };
    assert_eq!(transfer_id, ids[0]);
    assert_eq!(path, b.dir.path().join("save").join("report.pdf"));
    assert_eq!(std::fs::read(&path).unwrap().len(), 700_000);
    wait_for(&mut a.events, "sender done", |e| matches!(e, Event::TransferDone { path: None, .. })).await;
}

#[tokio::test]
async fn sending_to_an_unknown_peer_errors() {
    let (a, _b) = paired().await;
    assert!(a.node.send_files(uuid::Uuid::new_v4(), vec![]).await.is_err());
}

#[tokio::test]
async fn offline_peer_gets_queued_offers_on_reconnect_without_auto_apply() {
    let (mut a, b) = paired().await;
    let b_id = b.node.device_id();
    b.node.set_mode(a.node.device_id(), DeliveryMode::AutoSync).unwrap();
    let TestNode { node, dir, .. } = b;
    node.shutdown();
    wait_for(&mut a.events, "b offline", |e| matches!(e, Event::PeerOffline { .. })).await;

    for i in 0..3 {
        a.clip.set_external(ClipPayload::Text(format!("while away {i}")));
        tokio::time::sleep(Duration::from_millis(80)).await;
    }

    let mut b = start_in(dir, "PC").await;
    assert_eq!(b.node.device_id(), b_id);
    wait_for(&mut b.events, "b back online", |e| matches!(e, Event::PeerOnline { .. })).await;
    for _ in 0..3 {
        wait_for(&mut b.events, "queued offer", |e| matches!(e, Event::OfferReceived(_))).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(b.clip.current(), None, "a replayed offer must never auto-apply");
    assert_eq!(b.node.inbox().len(), 3);
}

#[tokio::test]
async fn a_forgotten_peer_redialing_does_not_use_up_pairing_attempts() {
    let (mut a, b) = paired().await;
    a.node.forget(b.node.device_id()).unwrap();
    // b still pins a and keeps redialing it every dial interval.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let code = a.node.begin_pairing();
    tokio::time::sleep(Duration::from_millis(600)).await;

    let c = start("Laptop").await;
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..2 {
        assert!(c.node.pair_with(a.addr(), wrong).await.is_err());
    }
    c.node.pair_with(a.addr(), &code).await.expect("the right code must still pair");

    tokio::time::sleep(Duration::from_millis(600)).await;
    let ids: Vec<uuid::Uuid> = a.node.peers().iter().map(|p| p.peer.device_id).collect();
    assert_eq!(ids, vec![c.node.device_id()], "the forgotten peer came back");
    let mut failures = 0;
    while let Ok(e) = a.events.try_recv() {
        if matches!(e, Event::PairingFailed { .. }) {
            failures += 1;
        }
    }
    assert_eq!(failures, 2, "only the two wrong codes may fail");
    drop(b);
}

/// Hands back images re-encoded, like an OS clipboard that converts what it is given.
#[derive(Default)]
struct Reencoding {
    inner: MemoryClipboard,
}

impl ClipboardBackend for Reencoding {
    fn read(&self) -> wisp_core::Result<Option<ClipPayload>> {
        Ok(self.inner.current().map(|p| match p {
            ClipPayload::Image { mut png, width, height } => {
                png.push(0);
                ClipPayload::Image { png, width, height }
            }
            other => other,
        }))
    }
    fn write(&self, payload: &ClipPayload) -> wisp_core::Result<()> {
        self.inner.write(payload)
    }
    fn change_count(&self) -> u64 {
        self.inner.change_count()
    }
}

async fn assert_no_bounce(events: &mut UnboundedReceiver<Event>) {
    tokio::time::sleep(Duration::from_millis(500)).await;
    while let Ok(e) = events.try_recv() {
        assert!(!matches!(e, Event::OfferReceived(_) | Event::ClipboardApplied { .. }), "item bounced back: {e:?}");
    }
}

#[tokio::test]
async fn auto_sync_both_ways_with_a_reencoding_clipboard_does_not_bounce() {
    let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (clip_a, clip_b) = (Arc::new(Reencoding::default()), Arc::new(Reencoding::default()));
    let (a, mut a_events) = Node::start(config(dir_a.path(), "Mac", clip_a.clone())).await.unwrap();
    let (b, mut b_events) = Node::start(config(dir_b.path(), "PC", clip_b.clone())).await.unwrap();
    let code = b.begin_pairing();
    a.pair_with(b.local_addr().unwrap(), &code).await.unwrap();
    wait_for(&mut a_events, "a online", |e| matches!(e, Event::PeerOnline { .. })).await;
    wait_for(&mut b_events, "b online", |e| matches!(e, Event::PeerOnline { .. })).await;
    a.set_mode(b.device_id(), DeliveryMode::AutoSync).unwrap();
    b.set_mode(a.device_id(), DeliveryMode::AutoSync).unwrap();

    let first = ClipPayload::Image { png: vec![1u8; 2_000], width: 20, height: 10 };
    clip_a.inner.set_external(first);
    wait_for(&mut b_events, "applied on b", |e| matches!(e, Event::ClipboardApplied { .. })).await;
    assert_no_bounce(&mut a_events).await;
    assert_no_bounce(&mut b_events).await;

    let second = ClipPayload::Image { png: vec![2u8; 2_000], width: 20, height: 10 };
    clip_b.inner.set_external(second);
    wait_for(&mut a_events, "applied on a", |e| matches!(e, Event::ClipboardApplied { .. })).await;
    assert_no_bounce(&mut b_events).await;
    assert_no_bounce(&mut a_events).await;

    for node in [&a, &b] {
        assert_eq!(node.history(&HistoryQuery::default()).unwrap().len(), 2, "one entry per copy, no echoes");
    }
}

#[tokio::test]
async fn timeline_items_export_to_files() {
    let mut a = start("Mac").await;
    // The monitor snapshots the change count when it starts; a copy before that goes unseen.
    tokio::time::sleep(Duration::from_millis(100)).await;
    a.clip.set_external(ClipPayload::Text("export me".into()));
    let Event::LocalItem { item_id } =
        wait_for(&mut a.events, "local item", |e| matches!(e, Event::LocalItem { .. })).await
    else {
        unreachable!()
    };
    let out = tempfile::tempdir().unwrap();
    let first = a.node.export_item(item_id, out.path()).await.unwrap();
    let second = a.node.export_item(item_id, out.path()).await.unwrap();
    assert_ne!(first, second);
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "export me");
    assert_eq!(first.extension().unwrap(), "txt");
    assert!(first.starts_with(out.path()));
    assert!(a.node.export_item(uuid::Uuid::new_v4(), out.path()).await.is_err());
}

#[tokio::test]
async fn renaming_reaches_connected_peers_and_persists() {
    let (a, mut b) = paired().await;
    let a_id = a.node.device_id();
    let stored = a.node.rename("  Studio Mac  ").await.unwrap();
    assert_eq!(stored, "Studio Mac");
    assert_eq!(a.node.device_name(), "Studio Mac");
    assert_eq!(a.node.device_id(), a_id);
    wait_for(&mut b.events, "b sees the rename", |e| {
        matches!(e, Event::PeerRenamed { peer, name } if *peer == a_id && name == "Studio Mac")
    })
    .await;
    assert!(b.node.peers().iter().any(|p| p.peer.device_id == a_id && p.peer.name == "Studio Mac"));
    let text = std::fs::read_to_string(a.dir.path().join("data").join("identity.json")).unwrap();
    assert!(text.contains("Studio Mac"));
    assert!(matches!(a.node.rename("   ").await, Err(wisp_core::CoreError::Invalid(_))));
}

#[tokio::test]
async fn offer_local_reaches_peers_without_touching_the_clipboard() {
    let (a, mut b) = paired().await;
    a.node.offer_local(ClipPayload::Text("from a screenshot".into())).await;
    wait_for(&mut b.events, "offer", |e| matches!(e, Event::OfferReceived(_))).await;
    assert_eq!(a.clip.current(), None);
    assert_eq!(a.node.history(&HistoryQuery::default()).unwrap().len(), 1);
}
