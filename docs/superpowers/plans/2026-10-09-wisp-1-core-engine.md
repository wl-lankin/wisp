# Wisp Core Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `wisp-core`, a headless Rust library that discovers, pairs with and talks to other Wisp devices, and moves clipboard items and files between them, proven by in-process integration tests with two nodes.

**Architecture:** One Cargo workspace. `crates/wisp-core` is a library of small modules (identity, protocol, peers, pairing, transport, discovery, clipboard, policy, history, transfer) wired together by an `engine` module (split into four files) that exposes a `Node` with a command API and an event stream. Plan 2 wraps `Node` in a Tauri app; this plan has no UI.

**Tech Stack:** Rust 1.98 (edition 2021), tokio, quinn 0.11 + rustls 0.23 (QUIC/TLS), rcgen 0.13, spake2 0.4, mdns-sd 0.13, clipboard-rs 0.2, rusqlite 0.32 (bundled, FTS5), blake3, postcard, serde, uuid, thiserror.

**Spec:** `docs/superpowers/specs/2026-10-09-wisp-design.md`

**Plan series:** 1 of 3. Plan 2 = Tauri shell + UI. Plan 3 = brand assets + CI builds.

**Crate versions:** the versions above are the targets. If a crate's API differs from the code shown (minor version drift), adapt the call to the installed version's API and note it in the commit message; do not change the module's public interface.

## Global Constraints

- Clipboard items up to 50 MB are sent as clipboard items; larger content becomes a file transfer.
- Thumbnails in offers are at most 256 px on the longest side.
- Timeline keeps the last 200 items per device.
- Offline offer queue keeps the latest 20 offers per peer.
- Pairing code: 6 digits; a pairing session ends after 3 wrong attempts.
- Transfer resume window: 10 minutes after a peer drops.
- Clipboard write retry: backoff for up to 2 seconds total.
- mDNS service type: `_wisp._udp.local.`
- Default delivery mode for a newly paired peer: Catch. A peer's mode is stored on the receiving device and means "how items from that peer reach my clipboard". Senders always send `Offer`; a receiver in AutoSync mode pulls live clipboard offers immediately. Nobody can push onto another device's clipboard unasked.
- Never accept a connection from a device whose certificate fingerprint is not pinned for its device ID.
- Writing style: no em-dashes or en-dashes in code, comments or commit messages. No ticket numbers in comments. Commit messages are plain sentences (this repo has no ticket prefix).
- Git identity is already set repo-locally (noreply address); do not change it.

## Review Focus

1. **Echo loop:** an item Wisp writes to the clipboard must never be re-broadcast, including when auto-sync is on in both directions. Pinned in Task 7 and Task 11.
2. **Hostile file names:** a received file named `../../evil.exe` or `C:\Windows\x.dll` must be saved inside the save folder under a sanitized name. Pinned in Task 10.
3. **Name collisions:** receiving `shot.png` twice must keep both (`shot.png`, `shot (1).png`). Pinned in Task 10.
4. **Same device on two interfaces:** a PC on both Wi-Fi and cable appears once, with two addresses, not as two devices. Pinned in Task 6.
5. **Peer offline when an item is created:** the offer is queued and delivered on reconnect, capped at 20, and never auto-applied. Pinned in Task 8 and Task 12.

---

## File Structure

```
Cargo.toml                         workspace root
crates/wisp-core/
  Cargo.toml
  src/
    lib.rs                         module declarations and re-exports
    error.rs                       CoreError, Result alias
    identity.rs                    Identity, Os, Fingerprint
    item.rs                        ClipPayload, ItemKind, Preview, ClipItem, thumbnails
    protocol.rs                    Message enum, frame read/write
    peers.rs                       Peer, DeliveryMode, PeerStore (JSON file)
    pairing.rs                     SPAKE2 pairing over any async stream
    transport.rs                   QUIC endpoint, pinned-cert verifier, multi-address connect
    discovery.rs                   mDNS advertise/browse, TXT encode/parse, address merge
    clipboard.rs                   ClipboardBackend trait, MemoryClipboard, SystemClipboard, EchoGuard
    policy.rs                      Delivery decisions, OfferQueue
    history.rs                     SQLite timeline with FTS5 and blob files
    transfer.rs                    Chunked file send/receive, resume, file name sanitizing
    engine/
      mod.rs                       NodeConfig, Event, Node public API
      session.rs                   pairing, connection registry, message handlers
      files.rs                     outgoing/incoming file transfers with retry
      links.rs                     reconnect dialer and discovery wiring
  tests/
    two_nodes.rs                   integration tests with two in-process nodes
```

---

### Task 1: Workspace scaffold, errors and device identity

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `crates/wisp-core/Cargo.toml`, `crates/wisp-core/src/lib.rs`, `crates/wisp-core/src/error.rs`, `crates/wisp-core/src/identity.rs`
- Test: inline `#[cfg(test)] mod tests` in `identity.rs`

**Interfaces:**
- Produces:
  - `pub enum CoreError` and `pub type Result<T> = std::result::Result<T, CoreError>`
  - `pub enum Os { Windows, MacOs, Linux }` with `Os::current() -> Os`
  - `pub struct Fingerprint(pub [u8; 32])` with `of(der: &[u8]) -> Fingerprint`, `to_hex(&self) -> String`, `from_hex(&str) -> Option<Fingerprint>`
  - `pub struct Identity { device_id: Uuid, name: String, os: Os, cert_der: Vec<u8>, key_der: Vec<u8> }` with `generate(name: &str) -> Result<Identity>`, `load_or_create(path: &Path, default_name: &str) -> Result<Identity>`, `fingerprint(&self) -> Fingerprint`

- [ ] **Step 1: Create the workspace files**

`Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/wisp-core"]
```

`.gitignore`:

```
/target
node_modules/
dist/
```

`crates/wisp-core/Cargo.toml`:

```toml
[package]
name = "wisp-core"
version = "0.1.0"
edition = "2021"

[dependencies]
blake3 = "1"
clipboard-rs = "0.2"
hex = "0.4"
image = { version = "0.25", default-features = false, features = ["png"] }
mdns-sd = "0.13"
postcard = { version = "1", features = ["alloc"] }
quinn = "0.11"
rcgen = "0.13"
rusqlite = { version = "0.32", features = ["bundled"] }
rustls = { version = "0.23", default-features = false, features = ["ring", "std"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
spake2 = "0.4"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }

[dev-dependencies]
tempfile = "3"
```

`crates/wisp-core/src/lib.rs`:

```rust
pub mod error;
pub mod identity;

pub use error::{CoreError, Result};
```

- [ ] **Step 2: Write `error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("encoding: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("certificate: {0}")]
    Cert(#[from] rcgen::Error),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("frame: {0}")]
    Frame(String),
    #[error("pairing: {0}")]
    Pairing(String),
    #[error("transport: {0}")]
    Transport(String),
    #[error("clipboard: {0}")]
    Clipboard(String),
    #[error("transfer: {0}")]
    Transfer(String),
    #[error("unknown peer: {0}")]
    UnknownPeer(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
```

- [ ] **Step 3: Write the failing identity tests**

Append to a new `identity.rs` (tests first, implementation follows in Step 5):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_creates_unique_devices() {
        let a = Identity::generate("Mac").unwrap();
        let b = Identity::generate("PC").unwrap();
        assert_ne!(a.device_id, b.device_id);
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.name, "Mac");
    }

    #[test]
    fn load_or_create_persists_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        let first = Identity::load_or_create(&path, "Laptop").unwrap();
        let second = Identity::load_or_create(&path, "Other name").unwrap();
        assert_eq!(first, second);
        assert_eq!(second.name, "Laptop");
    }

    #[test]
    fn fingerprint_hex_round_trip() {
        let id = Identity::generate("X").unwrap();
        let fp = id.fingerprint();
        assert_eq!(fp.to_hex().len(), 64);
        assert_eq!(Fingerprint::from_hex(&fp.to_hex()), Some(fp));
        assert_eq!(Fingerprint::from_hex("nothex"), None);
    }
}
```

Add `pub mod identity;` is already in `lib.rs`.

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p wisp-core identity`
Expected: compile errors, `Identity` / `Fingerprint` not found.

- [ ] **Step 5: Implement `identity.rs`** (above the test module)

