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
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();
        std::thread::Builder::new()
            .name("wisp-clipboard-watch".into())
            .spawn(move || {
                let mut watcher = match ClipboardWatcherContext::new() {
                    Ok(w) => w,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                watcher.add_handler(CountChanges(counter));
                let _ = ready_tx.send(Ok(()));
                watcher.start_watch();
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { changes }),
            Ok(Err(e)) => Err(clip_err(e)),
            Err(_) => Err(clip_err("clipboard watcher thread exited during startup")),
        }
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
        self.is_echo_at(hash, Instant::now())
    }

    fn is_echo_at(&mut self, hash: &[u8; 32], now: Instant) -> bool {
        self.recent.retain(|(_, at)| now.saturating_duration_since(*at) < ECHO_TTL);
        self.recent.iter().any(|(h, _)| h == hash)
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

/// Writes the payload, then records both its hash and the hash of what the OS
/// hands back on read (the OS may re-encode, e.g. images), so neither is
/// reported as a user copy. Nothing is recorded when the write fails.
pub async fn write_and_mark(
    backend: &dyn ClipboardBackend,
    guard: &Mutex<EchoGuard>,
    payload: &ClipPayload,
) -> Result<()> {
    write_with_retry(backend, payload).await?;
    guard.lock().unwrap().mark_written(payload.hash());
    if let Ok(Some(readback)) = backend.read() {
        guard.lock().unwrap().mark_written(readback.hash());
    }
    Ok(())
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
            if tx.is_closed() {
                break;
            }
            let now = backend.change_count();
            if now == seen {
                continue;
            }
            let payload = match backend.read() {
                Ok(Some(p)) => p,
                Ok(None) => {
                    seen = now;
                    continue;
                }
                Err(e) => {
                    tracing::warn!("clipboard read failed: {e}");
                    continue;
                }
            };
            seen = now;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn echo_guard_suppresses_repeats_until_ttl() {
        let mut g = EchoGuard::new();
        let h = [1u8; 32];
        assert!(!g.is_echo(&h));
        g.mark_written(h);
        assert!(g.is_echo(&h));
        assert!(g.is_echo(&h));
        let later = Instant::now() + ECHO_TTL + Duration::from_secs(1);
        assert!(!g.is_echo_at(&h, later));
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

    struct Reencoding {
        inner: MemoryClipboard,
    }

    impl ClipboardBackend for Reencoding {
        fn read(&self) -> Result<Option<ClipPayload>> {
            Ok(self.inner.current().map(|p| match p {
                ClipPayload::Image { mut png, width, height } => {
                    png.push(0);
                    ClipPayload::Image { png, width, height }
                }
                other => other,
            }))
        }
        fn write(&self, payload: &ClipPayload) -> Result<()> {
            self.inner.write(payload)
        }
        fn change_count(&self) -> u64 {
            self.inner.change_count()
        }
    }

    #[tokio::test]
    async fn write_and_mark_suppresses_reencoded_readback() {
        let clip = Arc::new(Reencoding { inner: MemoryClipboard::new() });
        let guard = Arc::new(Mutex::new(EchoGuard::new()));
        let mut rx = spawn_monitor(clip.clone(), guard.clone(), Duration::from_millis(10));

        let img = ClipPayload::Image { png: vec![1, 2, 3], width: 1, height: 1 };
        write_and_mark(clip.as_ref(), &guard, &img).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn write_and_mark_does_not_mark_failed_writes() {
        let flaky = Flaky { fails_left: AtomicU32::new(u32::MAX), inner: MemoryClipboard::new() };
        let guard = Mutex::new(EchoGuard::new());
        let p = ClipPayload::Text("x".into());
        assert!(write_and_mark(&flaky, &guard, &p).await.is_err());
        assert!(!guard.lock().unwrap().is_echo(&p.hash()));
    }

    struct FlakyRead {
        fails_left: AtomicU32,
        inner: MemoryClipboard,
    }

    impl ClipboardBackend for FlakyRead {
        fn read(&self) -> Result<Option<ClipPayload>> {
            if self.fails_left.load(Ordering::SeqCst) > 0 {
                self.fails_left.fetch_sub(1, Ordering::SeqCst);
                return Err(CoreError::Clipboard("locked".into()));
            }
            self.inner.read()
        }
        fn write(&self, payload: &ClipPayload) -> Result<()> {
            self.inner.write(payload)
        }
        fn change_count(&self) -> u64 {
            self.inner.change_count()
        }
    }

    #[tokio::test]
    async fn monitor_retries_after_a_failed_read() {
        let clip = Arc::new(FlakyRead { fails_left: AtomicU32::new(1), inner: MemoryClipboard::new() });
        let guard = Arc::new(Mutex::new(EchoGuard::new()));
        let mut rx = spawn_monitor(clip.clone(), guard, Duration::from_millis(10));
        tokio::time::sleep(Duration::from_millis(30)).await;

        clip.inner.set_external(ClipPayload::Text("user copy".into()));
        let got = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
        assert_eq!(got, ClipPayload::Text("user copy".into()));
    }

    #[tokio::test]
    #[ignore = "touches the real OS clipboard"]
    async fn system_clipboard_round_trips_text_and_images() {
        let clip = SystemClipboard::start().unwrap();
        clip.write(&ClipPayload::Text("wisp test".into())).unwrap();
        assert_eq!(clip.read().unwrap(), Some(ClipPayload::Text("wisp test".into())));

        let guard = Mutex::new(EchoGuard::new());
        let mut png = Vec::new();
        image::RgbaImage::new(4, 4)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let img = ClipPayload::Image { png, width: 4, height: 4 };
        write_and_mark(&clip, &guard, &img).await.unwrap();
        let readback = clip.read().unwrap().expect("image on clipboard");
        assert!(guard.lock().unwrap().is_echo(&readback.hash()));
    }
}
