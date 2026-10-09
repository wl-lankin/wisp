use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::{Conn, Event, Inner, OfferInfo, StreamKind};
use crate::clipboard::{spawn_monitor, write_and_mark};
use crate::fsutil::write_private_file;
use crate::identity::Fingerprint;
use crate::item::{ClipItem, ClipPayload, Preview, MAX_CLIP_BYTES};
use crate::pairing::{pair, PairInfo};
use crate::peers::{DeliveryMode, Peer};
use crate::policy::{offer_for, on_offer, InboxEntry, OfferAction};
use crate::protocol::{read_frame, read_value_max, write_frame, write_value, Message, PROTOCOL_VERSION};
use crate::transport::peer_fingerprint;
use crate::{CoreError, Result};

/// Bounds a whole pairing exchange, so a silent or stalled peer cannot hold it open.
const PAIRING_STREAM_TIMEOUT: Duration = Duration::from_secs(60);
/// A `StreamKind` frame is a few bytes; anything larger is not a valid header.
const MAX_STREAM_KIND_FRAME: usize = 16;
/// How long an unknown device has to open its pairing stream before it is dropped.
const STREAM_HEADER_TIMEOUT: Duration = Duration::from_secs(5);

/// Lets simultaneous dials settle on one connection before queued offers are replayed.
const QUEUE_SETTLE: Duration = Duration::from_millis(500);
/// Bounds one offer delivery to one peer.
const SEND_TIMEOUT: Duration = Duration::from_secs(2);

fn net_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Transport(e.to_string())
}

/// Opens a uni stream on this exact connection and sends one message.
async fn send_on(conn: &quinn::Connection, msg: &Message) -> Result<()> {
    let mut stream = conn.open_uni().await.map_err(net_err)?;
    write_frame(&mut stream, msg).await?;
    stream.finish().map_err(net_err)?;
    Ok(())
}

/// Like `send_on`, but only succeeds once the peer acknowledged every byte, so an offer on a
/// connection that dies right after the write is not mistaken for a delivered one.
async fn send_acked(conn: &quinn::Connection, msg: &Message) -> Result<()> {
    let mut stream = conn.open_uni().await.map_err(net_err)?;
    write_frame(&mut stream, msg).await?;
    stream.finish().map_err(net_err)?;
    match stream.stopped().await {
        Ok(None) => Ok(()),
        Ok(Some(code)) => Err(CoreError::Transport(format!("peer stopped the stream ({code})"))),
        Err(e) => Err(net_err(e)),
    }
}

fn pairing_timeout() -> CoreError {
    CoreError::Pairing("timed out".into())
}

impl Inner {
    pub(super) fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    pub(super) fn connection(&self, peer: Uuid) -> Option<quinn::Connection> {
        self.conns.lock().unwrap().get(&peer).map(|c| c.conn.clone())
    }

    pub(super) async fn send_to(&self, peer: Uuid, msg: &Message) -> Result<()> {
        let conn = self.connection(peer).ok_or_else(|| CoreError::UnknownPeer(peer.to_string()))?;
        let mut stream = conn.open_uni().await.map_err(net_err)?;
        write_frame(&mut stream, msg).await?;
        stream.finish().map_err(net_err)?;
        Ok(())
    }

    pub(super) async fn request_pull(&self, peer: Uuid, item_id: Uuid) -> Result<()> {
        self.pulls.lock().unwrap().insert(item_id, peer);
        let result = self.send_to(peer, &Message::Pull { item_id }).await;
        if result.is_err() {
            self.pulls.lock().unwrap().remove(&item_id);
        }
        result
    }

    pub(super) fn spawn_accept_loop(self: Arc<Self>) {
        tokio::spawn(async move {
            while let Some(incoming) = self.transport.accept().await {
                match incoming {
                    Ok(conn) => {
                        tokio::spawn(self.clone().handle_incoming(conn));
                    }
                    Err(e) => tracing::debug!("incoming connection failed: {e}"),
                }
            }
        });
    }

