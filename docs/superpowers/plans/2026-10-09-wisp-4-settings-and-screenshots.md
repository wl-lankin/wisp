# Wisp Device Rename, History Size and Screenshot Watcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user rename the device and choose the timeline size in Settings, and offer macOS file screenshots to the other devices.

**Architecture:** The core gets a runtime device name (persisted, re-advertised, sent via `Hello`), a runtime history cap, `Node::offer_local` (offer without touching the local clipboard) and a `screenshots` module (pure filters plus a `notify`-based folder watcher; macOS-only screenshot detection by extended attribute). The app wires these into settings, commands, the engine lifecycle and the Settings view.

**Tech Stack:** Rust (wisp-core, wisp-app), `notify` 8, `xattr` 1 (macOS only), `image` 0.25 with png and jpeg; Vue 3 + TypeScript + Vitest.

**Spec:** `docs/superpowers/specs/2026-10-09-wisp-settings-and-screenshots-design.md`
**Branch:** `settings-and-screenshots` (from `release`).

**Version drift:** adapt to the resolved crate APIs at the call site; keep the interfaces named here; note adaptations.

## Global Constraints

- Device names: trimmed, capped with `cap_device_name` (64 characters, 200 bytes), never empty.
- History size: 20 to 2,000 items, default 200; the app offers 50, 200, 500, 1,000.
- Screenshot detection by the `com.apple.metadata:kMDItemIsScreenCapture` extended attribute only, never by file name; extensions png, jpg, jpeg, heic; dot-files ignored; wait for a stable size (two checks 200 ms apart, max 10 s); each file offered once per session.
- Screenshot sharing setting default true, shown and active only on macOS.
- No std `Mutex` guard held across `.await`; lock order peers -> conns -> discovered stays intact.
- No em-dashes or en-dashes; no `#[allow]` / `@ts-ignore`; plain sentence commit messages; do not push.

## Review Focus

1. **Rename consistency:** after a rename, every place that sends or shows this device's name (mDNS TXT, `Hello`, pairing `PairInfo`, `device_name()`, snapshot) uses the new name; the device ID and key never change. Pinned in Task 1.
2. **Shrinking history:** lowering the cap deletes the oldest items and their blob files, never newer ones. Pinned in Task 2.
3. **No clipboard side effects:** `offer_local` must not write this device's clipboard or trigger the echo guard. Pinned in Task 3.
4. **Watcher robustness:** half-written files, temp dot-files, renames and duplicate events must not produce broken or duplicate offers. Pinned in Task 3.
5. **Restart safety:** the watcher survives engine restarts (save folder change) by offering through the current node, and stops when the setting is off. Pinned in Task 4.

---

### Task 1: Runtime device name in the core

**Files:**
- Modify: `crates/wisp-core/src/identity.rs` (`Identity::save`), `crates/wisp-core/src/engine/mod.rs` (`Inner.name`, `Inner.identity_path`, `Node::rename`, `device_name`), `crates/wisp-core/src/engine/session.rs` and `links.rs` (use the current name), `crates/wisp-core/src/discovery.rs` (`Discovery::update`)
- Test: `crates/wisp-core/tests/two_nodes.rs`, inline tests in `identity.rs`

**Interfaces:**
- Produces: `Identity::save(&self, path: &Path) -> Result<()>` (atomic, owner-only, same as creation), `Discovery::update(&self, advert: &Advert) -> Result<()>`, `Node::rename(&self, name: &str) -> Result<String>`; `Node::device_name()` returns the current name.

- [ ] **Step 1: Failing tests**

Append to `tests/two_nodes.rs`:

```rust
#[tokio::test]
async fn renaming_reaches_connected_peers_and_persists() {
    let (a, mut b) = paired().await;
    let stored = a.node.rename("  Studio Mac  ").await.unwrap();
    assert_eq!(stored, "Studio Mac");
    assert_eq!(a.node.device_name(), "Studio Mac");
    let a_id = a.node.device_id();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if b.node.peers().iter().any(|p| p.peer.device_id == a_id && p.peer.name == "Studio Mac") {
                break;
            }
            let _ = b.events.recv().await;
        }
    })
    .await
    .expect("peer never saw the new name");
    let text = std::fs::read_to_string(a.dir.path().join("data").join("identity.json")).unwrap();
    assert!(text.contains("Studio Mac"));
    assert!(a.node.rename("   ").await.is_err());
}
```

(The wait loop polls `peers()` after each event; if `b.events` yields nothing because the name update emits no event, replace the `recv` with a `tokio::time::sleep(Duration::from_millis(50))`.)

In `identity.rs` tests: `save` round trip (generate, change name, save, load_or_create returns the new name and the same device ID and key).

Run: `cargo test -p wisp-core rename` and `cargo test -p wisp-core identity`
Expected: FAIL (compile errors).