```rust
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Result;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    pub fn of(der: &[u8]) -> Self {
        Self(*blake3::hash(der).as_bytes())
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = hex::decode(s).ok()?;
        Some(Self(bytes.try_into().ok()?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Identity {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl Identity {
    pub fn generate(name: &str) -> Result<Self> {
        let device_id = Uuid::new_v4();
        let certified = rcgen::generate_simple_self_signed(vec![format!("{device_id}.wisp")])?;
        Ok(Self {
            device_id,
            name: name.to_string(),
            os: Os::current(),
            cert_der: certified.cert.der().to_vec(),
            key_der: certified.key_pair.serialize_der(),
        })
    }

    pub fn load_or_create(path: &Path, default_name: &str) -> Result<Self> {
        if path.exists() {
            return Ok(serde_json::from_slice(&std::fs::read(path)?)?);
        }
        let identity = Self::generate(default_name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(&identity)?)?;
        Ok(identity)
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of(&self.cert_der)
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p wisp-core identity`
Expected: 3 passed.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml .gitignore crates/
git commit -m "Add workspace, core errors and device identity"
```

---

### Task 2: Clipboard items and wire protocol

**Files:**
- Create: `crates/wisp-core/src/item.rs`, `crates/wisp-core/src/protocol.rs`
- Modify: `crates/wisp-core/src/lib.rs` (add `pub mod item; pub mod protocol;`)
- Test: inline test modules in both files

**Interfaces:**
- Consumes: `Os`, `Result`, `CoreError::Frame` (Task 1)
- Produces:
  - `pub const MAX_CLIP_BYTES: u64 = 50 * 1024 * 1024;`, `pub const THUMB_MAX: u32 = 256;`
  - `pub enum ClipPayload { Text(String), Html { html: String, plain: String }, Image { png: Vec<u8>, width: u32, height: u32 } }` with `kind(&self) -> ItemKind`, `size(&self) -> u64`, `hash(&self) -> [u8; 32]`, `preview(&self) -> Preview`
  - `pub enum ItemKind { Text, Html, Image, File }`
  - `pub enum Preview { Text(String), Thumbnail { png: Vec<u8>, width: u32, height: u32 }, File { name: String } }`
  - `pub struct ClipItem { id: Uuid, source: Uuid, created_ms: i64, payload: ClipPayload }` with `ClipItem::new(source: Uuid, payload: ClipPayload) -> ClipItem`
  - `pub enum Message { Hello{..}, Offer{..}, Push{..}, Pull{..}, FileStart{..}, FileChunk{..}, FileAck{..}, FileDone{..}, Ping{..}, Pong{..} }` (fields below)
  - `pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, msg: &Message) -> Result<()>`
  - `pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Message>`
  - `pub const MAX_FRAME: usize = 64 * 1024 * 1024;` and `pub const PROTOCOL_VERSION: u16 = 1;`

- [ ] **Step 1: Write the failing item tests** in a new `item.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([45, 226, 197, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn text_preview_is_truncated_to_200_chars() {
        let p = ClipPayload::Text("a".repeat(500)).preview();
        match p {
            Preview::Text(t) => assert_eq!(t.chars().count(), 200),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn image_preview_fits_256() {
        let payload = ClipPayload::Image { png: png(1024, 512), width: 1024, height: 512 };
        match payload.preview() {
            Preview::Thumbnail { width, height, .. } => {
                assert_eq!(width, 256);
                assert_eq!(height, 128);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn small_image_preview_is_not_upscaled() {
        let payload = ClipPayload::Image { png: png(40, 20), width: 40, height: 20 };
        match payload.preview() {
            Preview::Thumbnail { width, height, .. } => assert_eq!((width, height), (40, 20)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn hash_differs_by_content_and_kind() {
        let a = ClipPayload::Text("hi".into());
        let b = ClipPayload::Text("ho".into());
        let c = ClipPayload::Html { html: "hi".into(), plain: "hi".into() };
        assert_ne!(a.hash(), b.hash());
        assert_ne!(a.hash(), c.hash());
        assert_eq!(a.hash(), ClipPayload::Text("hi".into()).hash());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core item`
Expected: compile errors, `ClipPayload` not found.

- [ ] **Step 3: Implement `item.rs`** (above the test module)

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_CLIP_BYTES: u64 = 50 * 1024 * 1024;
pub const THUMB_MAX: u32 = 256;
const TEXT_PREVIEW_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ItemKind {
    Text,
    Html,
    Image,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClipPayload {
    Text(String),
    Html { html: String, plain: String },
    Image { png: Vec<u8>, width: u32, height: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Preview {
    Text(String),
    Thumbnail { png: Vec<u8>, width: u32, height: u32 },
    File { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClipItem {
    pub id: Uuid,
    pub source: Uuid,
    pub created_ms: i64,
    pub payload: ClipPayload,
}

impl ClipItem {
    pub fn new(source: Uuid, payload: ClipPayload) -> Self {
        let created_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self { id: Uuid::new_v4(), source, created_ms, payload }
    }
}

impl ClipPayload {
    pub fn kind(&self) -> ItemKind {
        match self {
            ClipPayload::Text(_) => ItemKind::Text,
            ClipPayload::Html { .. } => ItemKind::Html,
            ClipPayload::Image { .. } => ItemKind::Image,
        }
    }

    pub fn size(&self) -> u64 {
        match self {
            ClipPayload::Text(t) => t.len() as u64,
            ClipPayload::Html { html, plain } => (html.len() + plain.len()) as u64,
            ClipPayload::Image { png, .. } => png.len() as u64,
        }
    }

    pub fn hash(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        match self {
            ClipPayload::Text(t) => {
                h.update(b"text\0");
                h.update(t.as_bytes());
            }
            ClipPayload::Html { html, .. } => {
                h.update(b"html\0");
                h.update(html.as_bytes());
            }
            ClipPayload::Image { png, .. } => {
                h.update(b"image\0");
                h.update(png);
            }
        }
        *h.finalize().as_bytes()
    }

    pub fn preview(&self) -> Preview {
        match self {
            ClipPayload::Text(t) => Preview::Text(t.chars().take(TEXT_PREVIEW_CHARS).collect()),
            ClipPayload::Html { plain, .. } => {
                Preview::Text(plain.chars().take(TEXT_PREVIEW_CHARS).collect())
            }
            ClipPayload::Image { png, width, height } => thumbnail(png, *width, *height),
        }
    }
}

fn thumbnail(png: &[u8], width: u32, height: u32) -> Preview {
    if width <= THUMB_MAX && height <= THUMB_MAX {
        return Preview::Thumbnail { png: png.to_vec(), width, height };
    }
    let Ok(img) = image::load_from_memory(png) else {
        return Preview::Text(format!("Image {width} x {height}"));
    };
    let thumb = img.thumbnail(THUMB_MAX, THUMB_MAX);
    let mut out = std::io::Cursor::new(Vec::new());
    if thumb.write_to(&mut out, image::ImageFormat::Png).is_err() {
        return Preview::Text(format!("Image {width} x {height}"));
    }
    Preview::Thumbnail { png: out.into_inner(), width: thumb.width(), height: thumb.height() }
}
```

- [ ] **Step 4: Run item tests to verify they pass**

Run: `cargo test -p wisp-core item`
Expected: 4 passed.

- [ ] **Step 5: Write the failing protocol tests** in a new `protocol.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ClipPayload, ItemKind, Preview};
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn frames_round_trip() {
        let (mut a, mut b) = tokio::io::duplex(1024 * 1024);
        let msgs = vec![
            Message::Hello { device_id: Uuid::new_v4(), name: "Mac".into(), os: Os::MacOs, version: PROTOCOL_VERSION },
            Message::Offer { item_id: Uuid::new_v4(), kind: ItemKind::Text, size: 5, preview: Preview::Text("hello".into()), live: true },
            Message::Push { item_id: Uuid::new_v4(), payload: ClipPayload::Text("hello".into()) },
            Message::Ping { nonce: 7 },
        ];
        for m in &msgs {
            write_frame(&mut a, m).await.unwrap();
        }
        for m in &msgs {
            assert_eq!(&read_frame(&mut b).await.unwrap(), m);
        }
    }

    #[tokio::test]
    async fn oversized_frame_header_is_rejected_without_allocating() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_u32((MAX_FRAME + 1) as u32).await.unwrap();
        let err = read_frame(&mut b).await.unwrap_err();
        assert!(matches!(err, CoreError::Frame(_)));
    }

    #[tokio::test]
    async fn garbage_frame_is_an_error_not_a_panic() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_u32(3).await.unwrap();
        a.write_all(&[0xff, 0xff, 0xff]).await.unwrap();
        assert!(read_frame(&mut b).await.is_err());
    }
}
```

- [ ] **Step 6: Run to verify failure**

Run: `cargo test -p wisp-core protocol`
Expected: compile errors, `Message` not found.

- [ ] **Step 7: Implement `protocol.rs`** (above the test module)

```rust
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

use crate::identity::Os;
use crate::item::{ClipPayload, ItemKind, Preview};
use crate::{CoreError, Result};

pub const MAX_FRAME: usize = 64 * 1024 * 1024;
pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Message {
    Hello { device_id: Uuid, name: String, os: Os, version: u16 },
    /// `live` is false for offers replayed from the offline queue; receivers never auto-pull those.
    Offer { item_id: Uuid, kind: ItemKind, size: u64, preview: Preview, live: bool },
    /// Only ever sent in reply to `Pull`.
    Push { item_id: Uuid, payload: ClipPayload },
    Pull { item_id: Uuid },
    FileStart { transfer_id: Uuid, name: String, size: u64, hash: [u8; 32] },
    FileChunk { transfer_id: Uuid, offset: u64, bytes: Vec<u8> },
    FileAck { transfer_id: Uuid, offset: u64 },
    FileDone { transfer_id: Uuid },
    Ping { nonce: u64 },
    Pong { nonce: u64 },
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, msg: &Message) -> Result<()> {
    let bytes = postcard::to_allocvec(msg)?;
    if bytes.len() > MAX_FRAME {
        return Err(CoreError::Frame(format!("frame of {} bytes exceeds limit", bytes.len())));
    }
    w.write_u32(bytes.len() as u32).await?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Message> {
    let len = r.read_u32().await? as usize;
    if len > MAX_FRAME {
        return Err(CoreError::Frame(format!("frame of {len} bytes exceeds limit")));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(postcard::from_bytes(&buf)?)
}
```

Update `lib.rs`:

```rust
pub mod error;
pub mod identity;
pub mod item;
pub mod protocol;

pub use error::{CoreError, Result};
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p wisp-core`
Expected: all identity, item and protocol tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/wisp-core
git commit -m "Add clipboard item model and framed wire protocol"
```

---

### Task 3: Peer store

**Files:**
- Create: `crates/wisp-core/src/peers.rs`
- Modify: `crates/wisp-core/src/lib.rs` (add `pub mod peers;`)
- Test: inline test module in `peers.rs`

**Interfaces:**
- Consumes: `Os`, `Fingerprint`, `Result`, `CoreError::UnknownPeer` (Task 1)
- Produces:
  - `pub enum DeliveryMode { Catch, AutoSync }` (default `Catch`)
  - `pub struct Peer { device_id: Uuid, name: String, os: Os, fingerprint: Fingerprint, addresses: Vec<SocketAddr>, mode: DeliveryMode }`
  - `pub struct PeerStore` with `open(path: &Path) -> Result<PeerStore>`, `all(&self) -> &[Peer]`, `get(&self, id: Uuid) -> Option<&Peer>`, `upsert(&mut self, peer: Peer) -> Result<()>`, `set_mode(&mut self, id: Uuid, mode: DeliveryMode) -> Result<()>`, `remember_address(&mut self, id: Uuid, addr: SocketAddr) -> Result<()>`, `remove(&mut self, id: Uuid) -> Result<()>`, `is_trusted(&self, id: Uuid, fp: &Fingerprint) -> bool`
  - `pub const MAX_ADDRESSES: usize = 8;`

- [ ] **Step 1: Write the failing tests** in a new `peers.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn peer(name: &str) -> Peer {
        Peer {
            device_id: Uuid::new_v4(),
            name: name.into(),
            os: Os::Windows,
            fingerprint: Fingerprint([7; 32]),
            addresses: vec![],
            mode: DeliveryMode::Catch,
        }
    }

    #[test]
    fn peers_persist_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peers.json");
        let p = peer("PC");
        {
            let mut store = PeerStore::open(&path).unwrap();
            store.upsert(p.clone()).unwrap();
        }
        let store = PeerStore::open(&path).unwrap();
        assert_eq!(store.get(p.device_id), Some(&p));
    }

    #[test]
    fn upsert_keeps_existing_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PeerStore::open(&dir.path().join("peers.json")).unwrap();
        let p = peer("PC");
        store.upsert(p.clone()).unwrap();
        store.set_mode(p.device_id, DeliveryMode::AutoSync).unwrap();
        store.upsert(Peer { name: "PC renamed".into(), ..p.clone() }).unwrap();
        let stored = store.get(p.device_id).unwrap();
        assert_eq!(stored.mode, DeliveryMode::AutoSync);
        assert_eq!(stored.name, "PC renamed");
    }

    #[test]
    fn remember_address_dedupes_orders_and_caps() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PeerStore::open(&dir.path().join("peers.json")).unwrap();
        let p = peer("PC");
        store.upsert(p.clone()).unwrap();
        for i in 0..10u16 {
            store.remember_address(p.device_id, format!("192.168.1.{i}:4000").parse().unwrap()).unwrap();
        }
        store.remember_address(p.device_id, "192.168.1.5:4000".parse().unwrap()).unwrap();
        let addrs = &store.get(p.device_id).unwrap().addresses;
        assert_eq!(addrs.len(), MAX_ADDRESSES);
        assert_eq!(addrs[0], "192.168.1.5:4000".parse().unwrap());
        assert_eq!(addrs.iter().filter(|a| a.to_string() == "192.168.1.5:4000").count(), 1);
    }

    #[test]
    fn trust_requires_matching_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PeerStore::open(&dir.path().join("peers.json")).unwrap();
        let p = peer("PC");
        store.upsert(p.clone()).unwrap();
        assert!(store.is_trusted(p.device_id, &Fingerprint([7; 32])));
        assert!(!store.is_trusted(p.device_id, &Fingerprint([8; 32])));
        assert!(!store.is_trusted(Uuid::new_v4(), &Fingerprint([7; 32])));
    }

    #[test]
    fn set_mode_on_unknown_peer_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PeerStore::open(&dir.path().join("peers.json")).unwrap();
        let err = store.set_mode(Uuid::new_v4(), DeliveryMode::AutoSync).unwrap_err();
        assert!(matches!(err, CoreError::UnknownPeer(_)));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core peers`
Expected: compile errors, `PeerStore` not found.

- [ ] **Step 3: Implement `peers.rs`** (above the test module)

```rust
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::identity::{Fingerprint, Os};
use crate::{CoreError, Result};

pub const MAX_ADDRESSES: usize = 8;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeliveryMode {
    #[default]
    Catch,
    AutoSync,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Peer {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub fingerprint: Fingerprint,
    pub addresses: Vec<SocketAddr>,
    pub mode: DeliveryMode,
}

pub struct PeerStore {
    path: PathBuf,
    peers: Vec<Peer>,
}

impl PeerStore {
    pub fn open(path: &Path) -> Result<Self> {
        let peers = if path.exists() {
            serde_json::from_slice(&std::fs::read(path)?)?
        } else {
            Vec::new()
        };
        Ok(Self { path: path.to_path_buf(), peers })
    }

    pub fn all(&self) -> &[Peer] {
        &self.peers
    }

    pub fn get(&self, id: Uuid) -> Option<&Peer> {
        self.peers.iter().find(|p| p.device_id == id)
    }

    pub fn upsert(&mut self, peer: Peer) -> Result<()> {
        match self.peers.iter_mut().find(|p| p.device_id == peer.device_id) {
            Some(existing) => {
                let mode = existing.mode;
                *existing = Peer { mode, ..peer };
            }
            None => self.peers.push(peer),
        }
        self.save()
    }

    pub fn set_mode(&mut self, id: Uuid, mode: DeliveryMode) -> Result<()> {
        self.find_mut(id)?.mode = mode;
        self.save()
    }

    pub fn remember_address(&mut self, id: Uuid, addr: SocketAddr) -> Result<()> {
        let peer = self.find_mut(id)?;
        peer.addresses.retain(|a| *a != addr);
        peer.addresses.insert(0, addr);
        peer.addresses.truncate(MAX_ADDRESSES);
        self.save()
    }

    pub fn remove(&mut self, id: Uuid) -> Result<()> {
        self.peers.retain(|p| p.device_id != id);
        self.save()
    }

    pub fn is_trusted(&self, id: Uuid, fp: &Fingerprint) -> bool {
        self.get(id).is_some_and(|p| &p.fingerprint == fp)
    }

    fn find_mut(&mut self, id: Uuid) -> Result<&mut Peer> {
        self.peers
            .iter_mut()
            .find(|p| p.device_id == id)
            .ok_or_else(|| CoreError::UnknownPeer(id.to_string()))
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.peers)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wisp-core peers`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-core
git commit -m "Add persistent peer store with pinned fingerprints"
```

---

### Task 4: Pairing with a 6-digit code

Pairing runs over any bidirectional async stream. In the app it runs over a QUIC stream whose TLS certificate was not yet pinned; `observed_peer` is the fingerprint of the certificate the TLS layer actually saw, which binds the pairing result to the real connection and defeats a man in the middle.

**Files:**
- Create: `crates/wisp-core/src/pairing.rs`
- Modify: `crates/wisp-core/src/protocol.rs` (add generic `write_value` / `read_value`), `crates/wisp-core/src/lib.rs` (add `pub mod pairing;`), `crates/wisp-core/Cargo.toml` (add `rand = "0.8"`)
- Test: inline test module in `pairing.rs`

**Interfaces:**
- Consumes: `Identity`, `Fingerprint`, `Os` (Task 1), `MAX_FRAME`, `write_frame`, `read_frame` (Task 2)
- Produces:
  - In `protocol.rs`: `pub async fn write_value<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, value: &T) -> Result<()>` and `pub async fn read_value<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<T>`; `write_frame` / `read_frame` delegate to them.
  - `pub struct PairInfo { device_id: Uuid, name: String, os: Os, fingerprint: Fingerprint }`
  - `pub async fn pair<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, code: &str, me: &Identity, observed_peer: Option<Fingerprint>) -> Result<PairInfo>`
  - `pub struct PairingSession` with `new() -> PairingSession`, `code(&self) -> &str`, `is_open(&self) -> bool`, `record_failure(&mut self)`
  - `pub const MAX_PAIRING_FAILURES: u8 = 3;`

- [ ] **Step 1: Generalize framing in `protocol.rs`**

Replace the bodies of `write_frame` and `read_frame` and add the generic helpers:

```rust
use serde::de::DeserializeOwned;

pub async fn write_value<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, value: &T) -> Result<()> {
    let bytes = postcard::to_allocvec(value)?;
    if bytes.len() > MAX_FRAME {
        return Err(CoreError::Frame(format!("frame of {} bytes exceeds limit", bytes.len())));
    }
    w.write_u32(bytes.len() as u32).await?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_value<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<T> {
    let len = r.read_u32().await? as usize;
    if len > MAX_FRAME {
        return Err(CoreError::Frame(format!("frame of {len} bytes exceeds limit")));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(postcard::from_bytes(&buf)?)
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, msg: &Message) -> Result<()> {
    write_value(w, msg).await
}

pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Message> {
    read_value(r).await
}
```

Run: `cargo test -p wisp-core protocol`
Expected: the 3 protocol tests still pass.

- [ ] **Step 2: Add the `rand` dependency**

In `crates/wisp-core/Cargo.toml` under `[dependencies]` add `rand = "0.8"`.

- [ ] **Step 3: Write the failing pairing tests** in a new `pairing.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn same_code_exchanges_identities() {
        let mac = Identity::generate("Mac").unwrap();
        let pc = Identity::generate("PC").unwrap();
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let (ra, rb) = tokio::join!(
            pair(&mut a, "123456", &mac, Some(pc.fingerprint())),
            pair(&mut b, "123456", &pc, Some(mac.fingerprint())),
        );
        let seen_by_mac = ra.unwrap();
        let seen_by_pc = rb.unwrap();
        assert_eq!(seen_by_mac.device_id, pc.device_id);
        assert_eq!(seen_by_mac.fingerprint, pc.fingerprint());
        assert_eq!(seen_by_pc.device_id, mac.device_id);
        assert_eq!(seen_by_pc.name, "Mac");
    }

    #[tokio::test]
    async fn wrong_code_fails_on_both_sides() {
        let mac = Identity::generate("Mac").unwrap();
        let pc = Identity::generate("PC").unwrap();
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let (ra, rb) = tokio::join!(
            pair(&mut a, "123456", &mac, None),
            pair(&mut b, "654321", &pc, None),
        );
        assert!(matches!(ra, Err(CoreError::Pairing(_))));
        assert!(matches!(rb, Err(CoreError::Pairing(_))));
    }

    #[tokio::test]
    async fn fingerprint_not_matching_tls_certificate_is_rejected() {
        let mac = Identity::generate("Mac").unwrap();
        let pc = Identity::generate("PC").unwrap();
        let attacker = Identity::generate("Evil").unwrap();
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        let (ra, _rb) = tokio::join!(
            pair(&mut a, "123456", &mac, Some(attacker.fingerprint())),
            pair(&mut b, "123456", &pc, None),
        );
        assert!(matches!(ra, Err(CoreError::Pairing(_))));
    }

    #[test]
    fn session_closes_after_three_failures() {
        let mut s = PairingSession::new();
        assert_eq!(s.code().len(), 6);
        assert!(s.code().chars().all(|c| c.is_ascii_digit()));
        s.record_failure();
        s.record_failure();
        assert!(s.is_open());
        s.record_failure();
        assert!(!s.is_open());
    }
}
```

- [ ] **Step 4: Run to verify failure**

Run: `cargo test -p wisp-core pairing`
Expected: compile errors, `pair` not found.

- [ ] **Step 5: Implement `pairing.rs`** (above the test module)

Both sides send their SPAKE2 message, derive a shared key, then send their device info authenticated with a MAC under that key. A wrong code yields different keys, so the MAC check fails on both sides. Both sides send before they verify, so neither side blocks waiting for the other.

```rust
use rand::Rng;
use serde::{Deserialize, Serialize};
use spake2::{Ed25519Group, Identity as SpakeIdentity, Password, Spake2};
use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;

use crate::identity::{Fingerprint, Identity, Os};
use crate::protocol::{read_value, write_value};
use crate::{CoreError, Result};

pub const MAX_PAIRING_FAILURES: u8 = 3;
const SPAKE_CONTEXT: &[u8] = b"wisp-pair-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairInfo {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub fingerprint: Fingerprint,
}

#[derive(Serialize, Deserialize)]
struct SignedInfo {
    info: PairInfo,
    mac: [u8; 32],
}

pub async fn pair<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    code: &str,
    me: &Identity,
    observed_peer: Option<Fingerprint>,
) -> Result<PairInfo> {
    let (state, outbound) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.as_bytes()),
        &SpakeIdentity::new(SPAKE_CONTEXT),
    );
    write_value(stream, &outbound).await?;
    let inbound: Vec<u8> = read_value(stream).await?;
    let shared = state
        .finish(&inbound)
        .map_err(|e| CoreError::Pairing(format!("key exchange failed: {e:?}")))?;
    let key: [u8; 32] = *blake3::hash(&shared).as_bytes();

    let mine = PairInfo {
        device_id: me.device_id,
        name: me.name.clone(),
        os: me.os,
        fingerprint: me.fingerprint(),
    };
    let mac = mac_of(&key, &mine)?;
    write_value(stream, &SignedInfo { info: mine, mac }).await?;

    let theirs: SignedInfo = read_value(stream).await?;
    if mac_of(&key, &theirs.info)? != theirs.mac {
        return Err(CoreError::Pairing("wrong code".into()));
    }
    if let Some(fp) = observed_peer {
        if fp != theirs.info.fingerprint {
            return Err(CoreError::Pairing("certificate does not match paired identity".into()));
        }
    }
    Ok(theirs.info)
}

fn mac_of(key: &[u8; 32], info: &PairInfo) -> Result<[u8; 32]> {
    let bytes = postcard::to_allocvec(info)?;
    Ok(*blake3::keyed_hash(key, &bytes).as_bytes())
}

pub struct PairingSession {
    code: String,
    failures: u8,
}

impl PairingSession {
    pub fn new() -> Self {
        let n: u32 = rand::rngs::OsRng.gen_range(0..1_000_000);
        Self { code: format!("{n:06}"), failures: 0 }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn is_open(&self) -> bool {
        self.failures < MAX_PAIRING_FAILURES
    }

    pub fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
    }
}

impl Default for PairingSession {
    fn default() -> Self {
        Self::new()
    }
}
```

Add `pub mod pairing;` to `lib.rs`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p wisp-core pairing`
Expected: 4 passed.

- [ ] **Step 7: Commit**

```bash
git add crates/wisp-core
git commit -m "Add SPAKE2 pairing bound to the TLS certificate"
```

---

### Task 5: QUIC transport with pinned certificates

Every device has one QUIC endpoint that is both server and client. TLS 1.3 with mutual authentication: both sides present their self-signed certificate. The client checks the server against an expected fingerprint (or accepts any during pairing); the server accepts any client certificate and the engine (Task 11) decides trust from the fingerprint plus the `Hello` message.

**Files:**
- Create: `crates/wisp-core/src/transport.rs`
- Modify: `crates/wisp-core/src/lib.rs` (add `pub mod transport;`)
- Test: inline test module in `transport.rs`

**Interfaces:**
- Consumes: `Identity`, `Fingerprint` (Task 1), `Message`, `write_frame`, `read_frame` (Task 2)
- Produces:
  - `pub const ALPN: &[u8] = b"wisp/1";`, `pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);`
  - `pub struct Transport` (cheap to clone) with `bind(identity: &Identity, addr: SocketAddr) -> Result<Transport>`, `local_addr(&self) -> Result<SocketAddr>`, `accept(&self) -> Option<Result<quinn::Connection>>`, `connect(&self, addr: SocketAddr, expected: Option<Fingerprint>) -> Result<quinn::Connection>`, `connect_any(&self, addrs: &[SocketAddr], expected: Option<Fingerprint>) -> Result<(quinn::Connection, SocketAddr)>`, `close(&self)`
  - `pub fn peer_fingerprint(conn: &quinn::Connection) -> Option<Fingerprint>`

- [ ] **Step 1: Write the failing tests** in a new `transport.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{read_frame, write_frame, Message};

    fn localhost() -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }

    #[tokio::test]
    async fn pinned_connection_exchanges_frames_and_fingerprints() {
        let a_id = Identity::generate("A").unwrap();
        let b_id = Identity::generate("B").unwrap();
        let a = Transport::bind(&a_id, localhost()).unwrap();
        let b = Transport::bind(&b_id, localhost()).unwrap();
        let b_addr = b.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let conn = b.accept().await.unwrap().unwrap();
            let client_fp = peer_fingerprint(&conn).unwrap();
            let (mut send, mut recv) = conn.accept_bi().await.unwrap();
            let msg = read_frame(&mut recv).await.unwrap();
            assert_eq!(msg, Message::Ping { nonce: 1 });
            write_frame(&mut send, &Message::Pong { nonce: 1 }).await.unwrap();
            send.finish().unwrap();
            conn.closed().await;
            client_fp
        });

        let conn = a.connect(b_addr, Some(b_id.fingerprint())).await.unwrap();
        assert_eq!(peer_fingerprint(&conn), Some(b_id.fingerprint()));
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        write_frame(&mut send, &Message::Ping { nonce: 1 }).await.unwrap();
        assert_eq!(read_frame(&mut recv).await.unwrap(), Message::Pong { nonce: 1 });
        conn.close(0u32.into(), b"done");
        assert_eq!(server.await.unwrap(), a_id.fingerprint());
    }

    #[tokio::test]
    async fn wrong_expected_fingerprint_is_refused() {
        let a = Transport::bind(&Identity::generate("A").unwrap(), localhost()).unwrap();
        let b = Transport::bind(&Identity::generate("B").unwrap(), localhost()).unwrap();
        let b_addr = b.local_addr().unwrap();
        tokio::spawn(async move { while let Some(_) = b.accept().await {} });
        let other = Identity::generate("Other").unwrap().fingerprint();
        assert!(a.connect(b_addr, Some(other)).await.is_err());
    }

    #[tokio::test]
    async fn connect_any_skips_dead_addresses() {
        let a = Transport::bind(&Identity::generate("A").unwrap(), localhost()).unwrap();
        let b_id = Identity::generate("B").unwrap();
        let b = Transport::bind(&b_id, localhost()).unwrap();
        let b_addr = b.local_addr().unwrap();
        tokio::spawn(async move { while let Some(_) = b.accept().await {} });

        let dead: SocketAddr = "192.0.2.1:9".parse().unwrap();
        let started = std::time::Instant::now();
        let (_conn, used) = a.connect_any(&[dead, b_addr], Some(b_id.fingerprint())).await.unwrap();
        assert_eq!(used, b_addr);
        assert!(started.elapsed() < CONNECT_TIMEOUT);
    }

    #[tokio::test]
    async fn connect_any_with_no_reachable_address_errors() {
        let a = Transport::bind(&Identity::generate("A").unwrap(), localhost()).unwrap();
        let dead: SocketAddr = "192.0.2.1:9".parse().unwrap();
        assert!(a.connect_any(&[dead], None).await.is_err());
        assert!(a.connect_any(&[], None).await.is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core transport`
Expected: compile errors, `Transport` not found.

- [ ] **Step 3: Implement `transport.rs`** (above the test module)

```rust
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};

use crate::identity::{Fingerprint, Identity};
use crate::{CoreError, Result};

pub const ALPN: &[u8] = b"wisp/1";
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct Transport {
    endpoint: quinn::Endpoint,
    cert: CertificateDer<'static>,
    key_der: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

fn tls_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Transport(e.to_string())
}

impl Transport {
    pub fn bind(identity: &Identity, addr: SocketAddr) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cert = CertificateDer::from(identity.cert_der.clone());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_der.clone()));

        let mut server_tls = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(tls_err)?
            .with_client_cert_verifier(Arc::new(AcceptAnyClientCert { provider: provider.clone() }))
            .with_single_cert(vec![cert.clone()], key)
            .map_err(tls_err)?;
        server_tls.alpn_protocols = vec![ALPN.to_vec()];
        let quic_server = QuicServerConfig::try_from(server_tls).map_err(tls_err)?;
        let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_server));

        let endpoint = quinn::Endpoint::server(server_config, addr)?;
        Ok(Self { endpoint, cert, key_der: identity.key_der.clone(), provider })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.endpoint.local_addr()?)
    }

    pub async fn accept(&self) -> Option<Result<quinn::Connection>> {
        let incoming = self.endpoint.accept().await?;
        Some(incoming.await.map_err(tls_err))
    }

    pub async fn connect(&self, addr: SocketAddr, expected: Option<Fingerprint>) -> Result<quinn::Connection> {
        let config = self.client_config(expected)?;
        let connecting = self.endpoint.connect_with(config, addr, "wisp").map_err(tls_err)?;
        match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
            Ok(result) => result.map_err(tls_err),
            Err(_) => Err(CoreError::Transport(format!("timeout connecting to {addr}"))),
        }
    }

    /// Races all addresses; the first completed handshake wins, which in practice is the
    /// lowest-latency path (cable beats Wi-Fi).
    pub async fn connect_any(
        &self,
        addrs: &[SocketAddr],
        expected: Option<Fingerprint>,
    ) -> Result<(quinn::Connection, SocketAddr)> {
        let mut attempts = tokio::task::JoinSet::new();
        for &addr in addrs {
            let transport = self.clone();
            attempts.spawn(async move { (addr, transport.connect(addr, expected).await) });
        }
        let mut last_err = CoreError::Transport("no addresses to try".into());
        while let Some(joined) = attempts.join_next().await {
            match joined {
                Ok((addr, Ok(conn))) => {
                    attempts.abort_all();
                    return Ok((conn, addr));
                }
                Ok((_, Err(e))) => last_err = e,
                Err(e) => last_err = tls_err(e),
            }
        }
        Err(last_err)
    }

    pub fn close(&self) {
        self.endpoint.close(0u32.into(), b"bye");
    }

    fn client_config(&self, expected: Option<Fingerprint>) -> Result<quinn::ClientConfig> {
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()));
        let mut client_tls = rustls::ClientConfig::builder_with_provider(self.provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(tls_err)?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedServerCert {
                expected,
                provider: self.provider.clone(),
            }))
            .with_client_auth_cert(vec![self.cert.clone()], key)
            .map_err(tls_err)?;
        client_tls.alpn_protocols = vec![ALPN.to_vec()];
        let quic_client = QuicClientConfig::try_from(client_tls).map_err(tls_err)?;
        Ok(quinn::ClientConfig::new(Arc::new(quic_client)))
    }
}