    /// The transport accepts any client certificate; this lookup is the trust gate. A
    /// connection whose fingerprint is not pinned may only run pairing.
    async fn handle_incoming(self: Arc<Self>, conn: quinn::Connection) {
        let Some(fp) = peer_fingerprint(&conn) else {
            conn.close(1u32.into(), b"no certificate");
            return;
        };
        let known = self
            .peers
            .lock()
            .unwrap()
            .all()
            .iter()
            .find(|p| p.fingerprint == fp)
            .map(|p| p.device_id);
        match known {
            Some(peer) => self.register(peer, conn, false),
            None => self.accept_pairing(conn, fp).await,
        }
    }

    async fn accept_pairing(self: &Arc<Self>, conn: quinn::Connection, fp: Fingerprint) {
        // Only a connection that opens a pairing stream is a pairing attempt. A device that
        // still pins this one (for example after `forget`) redials without ever doing so, and
        // must neither use up attempts nor show a pairing failure.
        let header = async {
            let (send, recv) = conn.accept_bi().await.map_err(net_err)?;
            let mut stream = tokio::io::join(recv, send);
            let kind = read_value_max::<_, StreamKind>(&mut stream, MAX_STREAM_KIND_FRAME).await?;
            Ok::<_, CoreError>((kind, stream))
        };
        let mut stream = match tokio::time::timeout(STREAM_HEADER_TIMEOUT, header).await {
            Ok(Ok((StreamKind::Pair, stream))) => stream,
            _ => {
                conn.close(2u32.into(), b"not paired");
                return;
            }
        };
        // The attempt is counted before the exchange starts, so parallel connections cannot
        // run more exchanges than the session allows.
        let attempt = self.pairing.lock().unwrap().as_mut().and_then(|s| s.try_begin_attempt());
        let Some((code, session_id)) = attempt else {
            conn.close(2u32.into(), b"not pairing");
            return;
        };
        let exchange = pair(&mut stream, &code, &self.identity, Some(fp));
        let result = tokio::time::timeout(PAIRING_STREAM_TIMEOUT, exchange)
            .await
            .unwrap_or_else(|_| Err(pairing_timeout()));
        match result {
            Ok(info) => {
                let current = {
                    let mut guard = self.pairing.lock().unwrap();
                    let current = guard.as_ref().is_some_and(|s| s.id() == session_id);
                    if current {
                        *guard = None;
                    }
                    current
                };
                if !current {
                    conn.close(3u32.into(), b"pairing ended");
                    self.emit(Event::PairingFailed { reason: "The pairing session has ended. Start pairing again.".into() });
                    return;
                }
                if let Err(e) = self.finish_pairing(info, conn, false) {
                    self.emit(Event::PairingFailed { reason: e.to_string() });
                }
            }
            Err(e) => {
                let exhausted = {
                    let mut guard = self.pairing.lock().unwrap();
                    let exhausted = guard.as_ref().is_some_and(|s| s.id() == session_id && !s.is_open());
                    if exhausted {
                        *guard = None;
                    }
                    exhausted
                };
                let reason = if exhausted {
                    "Too many wrong codes. Start pairing again.".to_string()
                } else {
                    e.to_string()
                };
                self.emit(Event::PairingFailed { reason });
                conn.close(3u32.into(), b"pairing failed");
            }
        }
    }

    pub(super) async fn pair_with(self: &Arc<Self>, addr: SocketAddr, code: &str) -> Result<Peer> {
        let conn = self.transport.connect(addr, None).await?;
        let fp = peer_fingerprint(&conn).ok_or_else(|| CoreError::Pairing("peer sent no certificate".into()))?;
        let exchange = async {
            let (send, recv) = conn.open_bi().await.map_err(net_err)?;
            let mut stream = tokio::io::join(recv, send);
            write_value(&mut stream, &StreamKind::Pair).await?;
            pair(&mut stream, code, &self.identity, Some(fp)).await
        };
        let result = tokio::time::timeout(PAIRING_STREAM_TIMEOUT, exchange)
            .await
            .unwrap_or_else(|_| Err(pairing_timeout()));
        match result {
            Ok(info) => self.finish_pairing(info, conn, true),
            Err(e) => {
                conn.close(3u32.into(), b"pairing failed");
                Err(e)
            }
        }
    }

