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
            })
            .map_err(clip_err)?;
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