pub fn peer_fingerprint(conn: &quinn::Connection) -> Option<Fingerprint> {
    let identity = conn.peer_identity()?;
    let certs = identity.downcast::<Vec<CertificateDer<'static>>>().ok()?;
    certs.first().map(|c| Fingerprint::of(c.as_ref()))
}

#[derive(Debug)]
struct PinnedServerCert {
    expected: Option<Fingerprint>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedServerCert {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        match self.expected {
            Some(fp) if fp != Fingerprint::of(end_entity.as_ref()) => {
                Err(rustls::Error::General("certificate fingerprint mismatch".into()))
            }
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct AcceptAnyClientCert {
    provider: Arc<CryptoProvider>,
}

impl ClientCertVerifier for AcceptAnyClientCert {
    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}
```

Add `pub mod transport;` to `lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wisp-core transport`
Expected: 4 passed. If Windows Defender Firewall prompts for `wisp_core-*.exe`, allow private networks; localhost tests do not need it.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-core
git commit -m "Add QUIC transport with mutual TLS and pinned fingerprints"
```

---

### Task 6: mDNS discovery

**Files:**
- Create: `crates/wisp-core/src/discovery.rs`
- Modify: `crates/wisp-core/src/lib.rs` (add `pub mod discovery;`)
- Test: inline test module in `discovery.rs`

**Interfaces:**
- Consumes: `Os`, `Fingerprint` (Task 1)
- Produces:
  - `pub const SERVICE_TYPE: &str = "_wisp._udp.local.";`
  - `pub struct Advert { device_id: Uuid, name: String, os: Os, port: u16, fingerprint: Fingerprint }`
  - `pub struct Discovered { device_id: Uuid, name: String, os: Os, fingerprint: Fingerprint, addresses: Vec<SocketAddr> }`
  - `pub fn txt_properties(a: &Advert) -> HashMap<String, String>`
  - `pub fn parse_txt(props: &HashMap<String, String>, ips: &[IpAddr], port: u16) -> Option<Discovered>` (filters loopback and IPv6 link-local, IPv4 sorted first)
  - `pub struct DiscoveredSet` with `new()`, `merge(&mut self, d: Discovered) -> bool` (true if new device or new address), `remove(&mut self, id: Uuid)`, `get(&self, id: Uuid) -> Option<&Discovered>`, `all(&self) -> Vec<&Discovered>`
  - `pub enum DiscoveryEvent { Found(Discovered), Lost(Uuid) }`
  - `pub struct Discovery` with `start(advert: &Advert) -> Result<(Discovery, tokio::sync::mpsc::UnboundedReceiver<DiscoveryEvent>)>` and `stop(self)`

- [ ] **Step 1: Write the failing tests** in a new `discovery.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn advert() -> Advert {
        Advert {
            device_id: Uuid::new_v4(),
            name: "Wolfgang's PC".into(),
            os: Os::Windows,
            port: 47800,
            fingerprint: Fingerprint([3; 32]),
        }
    }

    #[test]
    fn txt_round_trip() {
        let a = advert();
        let ips: Vec<IpAddr> = vec!["192.168.1.20".parse().unwrap()];
        let d = parse_txt(&txt_properties(&a), &ips, a.port).unwrap();
        assert_eq!(d.device_id, a.device_id);
        assert_eq!(d.name, a.name);
        assert_eq!(d.os, Os::Windows);
        assert_eq!(d.fingerprint, a.fingerprint);
        assert_eq!(d.addresses, vec!["192.168.1.20:47800".parse::<SocketAddr>().unwrap()]);
    }

    #[test]
    fn parse_rejects_missing_or_bad_fields() {
        let a = advert();
        let mut props = txt_properties(&a);
        props.insert("fp".into(), "zz".into());
        assert!(parse_txt(&props, &[], a.port).is_none());
        let mut props = txt_properties(&a);
        props.remove("id");
        assert!(parse_txt(&props, &[], a.port).is_none());
    }

    #[test]
    fn parse_filters_loopback_and_link_local_and_puts_ipv4_first() {
        let a = advert();
        let ips: Vec<IpAddr> = vec![
            "fe80::1".parse().unwrap(),
            "127.0.0.1".parse().unwrap(),
            "fd00::20".parse().unwrap(),
            "10.0.0.5".parse().unwrap(),
        ];
        let d = parse_txt(&txt_properties(&a), &ips, a.port).unwrap();
        let ips: Vec<String> = d.addresses.iter().map(|s| s.ip().to_string()).collect();
        assert_eq!(ips, vec!["10.0.0.5", "fd00::20"]);
    }

    #[test]
    fn same_device_on_two_interfaces_is_one_entry() {
        let a = advert();
        let wifi = parse_txt(&txt_properties(&a), &["192.168.1.20".parse().unwrap()], a.port).unwrap();
        let cable = parse_txt(&txt_properties(&a), &["192.168.1.30".parse().unwrap()], a.port).unwrap();
        let mut set = DiscoveredSet::new();
        assert!(set.merge(wifi.clone()));
        assert!(set.merge(cable));
        assert!(!set.merge(wifi));
        assert_eq!(set.all().len(), 1);
        assert_eq!(set.get(a.device_id).unwrap().addresses.len(), 2);
        set.remove(a.device_id);
        assert!(set.all().is_empty());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core discovery`
Expected: compile errors, `Advert` not found.

- [ ] **Step 3: Implement `discovery.rs`** (above the test module)

The mDNS instance name is the device ID, so a `ServiceRemoved` event can be mapped back to a device. The browse loop ignores this device's own advert.

```rust
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::identity::{Fingerprint, Os};
use crate::{CoreError, Result};

pub const SERVICE_TYPE: &str = "_wisp._udp.local.";

#[derive(Debug, Clone, PartialEq)]
pub struct Advert {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub port: u16,
    pub fingerprint: Fingerprint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Discovered {
    pub device_id: Uuid,
    pub name: String,
    pub os: Os,
    pub fingerprint: Fingerprint,
    pub addresses: Vec<SocketAddr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiscoveryEvent {
    Found(Discovered),
    Lost(Uuid),
}

fn os_code(os: Os) -> &'static str {
    match os {
        Os::Windows => "win",
        Os::MacOs => "mac",
        Os::Linux => "linux",
    }
}

fn os_from_code(code: &str) -> Option<Os> {
    match code {
        "win" => Some(Os::Windows),
        "mac" => Some(Os::MacOs),
        "linux" => Some(Os::Linux),
        _ => None,
    }
}

pub fn txt_properties(a: &Advert) -> HashMap<String, String> {
    HashMap::from([
        ("v".to_string(), "1".to_string()),
        ("id".to_string(), a.device_id.to_string()),
        ("name".to_string(), a.name.clone()),
        ("os".to_string(), os_code(a.os).to_string()),
        ("fp".to_string(), a.fingerprint.to_hex()),
    ])
}

fn usable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_unspecified(),
        IpAddr::V6(v6) => {
            !v6.is_loopback() && !v6.is_unspecified() && (v6.segments()[0] & 0xffc0) != 0xfe80
        }
    }
}

pub fn parse_txt(props: &HashMap<String, String>, ips: &[IpAddr], port: u16) -> Option<Discovered> {
    let device_id = Uuid::parse_str(props.get("id")?).ok()?;
    let name = props.get("name")?.clone();
    let os = os_from_code(props.get("os")?)?;
    let fingerprint = Fingerprint::from_hex(props.get("fp")?)?;
    let mut ips: Vec<IpAddr> = ips.iter().copied().filter(usable).collect();
    ips.sort_by_key(|ip| (ip.is_ipv6(), *ip));
    let addresses = ips.into_iter().map(|ip| SocketAddr::new(ip, port)).collect();
    Some(Discovered { device_id, name, os, fingerprint, addresses })
}

#[derive(Default)]
pub struct DiscoveredSet {
    map: HashMap<Uuid, Discovered>,
}

impl DiscoveredSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn merge(&mut self, d: Discovered) -> bool {
        match self.map.get_mut(&d.device_id) {
            None => {
                self.map.insert(d.device_id, d);
                true
            }
            Some(existing) => {
                existing.name = d.name;
                let mut changed = false;
                for addr in d.addresses {
                    if !existing.addresses.contains(&addr) {
                        existing.addresses.push(addr);
                        changed = true;
                    }
                }
                changed
            }
        }
    }

    pub fn remove(&mut self, id: Uuid) {
        self.map.remove(&id);
    }

    pub fn get(&self, id: Uuid) -> Option<&Discovered> {
        self.map.get(&id)
    }

    pub fn all(&self) -> Vec<&Discovered> {
        self.map.values().collect()
    }
}

pub struct Discovery {
    daemon: ServiceDaemon,
}

impl Discovery {
    pub fn start(advert: &Advert) -> Result<(Self, mpsc::UnboundedReceiver<DiscoveryEvent>)> {
        let mdns_err = |e: mdns_sd::Error| CoreError::Transport(format!("mdns: {e}"));
        let daemon = ServiceDaemon::new().map_err(mdns_err)?;
        let instance = advert.device_id.to_string();
        let host = format!("{instance}.local.");
        let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", advert.port, txt_properties(advert))
            .map_err(mdns_err)?
            .enable_addr_auto();
        daemon.register(info).map_err(mdns_err)?;

        let browse = daemon.browse(SERVICE_TYPE).map_err(mdns_err)?;
        let (tx, rx) = mpsc::unbounded_channel();
        let own_id = advert.device_id;
        tokio::spawn(async move {
            while let Ok(event) = browse.recv_async().await {
                let out = match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let props: HashMap<String, String> = info
                            .get_properties()
                            .iter()
                            .map(|p| (p.key().to_string(), p.val_str().to_string()))
                            .collect();
                        let ips: Vec<IpAddr> = info.get_addresses().iter().copied().collect();
                        parse_txt(&props, &ips, info.get_port())
                            .filter(|d| d.device_id != own_id)
                            .map(DiscoveryEvent::Found)
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => fullname
                        .split('.')
                        .next()
                        .and_then(|id| Uuid::parse_str(id).ok())
                        .filter(|id| *id != own_id)
                        .map(DiscoveryEvent::Lost),
                    _ => None,
                };
                if let Some(e) = out {
                    if tx.send(e).is_err() {
                        break;
                    }
                }
            }
        });
        Ok((Self { daemon }, rx))
    }

    pub fn stop(self) {
        let _ = self.daemon.shutdown();
    }
}
```

Add `pub mod discovery;` to `lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wisp-core discovery`
Expected: 4 passed.

- [ ] **Step 5: Add an ignored live mDNS test** (run manually; multicast is unreliable in CI)

Append inside the test module:

```rust
    #[tokio::test]
    #[ignore = "needs a real network with multicast"]
    async fn two_daemons_find_each_other() {
        let a = advert();
        let b = Advert { device_id: Uuid::new_v4(), name: "Mac".into(), os: Os::MacOs, port: 47801, ..advert() };
        let (da, mut rx_a) = Discovery::start(&a).unwrap();
        let (db, _rx_b) = Discovery::start(&b).unwrap();
        let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(DiscoveryEvent::Found(d)) = rx_a.recv().await {
                    if d.device_id == b.device_id {
                        return d;
                    }
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(found.name, "Mac");
        da.stop();
        db.stop();
    }
```

Run: `cargo test -p wisp-core discovery -- --ignored`
Expected: PASS on a normal home network (allow the firewall prompt if shown).

- [ ] **Step 6: Commit**

```bash
git add crates/wisp-core
git commit -m "Add mDNS discovery with per-device address merging"
```

---

### Task 7: Clipboard backends, monitor and echo guard

**Files:**
- Create: `crates/wisp-core/src/clipboard.rs`
- Modify: `crates/wisp-core/src/lib.rs` (add `pub mod clipboard;`)
- Test: inline test module in `clipboard.rs`

**Interfaces:**
- Consumes: `ClipPayload` (Task 2), `CoreError::Clipboard` (Task 1)
- Produces:
  - `pub trait ClipboardBackend: Send + Sync { fn read(&self) -> Result<Option<ClipPayload>>; fn write(&self, payload: &ClipPayload) -> Result<()>; fn change_count(&self) -> u64; }`
  - `pub struct MemoryClipboard` with `new()`, `set_external(&self, p: ClipPayload)` (simulates the user copying), `current(&self) -> Option<ClipPayload>`
  - `pub struct SystemClipboard` with `start() -> Result<SystemClipboard>` (spawns the OS change watcher)
  - `pub struct EchoGuard` with `new()`, `mark_written(&mut self, hash: [u8; 32])`, `is_echo(&mut self, hash: &[u8; 32]) -> bool` (one-shot, entries expire after `ECHO_TTL` = 5 s)
  - `pub async fn write_with_retry(backend: &dyn ClipboardBackend, payload: &ClipPayload) -> Result<()>` (backoff 50, 100, 200, 400, 800 ms; under 2 s total)
  - `pub fn spawn_monitor(backend: Arc<dyn ClipboardBackend>, guard: Arc<Mutex<EchoGuard>>, interval: Duration) -> mpsc::UnboundedReceiver<ClipPayload>` (`std::sync::Mutex`)

- [ ] **Step 1: Write the failing tests** in a new `clipboard.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn echo_guard_is_one_shot() {
        let mut g = EchoGuard::new();
        let h = [1u8; 32];
        assert!(!g.is_echo(&h));
        g.mark_written(h);
        assert!(g.is_echo(&h));
        assert!(!g.is_echo(&h));
    }

    #[tokio::test]
    async fn monitor_reports_user_copies_but_not_wisp_writes() {
        let clip = Arc::new(MemoryClipboard::new());
        let guard = Arc::new(Mutex::new(EchoGuard::new()));
        let mut rx = spawn_monitor(clip.clone(), guard.clone(), Duration::from_millis(10));

        let from_peer = ClipPayload::Text("from peer".into());
        guard.lock().unwrap().mark_written(from_peer.hash());
        clip.write(&from_peer).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        clip.set_external(ClipPayload::Text("user copy".into()));
        let got = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
        assert_eq!(got, ClipPayload::Text("user copy".into()));
        assert!(rx.try_recv().is_err());
    }

    struct Flaky {
        fails_left: AtomicU32,
        inner: MemoryClipboard,
    }

    impl ClipboardBackend for Flaky {
        fn read(&self) -> Result<Option<ClipPayload>> {
            self.inner.read()
        }
        fn write(&self, payload: &ClipPayload) -> Result<()> {
            if self.fails_left.load(Ordering::SeqCst) > 0 {
                self.fails_left.fetch_sub(1, Ordering::SeqCst);
                return Err(CoreError::Clipboard("locked".into()));
            }
            self.inner.write(payload)
        }
        fn change_count(&self) -> u64 {
            self.inner.change_count()
        }
    }

    #[tokio::test]
    async fn write_retries_while_clipboard_is_locked() {
        let flaky = Flaky { fails_left: AtomicU32::new(3), inner: MemoryClipboard::new() };
        write_with_retry(&flaky, &ClipPayload::Text("x".into())).await.unwrap();
        assert_eq!(flaky.inner.current(), Some(ClipPayload::Text("x".into())));
    }

    #[tokio::test]
    async fn write_gives_up_within_two_seconds() {
        let flaky = Flaky { fails_left: AtomicU32::new(u32::MAX), inner: MemoryClipboard::new() };
        let started = std::time::Instant::now();
        assert!(write_with_retry(&flaky, &ClipPayload::Text("x".into())).await.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[ignore = "touches the real OS clipboard"]
    fn system_clipboard_round_trips_text() {
        let clip = SystemClipboard::start().unwrap();
        clip.write(&ClipPayload::Text("wisp test".into())).unwrap();
        assert_eq!(clip.read().unwrap(), Some(ClipPayload::Text("wisp test".into())));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core clipboard`
Expected: compile errors, `EchoGuard` not found.

- [ ] **Step 3: Implement `clipboard.rs`** (above the test module)

`SystemClipboard` creates a short-lived `ClipboardContext` per call (the context type is not guaranteed to be `Send` on every OS) and counts OS change notifications from a dedicated watcher thread. Read priority: image, then HTML (with plain text), then text.

```rust
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clipboard_rs::common::RustImage;
use clipboard_rs::{
    Clipboard, ClipboardContent, ClipboardContext, ClipboardHandler, ClipboardWatcher,
    ClipboardWatcherContext, ContentFormat, RustImageData,
};
use tokio::sync::mpsc;

use crate::item::ClipPayload;
use crate::{CoreError, Result};

pub const ECHO_TTL: Duration = Duration::from_secs(5);
const RETRY_DELAYS_MS: [u64; 5] = [50, 100, 200, 400, 800];

pub trait ClipboardBackend: Send + Sync {
    fn read(&self) -> Result<Option<ClipPayload>>;
    fn write(&self, payload: &ClipPayload) -> Result<()>;
    fn change_count(&self) -> u64;
}

#[derive(Default)]
pub struct MemoryClipboard {
    content: Mutex<Option<ClipPayload>>,
    changes: AtomicU64,
}

impl MemoryClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_external(&self, p: ClipPayload) {
        *self.content.lock().unwrap() = Some(p);
        self.changes.fetch_add(1, Ordering::SeqCst);
    }

    pub fn current(&self) -> Option<ClipPayload> {
        self.content.lock().unwrap().clone()
    }
}

impl ClipboardBackend for MemoryClipboard {
    fn read(&self) -> Result<Option<ClipPayload>> {
        Ok(self.current())
    }

    fn write(&self, payload: &ClipPayload) -> Result<()> {
        self.set_external(payload.clone());
        Ok(())
    }

    fn change_count(&self) -> u64 {
        self.changes.load(Ordering::SeqCst)
    }
}

pub struct SystemClipboard {
    changes: Arc<AtomicU64>,
}

struct CountChanges(Arc<AtomicU64>);

impl ClipboardHandler for CountChanges {
    fn on_clipboard_change(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn clip_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Clipboard(e.to_string())
}

impl SystemClipboard {
    pub fn start() -> Result<Self> {
        let changes = Arc::new(AtomicU64::new(0));
        let counter = changes.clone();
        std::thread::Builder::new()
            .name("wisp-clipboard-watch".into())
            .spawn(move || {
                let Ok(mut watcher) = ClipboardWatcherContext::new() else {
                    tracing::error!("clipboard watcher could not start");
                    return;
                };
                watcher.add_handler(CountChanges(counter));
                watcher.start_watch();
            })?;
        Ok(Self { changes })
    }

    fn ctx() -> Result<ClipboardContext> {
        ClipboardContext::new().map_err(clip_err)
    }
}

impl ClipboardBackend for SystemClipboard {
    fn read(&self) -> Result<Option<ClipPayload>> {
        let ctx = Self::ctx()?;
        if ctx.has(ContentFormat::Image) {
            let img = ctx.get_image().map_err(clip_err)?;
            let (width, height) = img.get_size();
            let png = img.to_png().map_err(clip_err)?.get_bytes().to_vec();
            return Ok(Some(ClipPayload::Image { png, width, height }));
        }
        if ctx.has(ContentFormat::Html) {
            let html = ctx.get_html().map_err(clip_err)?;
            let plain = ctx.get_text().unwrap_or_default();
            return Ok(Some(ClipPayload::Html { html, plain }));
        }
        if ctx.has(ContentFormat::Text) {
            return Ok(Some(ClipPayload::Text(ctx.get_text().map_err(clip_err)?)));
        }
        Ok(None)
    }

    fn write(&self, payload: &ClipPayload) -> Result<()> {
        let ctx = Self::ctx()?;
        match payload {
            ClipPayload::Text(t) => ctx.set_text(t.clone()).map_err(clip_err),
            ClipPayload::Html { html, plain } => ctx
                .set(vec![ClipboardContent::Html(html.clone()), ClipboardContent::Text(plain.clone())])
                .map_err(clip_err),
            ClipPayload::Image { png, .. } => {
                let img = RustImageData::from_bytes(png).map_err(clip_err)?;
                ctx.set_image(img).map_err(clip_err)
            }
        }
    }

    fn change_count(&self) -> u64 {
        self.changes.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
pub struct EchoGuard {
    recent: VecDeque<([u8; 32], Instant)>,
}

impl EchoGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_written(&mut self, hash: [u8; 32]) {
        self.recent.push_back((hash, Instant::now()));
        while self.recent.len() > 16 {
            self.recent.pop_front();
        }
    }

    pub fn is_echo(&mut self, hash: &[u8; 32]) -> bool {
        self.recent.retain(|(_, at)| at.elapsed() < ECHO_TTL);
        match self.recent.iter().position(|(h, _)| h == hash) {
            Some(i) => {
                self.recent.remove(i);
                true
            }
            None => false,
        }
    }
}

pub async fn write_with_retry(backend: &dyn ClipboardBackend, payload: &ClipPayload) -> Result<()> {
    let mut last = backend.write(payload);
    for delay in RETRY_DELAYS_MS {
        if last.is_ok() {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(delay)).await;
        last = backend.write(payload);
    }
    last
}

pub fn spawn_monitor(
    backend: Arc<dyn ClipboardBackend>,
    guard: Arc<Mutex<EchoGuard>>,
    interval: Duration,
) -> mpsc::UnboundedReceiver<ClipPayload> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut seen = backend.change_count();
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            let now = backend.change_count();
            if now == seen {
                continue;
            }
            seen = now;
            let payload = match backend.read() {
                Ok(Some(p)) => p,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!("clipboard read failed: {e}");
                    continue;
                }
            };
            if guard.lock().unwrap().is_echo(&payload.hash()) {
                continue;
            }
            if tx.send(payload).is_err() {
                break;
            }
        }
    });
    rx
}
```

Add `pub mod clipboard;` to `lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wisp-core clipboard`
Expected: 4 passed, 1 ignored.

- [ ] **Step 5: Run the ignored OS clipboard test once locally**

Run: `cargo test -p wisp-core clipboard -- --ignored`
Expected: PASS. (It overwrites your clipboard with "wisp test".)

- [ ] **Step 6: Commit**

```bash
git add crates/wisp-core
git commit -m "Add clipboard backends, change monitor and echo guard"
```

---

### Task 8: Delivery policy, offline queue and inbox

**Files:**
- Create: `crates/wisp-core/src/policy.rs`
- Modify: `crates/wisp-core/src/lib.rs` (add `pub mod policy;`)
- Test: inline test module in `policy.rs`

**Interfaces:**
- Consumes: `ClipItem`, `ClipPayload`, `ItemKind`, `Preview`, `MAX_CLIP_BYTES` (Task 2), `Message` (Task 2), `DeliveryMode` (Task 3)
- Produces:
  - `pub fn offer_for(item: &ClipItem, live: bool) -> Message` (oversized items become `ItemKind::File` with `Preview::File`)
  - `pub enum OfferAction { AutoPull, Inbox }`
  - `pub fn on_offer(mode: DeliveryMode, kind: ItemKind, live: bool) -> OfferAction` (receiver side: AutoPull only for AutoSync peers, live offers, and non-file kinds)
  - `pub const OFFLINE_QUEUE_CAP: usize = 20;`
  - `pub struct OfferQueue` with `new()`, `push(&mut self, peer: Uuid, item: &ClipItem)` (stores `offer_for(item, false)`), `drain(&mut self, peer: Uuid) -> Vec<Message>`
  - `pub struct InboxEntry { peer: Uuid, item_id: Uuid, kind: ItemKind, size: u64, preview: Preview, received: Instant }`
  - `pub struct Inbox` with `new()`, `add(&mut self, entry: InboxEntry)`, `latest(&self) -> Option<&InboxEntry>`, `take(&mut self, item_id: Uuid) -> Option<InboxEntry>`, `all(&self) -> Vec<&InboxEntry>` (newest first), `dismiss(&mut self, item_id: Uuid)`

- [ ] **Step 1: Write the failing tests** in a new `policy.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ClipPayload;

    fn text_item(s: &str) -> ClipItem {
        ClipItem::new(Uuid::new_v4(), ClipPayload::Text(s.into()))
    }

    #[test]
    fn autosync_pulls_live_clipboard_items_only() {
        use DeliveryMode::*;
        assert_eq!(on_offer(AutoSync, ItemKind::Image, true), OfferAction::AutoPull);
        assert_eq!(on_offer(AutoSync, ItemKind::Text, false), OfferAction::Inbox);
        assert_eq!(on_offer(AutoSync, ItemKind::File, true), OfferAction::Inbox);
        assert_eq!(on_offer(Catch, ItemKind::Text, true), OfferAction::Inbox);
    }

    #[test]
    fn offer_carries_preview_and_live_flag() {
        let item = text_item("hello");
        match offer_for(&item, true) {
            Message::Offer { item_id, kind, live, preview, .. } => {
                assert_eq!(item_id, item.id);
                assert_eq!(kind, ItemKind::Text);
                assert!(live);
                assert_eq!(preview, Preview::Text("hello".into()));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn oversized_item_is_always_a_file_offer() {
        let big = ClipPayload::Text("x".repeat(MAX_CLIP_BYTES as usize + 1));
        let item = ClipItem::new(Uuid::new_v4(), big);
        match offer_for(&item, true) {
            Message::Offer { kind, preview, .. } => {
                assert_eq!(kind, ItemKind::File);
                assert!(matches!(preview, Preview::File { .. }));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn offline_queue_keeps_latest_twenty_offers_per_peer() {
        let mut q = OfferQueue::new();
        let peer = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut last_id = Uuid::nil();
        for i in 0..25 {
            let item = text_item(&format!("item {i}"));
            last_id = item.id;
            q.push(peer, &item);
        }
        q.push(other, &text_item("other"));
        let drained = q.drain(peer);
        assert_eq!(drained.len(), OFFLINE_QUEUE_CAP);
        assert!(drained.iter().all(|m| matches!(m, Message::Offer { live: false, .. })));
        assert!(matches!(drained.last(), Some(Message::Offer { item_id, .. }) if *item_id == last_id));
        assert!(q.drain(peer).is_empty());
        assert_eq!(q.drain(other).len(), 1);
    }

    #[test]
    fn inbox_latest_take_and_dismiss() {
        let mut inbox = Inbox::new();
        let peer = Uuid::new_v4();
        let entry = |id| InboxEntry {
            peer,
            item_id: id,
            kind: ItemKind::Text,
            size: 1,
            preview: Preview::Text("x".into()),
            received: Instant::now(),
        };
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        inbox.add(entry(a));
        inbox.add(entry(b));
        inbox.add(entry(c));
        assert_eq!(inbox.latest().unwrap().item_id, c);
        assert_eq!(inbox.take(c).unwrap().item_id, c);
        inbox.dismiss(b);
        assert_eq!(inbox.latest().unwrap().item_id, a);
        assert_eq!(inbox.all().len(), 1);
        assert!(inbox.take(c).is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core policy`
Expected: compile errors, `on_offer` not found.

- [ ] **Step 3: Implement `policy.rs`** (above the test module)

```rust
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use uuid::Uuid;

use crate::item::{ClipItem, ItemKind, Preview, MAX_CLIP_BYTES};
use crate::peers::DeliveryMode;
use crate::protocol::Message;

pub const OFFLINE_QUEUE_CAP: usize = 20;

pub fn offer_for(item: &ClipItem, live: bool) -> Message {
    let size = item.payload.size();
    let (kind, preview) = if size > MAX_CLIP_BYTES {
        let ext = match item.payload.kind() {
            ItemKind::Image => "png",
            ItemKind::Html => "html",
            _ => "txt",
        };
        (ItemKind::File, Preview::File { name: format!("clipboard-{}.{ext}", item.created_ms) })
    } else {
        (item.payload.kind(), item.payload.preview())
    };
    Message::Offer { item_id: item.id, kind, size, preview, live }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferAction {
    AutoPull,
    Inbox,
}

/// Decided by the receiver: `mode` is how items from the sending peer reach this device.
pub fn on_offer(mode: DeliveryMode, kind: ItemKind, live: bool) -> OfferAction {
    if mode == DeliveryMode::AutoSync && live && kind != ItemKind::File {
        OfferAction::AutoPull
    } else {
        OfferAction::Inbox
    }
}

#[derive(Default)]
pub struct OfferQueue {
    queues: HashMap<Uuid, VecDeque<Message>>,
}

impl OfferQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replayed offers are not live, so an auto-sync receiver shows them instead of
    /// overwriting its clipboard with something stale.
    pub fn push(&mut self, peer: Uuid, item: &ClipItem) {
        let q = self.queues.entry(peer).or_default();
        q.push_back(offer_for(item, false));
        while q.len() > OFFLINE_QUEUE_CAP {
            q.pop_front();
        }
    }

    pub fn drain(&mut self, peer: Uuid) -> Vec<Message> {
        self.queues.remove(&peer).map(Vec::from).unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InboxEntry {
    pub peer: Uuid,
    pub item_id: Uuid,
    pub kind: ItemKind,
    pub size: u64,
    pub preview: Preview,
    pub received: Instant,
}

#[derive(Default)]
pub struct Inbox {
    entries: Vec<InboxEntry>,
}

impl Inbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, entry: InboxEntry) {
        self.entries.retain(|e| e.item_id != entry.item_id);
        self.entries.push(entry);
    }

    pub fn latest(&self) -> Option<&InboxEntry> {
        self.entries.last()
    }

    pub fn take(&mut self, item_id: Uuid) -> Option<InboxEntry> {
        let i = self.entries.iter().position(|e| e.item_id == item_id)?;
        Some(self.entries.remove(i))
    }

    pub fn dismiss(&mut self, item_id: Uuid) {
        self.entries.retain(|e| e.item_id != item_id);
    }

    pub fn all(&self) -> Vec<&InboxEntry> {
        self.entries.iter().rev().collect()
    }
}
```

Add `pub mod policy;` to `lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wisp-core policy`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-core
git commit -m "Add delivery policy, offline offer queue and inbox"
```

---

### Task 9: Timeline history (SQLite + FTS5)

**Files:**
- Create: `crates/wisp-core/src/history.rs`
- Modify: `crates/wisp-core/src/lib.rs` (add `pub mod history;`)
- Test: inline test module in `history.rs`

**Interfaces:**
- Consumes: `ClipItem`, `ClipPayload`, `ItemKind`, `Preview` (Task 2), `Result` (Task 1)
- Produces:
  - `pub const HISTORY_CAP: usize = 200;`
  - `pub struct HistoryEntry { id: Uuid, source: Uuid, kind: ItemKind, created_ms: i64, size: u64, preview: Preview }`
  - `pub struct HistoryQuery { text: Option<String>, source: Option<Uuid>, kind: Option<ItemKind>, limit: usize }` (implements `Default` with `limit: 50`)
  - `pub struct History` with `open(dir: &Path) -> Result<History>`, `insert(&mut self, item: &ClipItem) -> Result<()>` (prunes to `HISTORY_CAP`, ignores duplicate IDs), `load(&self, id: Uuid) -> Result<Option<ClipItem>>`, `query(&self, q: &HistoryQuery) -> Result<Vec<HistoryEntry>>` (newest first)
  - `pub fn fts_query(input: &str) -> Option<String>` (turns free user text into a safe FTS5 prefix query)

- [ ] **Step 1: Write the failing tests** in a new `history.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn item(source: Uuid, text: &str) -> ClipItem {
        ClipItem::new(source, ClipPayload::Text(text.into()))
    }

    #[test]
    fn insert_load_round_trip_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let src = Uuid::new_v4();
        let img = ClipItem::new(src, ClipPayload::Image { png: vec![1, 2, 3], width: 1, height: 1 });
        {
            let mut h = History::open(dir.path()).unwrap();
            h.insert(&img).unwrap();
            h.insert(&img).unwrap();
        }
        let h = History::open(dir.path()).unwrap();
        assert_eq!(h.load(img.id).unwrap(), Some(img.clone()));
        assert_eq!(h.query(&HistoryQuery::default()).unwrap().len(), 1);
    }

    #[test]
    fn text_search_matches_word_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        let src = Uuid::new_v4();
        h.insert(&item(src, "hello world")).unwrap();
        h.insert(&item(src, "goodbye")).unwrap();
        let q = HistoryQuery { text: Some("wor".into()), ..Default::default() };
        let found = h.query(&q).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].preview, Preview::Text("hello world".into()));
    }

    #[test]
    fn hostile_search_text_does_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        h.insert(&item(Uuid::new_v4(), "a \"quoted\" text")).unwrap();
        for text in ["\"", "a\" OR \"b", "NEAR(", "*", "-", "   "] {
            let q = HistoryQuery { text: Some(text.into()), ..Default::default() };
            assert!(h.query(&q).is_ok(), "query {text:?} failed");
        }
        let q = HistoryQuery { text: Some("\"quoted".into()), ..Default::default() };
        assert_eq!(h.query(&q).unwrap().len(), 1);
    }

    #[test]
    fn filters_by_source_and_kind_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        let (mac, pc) = (Uuid::new_v4(), Uuid::new_v4());
        let first = item(mac, "one");
        h.insert(&first).unwrap();
        let second = item(mac, "two");
        h.insert(&second).unwrap();
        h.insert(&item(pc, "three")).unwrap();
        let q = HistoryQuery { source: Some(mac), kind: Some(ItemKind::Text), ..Default::default() };
        let ids: Vec<Uuid> = h.query(&q).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![second.id, first.id]);
    }

    #[test]
    fn prune_keeps_newest_200_and_deletes_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        let src = Uuid::new_v4();
        let oldest = item(src, "oldest");
        h.insert(&oldest).unwrap();
        for i in 0..HISTORY_CAP {
            h.insert(&item(src, &format!("n{i}"))).unwrap();
        }
        let all = h.query(&HistoryQuery { limit: 1000, ..Default::default() }).unwrap();
        assert_eq!(all.len(), HISTORY_CAP);
        assert_eq!(h.load(oldest.id).unwrap(), None);
        assert!(!dir.path().join("blobs").join(format!("{}.bin", oldest.id)).exists());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core history`
Expected: compile errors, `History` not found.

- [ ] **Step 3: Implement `history.rs`** (above the test module)

Payloads live in `blobs/<id>.bin` (postcard); the database holds metadata, the encoded preview and a separate FTS5 table keyed by item ID.

```rust
use std::path::{Path, PathBuf};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql};
use uuid::Uuid;