    fn finish_pairing(self: &Arc<Self>, info: PairInfo, conn: quinn::Connection, initiated_by_me: bool) -> Result<Peer> {
        let peer = Peer {
            device_id: info.device_id,
            name: info.name,
            os: info.os,
            fingerprint: info.fingerprint,
            addresses: vec![conn.remote_address()],
            mode: DeliveryMode::Catch,
        };
        self.peers.lock().unwrap().upsert(peer.clone())?;
        self.emit(Event::Paired { peer: peer.device_id, name: peer.name.clone() });
        self.register(peer.device_id, conn, initiated_by_me);
        Ok(peer)
    }

    /// Both devices keep the connection initiated by the lower device ID, so simultaneous
    /// dials settle on the same single connection.
    ///
    /// The pin is checked again here, under the peers lock, so a dial or handshake that was
    /// in flight while the peer was forgotten (or re-paired) cannot slip in afterwards.
    pub(super) fn register(self: &Arc<Self>, peer: Uuid, conn: quinn::Connection, initiated_by_me: bool) {
        let addr = conn.remote_address();
        let name = {
            // Lock order everywhere: peers, then conns.
            let mut peers = self.peers.lock().unwrap();
            let pinned = peers.get(peer).map(|p| p.fingerprint);
            if pinned.is_none() || pinned != peer_fingerprint(&conn) {
                conn.close(0u32.into(), b"not paired");
                return;
            }
            {
                let mut conns = self.conns.lock().unwrap();
                if let Some(existing) = conns.get(&peer) {
                    let alive = existing.conn.close_reason().is_none();
                    // A reconnect from the same side replaces a connection that may be stale;
                    // otherwise the connection initiated by the lower device ID wins.
                    let new_wins = existing.initiated_by_me == initiated_by_me
                        || (self.identity.device_id < peer) == initiated_by_me;
                    if alive && !new_wins {
                        conn.close(0u32.into(), b"duplicate");
                        return;
                    }
                    if alive {
                        existing.conn.close(0u32.into(), b"replaced");
                    }
                }
                conns.insert(peer, Conn { conn: conn.clone(), initiated_by_me });
            }
            let _ = peers.remember_address(peer, addr);
            peers.get(peer).map(|p| p.name.clone()).unwrap_or_default()
        };
        self.emit(Event::PeerOnline { peer, name, addr });
        tokio::spawn(self.clone().serve(peer, conn.clone()));

        let this = self.clone();
        tokio::spawn(async move {
            let hello = Message::Hello {
                device_id: this.identity.device_id,
                name: this.identity.name.clone(),
                os: this.identity.os,
                version: PROTOCOL_VERSION,
            };
            let _ = send_on(&conn, &hello).await;
            this.deliver_queued(peer, conn).await;
        });
    }

    fn is_current(&self, peer: Uuid, conn: &quinn::Connection) -> bool {
        self.conns.lock().unwrap().get(&peer).is_some_and(|c| c.conn.stable_id() == conn.stable_id())
    }

