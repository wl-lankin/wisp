//! Watches a folder for new screenshots and turns them into image payloads.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use image::{ImageFormat, ImageReader};
use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::error::{CoreError, Result};
use crate::item::ClipPayload;

const STABLE_INTERVAL: Duration = Duration::from_millis(200);
const STABLE_MAX: Duration = Duration::from_secs(10);

/// True for image files a screenshot may be saved as; dot-files are ignored.
pub fn is_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') {
        return false;
    }
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    ["png", "jpg", "jpeg", "heic"].iter().any(|known| ext.eq_ignore_ascii_case(known))
}

/// Waits until the file size is unchanged across two consecutive checks and returns it.
/// An empty file is never considered stable, since it is still being created.
pub async fn wait_until_stable(path: &Path, interval: Duration, max: Duration) -> Result<u64> {
    let deadline = tokio::time::Instant::now() + max;
    let mut last = tokio::fs::metadata(path).await?.len();
    loop {
        if tokio::time::Instant::now() + interval > deadline {
            return Err(CoreError::Invalid(format!("{} did not settle", path.display())));
        }
        tokio::time::sleep(interval).await;
        let size = tokio::fs::metadata(path).await?.len();
        if size == last && size > 0 {
            return Ok(size);
        }
        last = size;
    }
}

/// Remembers which files were already offered in this session, by path and
/// modification time and by content, so a renamed or copied file is not offered again.
#[derive(Debug, Default)]
pub struct SeenFiles {
    seen: HashMap<PathBuf, SystemTime>,
    contents: HashSet<[u8; 32]>,
}

impl SeenFiles {
    pub fn new() -> Self {
        Self::default()
    }

    /// True the first time a file is seen with this (or a newer) modification time.
    pub fn first_time(&mut self, path: &Path, modified: SystemTime) -> bool {
        match self.seen.get(path) {
            Some(known) if *known >= modified => false,
            _ => {
                self.seen.insert(path.to_path_buf(), modified);
                true
            }
        }
    }

    /// True the first time these file contents (by blake3 hash) are seen.
    pub fn first_content(&mut self, hash: [u8; 32]) -> bool {
        self.contents.insert(hash)
    }
}

/// Turns an image file into a PNG payload. A PNG is kept as-is (only its header is read);
/// anything else is decoded and re-encoded. CPU heavy: call it off the async runtime.
pub fn image_payload(bytes: Vec<u8>) -> Result<ClipPayload> {
    let reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
    match reader.format() {
        Some(ImageFormat::Png) => {
            let (width, height) = reader.into_dimensions().map_err(image_error)?;
            Ok(ClipPayload::Image { png: bytes, width, height })
        }
        Some(_) => {
            let img = reader.decode().map_err(image_error)?;
            let mut out = Cursor::new(Vec::new());
            img.write_to(&mut out, ImageFormat::Png).map_err(image_error)?;
            Ok(ClipPayload::Image { png: out.into_inner(), width: img.width(), height: img.height() })
        }
        None => Err(CoreError::Invalid("image: unknown format".into())),
    }
}

fn image_error(e: image::ImageError) -> CoreError {
    CoreError::Invalid(format!("image: {e}"))
}

/// Watches one folder; dropping it stops watching.
pub struct ScreenshotWatcher {
    _watcher: RecommendedWatcher,
    task: JoinHandle<()>,
}

impl ScreenshotWatcher {
    pub fn start(
        folder: PathBuf,
        is_screenshot: fn(&Path) -> bool,
        on_shot: Arc<dyn Fn(ClipPayload) + Send + Sync>,
    ) -> Result<ScreenshotWatcher> {
        let (tx, rx) = mpsc::unbounded_channel();
        // The notify callback runs on its own thread; an unbounded send never blocks it.
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
            Ok(event) => {
                for path in changed_paths(&event) {
                    let _ = tx.send(path);
                }
            }
            Err(e) => tracing::debug!("screenshot watcher error: {e}"),
        })
        .map_err(watch_error)?;
        watcher.watch(&folder, RecursiveMode::NonRecursive).map_err(watch_error)?;
        let task = tokio::spawn(process(rx, is_screenshot, on_shot));
        Ok(ScreenshotWatcher { _watcher: watcher, task })
    }
}