use crate::item::{ClipItem, ClipPayload, ItemKind, Preview};
use crate::Result;

pub const HISTORY_CAP: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub source: Uuid,
    pub kind: ItemKind,
    pub created_ms: i64,
    pub size: u64,
    pub preview: Preview,
}

#[derive(Debug, Clone)]
pub struct HistoryQuery {
    pub text: Option<String>,
    pub source: Option<Uuid>,
    pub kind: Option<ItemKind>,
    pub limit: usize,
}

impl Default for HistoryQuery {
    fn default() -> Self {
        Self { text: None, source: None, kind: None, limit: 50 }
    }
}

pub struct History {
    conn: Connection,
    blobs: PathBuf,
}

fn kind_str(k: ItemKind) -> &'static str {
    match k {
        ItemKind::Text => "text",
        ItemKind::Html => "html",
        ItemKind::Image => "image",
        ItemKind::File => "file",
    }
}

fn kind_from(s: &str) -> ItemKind {
    match s {
        "html" => ItemKind::Html,
        "image" => ItemKind::Image,
        "file" => ItemKind::File,
        _ => ItemKind::Text,
    }
}

fn search_text(p: &ClipPayload) -> String {
    match p {
        ClipPayload::Text(t) => t.clone(),
        ClipPayload::Html { plain, .. } => plain.clone(),
        ClipPayload::Image { .. } => String::new(),
    }
}