    /// Replays offers queued while `peer` was away. It waits for simultaneous dials to settle
    /// and only uses `conn` while it is still the peer's connection, so a connection closed
    /// as a duplicate or replaced never takes the queue with it. Offers that fail go back.
    pub(super) async fn deliver_queued(self: Arc<Self>, peer: Uuid, mut conn: quinn::Connection) {
        loop {
            tokio::time::sleep(QUEUE_SETTLE).await;
            if !self.is_current(peer, &conn) {
                return;
            }
            let mut pending = self.queue.lock().unwrap().drain(peer).into_iter();
            let mut failed = None;
            for offer in pending.by_ref() {
                let delivered = tokio::time::timeout(SEND_TIMEOUT, send_acked(&conn, &offer)).await;
                if !matches!(delivered, Ok(Ok(()))) {
                    failed = Some(offer);
                    break;
                }
            }
            let Some(offer) = failed else { return };
            tracing::warn!("queued offers to {peer} were not delivered; keeping them");
            let mut rest = vec![offer];
            rest.extend(pending);
            self.queue.lock().unwrap().restore(peer, rest);
            // A connection that took over meanwhile may have found the queue empty.
            match self.connection(peer).filter(|c| c.stable_id() != conn.stable_id()) {
                Some(newer) => conn = newer,
                None => return,
            }
        }
    }