- [ ] **Step 2: Implement**

- `identity.rs`: `pub fn save(&self, path: &Path) -> Result<()>` serializing with the existing `write_atomic_private`.
- `discovery.rs`: `Discovery` keeps the daemon; add `pub fn update(&self, advert: &Advert) -> Result<()>` that unregisters the instance (`<device_id>.<SERVICE_TYPE>` full name) and registers a new `ServiceInfo` built exactly like in `start` (same instance, host, port, new TXT). If mdns-sd allows re-registering in place, a single register is fine; keep `start` and `update` sharing one `service_info(advert)` helper.
- `engine/mod.rs`: `Inner` gets `name: Mutex<String>` (initialized from the loaded identity, capped) and `identity_path: PathBuf`. `device_name()` returns `self.inner.name.lock().unwrap().clone()`. Add:

```rust
    /// Renames this device: persisted, re-advertised and sent to connected peers.
    pub async fn rename(&self, name: &str) -> Result<String> {
        self.inner.rename(name).await
    }
```

- `session.rs` (or a small helper in `mod.rs`): `Inner::current_name()`; `Inner::rename(name)`:
  1. `let name = cap_device_name(name.trim());` empty -> `CoreError::Pairing`-style clear error (use a new `CoreError::Invalid(String)` if no fitting variant exists).
  2. Save: clone `self.identity`, set `name`, `save(&self.identity_path)`; then set `*self.name.lock().unwrap() = name.clone()`.
  3. Re-advertise: build the `Advert` like `start_discovery` (move that construction into a helper `fn advert(&self) -> Result<Advert>` using `current_name()`), and call `update` on the discovery if present.
  4. Send `Hello` (same fields as in `register`, with the new name) to every connected peer, ignoring per-peer send failures.
- Replace every `self.identity.name` / `this.identity.name` use with `current_name()` (links.rs advert, session.rs Hello). For pairing, pass an `Identity` clone whose `name` is the current name.

- [ ] **Step 3: Verify** `cargo test -p wisp-core` (all), `cargo test -p wisp-core --test two_nodes` twice, `cargo clippy -p wisp-core --all-targets -- -D warnings`.

- [ ] **Step 4: Commit** `git commit -m "Let the device be renamed at runtime"`

---

### Task 2: Runtime history cap

**Files:**
- Modify: `crates/wisp-core/src/history.rs`, `crates/wisp-core/src/engine/mod.rs` (`NodeConfig.history_cap`, `Node::set_history_cap`, `Node::history_count`), every `NodeConfig { .. }` construction (tests, `two_nodes.rs`, `apps/wisp/src-tauri/src/engine.rs` `node_config` with `HISTORY_CAP` for now)
- Test: inline tests in `history.rs`, `engine/mod.rs`

**Interfaces:**
- Produces: `history::MIN_HISTORY = 20`, `history::MAX_HISTORY = 2000`, `HISTORY_CAP` stays the default (200); `History::set_cap(&mut self, cap: usize) -> Result<()>` (range-checked, prunes immediately), `History::cap(&self) -> usize`, `History::count(&self) -> Result<usize>`; `NodeConfig.history_cap: usize`; `Node::set_history_cap(&self, cap: usize) -> Result<()>`, `Node::history_count(&self) -> Result<usize>`.

- [ ] **Step 1: Failing tests** in `history.rs`:

```rust
    #[test]
    fn lowering_the_cap_removes_the_oldest_items_and_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        let src = Uuid::new_v4();
        let mut ids = vec![];
        for i in 0..30 {
            let it = item(src, &format!("n{i}"));
            ids.push(it.id);
            h.insert(&it).unwrap();
        }
        h.set_cap(20).unwrap();
        assert_eq!(h.count().unwrap(), 20);
        assert_eq!(h.load(ids[0]).unwrap(), None);
        assert!(h.load(ids[29]).unwrap().is_some());
        assert!(!dir.path().join("blobs").join(format!("{}.bin", ids[0])).exists());
        h.insert(&item(src, "new")).unwrap();
        assert_eq!(h.count().unwrap(), 20);
    }

    #[test]
    fn cap_outside_the_range_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::open(dir.path()).unwrap();
        assert!(h.set_cap(MIN_HISTORY - 1).is_err());
        assert!(h.set_cap(MAX_HISTORY + 1).is_err());
        assert_eq!(h.cap(), HISTORY_CAP);
    }
```