/// Every whitespace-separated token becomes a quoted prefix term, so user input can
/// never be parsed as FTS5 syntax.
pub fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|t| t.replace('"', "\"\""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

impl History {
    pub fn open(dir: &Path) -> Result<Self> {
        let blobs = dir.join("blobs");
        std::fs::create_dir_all(&blobs)?;
        let conn = Connection::open(dir.join("history.sqlite"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS items (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                kind TEXT NOT NULL,
                created_ms INTEGER NOT NULL,
                size INTEGER NOT NULL,
                preview BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS items_created ON items(created_ms);
            CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(id UNINDEXED, body);",
        )?;
        Ok(Self { conn, blobs })
    }

    fn blob_path(&self, id: Uuid) -> PathBuf {
        self.blobs.join(format!("{id}.bin"))
    }

    pub fn insert(&mut self, item: &ClipItem) -> Result<()> {
        let exists: bool = self
            .conn
            .query_row("SELECT 1 FROM items WHERE id = ?1", [item.id.to_string()], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        if exists {
            return Ok(());
        }
        std::fs::write(self.blob_path(item.id), postcard::to_allocvec(&item.payload)?)?;
        let preview = postcard::to_allocvec(&item.payload.preview())?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO items (id, source, kind, created_ms, size, preview) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                item.id.to_string(),
                item.source.to_string(),
                kind_str(item.payload.kind()),
                item.created_ms,
                item.payload.size() as i64,
                preview
            ],
        )?;
        tx.execute(
            "INSERT INTO items_fts (id, body) VALUES (?1, ?2)",
            params![item.id.to_string(), search_text(&item.payload)],
        )?;
        tx.commit()?;
        self.prune()
    }

    fn prune(&mut self) -> Result<()> {
        let stale: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT id FROM items ORDER BY created_ms DESC, rowid DESC LIMIT -1 OFFSET ?1",
            )?;
            let rows = stmt.query_map([HISTORY_CAP as i64], |r| r.get::<_, String>(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        for id in stale {
            self.conn.execute("DELETE FROM items WHERE id = ?1", [&id])?;
            self.conn.execute("DELETE FROM items_fts WHERE id = ?1", [&id])?;
            let _ = std::fs::remove_file(self.blobs.join(format!("{id}.bin")));
        }
        Ok(())
    }

    pub fn load(&self, id: Uuid) -> Result<Option<ClipItem>> {
        let row = self
            .conn
            .query_row(
                "SELECT source, created_ms FROM items WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;
        let Some((source, created_ms)) = row else {
            return Ok(None);
        };
        let payload: ClipPayload = postcard::from_bytes(&std::fs::read(self.blob_path(id))?)?;
        Ok(Some(ClipItem {
            id,
            source: Uuid::parse_str(&source).unwrap_or_default(),
            created_ms,
            payload,
        }))
    }

    pub fn query(&self, q: &HistoryQuery) -> Result<Vec<HistoryEntry>> {
        let mut sql = String::from(
            "SELECT id, source, kind, created_ms, size, preview FROM items WHERE 1 = 1",
        );
        let mut args: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(fts) = q.text.as_deref().and_then(fts_query) {
            sql.push_str(" AND id IN (SELECT id FROM items_fts WHERE items_fts MATCH ?)");
            args.push(Box::new(fts));
        }
        if let Some(source) = q.source {
            sql.push_str(" AND source = ?");
            args.push(Box::new(source.to_string()));
        }
        if let Some(kind) = q.kind {
            sql.push_str(" AND kind = ?");
            args.push(Box::new(kind_str(kind)));
        }
        sql.push_str(" ORDER BY created_ms DESC, rowid DESC LIMIT ?");
        args.push(Box::new(q.limit as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Vec<u8>>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, source, kind, created_ms, size, preview) = row?;
            out.push(HistoryEntry {
                id: Uuid::parse_str(&id).unwrap_or_default(),
                source: Uuid::parse_str(&source).unwrap_or_default(),
                kind: kind_from(&kind),
                created_ms,
                size: size as u64,
                preview: postcard::from_bytes(&preview)?,
            });
        }
        Ok(out)
    }
}
```

Add `pub mod history;` to `lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wisp-core history`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-core
git commit -m "Add searchable timeline history with pruning"
```

---

### Task 10: File transfer with resume and safe file names

Runs over one bidirectional stream per transfer. The receiver answers `FileStart` with `FileAck { offset }` = bytes it already has (resume). The sender streams `FileChunk`s of 256 KiB, then `FileDone`. The receiver verifies the BLAKE3 hash and answers `FileAck { offset: size }` on success, or `FileAck { offset: 0 }` after discarding the partial file on a checksum mismatch.

Partial files live in `<save_dir>/.wisp-partial/<transfer_id>.part`; partials older than 10 minutes are removed by `cleanup_partials`.

**Files:**
- Create: `crates/wisp-core/src/transfer.rs`
- Modify: `crates/wisp-core/src/lib.rs` (add `pub mod transfer;`)
- Test: inline test module in `transfer.rs`

**Interfaces:**
- Consumes: `Message`, `read_frame`, `write_frame` (Task 2), `CoreError::Transfer` (Task 1)
- Produces:
  - `pub const CHUNK: usize = 256 * 1024;`, `pub const RESUME_WINDOW: Duration = Duration::from_secs(600);`
  - `pub fn sanitize_relative_path(name: &str) -> PathBuf` (never absolute, never contains `..`)
  - `pub fn unique_path(path: &Path) -> PathBuf` (`shot.png` then `shot (1).png`, ...)
  - `pub fn expand_paths(paths: &[PathBuf]) -> Result<Vec<(PathBuf, String)>>` (folders become their files; second value is the relative name using `/`, starting with the folder's own name)
  - `pub async fn send_file<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, transfer_id: Uuid, path: &Path, name: &str, progress: &mut (dyn FnMut(u64) + Send)) -> Result<()>`
  - `pub async fn receive_file<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, save_dir: &Path, progress: &mut (dyn FnMut(u64, u64) + Send)) -> Result<PathBuf>` (progress gets `(received, total)`)
  - `pub fn cleanup_partials(save_dir: &Path, max_age: Duration) -> Result<()>`

- [ ] **Step 1: Write the failing tests** in a new `transfer.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(dir: &Path, name: &str, len: usize) -> PathBuf {
        let path = dir.join(name);
        let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn hostile_names_stay_inside_the_save_folder() {
        for name in ["../../evil.exe", "C:\\Windows\\x.dll", "/etc/passwd", "..", "a/../../b.txt"] {
            let rel = sanitize_relative_path(name);
            assert!(rel.is_relative(), "{name:?} became {rel:?}");
            assert!(!rel.components().any(|c| matches!(c, std::path::Component::ParentDir)));
            assert!(rel.file_name().is_some());
        }
        assert_eq!(sanitize_relative_path("../../evil.exe"), PathBuf::from("evil.exe"));
        assert_eq!(sanitize_relative_path("Shots/a:b?.png"), PathBuf::from("Shots").join("ab.png"));
        assert_eq!(sanitize_relative_path("CON.txt"), PathBuf::from("_CON.txt"));
        assert_eq!(sanitize_relative_path(""), PathBuf::from("file"));
    }

    #[test]
    fn unique_path_appends_counter() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("shot.png");
        assert_eq!(unique_path(&first), first);
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(unique_path(&first), dir.path().join("shot (1).png"));
        std::fs::write(dir.path().join("shot (1).png"), b"x").unwrap();
        assert_eq!(unique_path(&first), dir.path().join("shot (2).png"));
    }

    #[test]
    fn expand_paths_walks_folders() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Shots");
        std::fs::create_dir_all(folder.join("sub")).unwrap();
        write_file(&folder, "a.png", 10);
        write_file(&folder.join("sub"), "b.png", 10);
        let single = write_file(dir.path(), "c.txt", 10);
        let mut names: Vec<String> = expand_paths(&[folder, single]).unwrap().into_iter().map(|(_, n)| n).collect();
        names.sort();
        assert_eq!(names, vec!["Shots/a.png", "Shots/sub/b.png", "c.txt"]);
    }

    #[tokio::test]
    async fn transfer_round_trip_and_collision() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let file = write_file(src.path(), "shot.png", CHUNK * 3 + 17);
        for expected in ["shot.png", "shot (1).png"] {
            let (mut a, mut b) = tokio::io::duplex(1024 * 1024);
            let dst_path = dst.path().to_path_buf();
            let recv = tokio::spawn(async move { receive_file(&mut b, &dst_path, &mut |_, _| {}).await });
            send_file(&mut a, Uuid::new_v4(), &file, "shot.png", &mut |_| {}).await.unwrap();
            let saved = recv.await.unwrap().unwrap();
            assert_eq!(saved, dst.path().join(expected));
            assert_eq!(std::fs::read(&saved).unwrap(), std::fs::read(&file).unwrap());
        }
    }

    #[tokio::test]
    async fn interrupted_transfer_resumes() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let file = write_file(src.path(), "big.bin", CHUNK * 4);
        let data = std::fs::read(&file).unwrap();
        let hash = *blake3::hash(&data).as_bytes();
        let id = Uuid::new_v4();

        {
            let (mut a, mut b) = tokio::io::duplex(1024 * 1024);
            let dst_path = dst.path().to_path_buf();
            let recv = tokio::spawn(async move { receive_file(&mut b, &dst_path, &mut |_, _| {}).await });
            let start = Message::FileStart { transfer_id: id, name: "big.bin".into(), size: data.len() as u64, hash };
            write_frame(&mut a, &start).await.unwrap();
            assert_eq!(read_frame(&mut a).await.unwrap(), Message::FileAck { transfer_id: id, offset: 0 });
            let chunk = Message::FileChunk { transfer_id: id, offset: 0, bytes: data[..CHUNK].to_vec() };
            write_frame(&mut a, &chunk).await.unwrap();
            drop(a);
            assert!(recv.await.unwrap().is_err());
        }

        let (mut a, mut b) = tokio::io::duplex(1024 * 1024);
        let dst_path = dst.path().to_path_buf();
        let recv = tokio::spawn(async move { receive_file(&mut b, &dst_path, &mut |_, _| {}).await });
        let mut first_progress = None;
        send_file(&mut a, id, &file, "big.bin", &mut |sent| {
            first_progress.get_or_insert(sent);
        })
        .await
        .unwrap();
        let saved = recv.await.unwrap().unwrap();
        assert_eq!(std::fs::read(saved).unwrap(), data);
        assert!(first_progress.unwrap() > CHUNK as u64, "did not resume from the partial file");
    }

    #[test]
    fn cleanup_removes_old_partials() {
        let dst = tempfile::tempdir().unwrap();
        let partial_dir = dst.path().join(PARTIAL_DIR);
        std::fs::create_dir_all(&partial_dir).unwrap();
        std::fs::write(partial_dir.join("x.part"), b"x").unwrap();
        cleanup_partials(dst.path(), Duration::ZERO).unwrap();
        assert!(!partial_dir.join("x.part").exists());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core transfer`
Expected: compile errors, `sanitize_relative_path` not found.

- [ ] **Step 3: Implement `transfer.rs`** (above the test module)

```rust
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

use crate::protocol::{read_frame, write_frame, Message};
use crate::{CoreError, Result};

pub const CHUNK: usize = 256 * 1024;
pub const RESUME_WINDOW: Duration = Duration::from_secs(600);
pub const PARTIAL_DIR: &str = ".wisp-partial";
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn sanitize_component(c: &str) -> Option<String> {
    let cleaned: String = c
        .chars()
        .filter(|ch| !ch.is_control() && !"<>:\"|?*".contains(*ch))
        .collect();
    let trimmed = cleaned.trim_matches(|ch: char| ch == '.' || ch == ' ').to_string();
    if trimmed.is_empty() {
        return None;
    }
    let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    let safe = if RESERVED.contains(&stem.as_str()) { format!("_{trimmed}") } else { trimmed };
    Some(safe.chars().take(200).collect())
}

pub fn sanitize_relative_path(name: &str) -> PathBuf {
    let parts: Vec<String> = name
        .split(['/', '\\'])
        .filter(|p| *p != "..")
        .filter_map(sanitize_component)
        .collect();
    if parts.is_empty() {
        return PathBuf::from("file");
    }
    parts.iter().collect()
}

pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 1.. {
        let name = match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

pub fn expand_paths(paths: &[PathBuf]) -> Result<Vec<(PathBuf, String)>> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
            if entry.file_type()?.is_dir() {
                walk(&entry.path(), &name, out)?;
            } else {
                out.push((entry.path(), name));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    for path in paths {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
        if path.is_dir() {
            walk(path, &name, &mut out)?;
        } else {
            out.push((path.clone(), name));
        }
    }
    Ok(out)
}

async fn hash_file(path: &Path) -> Result<([u8; 32], u64)> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((*hasher.finalize().as_bytes(), total))
}

pub async fn send_file<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    transfer_id: Uuid,
    path: &Path,
    name: &str,
    progress: &mut (dyn FnMut(u64) + Send),
) -> Result<()> {
    let (hash, size) = hash_file(path).await?;
    write_frame(stream, &Message::FileStart { transfer_id, name: name.to_string(), size, hash }).await?;
    let mut offset = match read_frame(stream).await? {
        Message::FileAck { offset, .. } if offset <= size => offset,
        other => return Err(CoreError::Transfer(format!("unexpected reply {other:?}"))),
    };

    let mut file = tokio::fs::File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut buf = vec![0u8; CHUNK];
    while offset < size {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        write_frame(stream, &Message::FileChunk { transfer_id, offset, bytes: buf[..n].to_vec() }).await?;
        offset += n as u64;
        progress(offset);
    }
    write_frame(stream, &Message::FileDone { transfer_id }).await?;
    match read_frame(stream).await? {
        Message::FileAck { offset, .. } if offset == size => Ok(()),
        _ => Err(CoreError::Transfer("checksum mismatch on the receiving side".into())),
    }
}

pub async fn receive_file<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    save_dir: &Path,
    progress: &mut (dyn FnMut(u64, u64) + Send),
) -> Result<PathBuf> {
    let (transfer_id, name, size, hash) = match read_frame(stream).await? {
        Message::FileStart { transfer_id, name, size, hash } => (transfer_id, name, size, hash),
        other => return Err(CoreError::Transfer(format!("expected FileStart, got {other:?}"))),
    };
    let partial_dir = save_dir.join(PARTIAL_DIR);
    tokio::fs::create_dir_all(&partial_dir).await?;
    let partial = partial_dir.join(format!("{transfer_id}.part"));
    let mut offset = match tokio::fs::metadata(&partial).await {
        Ok(m) if m.len() <= size => m.len(),
        _ => 0,
    };
    let mut file = tokio::fs::OpenOptions::new().create(true).write(true).open(&partial).await?;
    file.set_len(offset).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    write_frame(stream, &Message::FileAck { transfer_id, offset }).await?;

    loop {
        match read_frame(stream).await? {
            Message::FileChunk { offset: at, bytes, .. } if at == offset => {
                file.write_all(&bytes).await?;
                offset += bytes.len() as u64;
                progress(offset, size);
            }
            Message::FileDone { .. } => break,
            other => return Err(CoreError::Transfer(format!("unexpected message {other:?}"))),
        }
    }
    file.flush().await?;
    drop(file);

    let (actual, _) = hash_file(&partial).await?;
    if actual != hash {
        let _ = tokio::fs::remove_file(&partial).await;
        write_frame(stream, &Message::FileAck { transfer_id, offset: 0 }).await?;
        return Err(CoreError::Transfer("checksum mismatch".into()));
    }
    let target = unique_path(&save_dir.join(sanitize_relative_path(&name)));
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::rename(&partial, &target).await?;
    write_frame(stream, &Message::FileAck { transfer_id, offset: size }).await?;
    Ok(target)
}

pub fn cleanup_partials(save_dir: &Path, max_age: Duration) -> Result<()> {
    let dir = save_dir.join(PARTIAL_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().unwrap_or_default() >= max_age)
            .unwrap_or(true);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}
```

Add `pub mod transfer;` to `lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wisp-core transfer`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-core
git commit -m "Add resumable file transfer with safe file names"
```

---

### Task 11: Engine core - node, pairing, connections, catch and auto-sync

The engine owns all state behind `std::sync::Mutex`es that are never held across an `.await`. Every QUIC connection is to one paired peer. Each small message travels on its own unidirectional stream (one `Message` frame per stream), so a large `Push` never blocks an `Offer`. Bidirectional streams start with a `StreamKind` frame and carry pairing or one file transfer.

When both devices dial each other at once, both apply the same rule: keep the connection initiated by the device with the lower device ID.

A `Push` is only accepted for an item this device asked for with `Pull` (tracked in `pulls`), so a paired device can never write onto another device's clipboard unasked.

**Files:**
- Create: `crates/wisp-core/src/engine/mod.rs` (config, events, `Node` public API), `crates/wisp-core/src/engine/session.rs` (connection handling and message handlers on `Inner`)
- Create: `crates/wisp-core/tests/two_nodes.rs`
- Modify: `crates/wisp-core/src/lib.rs` (add `pub mod engine;`)

**Interfaces:**
- Consumes: everything from Tasks 1-9: `Identity`, `Transport`, `peer_fingerprint`, `pair`, `PairingSession`, `PairInfo`, `PeerStore`, `Peer`, `DeliveryMode`, `History`, `HistoryQuery`, `HistoryEntry`, `ClipboardBackend`, `spawn_monitor`, `write_with_retry`, `EchoGuard`, `offer_for`, `on_offer`, `OfferAction`, `OfferQueue`, `Inbox`, `InboxEntry`, `Message`, `write_frame`, `read_frame`, `write_value`, `read_value`, `ClipItem`, `ClipPayload`
- Produces (used by Task 12 and by Plan 2):
  - `pub struct NodeConfig { data_dir: PathBuf, save_dir: PathBuf, device_name: String, bind: SocketAddr, discovery: bool, clipboard: Arc<dyn ClipboardBackend>, clipboard_poll: Duration, dial_interval: Duration }` (`discovery` and `dial_interval` are used from Task 12 on)
  - `pub enum Event` (serde `tag = "type"`, camelCase): `PeerOnline { peer, name, addr }`, `PeerOffline { peer }`, `Discovered { device, name, os, addresses }`, `IdentityChanged { peer }`, `Paired { peer, name }`, `PairingFailed { reason }`, `LocalItem { item_id }`, `OfferReceived(OfferInfo)`, `ClipboardApplied { item_id, from }`, `TransferProgress { transfer_id, peer, name, done, total, outgoing }`, `TransferPaused { transfer_id, peer }`, `TransferDone { transfer_id, peer, path }`, `TransferFailed { transfer_id, peer, reason }`, `Error { message }`
  - `pub struct OfferInfo { peer: Uuid, item_id: Uuid, kind: ItemKind, size: u64, preview: Preview, age_ms: u64 }`
  - `pub struct PeerStatus { peer: Peer, online: bool, addr: Option<SocketAddr> }`
  - `Node` (Clone): `start(cfg) -> Result<(Node, mpsc::UnboundedReceiver<Event>)>`, `device_id() -> Uuid`, `device_name() -> String`, `local_addr() -> Result<SocketAddr>`, `begin_pairing() -> String`, `cancel_pairing()`, `pair_with(addr, code) -> Result<Peer>`, `peers() -> Vec<PeerStatus>`, `set_mode(peer, mode) -> Result<()>`, `forget(peer) -> Result<()>`, `inbox() -> Vec<OfferInfo>`, `catch(item_id) -> Result<()>`, `catch_latest() -> Result<Option<Uuid>>`, `dismiss(item_id)`, `history(&HistoryQuery) -> Result<Vec<HistoryEntry>>`, `copy_from_history(id) -> Result<()>`, `send_clipboard_to(peer) -> Result<()>`, `shutdown()`
  - Inside the engine module: `enum StreamKind { Pair, File }`, `Inner::send_to(peer, &Message)`, `Inner::connection(peer)`, `Inner::register(peer, conn, initiated_by_me)`, `Inner::handle_bi(...)`, `Inner::serve_pull(...)`, `Inner::emit(Event)`

- [ ] **Step 1: Write the failing integration tests** in `crates/wisp-core/tests/two_nodes.rs`

```rust
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedReceiver;
use wisp_core::clipboard::MemoryClipboard;
use wisp_core::engine::{Event, Node, NodeConfig};
use wisp_core::history::HistoryQuery;
use wisp_core::item::{ClipPayload, Preview};
use wisp_core::peers::DeliveryMode;

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

fn config(dir: &Path, name: &str, clip: Arc<MemoryClipboard>) -> NodeConfig {
    NodeConfig {
        data_dir: dir.join("data"),
        save_dir: dir.join("save"),
        device_name: name.into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        discovery: false,
        clipboard: clip,
        clipboard_poll: Duration::from_millis(20),
        dial_interval: Duration::from_millis(200),
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
```

Add `pub mod engine;` to `lib.rs` (the test needs it to compile).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core --test two_nodes`
Expected: compile errors, `wisp_core::engine` not found.

- [ ] **Step 3: Implement `engine/mod.rs`**

```rust
mod session;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::clipboard::{write_with_retry, ClipboardBackend, EchoGuard};
use crate::discovery::{DiscoveredSet, Discovery};
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
}

struct Inner {
    identity: Identity,
    transport: Transport,
    save_dir: PathBuf,
    tmp_dir: PathBuf,
    clipboard: Arc<dyn ClipboardBackend>,
    events: mpsc::UnboundedSender<Event>,
    peers: Mutex<PeerStore>,
    conns: Mutex<HashMap<Uuid, Conn>>,
    history: Mutex<History>,
    inbox: Mutex<Inbox>,
    queue: Mutex<OfferQueue>,
    pulls: Mutex<HashSet<Uuid>>,
    echo: Arc<Mutex<EchoGuard>>,
    pairing: Mutex<Option<PairingSession>>,
    discovered: Mutex<DiscoveredSet>,
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
        let tmp_dir = cfg.data_dir.join("tmp");
        std::fs::create_dir_all(&cfg.save_dir)?;
        std::fs::create_dir_all(&tmp_dir)?;
        cleanup_partials(&cfg.save_dir, RESUME_WINDOW)?;

        let (events, rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            identity,
            transport,
            save_dir: cfg.save_dir,
            tmp_dir,
            clipboard: cfg.clipboard,
            events,
            peers: Mutex::new(peers),
            conns: Mutex::new(HashMap::new()),
            history: Mutex::new(history),
            inbox: Mutex::new(Inbox::new()),
            queue: Mutex::new(OfferQueue::new()),
            pulls: Mutex::new(HashSet::new()),
            echo: Arc::new(Mutex::new(EchoGuard::new())),
            pairing: Mutex::new(None),
            discovered: Mutex::new(DiscoveredSet::new()),
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
        // Lock order everywhere: peers, then conns, then discovered.
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
        self.inner.request_pull(entry.peer, item_id).await
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
        self.inner.echo.lock().unwrap().mark_written(item.payload.hash());
        write_with_retry(self.inner.clipboard.as_ref(), &item.payload).await
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
```

- [ ] **Step 4: Implement `engine/session.rs`**

```rust
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::{Conn, Event, Inner, OfferInfo, StreamKind};
use crate::clipboard::{spawn_monitor, write_with_retry};
use crate::identity::Fingerprint;
use crate::item::{ClipItem, ClipPayload};
use crate::pairing::{pair, PairInfo};
use crate::peers::{DeliveryMode, Peer};
use crate::policy::{offer_for, on_offer, InboxEntry, OfferAction};
use crate::protocol::{read_frame, read_value, write_frame, write_value, Message, PROTOCOL_VERSION};
use crate::transport::peer_fingerprint;
use crate::{CoreError, Result};

const PAIRING_STREAM_TIMEOUT: Duration = Duration::from_secs(60);

fn net_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Transport(e.to_string())
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
        self.pulls.lock().unwrap().insert(item_id);
        self.send_to(peer, &Message::Pull { item_id }).await
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
        let code = match self.pairing.lock().unwrap().as_ref() {
            Some(s) if s.is_open() => s.code().to_string(),
            _ => String::new(),
        };
        if code.is_empty() {
            conn.close(2u32.into(), b"not pairing");
            return;
        }
        let result = async {
            let (send, recv) = tokio::time::timeout(PAIRING_STREAM_TIMEOUT, conn.accept_bi())
                .await
                .map_err(|_| CoreError::Pairing("timed out".into()))?
                .map_err(net_err)?;
            let mut stream = tokio::io::join(recv, send);
            match read_value::<_, StreamKind>(&mut stream).await? {
                StreamKind::Pair => pair(&mut stream, &code, &self.identity, Some(fp)).await,
                StreamKind::File => Err(CoreError::Pairing("not paired yet".into())),
            }
        }
        .await;
        match result {
            Ok(info) => {
                *self.pairing.lock().unwrap() = None;
                if let Err(e) = self.finish_pairing(info, conn, false) {
                    self.emit(Event::PairingFailed { reason: e.to_string() });
                }
            }
            Err(e) => {
                let closed = {
                    let mut guard = self.pairing.lock().unwrap();
                    match guard.as_mut() {
                        Some(s) => {
                            s.record_failure();
                            let closed = !s.is_open();
                            if closed {
                                *guard = None;
                            }
                            closed
                        }
                        None => false,
                    }
                };
                let reason = if closed {
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
        let result = async {
            let (send, recv) = conn.open_bi().await.map_err(net_err)?;
            let mut stream = tokio::io::join(recv, send);
            write_value(&mut stream, &StreamKind::Pair).await?;
            pair(&mut stream, code, &self.identity, Some(fp)).await
        }
        .await;
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
    pub(super) fn register(self: &Arc<Self>, peer: Uuid, conn: quinn::Connection, initiated_by_me: bool) {
        {
            let mut conns = self.conns.lock().unwrap();
            if let Some(existing) = conns.get(&peer) {
                let alive = existing.conn.close_reason().is_none();
                let new_wins = (self.identity.device_id < peer) == initiated_by_me;
                if alive && !new_wins {
                    conn.close(0u32.into(), b"duplicate");
                    return;
                }
                if alive {
                    existing.conn.close(0u32.into(), b"replaced");
                }
            }
            conns.insert(peer, Conn { conn: conn.clone() });
        }
        let addr = conn.remote_address();
        let name = {
            let mut peers = self.peers.lock().unwrap();
            let _ = peers.remember_address(peer, addr);
            peers.get(peer).map(|p| p.name.clone()).unwrap_or_default()
        };
        self.emit(Event::PeerOnline { peer, name, addr });
        tokio::spawn(self.clone().serve(peer, conn));

        let this = self.clone();
        tokio::spawn(async move {
            let hello = Message::Hello {
                device_id: this.identity.device_id,
                name: this.identity.name.clone(),
                os: this.identity.os,
                version: PROTOCOL_VERSION,
            };
            let _ = this.send_to(peer, &hello).await;
            let queued = this.queue.lock().unwrap().drain(peer);
            for msg in queued {
                if let Err(e) = this.send_to(peer, &msg).await {
                    tracing::warn!("queued offer to {peer} failed: {e}");
                }
            }
        });
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
        match read_value::<_, StreamKind>(&mut stream).await? {
            StreamKind::Pair => Err(CoreError::Pairing(format!("{peer} is already paired"))),
            StreamKind::File => Err(CoreError::Transfer("file transfer arrives in the next task".into())),
        }
    }

    async fn apply_remote(self: &Arc<Self>, peer: Uuid, item_id: Uuid, payload: ClipPayload) {
        if !self.pulls.lock().unwrap().remove(&item_id) {
            tracing::warn!("ignored an unrequested push from {peer}");
            return;
        }
        let mut item = ClipItem::new(peer, payload);
        item.id = item_id;
        if let Err(e) = self.history.lock().unwrap().insert(&item) {
            tracing::warn!("timeline insert failed: {e}");
        }
        self.echo.lock().unwrap().mark_written(item.payload.hash());
        match write_with_retry(self.clipboard.as_ref(), &item.payload).await {
            Ok(()) => self.emit(Event::ClipboardApplied { item_id, from: peer }),
            Err(_) => self.emit(Event::Error { message: "Couldn't write the clipboard. Try again.".into() }),
        }
    }

    pub(super) async fn serve_pull(self: &Arc<Self>, peer: Uuid, item_id: Uuid) -> Result<()> {
        let item = self.history.lock().unwrap().load(item_id)?;
        let item = item.ok_or_else(|| CoreError::Transfer(format!("item {item_id} is no longer in the timeline")))?;
        self.send_to(peer, &Message::Push { item_id, payload: item.payload }).await
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
        let offer = offer_for(&item, true);
        let peers: Vec<Uuid> = self.peers.lock().unwrap().all().iter().map(|p| p.device_id).collect();
        for peer in peers {
            let sent = self.connection(peer).is_some() && self.send_to(peer, &offer).await.is_ok();
            if !sent {
                self.queue.lock().unwrap().push(peer, &item);
            }
        }
    }
}
```

Note on `accept_pairing`: the `match` on the locked session produces the code (or an empty string when no session is open) so the lock is released before any `.await`.

- [ ] **Step 5: Run the integration tests**

Run: `cargo test -p wisp-core --test two_nodes`
Expected: 5 passed.

- [ ] **Step 6: Run the whole crate's tests**

Run: `cargo test -p wisp-core`
Expected: all unit and integration tests pass (ignored tests excluded).

- [ ] **Step 7: Commit**

```bash
git add crates/wisp-core
git commit -m "Add engine with pairing, connections, catch and auto-sync"
```

---

### Task 12: Engine - file transfers, reconnect dialer, offline replay and discovery

**Files:**
- Create: `crates/wisp-core/src/engine/files.rs`, `crates/wisp-core/src/engine/links.rs`
- Modify: `crates/wisp-core/src/engine/mod.rs` (new fields, `NearbyDevice`, `send_files`, `add_address`, `nearby`, start the dialer and discovery, shutdown flag), `crates/wisp-core/src/engine/session.rs` (`handle_bi` receives files, `serve_pull` spills oversized items to a file)
- Test: `crates/wisp-core/tests/two_nodes.rs` (append)

**Interfaces:**
- Consumes: Task 11 engine internals (`Inner`, `StreamKind`, `send_to`, `connection`, `register`, `emit`), Task 10 (`send_file`, `receive_file`, `expand_paths`, `RESUME_WINDOW`), Task 6 (`Discovery`, `Advert`, `Discovered`, `DiscoveryEvent`), Task 8 (`offer_for`)
- Produces:
  - `Node::send_files(&self, peer: Uuid, paths: Vec<PathBuf>) -> Result<Vec<Uuid>>` (one transfer ID per file; folders expand)
  - `Node::add_address(&self, peer: Uuid, addr: SocketAddr) -> Result<()>` (manual "add by IP" for a paired device; dials immediately)
  - `Node::nearby(&self) -> Vec<NearbyDevice>` with `pub struct NearbyDevice { device: Uuid, name: String, os: Os, addresses: Vec<SocketAddr> }` (discovered, not yet paired)
  - Transfer events: outgoing `TransferProgress { outgoing: true, name }`, `TransferPaused` on the first failure, `TransferDone { path: None }` on the sender, `TransferDone { path: Some(..) }` on the receiver, `TransferFailed` after `RESUME_WINDOW` without success. Incoming progress events carry an empty `name` (the receiver learns it only at the end).

- [ ] **Step 1: Write the failing integration tests** (append to `tests/two_nodes.rs`)

```rust
#[tokio::test]
async fn files_arrive_in_the_save_folder() {
    let (mut a, mut b) = paired().await;
    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("report.pdf"), vec![7u8; 700_000]).unwrap();
    let ids = a.node.send_files(b.node.device_id(), vec![src.path().join("report.pdf")]).await.unwrap();
    assert_eq!(ids.len(), 1);

    let Event::TransferDone { path: Some(path), .. } =
        wait_for(&mut b.events, "file saved", |e| matches!(e, Event::TransferDone { path: Some(_), .. })).await
    else {
        unreachable!()
    };
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
```

`uuid` is a normal dependency of `wisp-core`, so integration tests can use `uuid::Uuid` directly.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core --test two_nodes`
Expected: compile error, `send_files` not found on `Node`.

- [ ] **Step 3: Extend `engine/mod.rs`**

Add the module declarations at the top next to `mod session;`:

```rust
mod files;
mod links;
```

Add these imports:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

use crate::transfer::expand_paths;
```

Add the public type below `PeerStatus`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct NearbyDevice {
    pub device: Uuid,
    pub name: String,
    pub os: Os,
    pub addresses: Vec<SocketAddr>,
}
```

Add two fields to `Inner`:

```rust
    dialing: Mutex<HashSet<Uuid>>,
    closed: AtomicBool,
```

Initialize them in `Node::start` (inside `Arc::new(Inner { .. })`):

```rust
            dialing: Mutex::new(HashSet::new()),
            closed: AtomicBool::new(false),
```

At the end of `Node::start`, replace `Ok((Node { inner }, rx))` with:

```rust
        if cfg.discovery {
            inner.start_discovery()?;
        }
        inner.clone().spawn_dialer(cfg.dial_interval);
        Ok((Node { inner }, rx))
```

Add these methods to `impl Node`:

```rust
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

    pub fn add_address(&self, peer: Uuid, addr: SocketAddr) -> Result<()> {
        self.inner.peers.lock().unwrap().remember_address(peer, addr)?;
        self.inner.clone().dial(peer);
        Ok(())
    }

    pub fn nearby(&self) -> Vec<NearbyDevice> {
        let peers = self.inner.peers.lock().unwrap();
        let discovered = self.inner.discovered.lock().unwrap();
        discovered
            .all()
            .into_iter()
            .filter(|d| peers.get(d.device_id).is_none())
            .map(|d| NearbyDevice { device: d.device_id, name: d.name.clone(), os: d.os, addresses: d.addresses.clone() })
            .collect()
    }
```

In `Node::shutdown`, add as the first line:

```rust
        self.inner.closed.store(true, Ordering::SeqCst);
```

- [ ] **Step 4: Create `engine/files.rs`**

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;

use super::{Event, Inner, StreamKind};
use crate::protocol::write_value;
use crate::transfer::{receive_file, send_file, RESUME_WINDOW};
use crate::{CoreError, Result};

const RETRY_EVERY: Duration = Duration::from_secs(3);
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

fn due(last: &mut Option<Instant>, finished: bool) -> bool {
    if finished || last.map_or(true, |t| t.elapsed() >= PROGRESS_EVERY) {
        *last = Some(Instant::now());
        return true;
    }
    false
}

impl Inner {
    pub(super) async fn receive_on<S: AsyncRead + AsyncWrite + Unpin>(
        self: &Arc<Self>,
        peer: Uuid,
        mut stream: S,
    ) -> Result<()> {
        let transfer_id = Uuid::new_v4();
        let events = self.events.clone();
        let mut last = None;
        let mut progress = move |done: u64, total: u64| {
            if due(&mut last, done == total) {
                let _ = events.send(Event::TransferProgress {
                    transfer_id,
                    peer,
                    name: String::new(),
                    done,
                    total,
                    outgoing: false,
                });
            }
        };
        let path = receive_file(&mut stream, &self.save_dir, &mut progress).await?;
        self.emit(Event::TransferDone { transfer_id, peer, path: Some(path) });
        Ok(())
    }

    async fn send_one(self: &Arc<Self>, peer: Uuid, transfer_id: Uuid, path: &Path, name: &str) -> Result<()> {
        let conn = self.connection(peer).ok_or_else(|| CoreError::UnknownPeer(peer.to_string()))?;
        let (send, recv) = conn.open_bi().await.map_err(|e| CoreError::Transport(e.to_string()))?;
        let mut stream = tokio::io::join(recv, send);
        write_value(&mut stream, &StreamKind::File).await?;
        let total = tokio::fs::metadata(path).await?.len();
        let events = self.events.clone();
        let label = name.to_string();
        let mut last = None;
        let mut progress = move |done: u64| {
            if due(&mut last, done == total) {
                let _ = events.send(Event::TransferProgress {
                    transfer_id,
                    peer,
                    name: label.clone(),
                    done,
                    total,
                    outgoing: true,
                });
            }
        };
        send_file(&mut stream, transfer_id, path, name, &mut progress).await
    }

    /// Retries with the same transfer ID, so the receiver resumes from its partial file.
    pub(super) fn spawn_send(self: &Arc<Self>, peer: Uuid, path: PathBuf, name: String, remove_after: bool) -> Uuid {
        let transfer_id = Uuid::new_v4();
        let this = self.clone();
        tokio::spawn(async move {
            let mut first_failure: Option<Instant> = None;
            loop {
                match this.send_one(peer, transfer_id, &path, &name).await {
                    Ok(()) => {
                        this.emit(Event::TransferDone { transfer_id, peer, path: None });
                        break;
                    }
                    Err(e) => {
                        let since = *first_failure.get_or_insert_with(Instant::now);
                        if since.elapsed() >= RESUME_WINDOW {
                            this.emit(Event::TransferFailed { transfer_id, peer, reason: e.to_string() });
                            break;
                        }
                        if since.elapsed() < RETRY_EVERY {
                            this.emit(Event::TransferPaused { transfer_id, peer });
                        }
                        tokio::time::sleep(RETRY_EVERY).await;
                    }
                }
            }
            if remove_after {
                let _ = tokio::fs::remove_file(&path).await;
            }
        });
        transfer_id
    }
}
```

- [ ] **Step 5: Create `engine/links.rs`**

```rust
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use super::{Event, Inner};
use crate::discovery::{Advert, Discovered, Discovery, DiscoveryEvent};
use crate::Result;

impl Inner {
    /// Periodically dials every paired peer that is not connected, using remembered and
    /// discovered addresses. Both sides dial; `register` keeps one connection.
    pub(super) fn spawn_dialer(self: Arc<Self>, every: Duration) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(every);
            loop {
                ticker.tick().await;
                if self.closed.load(Ordering::SeqCst) {
                    break;
                }
                let offline: Vec<Uuid> = {
                    let peers = self.peers.lock().unwrap();
                    let conns = self.conns.lock().unwrap();
                    peers.all().iter().map(|p| p.device_id).filter(|id| !conns.contains_key(id)).collect()
                };
                for peer in offline {
                    self.clone().dial(peer);
                }
            }
        });
    }

    pub(super) fn dial(self: Arc<Self>, peer: Uuid) {
        if !self.dialing.lock().unwrap().insert(peer) {
            return;
        }
        let target = {
            let peers = self.peers.lock().unwrap();
            let discovered = self.discovered.lock().unwrap();
            peers.get(peer).map(|p| {
                let mut addrs = p.addresses.clone();
                if let Some(d) = discovered.get(peer) {
                    for a in &d.addresses {
                        if !addrs.contains(a) {
                            addrs.push(*a);
                        }
                    }
                }
                (p.fingerprint, addrs)
            })
        };
        tokio::spawn(async move {
            if let Some((fp, addrs)) = target {
                if !addrs.is_empty() {
                    if let Ok((conn, _)) = self.transport.connect_any(&addrs, Some(fp)).await {
                        self.register(peer, conn, true);
                    }
                }
            }
            self.dialing.lock().unwrap().remove(&peer);
        });
    }

    pub(super) fn start_discovery(self: &Arc<Self>) -> Result<()> {
        let advert = Advert {
            device_id: self.identity.device_id,
            name: self.identity.name.clone(),
            os: self.identity.os,
            port: self.transport.local_addr()?.port(),
            fingerprint: self.identity.fingerprint(),
        };
        let (discovery, mut rx) = Discovery::start(&advert)?;
        *self.discovery.lock().unwrap() = Some(discovery);
        let this = self.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    DiscoveryEvent::Found(d) => this.on_discovered(d),
                    DiscoveryEvent::Lost(id) => this.discovered.lock().unwrap().remove(id),
                }
            }
        });
        Ok(())
    }

    fn on_discovered(&self, d: Discovered) {
        let pinned = self.peers.lock().unwrap().get(d.device_id).map(|p| p.fingerprint);
        match pinned {
            Some(fp) if fp != d.fingerprint => self.emit(Event::IdentityChanged { peer: d.device_id }),
            Some(_) => {
                self.discovered.lock().unwrap().merge(d);
            }
            None => {
                let (device, name, os, addresses) = (d.device_id, d.name.clone(), d.os, d.addresses.clone());
                if self.discovered.lock().unwrap().merge(d) {
                    self.emit(Event::Discovered { device, name, os, addresses });
                }
            }
        }
    }
}
```

- [ ] **Step 6: Wire file streams and oversized pulls in `engine/session.rs`**

Replace the body of `handle_bi`:

```rust
        let mut stream = tokio::io::join(recv, send);
        match read_value::<_, StreamKind>(&mut stream).await? {
            StreamKind::Pair => Err(CoreError::Pairing(format!("{peer} is already paired"))),
            StreamKind::File => self.receive_on(peer, stream).await,
        }
```

Replace `serve_pull` (oversized items are written to a temp file and sent as a file transfer, named like the offer's `Preview::File` name):

```rust
    pub(super) async fn serve_pull(self: &Arc<Self>, peer: Uuid, item_id: Uuid) -> Result<()> {
        let item = self.history.lock().unwrap().load(item_id)?;
        let item = item.ok_or_else(|| CoreError::Transfer(format!("item {item_id} is no longer in the timeline")))?;
        if item.payload.size() <= MAX_CLIP_BYTES {
            return self.send_to(peer, &Message::Push { item_id, payload: item.payload }).await;
        }
        let name = match offer_for(&item, false) {
            Message::Offer { preview: Preview::File { name }, .. } => name,
            _ => format!("clipboard-{}", item.created_ms),
        };
        let bytes = match item.payload {
            ClipPayload::Text(t) => t.into_bytes(),
            ClipPayload::Html { html, .. } => html.into_bytes(),
            ClipPayload::Image { png, .. } => png,
        };
        let path = self.tmp_dir.join(format!("{item_id}-{name}"));
        tokio::fs::write(&path, bytes).await?;
        self.spawn_send(peer, path, name, true);
        Ok(())
    }
```

Update the imports at the top of `session.rs`:

```rust
use crate::item::{ClipItem, ClipPayload, Preview, MAX_CLIP_BYTES};
```

- [ ] **Step 7: Run the integration tests**

Run: `cargo test -p wisp-core --test two_nodes`
Expected: 8 passed.

- [ ] **Step 8: Run everything plus clippy**

Run: `cargo test -p wisp-core && cargo clippy -p wisp-core --all-targets -- -D warnings`
Expected: all tests pass, no clippy warnings. Fix any clippy findings in the code (do not add `allow` attributes).

- [ ] **Step 9: Run the ignored live tests once on the real machine**

Run: `cargo test -p wisp-core -- --ignored`
Expected: mDNS discovery and OS clipboard tests pass (allow the firewall prompt for private networks).

- [ ] **Step 10: Commit**

```bash
git add crates/wisp-core
git commit -m "Add file transfers, reconnect dialer, offline replay and discovery to the engine"
```

---

## Done criteria for Plan 1

- `cargo test -p wisp-core` passes on Windows (and on macOS when run there).
- `cargo clippy -p wisp-core --all-targets -- -D warnings` is clean.
- The ignored live tests pass once on a real home network.
- Plan 2 (Tauri shell + UI) can consume `wisp_core::engine::{Node, NodeConfig, Event, OfferInfo, PeerStatus, NearbyDevice}` without changes to the core.