fn watch_error(e: notify::Error) -> CoreError {
    CoreError::Invalid(format!("watch: {e}"))
}

/// Candidate paths a file system event may have finished writing. Platforms report
/// the same save differently (Windows: create then modify, macOS: rename or metadata),
/// so every kind that can mark the end of a write is accepted and repeats are deduped later.
fn changed_paths(event: &notify::Event) -> Vec<PathBuf> {
    let paths: &[PathBuf] = match event.kind {
        EventKind::Create(_)
        | EventKind::Modify(ModifyKind::Data(_))
        | EventKind::Modify(ModifyKind::Metadata(_))
        | EventKind::Modify(ModifyKind::Any)
        | EventKind::Modify(ModifyKind::Name(RenameMode::To))
        | EventKind::Modify(ModifyKind::Name(RenameMode::Any)) => &event.paths,
        // Paths are [from, to]; only the destination matters.
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => event.paths.get(1..).unwrap_or(&[]),
        _ => &[],
    };
    paths.iter().filter(|p| is_candidate(p)).cloned().collect()
}

async fn process(
    mut rx: mpsc::UnboundedReceiver<PathBuf>,
    is_screenshot: fn(&Path) -> bool,
    on_shot: Arc<dyn Fn(ClipPayload) + Send + Sync>,
) {
    let mut seen = SeenFiles::new();
    // A path already waiting is not queued again. It leaves the set when its turn comes,
    // so events arriving while it is processed queue it once more.
    let mut queue = VecDeque::new();
    let mut pending = HashSet::new();
    loop {
        if queue.is_empty() {
            match rx.recv().await {
                Some(path) => enqueue(&mut queue, &mut pending, path),
                None => return,
            }
        }
        while let Ok(path) = rx.try_recv() {
            enqueue(&mut queue, &mut pending, path);
        }
        let Some(path) = queue.pop_front() else { continue };
        pending.remove(&path);
        match screenshot_payload(&path, is_screenshot, &mut seen).await {
            Ok(Some(payload)) => on_shot(payload),
            Ok(None) => {}
            Err(e) => tracing::debug!("skipping {}: {e}", path.display()),
        }
    }
}

fn enqueue(queue: &mut VecDeque<PathBuf>, pending: &mut HashSet<PathBuf>, path: PathBuf) {
    if pending.insert(path.clone()) {
        queue.push_back(path);
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| CoreError::Invalid(format!("worker: {e}")))
}

async fn screenshot_payload(
    path: &Path,
    is_screenshot: fn(&Path) -> bool,
    seen: &mut SeenFiles,
) -> Result<Option<ClipPayload>> {
    // Cheap check first; an attribute set late arrives as a later metadata event.
    if !is_screenshot(path) {
        return Ok(None);
    }
    wait_until_stable(path, STABLE_INTERVAL, STABLE_MAX).await?;
    let modified = tokio::fs::metadata(path).await?.modified()?;
    if !seen.first_time(path, modified) {
        return Ok(None);
    }
    let bytes = tokio::fs::read(path).await?;
    let (bytes, hash) = blocking(move || {
        let hash = *blake3::hash(&bytes).as_bytes();
        (bytes, hash)
    })
    .await?;
    if !seen.first_content(hash) {
        return Ok(None);
    }
    let bytes = if is_heic(path) { heic_to_png(path.to_path_buf()).await? } else { bytes };
    blocking(move || image_payload(bytes)).await?.map(Some)
}

fn is_heic(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("heic"))
}

/// Converts a HEIC file to PNG bytes with the system `sips` tool.
#[cfg(target_os = "macos")]
async fn heic_to_png(src: PathBuf) -> Result<Vec<u8>> {
    blocking(move || {
        let out = std::env::temp_dir().join(format!("wisp-heic-{}.png", uuid::Uuid::new_v4()));
        let result = match std::process::Command::new("sips")
            .args(["-s", "format", "png"])
            .arg(&src)
            .arg("--out")
            .arg(&out)
            .output()
        {
            Ok(o) if o.status.success() => std::fs::read(&out).map_err(CoreError::from),
            Ok(o) => Err(CoreError::Invalid(format!("sips failed: {}", String::from_utf8_lossy(&o.stderr).trim()))),
            Err(e) => Err(CoreError::from(e)),
        };
        let _ = std::fs::remove_file(&out);
        result
    })
    .await?
}