(Use the module's existing `item` helper; if created_ms ties make ordering ambiguous, keep the existing rowid tiebreak which preserves insertion order.)

Run `cargo test -p wisp-core history`; expected FAIL.

- [ ] **Step 2: Implement** `cap: usize` on `History` (default `HISTORY_CAP` in `open`), `prune` uses `self.cap`, `set_cap` validates, stores, prunes; `count` = `SELECT COUNT(*) FROM items`. `NodeConfig.history_cap` applied in `Node::start` right after opening history (invalid value: return the error). `Node::set_history_cap` and `history_count` lock `history` briefly (no await while held). Update all `NodeConfig` literals (tests and the app's `node_config` uses `wisp_core::history::HISTORY_CAP` until Task 4).

- [ ] **Step 3: Verify** `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 4: Commit** `git commit -m "Make the timeline size configurable at runtime"`

---

### Task 3: Offer without the clipboard, and the screenshots module

**Files:**
- Modify: `crates/wisp-core/Cargo.toml` (`notify = "8"`; `image` features add `jpeg`; `[target.'cfg(target_os = "macos")'.dependencies] xattr = "1"`), `crates/wisp-core/src/engine/mod.rs` (`Node::offer_local`), `crates/wisp-core/src/engine/session.rs` (`on_local` becomes `pub(super)`), `crates/wisp-core/src/lib.rs` (`pub mod screenshots;`)
- Create: `crates/wisp-core/src/screenshots.rs`
- Test: inline tests in `screenshots.rs`, `tests/two_nodes.rs`

**Interfaces:**
- Produces:
  - `Node::offer_local(&self, payload: ClipPayload)` (async): timeline item, `LocalItem` event and offers exactly like a local copy; never writes the clipboard, never touches the echo guard.
  - `screenshots::is_candidate(path: &Path) -> bool` (extension png/jpg/jpeg/heic, case-insensitive; file name not starting with `.`)
  - `screenshots::wait_until_stable(path: &Path, interval: Duration, max: Duration) -> Result<u64>` (async; size unchanged across two consecutive checks; error after `max`)
  - `screenshots::SeenFiles` with `new()` and `first_time(&mut self, path: &Path, modified: SystemTime) -> bool`
  - `screenshots::image_payload(bytes: &[u8]) -> Result<ClipPayload>` (decode with `image`, re-encode PNG unless already PNG, width/height from the decoded image)
  - `screenshots::ScreenshotWatcher::start(folder: PathBuf, is_screenshot: fn(&Path) -> bool, on_shot: Arc<dyn Fn(ClipPayload) + Send + Sync>) -> Result<ScreenshotWatcher>`; dropping it stops watching
  - macOS only: `screenshots::is_screenshot(path: &Path) -> bool` (xattr present), `screenshots::screenshot_folder() -> PathBuf` (`defaults read com.apple.screencapture location`, tilde expanded, fallback `$HOME/Desktop` when unset or missing)

- [ ] **Step 1: Failing tests**

`two_nodes.rs`:

```rust
#[tokio::test]
async fn offer_local_reaches_peers_without_touching_the_clipboard() {
    let (a, mut b) = paired().await;
    a.node.offer_local(ClipPayload::Text("from a screenshot".into())).await;
    wait_for(&mut b.events, "offer", |e| matches!(e, Event::OfferReceived(_))).await;
    assert_eq!(a.clip.current(), None);
    assert_eq!(a.node.history(&HistoryQuery::default()).unwrap().len(), 1);
}
```

`screenshots.rs` tests (cross-platform): `is_candidate` (png/JPG/heic yes, txt no, `.Screenshot.png` no); `wait_until_stable` returns the final size for a file written in two chunks 100 ms apart and errors for a file that keeps growing past `max`; `SeenFiles` true once then false, true again for a newer mtime; `image_payload` turns a small JPEG (encode one with `image` in the test) into a PNG payload with the right dimensions; `ScreenshotWatcher` with `is_screenshot = |_| true` on a temp dir: writing a PNG produces exactly one callback (collect via a channel), a `.tmp` file produces none, writing the same file event twice produces one. macOS-only (`#[cfg(target_os = "macos")]`): `is_screenshot` false for a plain temp PNG; `screenshot_folder()` returns an existing directory.

Run `cargo test -p wisp-core screenshots` / `offer_local`; expected FAIL.

- [ ] **Step 2: Implement**

- `Node::offer_local`: `self.inner.on_local(payload).await` (make `on_local` `pub(super)`).
- `screenshots.rs`: the functions above. The watcher: `notify::recommended_watcher` on `folder` (`RecursiveMode::NonRecursive`), events forwarded through a `tokio::sync::mpsc` channel to a task that, for create/modify/rename-to events of candidate paths: waits `wait_until_stable(path, 200 ms, 10 s)`, checks `is_screenshot(path)`, reads the file, checks `SeenFiles::first_time(path, mtime)`, builds `image_payload`, and calls `on_shot`. Failures are logged with `tracing::debug!` and skipped. The struct holds the notify watcher and the task's `JoinHandle` (abort on drop).
- macOS helpers behind `#[cfg(target_os = "macos")]` using `std::process::Command` and the `xattr` crate.

- [ ] **Step 3: Verify** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 4: Commit** `git commit -m "Add offering without the clipboard and a screenshot folder watcher"`

---

### Task 4: App wiring: settings, commands, engine and watcher

**Files:**
- Modify: `apps/wisp/src-tauri/src/settings.rs` (fields), `engine.rs` (`node_config` uses `history_size`), `commands.rs` (`rename_device`, `history_count`, `update_settings` applies history size and screenshot sharing), `lib.rs` (state for the watcher, start after engine start, `deviceRenamed` event)
- Create: `apps/wisp/src-tauri/src/screenshots.rs` (app-side service)
- Test: inline tests in `settings.rs`, `engine.rs`

**Interfaces:**
- Produces:
  - `AppSettings.history_size: u32` (default 200) and `AppSettings.share_screenshot_files: bool` (default true), camelCase `historySize`, `shareScreenshotFiles`.
  - Commands `rename_device(name: String) -> String` (calls `Node::rename`, stores `device_name` in settings under the settings lock, emits `{type:"deviceRenamed"}` on `wisp://event`), `history_count() -> usize`.
  - `update_settings`: when `history_size` changes, validate 20..=2000 and call `Node::set_history_cap` before saving (error -> not saved); when `share_screenshot_files` changes, start or stop the watcher.
  - `screenshots::ScreenshotService` in `AppState` (`Mutex<Option<ScreenshotWatcher>>`), `sync(app, enabled)`: on macOS with `enabled`, start a watcher on `wisp_core::screenshots::screenshot_folder()` whose callback spawns `app.state::<AppState>().engine.node().await` then `offer_local(payload)` (so engine restarts are transparent); otherwise drop it. Called after the engine starts, after onboarding, and on toggle. On a watcher start error, emit `{type:"error", message:"Wisp can't watch your screenshot folder."}`. Non-macOS builds compile the service as a no-op.

- [ ] **Step 1: Failing tests**: settings defaults include `history_size == 200` and `share_screenshot_files`; a partial settings file without the new keys loads them as defaults; `node_config` uses `history_size` as `history_cap`.

Run `cargo test -p wisp-app`; expected FAIL.

- [ ] **Step 2: Implement** as described. Keep std `Mutex` guards out of awaits; reuse the `settings_change` tokio mutex in `rename_device` and `update_settings`.

- [ ] **Step 3: Verify** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 4: Commit** `git commit -m "Wire rename, history size and screenshot sharing into the app"`

---

### Task 5: Settings view

**Files:**
- Modify: `apps/wisp/src/lib/types.ts` (`AppSettings.historySize`, `AppSettings.shareScreenshotFiles`, `EngineEvent` `deviceRenamed`), `apps/wisp/src/lib/api.ts` (`renameDevice`, `historyCount`), `apps/wisp/src/lib/store.ts` (`deviceRenamed` resyncs), `apps/wisp/src/views/SettingsView.vue`
- Test: `apps/wisp/src/__tests__/settings.spec.ts` (extend), `store.spec.ts`

**Interfaces:**
- Settings view sections:
  - "This device": name input prefilled with `state.deviceName`, Save disabled while empty or unchanged, friendly status ("Renamed", or the error).
  - "Timeline size": chips 50, 200, 500, 1,000 items (current highlighted). Choosing a size below `historyCount()` first shows an inline confirmation "This removes N older items." with Remove and Cancel; raising saves directly.
  - "Share screenshots saved as files" checkbox, only when `state.platform === 'macos'`.
- Saves go through the existing chained save queue.

- [ ] **Step 1: Failing tests**: renaming calls `api.renameDevice` with the trimmed name and Save is disabled for an empty name; picking 50 with `historyCount` 120 shows "This removes 70 older items." and only saves after Remove; picking 1,000 saves immediately; the screenshot toggle is hidden on Windows and shown on macOS; `reduce` returns true for `deviceRenamed`.

Run `cd apps/wisp && npm test`; expected FAIL.

- [ ] **Step 2: Implement** as described, matching the existing view patterns (script handlers, `pushNotice`, friendly copy, no dashes).

- [ ] **Step 3: Verify** `npm test`, `npm run build`, `npm run check-versions`.

- [ ] **Step 4: Commit** `git commit -m "Add rename, timeline size and screenshot sharing to Settings"`

---

## Done criteria

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `npm test`, `npm run build` pass on Windows; the macOS-only tests run in CI on macOS.
- Manual (with the owner): rename on one device shows on the other; lowering the timeline size asks and prunes; `Cmd+Shift+4` on the MacBook Air offers the screenshot on the Windows laptop, while another image saved to the Desktop is not offered.