    async fn serve(self: Arc<Self>, peer: Uuid, conn: quinn::Connection) {
        loop {
            tokio::select! {
                uni = conn.accept_uni() => match uni {
                    Ok(recv) => {
                        let this = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = this.handle_uni(peer, recv).await {
                                tracing::warn!("message from {peer} failed: {e}");
                            }
                        });
                    }
                    Err(_) => break,
                },
                bi = conn.accept_bi() => match bi {
                    Ok((send, recv)) => {
                        let this = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = this.handle_bi(peer, send, recv).await {
                                tracing::warn!("stream from {peer} failed: {e}");
                            }
                        });
                    }
                    Err(_) => break,
                },
            }
        }
        let removed = {
            let mut conns = self.conns.lock().unwrap();
            match conns.get(&peer) {
                Some(c) if c.conn.stable_id() == conn.stable_id() => {
                    conns.remove(&peer);
                    true
                }
                _ => false,
            }
        };
        if removed {
            self.emit(Event::PeerOffline { peer });
        }
    }

    async fn handle_uni(self: &Arc<Self>, peer: Uuid, mut recv: quinn::RecvStream) -> Result<()> {
        match read_frame(&mut recv).await? {
            Message::Hello { name, .. } => {
                let mut peers = self.peers.lock().unwrap();
                if let Some(existing) = peers.get(peer).cloned() {
                    if existing.name != name {
                        peers.upsert(Peer { name, ..existing })?;
                    }
                }
            }
            Message::Offer { item_id, kind, size, preview, live } => {
                let taken = self.inbox.lock().unwrap().all().iter().any(|e| e.item_id == item_id && e.peer != peer);
                if taken {
                    tracing::warn!("ignored an offer from {peer} for an item another peer already offered");
                    return Ok(());
                }
                let mode = self.peers.lock().unwrap().get(peer).map(|p| p.mode).unwrap_or_default();
                match on_offer(mode, kind, live) {
                    OfferAction::AutoPull => self.request_pull(peer, item_id).await?,
                    OfferAction::Inbox => {
                        let entry = InboxEntry { peer, item_id, kind, size, preview, received: Instant::now() };
                        let info = OfferInfo::from(&entry);
                        self.inbox.lock().unwrap().add(entry);
                        self.emit(Event::OfferReceived(info));
                    }
                }
            }
            Message::Push { item_id, payload } => self.apply_remote(peer, item_id, payload).await,
            Message::Pull { item_id } => self.serve_pull(peer, item_id).await?,
            Message::Ping { nonce } => self.send_to(peer, &Message::Pong { nonce }).await?,
            Message::Pong { .. } => {}
            other => tracing::warn!("unexpected message on a uni stream: {other:?}"),
        }
        Ok(())
    }

    pub(super) async fn handle_bi(
        self: &Arc<Self>,
        peer: Uuid,
        send: quinn::SendStream,
        recv: quinn::RecvStream,
    ) -> Result<()> {
        let mut stream = tokio::io::join(recv, send);
        match read_value_max::<_, StreamKind>(&mut stream, MAX_STREAM_KIND_FRAME).await? {
            StreamKind::Pair => Err(CoreError::Pairing(format!("{peer} is already paired"))),
            StreamKind::File => self.receive_on(peer, stream).await,
        }
    }

    /// Applies a `Push` only when this device asked exactly this peer for the item.
    pub(super) async fn apply_remote(self: &Arc<Self>, peer: Uuid, item_id: Uuid, payload: ClipPayload) {
        let requested = {
            let mut pulls = self.pulls.lock().unwrap();
            let asked = pulls.get(&item_id) == Some(&peer);
            if asked {
                pulls.remove(&item_id);
            }
            asked
        };
        if !requested {
            tracing::warn!("ignored an unrequested push from {peer}");
            return;
        }
        let mut item = ClipItem::new(peer, payload);
        item.id = item_id;
        if let Err(e) = self.history.lock().unwrap().insert(&item) {
            tracing::warn!("timeline insert failed: {e}");
        }
        match write_and_mark(self.clipboard.as_ref(), &self.echo, &item.payload).await {
            Ok(()) => self.emit(Event::ClipboardApplied { item_id, from: peer }),
            Err(_) => self.emit(Event::Error { message: "Couldn't write the clipboard. Try again.".into() }),
        }
    }

    pub(super) async fn serve_pull(self: &Arc<Self>, peer: Uuid, item_id: Uuid) -> Result<()> {
        let item = self.history.lock().unwrap().load(item_id)?;
        let item = item.ok_or_else(|| CoreError::Transfer(format!("item {item_id} is no longer in the timeline")))?;
        if item.source != self.identity.device_id {
            return Err(CoreError::Transfer(format!("item {item_id} is not from this device")));
        }
        if item.payload.size() <= MAX_CLIP_BYTES {
            return self.send_to(peer, &Message::Push { item_id, payload: item.payload }).await;
        }
        // Oversized items travel as a file transfer, named like the offer's file preview.
        let name = match offer_for(&item, false) {
            Message::Offer { preview: Preview::File { name }, .. } => name,
            _ => format!("clipboard-{}", item.created_ms),
        };
        let bytes = match item.payload {
            ClipPayload::Text(t) => t.into_bytes(),
            ClipPayload::Html { html, .. } => html.into_bytes(),
            ClipPayload::Image { png, .. } => png,
        };
        // A fresh path per serve, so concurrent pulls of one item never share a spill file.
        let path = self.tmp_dir.join(format!("{}-{name}", Uuid::new_v4()));
        let spill = path.clone();
        tokio::task::spawn_blocking(move || write_private_file(&spill, &bytes))
            .await
            .map_err(|e| CoreError::Transfer(e.to_string()))??;
        self.spawn_send(peer, path, name, true);
        Ok(())
    }

    pub(super) fn spawn_clipboard_loop(self: Arc<Self>, poll: Duration) {
        let mut rx = spawn_monitor(self.clipboard.clone(), self.echo.clone(), poll);
        tokio::spawn(async move {
            while let Some(payload) = rx.recv().await {
                self.on_local(payload).await;
            }
        });
    }

    async fn on_local(self: &Arc<Self>, payload: ClipPayload) {
        let item = ClipItem::new(self.identity.device_id, payload);
        if let Err(e) = self.history.lock().unwrap().insert(&item) {
            tracing::warn!("timeline insert failed: {e}");
        }
        self.emit(Event::LocalItem { item_id: item.id });
        let offer = Arc::new(offer_for(&item, true));
        let peers: Vec<Uuid> = self.peers.lock().unwrap().all().iter().map(|p| p.device_id).collect();
        // One task per peer, so a slow or half-dead peer cannot hold up the others.
        for peer in peers {
            let this = self.clone();
            let offer = offer.clone();
            tokio::spawn(async move {
                let sent = match this.connection(peer) {
                    Some(conn) => matches!(tokio::time::timeout(SEND_TIMEOUT, send_acked(&conn, &offer)).await, Ok(Ok(()))),
                    None => false,
                };
                if !sent {
                    this.queue.lock().unwrap().push_offer(peer, (*offer).clone());
                }
            });
        }
    }
}