/// HEIC needs the macOS `sips` tool; elsewhere such files are skipped.
#[cfg(not(target_os = "macos"))]
async fn heic_to_png(src: PathBuf) -> Result<Vec<u8>> {
    Err(CoreError::Invalid(format!("{}: HEIC is only supported on macOS", src.display())))
}

/// True when macOS marked the file as a screen capture.
#[cfg(target_os = "macos")]
pub fn is_screenshot(path: &Path) -> bool {
    matches!(xattr::get(path, "com.apple.metadata:kMDItemIsScreenCapture"), Ok(Some(_)))
}

/// The folder macOS saves screenshots to: the user's setting, or the Desktop.
#[cfg(target_os = "macos")]
pub fn screenshot_folder() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let configured = std::process::Command::new("defaults")
        .args(["read", "com.apple.screencapture", "location"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| match s.strip_prefix('~') {
            Some(rest) => home.join(rest.trim_start_matches('/')),
            None => PathBuf::from(s),
        });
    match configured {
        Some(dir) if dir.is_dir() => dir,
        _ => home.join("Desktop"),
    }
}

impl Drop for ScreenshotWatcher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn encoded(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(width, height, image::Rgb([45, 226, 197]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    #[test]
    fn candidates_are_images_and_not_dot_files() {
        assert!(is_candidate(Path::new("/x/Screenshot 2026.png")));
        assert!(is_candidate(Path::new("/x/shot.JPG")));
        assert!(is_candidate(Path::new("/x/shot.jpeg")));
        assert!(is_candidate(Path::new("/x/shot.heic")));
        assert!(!is_candidate(Path::new("/x/notes.txt")));
        assert!(!is_candidate(Path::new("/x/.Screenshot.png")));
        assert!(!is_candidate(Path::new("/x/shot.png.tmp")));
        assert!(!is_candidate(Path::new("/x/noext")));
    }

    #[tokio::test]
    async fn stable_size_is_returned_after_the_last_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[1; 10]).unwrap();
        f.flush().unwrap();
        let writer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            f.write_all(&[2; 20]).unwrap();
            f.flush().unwrap();
        });
        let size = wait_until_stable(&path, Duration::from_millis(200), Duration::from_secs(5)).await.unwrap();
        writer.await.unwrap();
        assert_eq!(size, 30);
    }

    #[tokio::test]
    async fn a_growing_file_errors_after_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        let mut f = std::fs::File::create(&path).unwrap();
        let writer = tokio::spawn(async move {
            for _ in 0..40 {
                f.write_all(&[1; 10]).unwrap();
                f.flush().unwrap();
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        let result = wait_until_stable(&path, Duration::from_millis(50), Duration::from_millis(400)).await;
        writer.abort();
        assert!(result.is_err());
    }

    #[test]
    fn seen_files_are_offered_once_per_modification() {
        let mut seen = SeenFiles::new();
        let path = Path::new("/x/a.png");
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        assert!(seen.first_time(path, t));
        assert!(!seen.first_time(path, t));
        assert!(seen.first_time(path, t + Duration::from_secs(1)));
        assert!(seen.first_time(Path::new("/x/b.png"), t));
        assert!(seen.first_content([1; 32]));
        assert!(!seen.first_content([1; 32]));
        assert!(seen.first_content([2; 32]));
    }

    #[test]
    fn jpeg_becomes_a_png_payload() {
        let jpeg = encoded(30, 20, image::ImageFormat::Jpeg);
        match image_payload(jpeg).unwrap() {
            ClipPayload::Image { png, width, height } => {
                assert_eq!((width, height), (30, 20));
                assert_eq!(image::guess_format(&png).unwrap(), image::ImageFormat::Png);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn png_is_kept_as_is() {
        let png = encoded(12, 7, image::ImageFormat::Png);
        assert_eq!(image_payload(png.clone()).unwrap(), ClipPayload::Image { png, width: 12, height: 7 });
        assert!(image_payload(b"not an image".to_vec()).is_err());
    }

    fn collecting() -> (Arc<dyn Fn(ClipPayload) + Send + Sync>, mpsc::UnboundedReceiver<ClipPayload>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Arc::new(move |p| drop(tx.send(p))), rx)
    }

    async fn next(rx: &mut mpsc::UnboundedReceiver<ClipPayload>) -> ClipPayload {
        tokio::time::timeout(Duration::from_secs(15), rx.recv())
            .await
            .expect("timed out waiting for a screenshot")
            .expect("callback channel closed")
    }

    async fn assert_quiet(rx: &mut mpsc::UnboundedReceiver<ClipPayload>) {
        let extra = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
        assert!(extra.is_err(), "unexpected extra callback");
    }

    #[tokio::test]
    async fn watcher_offers_a_new_png_once_and_ignores_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let (on_shot, mut rx) = collecting();
        let _watcher = ScreenshotWatcher::start(dir.path().to_path_buf(), |_| true, on_shot).unwrap();
        std::fs::write(dir.path().join("download.tmp"), b"partial").unwrap();
        std::fs::write(dir.path().join("Screenshot.png"), encoded(16, 9, image::ImageFormat::Png)).unwrap();
        match next(&mut rx).await {
            ClipPayload::Image { width, height, .. } => assert_eq!((width, height), (16, 9)),
            other => panic!("unexpected {other:?}"),
        }
        assert_quiet(&mut rx).await;
    }

    #[tokio::test]
    async fn watcher_offers_a_file_once_when_events_repeat() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Screenshot.png");
        let (on_shot, mut rx) = collecting();
        let _watcher = ScreenshotWatcher::start(dir.path().to_path_buf(), |_| true, on_shot).unwrap();
        let bytes = encoded(8, 8, image::ImageFormat::Png);
        std::fs::write(&path, &bytes).unwrap();
        next(&mut rx).await;
        // Same content, same modification time: new events, but the same file.
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        (&f).write_all(&bytes).unwrap();
        f.set_modified(modified).unwrap();
        drop(f);
        assert_quiet(&mut rx).await;
    }

    #[tokio::test]
    async fn a_renamed_screenshot_is_not_offered_again() {
        let dir = tempfile::tempdir().unwrap();
        let (on_shot, mut rx) = collecting();
        let _watcher = ScreenshotWatcher::start(dir.path().to_path_buf(), |_| true, on_shot).unwrap();
        let first = dir.path().join("Screenshot.png");
        std::fs::write(&first, encoded(8, 6, image::ImageFormat::Png)).unwrap();
        next(&mut rx).await;
        std::fs::rename(&first, dir.path().join("Renamed.png")).unwrap();
        assert_quiet(&mut rx).await;
    }

    #[tokio::test]
    async fn a_hidden_temp_file_renamed_into_place_is_offered_once() {
        let dir = tempfile::tempdir().unwrap();
        let (on_shot, mut rx) = collecting();
        let _watcher = ScreenshotWatcher::start(dir.path().to_path_buf(), |_| true, on_shot).unwrap();
        let temp = dir.path().join(".Screenshot x.png");
        std::fs::write(&temp, encoded(10, 5, image::ImageFormat::Png)).unwrap();
        std::fs::rename(&temp, dir.path().join("Screenshot x.png")).unwrap();
        match next(&mut rx).await {
            ClipPayload::Image { width, height, .. } => assert_eq!((width, height), (10, 5)),
            other => panic!("unexpected {other:?}"),
        }
        assert_quiet(&mut rx).await;
    }

    #[tokio::test]
    async fn watcher_skips_files_that_are_not_screenshots() {
        let dir = tempfile::tempdir().unwrap();
        let (on_shot, mut rx) = collecting();
        let _watcher = ScreenshotWatcher::start(dir.path().to_path_buf(), |_| false, on_shot).unwrap();
        std::fs::write(dir.path().join("photo.png"), encoded(4, 4, image::ImageFormat::Png)).unwrap();
        assert_quiet(&mut rx).await;
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_plain_png_is_not_a_screenshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.png");
        std::fs::write(&path, encoded(4, 4, image::ImageFormat::Png)).unwrap();
        assert!(!is_screenshot(&path));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_screenshot_folder_exists() {
        assert!(screenshot_folder().is_dir());
    }
}
