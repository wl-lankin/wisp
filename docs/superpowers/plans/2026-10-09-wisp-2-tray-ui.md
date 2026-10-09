# Wisp Tray App and UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `wisp-core` into a runnable Wisp desktop app: a Windows tray / macOS menu bar app with the constellation panel, catch toast, timeline, settings, first-run pairing, global hotkey and start at login.

**Architecture:** A Tauri 2 app at `apps/wisp` (Rust crate `wisp-app` in the workspace, Vue 3 + TypeScript + Tailwind 4 frontend). The Rust side owns the `wisp_core::engine::Node`, exposes thin Tauri commands, maps core types to UI DTOs (base64 thumbnails, smart preview kinds), forwards engine events to all windows, and manages the tray icon and five window kinds. The frontend is one Vite app; each window loads `index.html?view=<name>` and mounts the matching view. All state flows from one event reducer that is unit-tested without Tauri.

**Tech Stack:** Rust 1.98, Tauri 2 (tauri 2.x, plugins: single-instance, global-shortcut, autostart, positioner, dialog, opener), window-vibrancy 0.6, base64 0.22; Node 24, Vue 3.5, TypeScript 5, Vite 7, Tailwind CSS 4 (`@tailwindcss/vite`), `@tauri-apps/api` 2, highlight.js 11, Vitest 3 + @vue/test-utils + jsdom; fonts via Fontsource Geist (bundled, offline).

**Spec:** `docs/superpowers/specs/2026-10-09-wisp-design.md` (sections 2, 4, 5).
**Builds on:** Plan 1 (`docs/superpowers/plans/2026-10-09-wisp-1-core-engine.md`), branch `core-engine`. Work on a new branch `tray-ui` created from `core-engine`.
**Plan series:** 2 of 3. Plan 3 = final logo and icons + CI installers.

**Version drift:** versions above are targets. If an API differs in the resolved version, adapt at the call site, keep the interfaces this plan names, and note it in the report and commit message.

## Global Constraints

- Default hotkey: `Ctrl+Alt+V` on Windows and `Ctrl+Option+V` on macOS (both expressed as the Tauri shortcut string `Ctrl+Alt+V`); configurable. Never default to `Ctrl+Shift+V`.
- Catch toast: bottom right on Windows, top right on macOS; fades into the tray after 8 seconds; offers stay catchable from the panel; several offers from the same device stack ("3 items from MacBook").
- Tray panel: about 360 x 480 px, glass material; hides when it loses focus.
- Palette "aurora on ink": ink `#0B0D14`, accent gradient teal `#2DE2C5` to violet `#8B5CF6`; light theme follows the OS unless overridden.
- Type: Geist and Geist Mono, bundled locally (no network font loading).
- Reduced motion: remove large movements (orb springs, slide-ins) but keep short fades and hover transitions. The owner runs Windows with animations off; the UI must never feel choppy.
- Default save folder: `<Downloads>/Wisp`.
- The app has no dock icon on macOS; the panel, toast and onboarding windows never show a taskbar button on Windows (Timeline and Settings are normal windows while open). Closing windows never quits the app; only the tray menu "Quit Wisp" does.
- Only one instance runs; a second launch shows the panel of the first.
- Never put secrets in logs. Never use `#[allow(...)]` or `// @ts-ignore` to silence warnings.
- Writing style: no em-dashes or en-dashes anywhere (code, comments, UI copy, commits). UI copy is short, friendly English.
- Commit messages are plain sentences. Do not push.

## Review Focus

1. **Event storms:** `IdentityChanged` can be spoofed by any LAN device and fires on every mDNS resolve; the UI must dedupe it per device and present it as an unverified warning, not a modal. Pinned in Task 6.
2. **Panel focus loss vs drag and drop:** dragging a file from Explorer/Finder onto an orb must not hide the panel mid-drag. Pinned in Task 8.
3. **Hotkey conflicts:** registering a shortcut another app already owns must surface a readable error in Settings and keep the previous shortcut working. Pinned in Task 11.
4. **Huge previews:** a 50 MB image offer must not freeze the UI; only the 256 px thumbnail ever crosses into the webview. Pinned in Task 3.
5. **Engine restart:** changing the save folder restarts the engine; offers, peers and transfers in the UI must resync from commands, not keep stale state. Pinned in Task 4 and Task 6.

---

## File Structure

```
Cargo.toml                                  workspace: add "apps/wisp/src-tauri"
crates/wisp-core/src/...                    Task 2 adds small core follow-ups
apps/wisp/
  package.json, tsconfig.json, vite.config.ts, vitest.config.ts, index.html
  app-icon.svg                              placeholder swirl (Plan 3 replaces)
  src/
    main.ts                                 picks the view from ?view=
    styles.css                              tokens, themes, fonts, motion rules
    lib/
      types.ts                              DTO types mirrored from Rust
      api.ts                                typed invoke wrappers + event listener
      store.ts                              reactive app state + reducer
      constellation.ts                      orb layout math
      toasts.ts                             toast grouping
      format.ts                             sizes, ages, OS labels
    components/
      DeviceOrb.vue, Constellation.vue, ItemCard.vue, PreviewBody.vue,
      SearchField.vue, TransferRow.vue, IconGlyph.vue
    views/
      PanelView.vue, ToastView.vue, TimelineView.vue, SettingsView.vue,
      OnboardingView.vue, PairingCard.vue
    __tests__/                              Vitest specs
  src-tauri/
    Cargo.toml, build.rs, tauri.conf.json
    capabilities/default.json
    icons/                                  generated from app-icon.svg
    src/
      main.rs                               entry: wisp_app::run()
      lib.rs                                builder, plugins, setup, run loop
      settings.rs                           AppSettings load/save
      dto.rs                                core -> UI DTOs, smart text kinds
      engine.rs                             EngineHandle: start/restart Node, event pump
      commands.rs                           #[tauri::command] functions
      windows.rs                            create/show/position windows, vibrancy
      tray.rs                               tray icon, menu, icon states
      hotkey.rs                             global shortcut registration
```

---

### Task 1: Scaffold the Tauri app in the workspace

**Files:**
- Modify: `Cargo.toml` (workspace members), `.gitignore`
- Create: everything under `apps/wisp/` listed below

**Interfaces:**
- Produces: a building Tauri crate `wisp-app` with `pub fn run()`; a Vite app whose `main.ts` mounts a view chosen by the `view` query parameter (`panel`, `toast`, `timeline`, `settings`, `onboarding`); placeholder views that render their name; Vitest configured with the Vue plugin and jsdom.

- [ ] **Step 1: Check the branch**

Run: `git branch --show-current`
Expected: `tray-ui` (created from `core-engine`; this plan is committed there).

- [ ] **Step 2: Workspace and ignore rules**

`Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/wisp-core", "apps/wisp/src-tauri"]
```

Append to `.gitignore`:

```
apps/wisp/node_modules/
apps/wisp/dist/
apps/wisp/src-tauri/gen/
```

- [ ] **Step 3: Frontend package files**

`apps/wisp/package.json`:

```json
{
  "name": "wisp",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "test": "vitest run",
    "tauri": "tauri"
  },
  "dependencies": {
    "@fontsource-variable/geist": "^5.1.0",
    "@fontsource-variable/geist-mono": "^5.1.0",
    "@tauri-apps/api": "^2.8.0",
    "highlight.js": "^11.11.0",
    "vue": "^3.5.0"
  },
  "devDependencies": {
    "@tailwindcss/vite": "^4.1.0",
    "@tauri-apps/cli": "^2.8.0",
    "@vitejs/plugin-vue": "^6.0.0",
    "@vue/test-utils": "^2.4.0",
    "jsdom": "^26.0.0",
    "tailwindcss": "^4.1.0",
    "typescript": "^5.8.0",
    "vite": "^7.0.0",
    "vitest": "^3.2.0",
    "vue-tsc": "^3.0.0"
  }
}
```

If a package name does not resolve (check with `npm view <name> version`), use the closest official Fontsource package for Geist / Geist Mono and note it.

`apps/wisp/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "jsx": "preserve",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "types": ["vite/client"],
    "skipLibCheck": true,
    "noEmit": true,
    "isolatedModules": true,
    "resolveJsonModule": true
  },
  "include": ["src/**/*.ts", "src/**/*.vue"]
}
```

`apps/wisp/vite.config.ts`:

```ts
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: 'es2022' },
})
```

`apps/wisp/vitest.config.ts`:

```ts
import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  test: { environment: 'jsdom', include: ['src/__tests__/**/*.spec.ts'] },
})
```

`apps/wisp/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Wisp</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

`apps/wisp/src/env.d.ts`:

```ts
declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<object, object, unknown>
  export default component
}
```

- [ ] **Step 4: View switch and placeholder views**

`apps/wisp/src/main.ts`:

```ts
import { createApp, type Component } from 'vue'
import './styles.css'
import PanelView from './views/PanelView.vue'
import ToastView from './views/ToastView.vue'
import TimelineView from './views/TimelineView.vue'
import SettingsView from './views/SettingsView.vue'
import OnboardingView from './views/OnboardingView.vue'

export const views: Record<string, Component> = {
  panel: PanelView,
  toast: ToastView,
  timeline: TimelineView,
  settings: SettingsView,
  onboarding: OnboardingView,
}

export function viewFor(search: string): Component {
  const name = new URLSearchParams(search).get('view') ?? 'panel'
  return views[name] ?? PanelView
}

document.documentElement.dataset.view = new URLSearchParams(location.search).get('view') ?? 'panel'
createApp(viewFor(location.search)).mount('#app')
```

Each of `PanelView.vue`, `ToastView.vue`, `TimelineView.vue`, `SettingsView.vue`, `OnboardingView.vue` in `src/views/` starts as a placeholder (later tasks replace them). Example for the panel; the others are identical except the text:

```vue
<template>
  <main class="p-4 text-sm">Panel</main>
</template>
```

`apps/wisp/src/styles.css` starts as:

```css
@import 'tailwindcss';
```

- [ ] **Step 5: Write the failing view-switch test**

`apps/wisp/src/__tests__/main.spec.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'

vi.mock('../styles.css', () => ({}))

describe('viewFor', () => {
  it('picks the view from the query and falls back to the panel', async () => {
    document.body.innerHTML = '<div id="app"></div>'
    const { viewFor, views } = await import('../main')
    expect(viewFor('?view=timeline')).toBe(views.timeline)
    expect(viewFor('?view=nope')).toBe(views.panel)
    expect(viewFor('')).toBe(views.panel)
  })
})
```

Run: `cd apps/wisp && npm install && npm test`
Expected before Step 4 exists: FAIL (module not found). After Step 4: PASS.

- [ ] **Step 6: Placeholder icon and Tauri crate**

`apps/wisp/app-icon.svg` (placeholder swirl; Plan 3 replaces it):

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#2DE2C5"/>
      <stop offset="1" stop-color="#8B5CF6"/>
    </linearGradient>
  </defs>
  <rect width="512" height="512" rx="112" fill="#0B0D14"/>
  <path d="M150 300c0-80 70-140 150-120 60 15 80 85 30 115-40 24-90-5-75-45 10-25 45-30 55-10"
        fill="none" stroke="url(#g)" stroke-width="44" stroke-linecap="round"/>
</svg>
```

`apps/wisp/src-tauri/Cargo.toml`:

```toml
[package]
name = "wisp-app"
version = "0.1.0"
edition = "2021"

[lib]
name = "wisp_app"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
base64 = "0.22"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2", features = ["tray-icon", "image-png", "macos-private-api"] }
tauri-plugin-autostart = "2"
tauri-plugin-dialog = "2"
tauri-plugin-global-shortcut = "2"
tauri-plugin-opener = "2"
tauri-plugin-positioner = { version = "2", features = ["tray-icon"] }
tauri-plugin-single-instance = "2"
thiserror = "2"
tokio = { version = "1", features = ["sync", "time", "macros", "rt-multi-thread"] }
tracing = "0.1"
uuid = { version = "1", features = ["serde"] }
window-vibrancy = "0.6"
wisp-core = { path = "../../../crates/wisp-core" }

[dev-dependencies]
tempfile = "3"
```

`apps/wisp/src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

`apps/wisp/src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Wisp",
  "version": "0.1.0",
  "identifier": "io.github.wl-lankin.wisp",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:1420",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [],
    "macOSPrivateApi": true,
    "security": {
      "csp": "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"]
  }
}
```

`apps/wisp/src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["*"],
  "permissions": [
    "core:default",
    "core:window:allow-hide",
    "core:window:allow-show",
    "core:window:allow-set-focus",
    "core:window:allow-start-dragging",
    "core:window:allow-close"
  ]
}
```

`apps/wisp/src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    wisp_app::run()
}
```

`apps/wisp/src-tauri/src/lib.rs` (minimal; later tasks grow it):

```rust
pub fn run() {
    tauri::Builder::default()
        .build(tauri::generate_context!())
        .expect("failed to build the Wisp app")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
```

Generate icons:

```bash
cd apps/wisp && npx tauri icon app-icon.svg
```

Expected: `src-tauri/icons/` contains `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`, `icon.png` and more. If the CLI cannot read SVG, render the SVG to a 1024 px PNG first (any tool) and run `npx tauri icon app-icon.png`; commit both sources.

- [ ] **Step 7: Verify both halves build**

Run: `cd apps/wisp && npm run build && npm test`
Expected: Vite build succeeds, 1 test passes.

Run: `cargo build -p wisp-app` (first build downloads Tauri; allow 10 minutes)
Expected: success, no warnings.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore apps/wisp
git commit -m "Scaffold the Wisp Tauri app"
```

---

### Task 2: Core follow-ups the app needs

Small `wisp-core` changes deferred from Plan 1 (see `docs/superpowers/followups/2026-10-09-plan-1-deferred.md`) plus one new API for "Save as file".

**Files:**
- Modify: `crates/wisp-core/src/engine/mod.rs` (new `Node::export_item`), `crates/wisp-core/src/transfer.rs` (`reserve_path` becomes `pub(crate)`), `crates/wisp-core/src/policy.rs` (inbox cap, `OfferQueue::is_empty_for`), `crates/wisp-core/src/engine/session.rs` (read uni streams to EOF, single delivery task per peer), `crates/wisp-core/src/engine/links.rs` (dialer drains queues of connected peers), `crates/wisp-core/src/clipboard.rs` (monitor reads on the blocking pool)
- Test: `crates/wisp-core/tests/two_nodes.rs`, inline tests in `policy.rs` and `engine/mod.rs`

**Interfaces:**
- Produces:
  - `Node::export_item(&self, id: Uuid, dir: &Path) -> Result<PathBuf>`: writes the timeline item as `wisp-<created_ms>.<txt|html|png>` into `dir` using the same atomic name reservation as received files; returns the path.
  - `policy::INBOX_CAP: usize = 100`; `Inbox::add` drops the oldest entries beyond the cap.
  - `OfferQueue::is_empty_for(&self, peer: Uuid) -> bool`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/wisp-core/tests/two_nodes.rs`:

```rust
#[tokio::test]
async fn timeline_items_export_to_files() {
    let mut a = start("Mac").await;
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
```

Append inside the test module of `crates/wisp-core/src/policy.rs`:

```rust
    #[test]
    fn inbox_keeps_only_the_newest_entries() {
        let mut inbox = Inbox::new();
        let peer = Uuid::new_v4();
        let mut last = Uuid::nil();
        for _ in 0..(INBOX_CAP + 5) {
            last = Uuid::new_v4();
            inbox.add(InboxEntry {
                peer,
                item_id: last,
                kind: ItemKind::Text,
                size: 1,
                preview: Preview::Text("x".into()),
                received: Instant::now(),
            });
        }
        assert_eq!(inbox.all().len(), INBOX_CAP);
        assert_eq!(inbox.latest().unwrap().item_id, last);
    }

    #[test]
    fn queue_reports_whether_a_peer_has_offers() {
        let mut q = OfferQueue::new();
        let peer = Uuid::new_v4();
        assert!(q.is_empty_for(peer));
        q.push(peer, &text_item("x"));
        assert!(!q.is_empty_for(peer));
        q.drain(peer);
        assert!(q.is_empty_for(peer));
    }
```

(Use the existing `text_item` helper and imports of that test module; add `Instant` to the imports if missing.)

In the test module of `crates/wisp-core/src/engine/mod.rs`, add a test that puts an offer into `a`'s queue for `b` while both are connected (call the queue directly through the node's inner state, the way the existing engine unit tests reach inner state) and asserts that `b` receives `Event::OfferReceived` within 3 seconds without any reconnect. Name it `queued_offers_for_connected_peers_are_delivered`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-core`
Expected: compile errors (`export_item`, `INBOX_CAP`, `is_empty_for` not found).

- [ ] **Step 3: Implement**

`transfer.rs`: change `async fn reserve_path` to `pub(crate) async fn reserve_path`.

`policy.rs`:

```rust
pub const INBOX_CAP: usize = 100;
```

In `Inbox::add`, after `self.entries.push(entry);` add:

```rust
        if self.entries.len() > INBOX_CAP {
            let excess = self.entries.len() - INBOX_CAP;
            self.entries.drain(..excess);
        }
```

In `impl OfferQueue`:

```rust
    pub fn is_empty_for(&self, peer: Uuid) -> bool {
        self.queues.get(&peer).map_or(true, |q| q.is_empty())
    }
```

(If clippy prefers `is_none_or`, use it.)

`engine/mod.rs`, in `impl Node`:

```rust
    /// Writes a timeline item to `dir` as a file and returns the path it got.
    pub async fn export_item(&self, id: Uuid, dir: &Path) -> Result<PathBuf> {
        let item = self.inner.history.lock().unwrap().load(id)?;
        let item = item.ok_or_else(|| CoreError::Transfer("item is no longer in the timeline".into()))?;
        let (ext, bytes) = match item.payload {
            ClipPayload::Text(t) => ("txt", t.into_bytes()),
            ClipPayload::Html { html, .. } => ("html", html.into_bytes()),
            ClipPayload::Image { png, .. } => ("png", png),
        };
        tokio::fs::create_dir_all(dir).await?;
        let wanted = dir.join(format!("wisp-{}.{ext}", item.created_ms));
        let path = crate::transfer::reserve_path(&wanted).await?;
        tokio::fs::write(&path, bytes).await?;
        Ok(path)
    }
```

(Add `use std::path::Path;` and `use crate::item::ClipPayload;` if not imported.)

`engine/session.rs`, in `handle_uni`: read the frame, then wait for the stream's end before acting, so the sender's acknowledgement never races a STOP_SENDING:

```rust
        let msg = read_frame(&mut recv).await?;
        // Consume the FIN so the sender sees a clean finish instead of STOP_SENDING.
        let _ = recv.read_to_end(0).await;
        match msg {
```

(replacing `match read_frame(&mut recv).await? {`).

Single delivery task per peer: add `delivering: Mutex<HashSet<Uuid>>` to `Inner` (initialize empty in `Node::start`) and a helper in `session.rs`:

```rust
    pub(super) fn spawn_delivery(self: &Arc<Self>, peer: Uuid, conn: quinn::Connection) {
        if !self.delivering.lock().unwrap().insert(peer) {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            this.clone().deliver_queued(peer, conn).await;
            this.delivering.lock().unwrap().remove(&peer);
        });
    }
```

Replace the existing direct `tokio::spawn(... deliver_queued ...)` call in `register` with `self.spawn_delivery(peer, conn.clone())`.

`engine/links.rs`, in the dialer loop, after dialing offline peers:

```rust
                let waiting: Vec<Uuid> = {
                    let peers = self.peers.lock().unwrap();
                    let conns = self.conns.lock().unwrap();
                    let queue = self.queue.lock().unwrap();
                    peers
                        .all()
                        .iter()
                        .map(|p| p.device_id)
                        .filter(|id| conns.contains_key(id) && !queue.is_empty_for(*id))
                        .collect()
                };
                for peer in waiting {
                    if let Some(conn) = self.connection(peer) {
                        self.spawn_delivery(peer, conn);
                    }
                }
```

(`queue` is a leaf lock; taking it after `conns` keeps the documented order.)

`clipboard.rs`, in `spawn_monitor`, replace the direct `backend.read()` with a read on the blocking pool:

```rust
            let reader = backend.clone();
            let read = tokio::task::spawn_blocking(move || reader.read())
                .await
                .unwrap_or_else(|e| Err(CoreError::Clipboard(e.to_string())));
            let payload = match read {
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p wisp-core` then `cargo test -p wisp-core --test two_nodes` two more times, then `cargo clippy -p wisp-core --all-targets -- -D warnings`
Expected: all pass, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/wisp-core
git commit -m "Add item export and close the queue and blocking-read follow-ups"
```

---

### Task 3: App settings and UI data shapes

**Files:**
- Create: `apps/wisp/src-tauri/src/settings.rs`, `apps/wisp/src-tauri/src/dto.rs`
- Modify: `apps/wisp/src-tauri/src/lib.rs` (`mod settings; mod dto;`)
- Test: inline test modules

**Interfaces:**
- Produces:
  - `settings::DEFAULT_HOTKEY: &str = "Ctrl+Alt+V"`
  - `settings::Theme { System, Dark, Light }` (serde lowercase)
  - `settings::AppSettings { device_name: Option<String>, save_dir: Option<PathBuf>, hotkey: String, autostart: bool, theme: Theme, onboarded: bool }` (serde camelCase, `#[serde(default)]`), `AppSettings::load(path: &Path) -> AppSettings` (missing or corrupt file gives defaults), `AppSettings::save(&self, path: &Path) -> Result<(), String>` (temp file + rename)
  - `dto::SmartKind { Plain, Link { domain }, Color { value }, Code }` (serde tag `type`)
  - `dto::PreviewDto { Text { text, smart }, Image { data_url, width, height }, File { name } }` (serde tag `kind`, camelCase)
  - `dto::classify(text: &str) -> SmartKind`, `dto::preview_dto(p: &Preview) -> PreviewDto`, `dto::with_ui_preview<T: Serialize>(value: &T, preview: &Preview) -> serde_json::Value`, `dto::event_to_ui(e: &Event) -> serde_json::Value`

- [ ] **Step 1: Write the failing settings tests** (new `settings.rs`, test module first)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let s = AppSettings::default();
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert!(s.autostart);
        assert_eq!(s.theme, Theme::System);
        assert!(!s.onboarded);
        assert_eq!(s.device_name, None);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = AppSettings {
            device_name: Some("Wolfgang's PC".into()),
            save_dir: Some(dir.path().join("in")),
            hotkey: "Ctrl+Shift+Space".into(),
            autostart: false,
            theme: Theme::Light,
            onboarded: true,
        };
        s.save(&path).unwrap();
        assert_eq!(AppSettings::load(&path), s);
    }

    #[test]
    fn missing_corrupt_or_partial_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(AppSettings::load(&path), AppSettings::default());
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(AppSettings::load(&path), AppSettings::default());
        std::fs::write(&path, r#"{"onboarded":true,"futureField":1}"#).unwrap();
        let s = AppSettings::load(&path);
        assert!(s.onboarded);
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
    }
}
```

- [ ] **Step 2: Implement `settings.rs`** (above the tests)

```rust
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+V";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub device_name: Option<String>,
    pub save_dir: Option<PathBuf>,
    pub hotkey: String,
    pub autostart: bool,
    pub theme: Theme,
    pub onboarded: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            device_name: None,
            save_dir: None,
            hotkey: DEFAULT_HOTKEY.to_string(),
            autostart: true,
            theme: Theme::System,
            onboarded: false,
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!("settings file unreadable, using defaults: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}
```

- [ ] **Step 3: Write the failing DTO tests** (new `dto.rs`, test module first)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use wisp_core::engine::OfferInfo;
    use wisp_core::item::ItemKind;

    #[test]
    fn classifies_links_colors_code_and_plain_text() {
        assert_eq!(
            classify("https://www.example.org/path?q=1"),
            SmartKind::Link { domain: "www.example.org".into() }
        );
        assert_eq!(classify("  http://192.168.1.20:8080/x  "), SmartKind::Link { domain: "192.168.1.20".into() });
        assert_eq!(classify("#2DE2C5"), SmartKind::Color { value: "#2DE2C5".into() });
        assert_eq!(classify("#abc"), SmartKind::Color { value: "#abc".into() });
        assert_eq!(classify("rgb(45, 226, 197)"), SmartKind::Color { value: "rgb(45, 226, 197)".into() });
        assert_eq!(classify("fn main() {\n    println!(\"hi\");\n}"), SmartKind::Code);
        assert_eq!(classify("const a = 1;\nconst b = 2;"), SmartKind::Code);
        assert_eq!(classify("Meeting at 3, bring snacks"), SmartKind::Plain);
        assert_eq!(classify("see https://example.org for details"), SmartKind::Plain);
        assert_eq!(classify("#hashtag"), SmartKind::Plain);
    }

    #[test]
    fn thumbnails_become_data_urls() {
        let p = Preview::Thumbnail { png: vec![1, 2, 3], width: 10, height: 5 };
        assert_eq!(
            preview_dto(&p),
            PreviewDto::Image { data_url: "data:image/png;base64,AQID".into(), width: 10, height: 5 }
        );
    }

    #[test]
    fn offer_events_carry_ui_previews_and_no_raw_bytes() {
        let info = OfferInfo {
            peer: Uuid::new_v4(),
            item_id: Uuid::new_v4(),
            kind: ItemKind::Image,
            size: 50_000_000,
            preview: Preview::Thumbnail { png: vec![9; 64], width: 256, height: 128 },
            age_ms: 0,
        };
        let v = event_to_ui(&Event::OfferReceived(info.clone()));
        assert_eq!(v["type"], "offerReceived");
        assert_eq!(v["itemId"], info.item_id.to_string());
        assert_eq!(v["preview"]["kind"], "image");
        assert!(v["preview"]["dataUrl"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert!(!v.to_string().contains("\"png\""));
    }

    #[test]
    fn other_events_pass_through_unchanged() {
        let peer = Uuid::new_v4();
        let v = event_to_ui(&Event::PeerOffline { peer });
        assert_eq!(v, serde_json::json!({ "type": "peerOffline", "peer": peer.to_string() }));
    }
}
```

- [ ] **Step 4: Run to verify failure**

Run: `cargo test -p wisp-app`
Expected: compile errors (`AppSettings`, `classify` not found) until both modules are implemented.

- [ ] **Step 5: Implement `dto.rs`** (above the tests)

```rust
use base64::Engine as _;
use serde::Serialize;
use serde_json::Value;
use wisp_core::engine::Event;
use wisp_core::item::Preview;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SmartKind {
    Plain,
    Link { domain: String },
    Color { value: String },
    Code,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PreviewDto {
    Text { text: String, smart: SmartKind },
    Image { data_url: String, width: u32, height: u32 },
    File { name: String },
}

const CODE_MARKERS: [&str; 14] = [
    ";\n", "{\n", "}", "=>", "fn ", "function ", "def ", "class ", "import ", "const ", "let ", "</", "#include", "<?php",
];

pub fn classify(text: &str) -> SmartKind {
    let t = text.trim();
    if let Some(domain) = link_domain(t) {
        return SmartKind::Link { domain };
    }
    if is_color(t) {
        return SmartKind::Color { value: t.to_string() };
    }
    if t.lines().count() >= 2 && CODE_MARKERS.iter().filter(|m| t.contains(*m)).count() >= 2 {
        return SmartKind::Code;
    }
    SmartKind::Plain
}

fn link_domain(t: &str) -> Option<String> {
    if t.chars().any(char::is_whitespace) {
        return None;
    }
    let rest = t.strip_prefix("https://").or_else(|| t.strip_prefix("http://"))?;
    let host_port = rest.split(['/', '?', '#']).next()?;
    let host = host_port.rsplit_once(':').map_or(host_port, |(h, _)| h);
    (!host.is_empty()).then(|| host.to_lowercase())
}

fn is_color(t: &str) -> bool {
    if let Some(hex) = t.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    let lower = t.to_ascii_lowercase();
    ["rgb(", "rgba(", "hsl(", "hsla("].iter().any(|p| lower.starts_with(p)) && lower.ends_with(')')
}

pub fn preview_dto(p: &Preview) -> PreviewDto {
    match p {
        Preview::Text(text) => PreviewDto::Text { text: text.clone(), smart: classify(text) },
        Preview::Thumbnail { png, width, height } => PreviewDto::Image {
            data_url: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)),
            width: *width,
            height: *height,
        },
        Preview::File { name } => PreviewDto::File { name: name.clone() },
    }
}

/// Serializes `value` and swaps its raw `preview` field for the UI preview.
pub fn with_ui_preview<T: Serialize>(value: &T, preview: &Preview) -> Value {
    let mut v = serde_json::to_value(value).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut v {
        map.insert("preview".into(), serde_json::to_value(preview_dto(preview)).unwrap_or(Value::Null));
    }
    v
}

pub fn event_to_ui(e: &Event) -> Value {
    match e {
        Event::OfferReceived(info) => with_ui_preview(e, &info.preview),
        other => serde_json::to_value(other).unwrap_or(Value::Null),
    }
}
```

Note the link test: `"http://192.168.1.20:8080/x"` keeps the host without port; IPv6 literal hosts are out of scope.

Add `mod dto; mod settings;` to `lib.rs` (keep them private to the crate; mark items `pub` inside).

- [ ] **Step 6: Run tests**

Run: `cargo test -p wisp-app`
Expected: 7 passed. `dead_code` warnings for items nothing uses yet are expected until Task 4 wires them in; do not add `#[allow]`, just list them in the report.

- [ ] **Step 7: Commit**

```bash
git add apps/wisp/src-tauri
git commit -m "Add app settings and UI data shapes"
```

---

### Task 4: Engine handle, event pump and commands

**Files:**
- Create: `apps/wisp/src-tauri/src/engine.rs`, `apps/wisp/src-tauri/src/commands.rs`
- Modify: `apps/wisp/src-tauri/src/lib.rs` (state, setup, command registration), `apps/wisp/src-tauri/Cargo.toml` (add `gethostname = "1"`)
- Test: inline test module in `engine.rs`

**Interfaces:**
- Consumes: `AppSettings`, `event_to_ui`, `with_ui_preview` (Task 3); `Node`, `NodeConfig`, `Event`, `SystemClipboard`, `ClipboardBackend`, `HistoryQuery`, `DeliveryMode`, `ItemKind` (core); `Node::export_item` (Task 2)
- Produces:
  - `engine::DEFAULT_PORT: u16 = 47800`
  - `engine::EventSink` trait: `fn emit(&self, value: serde_json::Value)`, `fn react(&self, event: &Event)`
  - `engine::pump(rx, sink: Arc<dyn EventSink>)` (async)
  - `engine::Paths { data_dir: PathBuf, default_save_dir: PathBuf }`
  - `engine::node_config(settings, paths, clipboard, port) -> NodeConfig`, `engine::default_device_name() -> String`, `engine::parse_peer_addr(&str) -> Result<SocketAddr, String>`
  - `engine::Engine` (Default): `start(&self, settings, paths, sink) -> Result<(), String>` (restarts if running; keeps one system clipboard for the app's lifetime; tries port 47800 a few times, then a random port), `node(&self) -> Result<Node, String>`, `stop(&self)`
  - `lib.rs`: `pub struct AppState { engine: Engine, settings: std::sync::Mutex<AppSettings>, paths: Paths, settings_path: PathBuf, dragging: AtomicBool }`, `struct TauriSink { app: AppHandle }` implementing `EventSink` (emits `"wisp://event"`), `async fn start_engine(app: &AppHandle)`
  - Commands (all return `Result<T, String>`): `snapshot`, `get_settings`, `update_settings`, `complete_onboarding`, `pick_folder`, `begin_pairing`, `cancel_pairing`, `pair_with`, `set_mode`, `forget`, `catch`, `catch_latest`, `dismiss`, `save_offer`, `history`, `copy_from_history`, `export_item`, `send_clipboard_to`, `send_files`, `add_address`
  - Frontend event channel names: `"wisp://event"` (engine events), `"wisp://settings"` (settings changed, payload = AppSettings)

- [ ] **Step 1: Write the failing tests** (new `engine.rs`, test module first)

```rust
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
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-app engine`
Expected: compile errors (`parse_peer_addr` etc. not found).

- [ ] **Step 3: Implement `engine.rs`** (above the tests)

```rust
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, Mutex};
use wisp_core::clipboard::{ClipboardBackend, SystemClipboard};
use wisp_core::engine::{Event, Node, NodeConfig};

use crate::dto::event_to_ui;
use crate::settings::AppSettings;

pub const DEFAULT_PORT: u16 = 47800;
const PORT_RETRIES: u32 = 5;

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

#[derive(Default)]
pub struct Engine {
    node: Mutex<Option<Node>>,
    clipboard: Mutex<Option<Arc<dyn ClipboardBackend>>>,
}

impl Engine {
    pub async fn start(&self, settings: &AppSettings, paths: &Paths, sink: Arc<dyn EventSink>) -> Result<(), String> {
        let mut slot = self.node.lock().await;
        if let Some(old) = slot.take() {
            old.shutdown();
        }
        let clipboard = self.clipboard().await?;
        let mut attempt = 0;
        let (node, rx) = loop {
            match Node::start(node_config(settings, paths, clipboard.clone(), DEFAULT_PORT)).await {
                Ok(started) => break started,
                Err(e) if attempt < PORT_RETRIES => {
                    attempt += 1;
                    tracing::debug!("port {DEFAULT_PORT} busy ({e}), retrying");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                Err(e) => {
                    tracing::warn!("port {DEFAULT_PORT} unavailable ({e}), using a random port");
                    break Node::start(node_config(settings, paths, clipboard.clone(), 0))
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
        };
        tauri::async_runtime::spawn(pump(rx, sink));
        *slot = Some(node);
        Ok(())
    }

    pub async fn node(&self) -> Result<Node, String> {
        self.node.lock().await.clone().ok_or_else(|| "Wisp is still starting".to_string())
    }

    pub async fn stop(&self) {
        if let Some(node) = self.node.lock().await.take() {
            node.shutdown();
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
```

- [ ] **Step 4: Implement `lib.rs` state and setup**

```rust
mod commands;
mod dto;
mod engine;
mod settings;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use wisp_core::engine::Event;

use crate::engine::{Engine, EventSink, Paths};
use crate::settings::AppSettings;

pub struct AppState {
    pub engine: Engine,
    pub settings: Mutex<AppSettings>,
    pub paths: Paths,
    pub settings_path: PathBuf,
    /// True while a file drag hovers a Wisp window, so the panel does not hide mid-drag.
    pub dragging: AtomicBool,
}

struct TauriSink {
    app: AppHandle,
}

impl EventSink for TauriSink {
    fn emit(&self, value: serde_json::Value) {
        let _ = self.app.emit("wisp://event", value);
    }

    fn react(&self, _event: &Event) {}
}

pub(crate) async fn start_engine(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let sink = Arc::new(TauriSink { app: app.clone() });
    if let Err(message) = state.engine.start(&settings, &state.paths, sink).await {
        let _ = app.emit("wisp://event", serde_json::json!({ "type": "error", "message": message }));
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let default_save_dir = app.path().download_dir()?.join("Wisp");
            let settings_path = data_dir.join("settings.json");
            let settings = AppSettings::load(&settings_path);
            let onboarded = settings.onboarded;
            app.manage(AppState {
                engine: Engine::default(),
                settings: Mutex::new(settings),
                paths: Paths { data_dir, default_save_dir },
                settings_path,
                dragging: AtomicBool::new(false),
            });
            if onboarded {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { start_engine(&handle).await });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::get_settings,
            commands::update_settings,
            commands::complete_onboarding,
            commands::pick_folder,
            commands::begin_pairing,
            commands::cancel_pairing,
            commands::pair_with,
            commands::set_mode,
            commands::forget,
            commands::catch,
            commands::catch_latest,
            commands::dismiss,
            commands::save_offer,
            commands::history,
            commands::copy_from_history,
            commands::export_item,
            commands::send_clipboard_to,
            commands::send_files,
            commands::add_address,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the Wisp app")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
```

(`TauriSink::react` stays empty here on purpose; Task 5 fills it with toast and tray reactions.)

- [ ] **Step 5: Implement `commands.rs`**

```rust
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;
use wisp_core::engine::{NearbyDevice, PeerStatus};
use wisp_core::history::HistoryQuery;
use wisp_core::item::ItemKind;
use wisp_core::peers::DeliveryMode;

use crate::dto::with_ui_preview;
use crate::engine::parse_peer_addr;
use crate::settings::AppSettings;
use crate::AppState;

type Cmd<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    device_id: Uuid,
    device_name: String,
    platform: &'static str,
    port: u16,
    peers: Vec<PeerStatus>,
    nearby: Vec<NearbyDevice>,
    inbox: Vec<Value>,
}

#[tauri::command]
pub async fn snapshot(state: State<'_, AppState>) -> Cmd<Snapshot> {
    let node = state.engine.node().await?;
    Ok(Snapshot {
        device_id: node.device_id(),
        device_name: node.device_name(),
        platform: std::env::consts::OS,
        port: node.local_addr().map_err(err)?.port(),
        peers: node.peers(),
        nearby: node.nearby(),
        inbox: node.inbox().iter().map(|o| with_ui_preview(o, &o.preview)).collect(),
    })
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

fn save_settings(app: &AppHandle, state: &AppState, next: AppSettings) -> Cmd<AppSettings> {
    next.save(&state.settings_path)?;
    *state.settings.lock().unwrap() = next.clone();
    let _ = app.emit("wisp://settings", &next);
    Ok(next)
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, state: State<'_, AppState>, next: AppSettings) -> Cmd<AppSettings> {
    let previous = state.settings.lock().unwrap().clone();
    let saved = save_settings(&app, &state, next)?;
    if saved.onboarded && saved.save_dir != previous.save_dir {
        crate::start_engine(&app).await;
    }
    Ok(saved)
}

#[tauri::command]
pub async fn complete_onboarding(app: AppHandle, state: State<'_, AppState>, device_name: String) -> Cmd<()> {
    let name = device_name.trim();
    if name.is_empty() {
        return Err("Please give this device a name".into());
    }
    let mut next = state.settings.lock().unwrap().clone();
    next.device_name = Some(name.chars().take(64).collect());
    next.onboarded = true;
    save_settings(&app, &state, next)?;
    crate::start_engine(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Cmd<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
        .await
        .map_err(err)?;
    Ok(picked.map(|p| p.to_string()))
}

#[tauri::command]
pub async fn begin_pairing(state: State<'_, AppState>) -> Cmd<String> {
    Ok(state.engine.node().await?.begin_pairing())
}

#[tauri::command]
pub async fn cancel_pairing(state: State<'_, AppState>) -> Cmd<()> {
    state.engine.node().await?.cancel_pairing();
    Ok(())
}

#[tauri::command]
pub async fn pair_with(state: State<'_, AppState>, addr: String, code: String) -> Cmd<Value> {
    let addr = parse_peer_addr(&addr)?;
    let peer = state.engine.node().await?.pair_with(addr, code.trim()).await.map_err(err)?;
    serde_json::to_value(peer).map_err(err)
}

#[tauri::command]
pub async fn set_mode(state: State<'_, AppState>, peer: Uuid, mode: DeliveryMode) -> Cmd<()> {
    state.engine.node().await?.set_mode(peer, mode).map_err(err)
}

#[tauri::command]
pub async fn forget(state: State<'_, AppState>, peer: Uuid) -> Cmd<()> {
    state.engine.node().await?.forget(peer).map_err(err)
}

#[tauri::command]
pub async fn catch(state: State<'_, AppState>, item_id: Uuid) -> Cmd<()> {
    state.engine.node().await?.catch(item_id).await.map_err(err)
}

#[tauri::command]
pub async fn catch_latest(state: State<'_, AppState>) -> Cmd<Option<Uuid>> {
    state.engine.node().await?.catch_latest().await.map_err(err)
}

#[tauri::command]
pub async fn dismiss(state: State<'_, AppState>, item_id: Uuid) -> Cmd<()> {
    state.engine.node().await?.dismiss(item_id);
    Ok(())
}

fn save_dir(state: &AppState) -> PathBuf {
    let settings = state.settings.lock().unwrap();
    settings.save_dir.clone().unwrap_or_else(|| state.paths.default_save_dir.clone())
}

/// Catches an offer, waits until it is in the timeline, writes it to the save folder and reveals it.
#[tauri::command]
pub async fn save_offer(app: AppHandle, state: State<'_, AppState>, item_id: Uuid) -> Cmd<String> {
    let node = state.engine.node().await?;
    node.catch(item_id).await.map_err(err)?;
    let dir = save_dir(&state);
    for _ in 0..50 {
        if let Ok(path) = node.export_item(item_id, &dir).await {
            let _ = app.opener().reveal_item_in_dir(&path);
            return Ok(path.to_string_lossy().into_owned());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("The item did not arrive in time. Try again.".into())
}

#[tauri::command]
pub async fn history(
    state: State<'_, AppState>,
    text: Option<String>,
    source: Option<Uuid>,
    kind: Option<ItemKind>,
    limit: Option<usize>,
) -> Cmd<Vec<Value>> {
    let q = HistoryQuery { text, source, kind, limit: limit.unwrap_or(200) };
    let entries = state.engine.node().await?.history(&q).map_err(err)?;
    Ok(entries.iter().map(|e| with_ui_preview(e, &e.preview)).collect())
}

#[tauri::command]
pub async fn copy_from_history(state: State<'_, AppState>, id: Uuid) -> Cmd<()> {
    state.engine.node().await?.copy_from_history(id).await.map_err(err)
}

#[tauri::command]
pub async fn export_item(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> Cmd<String> {
    let dir = save_dir(&state);
    let path = state.engine.node().await?.export_item(id, &dir).await.map_err(err)?;
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn send_clipboard_to(state: State<'_, AppState>, peer: Uuid) -> Cmd<()> {
    state.engine.node().await?.send_clipboard_to(peer).await.map_err(err)
}

#[tauri::command]
pub async fn send_files(state: State<'_, AppState>, peer: Uuid, paths: Vec<String>) -> Cmd<Vec<Uuid>> {
    let paths = paths.into_iter().map(PathBuf::from).collect();
    state.engine.node().await?.send_files(peer, paths).await.map_err(err)
}

#[tauri::command]
pub async fn add_address(state: State<'_, AppState>, peer: Uuid, addr: String) -> Cmd<()> {
    let addr = parse_peer_addr(&addr)?;
    state.engine.node().await?.add_address(peer, addr).map_err(err)
}
```

If the compiler flags `Ordering` or `Manager` as unused in `commands.rs`, remove those imports (they are used from Task 5 on).

- [ ] **Step 6: Run tests and build**

Run: `cargo test -p wisp-app` then `cargo clippy -p wisp-app --all-targets -- -D warnings`
Expected: all tests pass (3 new), clippy clean.

- [ ] **Step 7: Commit**

```bash
git add apps/wisp/src-tauri Cargo.lock
git commit -m "Bridge the core engine into the app with commands and an event pump"
```

---

### Task 5: Windows, materials and the tray icon

**Files:**
- Create: `apps/wisp/src-tauri/src/windows.rs`, `apps/wisp/src-tauri/src/tray.rs`, tray artwork `apps/wisp/tray-mono.svg`, `apps/wisp/tray-mono-pending.svg`, `apps/wisp/tray-color.svg`, `apps/wisp/tray-color-pending.svg` and the PNGs generated from them
- Modify: `apps/wisp/src-tauri/src/lib.rs` (plugins, tray setup, `TauriSink::react`, macOS accessory policy), `apps/wisp/src-tauri/src/commands.rs` (`open_window`, `hide_window`, `fit_toast`, tray refresh after inbox changes)
- Test: inline test module in `windows.rs`

**Interfaces:**
- Consumes: `AppState`, `start_engine` (Task 4)
- Produces:
  - `windows::View { Panel, Toast, Timeline, Settings, Onboarding }` with `label() -> &'static str`, `url() -> String` (`index.html?view=<label>`), `from_label(&str) -> Option<View>`
  - `windows::Rect { x: i32, y: i32, width: u32, height: u32 }`, `windows::toast_origin(work: Rect, size: (u32, u32), macos: bool) -> (i32, i32)` (16 px logical margin scaled by the caller; bottom right on Windows, top right on macOS)
  - `windows::show(app, View) -> tauri::Result<WebviewWindow>` (creates or reuses; panel is positioned at the tray; toast at the corner without taking focus), `windows::toggle_panel(app)`, `windows::hide(app, View)`
  - `tray::install(app) -> tauri::Result<()>`, `tray::refresh(app)` (async; pending dot when the inbox is not empty)
  - Commands: `open_window(app, name: String)`, `hide_window(app, name: String)`, `fit_toast(app, height: f64)`

- [ ] **Step 1: Write the failing window tests** (new `windows.rs`, test module first)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_round_trip_through_labels() {
        for v in [View::Panel, View::Toast, View::Timeline, View::Settings, View::Onboarding] {
            assert_eq!(View::from_label(v.label()), Some(v));
            assert_eq!(v.url(), format!("index.html?view={}", v.label()));
        }
        assert_eq!(View::from_label("nope"), None);
    }

    #[test]
    fn toast_sits_bottom_right_on_windows_and_top_right_on_macos() {
        let work = Rect { x: 0, y: 0, width: 1920, height: 1040 };
        assert_eq!(toast_origin(work, (360, 200), false), (1920 - 360 - 16, 1040 - 200 - 16));
        assert_eq!(toast_origin(work, (360, 200), true), (1920 - 360 - 16, 16));
        let second = Rect { x: 1920, y: 0, width: 2560, height: 1400 };
        assert_eq!(toast_origin(second, (360, 200), false), (1920 + 2560 - 360 - 16, 1400 - 200 - 16));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p wisp-app windows`
Expected: compile errors.

- [ ] **Step 3: Implement `windows.rs`** (above the tests)

```rust
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::AppState;

const MARGIN: f64 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Panel,
    Toast,
    Timeline,
    Settings,
    Onboarding,
}

impl View {
    pub fn label(&self) -> &'static str {
        match self {
            View::Panel => "panel",
            View::Toast => "toast",
            View::Timeline => "timeline",
            View::Settings => "settings",
            View::Onboarding => "onboarding",
        }
    }

    pub fn url(&self) -> String {
        format!("index.html?view={}", self.label())
    }

    pub fn from_label(label: &str) -> Option<View> {
        [View::Panel, View::Toast, View::Timeline, View::Settings, View::Onboarding]
            .into_iter()
            .find(|v| v.label() == label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// `margin` is already in physical pixels when called with a monitor rect.
fn origin_with_margin(work: Rect, size: (u32, u32), macos: bool, margin: i32) -> (i32, i32) {
    let x = work.x + work.width as i32 - size.0 as i32 - margin;
    let y = if macos { work.y + margin } else { work.y + work.height as i32 - size.1 as i32 - margin };
    (x, y)
}

pub fn toast_origin(work: Rect, size: (u32, u32), macos: bool) -> (i32, i32) {
    origin_with_margin(work, size, macos, MARGIN as i32)
}

fn builder<'a>(app: &'a AppHandle, view: View) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let b = WebviewWindowBuilder::new(app, view.label(), WebviewUrl::App(view.url().into())).visible(false);
    match view {
        View::Panel => b
            .title("Wisp")
            .inner_size(360.0, 480.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true),
        View::Toast => b
            .title("Wisp")
            .inner_size(360.0, 160.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .focused(false)
            .shadow(false),
        View::Onboarding => b
            .title("Welcome to Wisp")
            .inner_size(520.0, 600.0)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .center(),
        View::Timeline => b.title("Wisp timeline").inner_size(1040.0, 720.0).min_inner_size(640.0, 480.0).center(),
        View::Settings => b.title("Wisp settings").inner_size(560.0, 640.0).resizable(false).center(),
    }
}

fn apply_material(window: &WebviewWindow, view: View) {
    if !matches!(view, View::Panel | View::Toast | View::Onboarding) {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        let applied = window_vibrancy::apply_mica(window, None).is_ok();
        if !applied {
            let _ = window_vibrancy::apply_acrylic(window, Some((11, 13, 20, 200)));
        }
    }
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState};
        let _ = window_vibrancy::apply_vibrancy(
            window,
            NSVisualEffectMaterial::HudWindow,
            Some(NSVisualEffectState::Active),
            Some(16.0),
        );
    }
}

fn place(window: &WebviewWindow, view: View) {
    match view {
        View::Panel => {
            #[cfg(target_os = "macos")]
            let _ = window.move_window(Position::TrayCenter);
            #[cfg(not(target_os = "macos"))]
            let _ = window.move_window(Position::TrayBottomCenter);
        }
        View::Toast => place_toast(window),
        _ => {}
    }
}

fn place_toast(window: &WebviewWindow) {
    let (Ok(Some(monitor)), Ok(size)) = (window.primary_monitor(), window.outer_size()) else {
        return;
    };
    let scale = monitor.scale_factor();
    // Leave room for the taskbar on Windows and the menu bar on macOS.
    let (top_inset, bottom_inset) = if cfg!(target_os = "macos") { (28.0, 0.0) } else { (0.0, 48.0) };
    let work = Rect {
        x: monitor.position().x,
        y: monitor.position().y + (top_inset * scale) as i32,
        width: monitor.size().width,
        height: monitor.size().height - ((top_inset + bottom_inset) * scale) as u32,
    };
    let (x, y) = origin_with_margin(work, (size.width, size.height), cfg!(target_os = "macos"), (MARGIN * scale) as i32);
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

pub fn show(app: &AppHandle, view: View) -> tauri::Result<WebviewWindow> {
    let window = match app.get_webview_window(view.label()) {
        Some(w) => w,
        None => {
            let w = builder(app, view).build()?;
            apply_material(&w, view);
            if view == View::Panel {
                let handle = app.clone();
                w.on_window_event(move |event| match event {
                    WindowEvent::Focused(false) => {
                        let dragging = handle.state::<AppState>().dragging.load(Ordering::SeqCst);
                        if !dragging {
                            hide(&handle, View::Panel);
                        }
                    }
                    WindowEvent::DragDrop(drag) => {
                        let active = matches!(drag, tauri::DragDropEvent::Enter { .. } | tauri::DragDropEvent::Over { .. });
                        handle.state::<AppState>().dragging.store(active, Ordering::SeqCst);
                    }
                    _ => {}
                });
            }
            w
        }
    };
    place(&window, view);
    window.show()?;
    if view != View::Toast {
        window.set_focus()?;
    }
    Ok(window)
}

pub fn hide(app: &AppHandle, view: View) {
    if let Some(w) = app.get_webview_window(view.label()) {
        let _ = w.hide();
    }
}

pub fn toggle_panel(app: &AppHandle) {
    match app.get_webview_window(View::Panel.label()) {
        Some(w) if w.is_visible().unwrap_or(false) => {
            let _ = w.hide();
        }
        _ => {
            let _ = show(app, View::Panel);
        }
    }
}

pub fn fit_toast(app: &AppHandle, logical_height: f64) {
    if let Some(w) = app.get_webview_window(View::Toast.label()) {
        let scale = w.scale_factor().unwrap_or(1.0);
        let height = (logical_height.clamp(80.0, 520.0) * scale) as u32;
        let width = (360.0 * scale) as u32;
        let _ = w.set_size(PhysicalSize::new(width, height));
        place_toast(&w);
    }
}
```

If the positioner variant names differ in the resolved plugin version, pick the variant that puts the panel next to the tray icon (below it on macOS, above it on Windows) and verify visually in Step 8.

- [ ] **Step 4: Tray artwork**

`apps/wisp/tray-mono.svg` (macOS template image: black on transparent):

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <path d="M14 38c0-12 10-21 22-18 9 2 12 13 4 17-6 4-13-1-11-7 1-4 7-5 8-1"
        fill="none" stroke="#000" stroke-width="6" stroke-linecap="round"/>
</svg>
```

`apps/wisp/tray-mono-pending.svg`: same path plus `<circle cx="52" cy="12" r="8" fill="#000"/>`.

`apps/wisp/tray-color.svg` (Windows, visible on light and dark taskbars):

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#2DE2C5"/>
      <stop offset="1" stop-color="#8B5CF6"/>
    </linearGradient>
  </defs>
  <path d="M14 38c0-12 10-21 22-18 9 2 12 13 4 17-6 4-13-1-11-7 1-4 7-5 8-1"
        fill="none" stroke="url(#g)" stroke-width="7" stroke-linecap="round"/>
</svg>
```

`apps/wisp/tray-color-pending.svg`: same plus `<circle cx="52" cy="12" r="9" fill="#2DE2C5" stroke="#0B0D14" stroke-width="3"/>`.

Generate 32 px PNGs (run each; if `--png` is not supported by the CLI version, render the SVGs to 32 px PNGs with any tool and commit them under the same names):

```bash
cd apps/wisp
npx tauri icon tray-mono.svg -o src-tauri/icons/tray-mono --png 32
npx tauri icon tray-mono-pending.svg -o src-tauri/icons/tray-mono-pending --png 32
npx tauri icon tray-color.svg -o src-tauri/icons/tray-color --png 32
npx tauri icon tray-color-pending.svg -o src-tauri/icons/tray-color-pending --png 32
```

Expected: `src-tauri/icons/<name>/32x32.png` for each.

- [ ] **Step 5: Implement `tray.rs`**

```rust
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::windows::{self, View};
use crate::AppState;

const TRAY_ID: &str = "wisp";

fn icon(pending: bool) -> Image<'static> {
    let bytes: &'static [u8] = match (cfg!(target_os = "macos"), pending) {
        (true, false) => include_bytes!("../icons/tray-mono/32x32.png"),
        (true, true) => include_bytes!("../icons/tray-mono-pending/32x32.png"),
        (false, false) => include_bytes!("../icons/tray-color/32x32.png"),
        (false, true) => include_bytes!("../icons/tray-color-pending/32x32.png"),
    };
    Image::from_bytes(bytes).expect("tray icon is a valid PNG")
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Wisp", true, None::<&str>)?;
    let timeline = MenuItem::with_id(app, "timeline", "Timeline", true, None::<&str>)?;
    let pair = MenuItem::with_id(app, "pair", "Pair a device", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Wisp", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &timeline, &pair, &settings, &separator, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(false))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Wisp")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                let _ = windows::show(app, View::Panel);
            }
            "timeline" => {
                let _ = windows::show(app, View::Timeline);
            }
            "pair" | "settings" => {
                let _ = windows::show(app, View::Settings);
            }
            "quit" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    handle.state::<AppState>().engine.stop().await;
                    handle.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                windows::toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Shows the pending dot while uncaught offers wait in the inbox.
pub async fn refresh(app: &AppHandle) {
    let pending = match app.state::<AppState>().engine.node().await {
        Ok(node) => !node.inbox().is_empty(),
        Err(_) => false,
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon(pending)));
        let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
    }
}
```

(The "Pair a device" item opens Settings, which hosts pairing in Task 11. If `show_menu_on_left_click` does not exist in the resolved Tauri version, use `menu_on_left_click(false)`.)

- [ ] **Step 6: Wire into `lib.rs` and `commands.rs`**

In `lib.rs` add `mod tray; mod windows;`, register `tauri_plugin_positioner::init()` as a plugin, and in `setup` after `app.manage(...)`:

```rust
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::install(app.handle())?;
            if !onboarded {
                windows::show(app.handle(), windows::View::Onboarding)?;
            }
```

Replace `TauriSink::react` with:

```rust
    fn react(&self, event: &Event) {
        match event {
            Event::OfferReceived(_) => {
                let _ = windows::show(&self.app, windows::View::Toast);
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move { tray::refresh(&app).await });
            }
            Event::ClipboardApplied { .. } => {
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move { tray::refresh(&app).await });
            }
            _ => {}
        }
    }
```

In `commands.rs` add:

```rust
#[tauri::command]
pub fn open_window(app: AppHandle, name: String) -> Cmd<()> {
    let view = crate::windows::View::from_label(&name).ok_or_else(|| format!("unknown window {name}"))?;
    crate::windows::show(&app, view).map(|_| ()).map_err(err)
}

#[tauri::command]
pub fn hide_window(app: AppHandle, name: String) -> Cmd<()> {
    let view = crate::windows::View::from_label(&name).ok_or_else(|| format!("unknown window {name}"))?;
    crate::windows::hide(&app, view);
    Ok(())
}

#[tauri::command]
pub fn fit_toast(app: AppHandle, height: f64) {
    crate::windows::fit_toast(&app, height);
}
```

and call `crate::tray::refresh(&app).await;` at the end of `catch`, `catch_latest`, `dismiss` and `save_offer` (add an `app: AppHandle` parameter to those commands). `complete_onboarding` ends with: hide the onboarding window and show the panel (`crate::windows::hide(&app, View::Onboarding); let _ = crate::windows::show(&app, View::Panel);`). Register the three new commands in `generate_handler!`. Remove the now-unused `Ordering`/`Manager` imports from `commands.rs` if still unused.

- [ ] **Step 7: Run tests and clippy**

Run: `cargo test -p wisp-app` then `cargo clippy -p wisp-app --all-targets -- -D warnings`
Expected: pass and clean.

- [ ] **Step 8: Manual smoke check**

Run: `cd apps/wisp && npm run tauri dev`
Expected: with a fresh profile, the onboarding placeholder window appears; the tray shows the Wisp swirl; left click toggles the placeholder panel next to the tray; clicking elsewhere hides it; the tray menu opens Timeline and Settings placeholders; "Quit Wisp" exits. Note anything that looks off in the report (positions, materials).

- [ ] **Step 9: Commit**

```bash
git add apps/wisp
git commit -m "Add the tray icon and window management"
```

---

### Task 6: Frontend foundation: tokens, types, API and the state reducer

**Files:**
- Create: `apps/wisp/src/lib/types.ts`, `apps/wisp/src/lib/api.ts`, `apps/wisp/src/lib/store.ts`, `apps/wisp/src/lib/appStore.ts`, `apps/wisp/src/lib/format.ts`
- Modify: `apps/wisp/src/styles.css`
- Test: `apps/wisp/src/__tests__/store.spec.ts`, `apps/wisp/src/__tests__/format.spec.ts`

**Interfaces:**
- Consumes: command names and JSON shapes from Tasks 3-5 (`snapshot`, `get_settings`, `"wisp://event"`, `"wisp://settings"`, camelCase fields, `preview.kind` in `text|image|file`, `smart.type` in `plain|link|color|code`)
- Produces:
  - `types.ts`: `Os`, `DeliveryMode`, `ItemKind`, `SmartKind`, `Preview`, `Peer`, `PeerStatus`, `NearbyDevice`, `Offer`, `HistoryEntry`, `Snapshot`, `Theme`, `AppSettings`, `EngineEvent`
  - `api.ts`: one typed function per command (`api.snapshot()`, `api.catchItem(id)`, ...), `onEngineEvent(cb)`, `onSettings(cb)`
  - `store.ts` (pure, no Tauri imports): `State`, `Transfer`, `Notice`, `initialState()`, `applySnapshot(state, snap)`, `reduce(state, event) -> boolean` (true when the caller should resync from `snapshot`), `dismissNotice(state, id)`
  - `appStore.ts`: `useAppStore()` returning a shared reactive `State` plus `settings` ref; loads the snapshot, subscribes to events, resyncs when `reduce` asks, retries the snapshot while the engine starts, applies the theme to `document.documentElement.dataset.theme`
  - `format.ts`: `formatBytes(n)`, `formatAge(ms)`, `osLabel(os)`
  - CSS: tokens `--bg --surface --surface-strong --border --text --muted --accent-a --accent-b --danger`, classes `.glass`, `.accent-text`, `.move`, `.hoverable`, `.pulse`; reduced-motion rules

- [ ] **Step 1: Write the failing tests**

`apps/wisp/src/__tests__/store.spec.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { applySnapshot, initialState, reduce } from '../lib/store'
import type { Offer, Snapshot } from '../lib/types'

const mac = '11111111-1111-1111-1111-111111111111'
const pc = '22222222-2222-2222-2222-222222222222'

function snap(): Snapshot {
  return {
    deviceId: pc,
    deviceName: 'PC',
    platform: 'windows',
    port: 47800,
    peers: [
      {
        peer: { deviceId: mac, name: 'MacBook', os: 'MacOs', fingerprint: 'ab', addresses: [], mode: 'Catch' },
        online: false,
        addr: null,
      },
    ],
    nearby: [],
    inbox: [],
  }
}

function offer(itemId: string): Offer {
  return { peer: mac, itemId, kind: 'Text', size: 5, preview: { kind: 'text', text: 'hello', smart: { type: 'plain' } }, ageMs: 0 }
}

describe('reduce', () => {
  it('tracks peers going online and offline', () => {
    const s = initialState()
    applySnapshot(s, snap())
    expect(s.ready).toBe(true)
    reduce(s, { type: 'peerOnline', peer: mac, name: 'MacBook', addr: '192.168.1.5:47800' })
    expect(s.peers[0].online).toBe(true)
    expect(s.peers[0].addr).toBe('192.168.1.5:47800')
    reduce(s, { type: 'peerOffline', peer: mac })
    expect(s.peers[0].online).toBe(false)
  })

  it('asks for a resync when an unknown peer appears or pairing completes', () => {
    const s = initialState()
    applySnapshot(s, snap())
    expect(reduce(s, { type: 'peerOnline', peer: 'new', name: 'X', addr: '1.2.3.4:1' })).toBe(true)
    expect(reduce(s, { type: 'paired', peer: 'new', name: 'X' })).toBe(true)
    expect(reduce(s, { type: 'peerOffline', peer: mac })).toBe(false)
  })

  it('keeps the inbox newest first without duplicates and drops caught items', () => {
    const s = initialState()
    applySnapshot(s, snap())
    reduce(s, { type: 'offerReceived', ...offer('a') })
    reduce(s, { type: 'offerReceived', ...offer('b') })
    reduce(s, { type: 'offerReceived', ...offer('a') })
    expect(s.inbox.map((o) => o.itemId)).toEqual(['a', 'b'])
    reduce(s, { type: 'clipboardApplied', itemId: 'a', from: mac })
    expect(s.inbox.map((o) => o.itemId)).toEqual(['b'])
    expect(s.historyVersion).toBe(1)
  })

  it('shows an identity warning once per device', () => {
    const s = initialState()
    applySnapshot(s, snap())
    for (let i = 0; i < 5; i++) reduce(s, { type: 'identityChanged', peer: mac })
    expect(s.notices.filter((n) => n.kind === 'warning')).toHaveLength(1)
    expect(s.notices[0].text).toContain('MacBook')
  })

  it('lists nearby devices that are not paired yet', () => {
    const s = initialState()
    applySnapshot(s, snap())
    reduce(s, { type: 'discovered', device: 'x', name: 'Laptop', os: 'Windows', addresses: ['10.0.0.2:47800'] })
    reduce(s, { type: 'discovered', device: mac, name: 'MacBook', os: 'MacOs', addresses: [] })
    expect(s.nearby.map((n) => n.name)).toEqual(['Laptop'])
  })

  it('follows a transfer from progress to done or failed', () => {
    const s = initialState()
    reduce(s, { type: 'transferProgress', transferId: 't', peer: mac, name: 'a.pdf', done: 10, total: 100, outgoing: false })
    expect(s.transfers.t.status).toBe('active')
    reduce(s, { type: 'transferPaused', transferId: 't', peer: mac })
    expect(s.transfers.t.status).toBe('paused')
    reduce(s, { type: 'transferDone', transferId: 't', peer: mac, path: 'C:/x/a.pdf' })
    expect(s.transfers.t.status).toBe('done')
    expect(s.transfers.t.path).toBe('C:/x/a.pdf')
    reduce(s, { type: 'transferFailed', transferId: 'u', peer: mac, reason: 'nope' })
    expect(s.transfers.u.status).toBe('failed')
  })

  it('caps notices at five', () => {
    const s = initialState()
    for (let i = 0; i < 8; i++) reduce(s, { type: 'error', message: `e${i}` })
    expect(s.notices).toHaveLength(5)
    expect(s.notices[4].text).toBe('e7')
  })
})
```

`apps/wisp/src/__tests__/format.spec.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { formatAge, formatBytes, osLabel } from '../lib/format'

describe('format', () => {
  it('formats sizes', () => {
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(2048)).toBe('2 KB')
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB')
    expect(formatBytes(3 * 1024 ** 3)).toBe('3.0 GB')
  })
  it('formats ages', () => {
    expect(formatAge(800)).toBe('just now')
    expect(formatAge(2_000)).toBe('2s ago')
    expect(formatAge(3 * 60_000)).toBe('3 min ago')
    expect(formatAge(2 * 3_600_000)).toBe('2 h ago')
  })
  it('labels operating systems', () => {
    expect(osLabel('MacOs')).toBe('macOS')
    expect(osLabel('Windows')).toBe('Windows')
  })
})
```

Run: `cd apps/wisp && npm test`
Expected: FAIL (modules not found).

- [ ] **Step 2: Implement `types.ts`**

```ts
export type Os = 'Windows' | 'MacOs' | 'Linux'
export type DeliveryMode = 'Catch' | 'AutoSync'
export type ItemKind = 'Text' | 'Html' | 'Image' | 'File'
export type Theme = 'system' | 'dark' | 'light'

export type SmartKind =
  | { type: 'plain' }
  | { type: 'link'; domain: string }
  | { type: 'color'; value: string }
  | { type: 'code' }

export type Preview =
  | { kind: 'text'; text: string; smart: SmartKind }
  | { kind: 'image'; dataUrl: string; width: number; height: number }
  | { kind: 'file'; name: string }

export interface Peer {
  deviceId: string
  name: string
  os: Os
  fingerprint: string
  addresses: string[]
  mode: DeliveryMode
}

export interface PeerStatus {
  peer: Peer
  online: boolean
  addr: string | null
}

export interface NearbyDevice {
  device: string
  name: string
  os: Os
  addresses: string[]
}

export interface Offer {
  peer: string
  itemId: string
  kind: ItemKind
  size: number
  preview: Preview
  ageMs: number
}

export interface HistoryEntry {
  id: string
  source: string
  kind: ItemKind
  createdMs: number
  size: number
  preview: Preview
}

export interface Snapshot {
  deviceId: string
  deviceName: string
  platform: string
  port: number
  peers: PeerStatus[]
  nearby: NearbyDevice[]
  inbox: Offer[]
}

export interface AppSettings {
  deviceName: string | null
  saveDir: string | null
  hotkey: string
  autostart: boolean
  theme: Theme
  onboarded: boolean
}

export type EngineEvent =
  | { type: 'peerOnline'; peer: string; name: string; addr: string }
  | { type: 'peerOffline'; peer: string }
  | { type: 'discovered'; device: string; name: string; os: Os; addresses: string[] }
  | { type: 'identityChanged'; peer: string }
  | { type: 'paired'; peer: string; name: string }
  | { type: 'pairingFailed'; reason: string }
  | { type: 'localItem'; itemId: string }
  | ({ type: 'offerReceived' } & Offer)
  | { type: 'clipboardApplied'; itemId: string; from: string }
  | { type: 'transferProgress'; transferId: string; peer: string; name: string; done: number; total: number; outgoing: boolean }
  | { type: 'transferPaused'; transferId: string; peer: string }
  | { type: 'transferDone'; transferId: string; peer: string; path: string | null }
  | { type: 'transferFailed'; transferId: string; peer: string; reason: string }
  | { type: 'error'; message: string }
```

- [ ] **Step 3: Implement `store.ts`**

```ts
import type { EngineEvent, NearbyDevice, Offer, PeerStatus, Snapshot } from './types'

export interface Transfer {
  id: string
  peer: string
  name: string
  done: number
  total: number
  outgoing: boolean
  status: 'active' | 'paused' | 'done' | 'failed'
  path?: string | null
  reason?: string
}

export interface Notice {
  id: number
  kind: 'error' | 'warning' | 'info'
  text: string
}

export interface State {
  ready: boolean
  deviceId: string
  deviceName: string
  platform: string
  port: number
  peers: PeerStatus[]
  nearby: NearbyDevice[]
  inbox: Offer[]
  transfers: Record<string, Transfer>
  notices: Notice[]
  identityWarnings: string[]
  historyVersion: number
}

const MAX_NOTICES = 5
let noticeSeq = 0

export function initialState(): State {
  return {
    ready: false,
    deviceId: '',
    deviceName: '',
    platform: '',
    port: 0,
    peers: [],
    nearby: [],
    inbox: [],
    transfers: {},
    notices: [],
    identityWarnings: [],
    historyVersion: 0,
  }
}

export function applySnapshot(s: State, snap: Snapshot): void {
  s.ready = true
  s.deviceId = snap.deviceId
  s.deviceName = snap.deviceName
  s.platform = snap.platform
  s.port = snap.port
  s.peers = snap.peers
  s.nearby = snap.nearby.filter((n) => !snap.peers.some((p) => p.peer.deviceId === n.device))
  s.inbox = snap.inbox
}

function notify(s: State, kind: Notice['kind'], text: string): void {
  s.notices.push({ id: ++noticeSeq, kind, text })
  if (s.notices.length > MAX_NOTICES) s.notices.splice(0, s.notices.length - MAX_NOTICES)
}

export function dismissNotice(s: State, id: number): void {
  s.notices = s.notices.filter((n) => n.id !== id)
}

function peerName(s: State, id: string): string {
  return s.peers.find((p) => p.peer.deviceId === id)?.peer.name ?? 'A device'
}

function transfer(s: State, id: string, peer: string): Transfer {
  s.transfers[id] ??= { id, peer, name: '', done: 0, total: 0, outgoing: false, status: 'active' }
  return s.transfers[id]
}

/** Applies one engine event. Returns true when the caller should reload the snapshot. */
export function reduce(s: State, e: EngineEvent): boolean {
  switch (e.type) {
    case 'peerOnline': {
      const p = s.peers.find((x) => x.peer.deviceId === e.peer)
      if (!p) return true
      p.online = true
      p.addr = e.addr
      return false
    }
    case 'peerOffline': {
      const p = s.peers.find((x) => x.peer.deviceId === e.peer)
      if (p) {
        p.online = false
        p.addr = null
      }
      return false
    }
    case 'discovered': {
      if (s.peers.some((p) => p.peer.deviceId === e.device)) return false
      const entry = { device: e.device, name: e.name, os: e.os, addresses: e.addresses }
      const i = s.nearby.findIndex((n) => n.device === e.device)
      if (i >= 0) s.nearby[i] = entry
      else s.nearby.push(entry)
      return false
    }
    case 'identityChanged': {
      if (s.identityWarnings.includes(e.peer)) return false
      s.identityWarnings.push(e.peer)
      notify(s, 'warning', `${peerName(s, e.peer)} announced a different identity. If you did not reinstall it, ignore it and keep the old pairing.`)
      return false
    }
    case 'paired':
      s.nearby = s.nearby.filter((n) => n.device !== e.peer)
      return true
    case 'pairingFailed':
      notify(s, 'error', e.reason)
      return false
    case 'localItem':
      s.historyVersion++
      return false
    case 'offerReceived': {
      const { type: _type, ...offer } = e
      s.inbox = [offer, ...s.inbox.filter((o) => o.itemId !== offer.itemId)]
      return false
    }
    case 'clipboardApplied':
      s.inbox = s.inbox.filter((o) => o.itemId !== e.itemId)
      s.historyVersion++
      return false
    case 'transferProgress': {
      const t = transfer(s, e.transferId, e.peer)
      Object.assign(t, { name: e.name || t.name, done: e.done, total: e.total, outgoing: e.outgoing, status: 'active' })
      return false
    }
    case 'transferPaused':
      transfer(s, e.transferId, e.peer).status = 'paused'
      return false
    case 'transferDone':
      Object.assign(transfer(s, e.transferId, e.peer), { status: 'done', path: e.path })
      return false
    case 'transferFailed':
      Object.assign(transfer(s, e.transferId, e.peer), { status: 'failed', reason: e.reason })
      return false
    case 'error':
      notify(s, 'error', e.message)
      return false
  }
}
```

(If `noUnusedLocals`-style checks complain about `_type`, destructure differently; do not add ignore comments.)

- [ ] **Step 4: Implement `format.ts`**

```ts
import type { Os } from './types'

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 ** 2) return `${Math.round(n / 1024)} KB`
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`
  return `${(n / 1024 ** 3).toFixed(1)} GB`
}

export function formatAge(ms: number): string {
  if (ms < 1_000) return 'just now'
  if (ms < 60_000) return `${Math.floor(ms / 1_000)}s ago`
  if (ms < 3_600_000) return `${Math.floor(ms / 60_000)} min ago`
  if (ms < 86_400_000) return `${Math.floor(ms / 3_600_000)} h ago`
  return `${Math.floor(ms / 86_400_000)} d ago`
}

export function osLabel(os: Os): string {
  return os === 'MacOs' ? 'macOS' : os
}
```

- [ ] **Step 5: Implement `api.ts`**

```ts
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { AppSettings, DeliveryMode, EngineEvent, HistoryEntry, ItemKind, Offer, Peer, Snapshot } from './types'

export const api = {
  snapshot: () => invoke<Snapshot>('snapshot'),
  getSettings: () => invoke<AppSettings>('get_settings'),
  updateSettings: (next: AppSettings) => invoke<AppSettings>('update_settings', { next }),
  completeOnboarding: (deviceName: string) => invoke<void>('complete_onboarding', { deviceName }),
  pickFolder: () => invoke<string | null>('pick_folder'),
  beginPairing: () => invoke<string>('begin_pairing'),
  cancelPairing: () => invoke<void>('cancel_pairing'),
  pairWith: (addr: string, code: string) => invoke<Peer>('pair_with', { addr, code }),
  setMode: (peer: string, mode: DeliveryMode) => invoke<void>('set_mode', { peer, mode }),
  forget: (peer: string) => invoke<void>('forget', { peer }),
  catchItem: (itemId: string) => invoke<void>('catch', { itemId }),
  catchLatest: () => invoke<string | null>('catch_latest'),
  dismiss: (itemId: string) => invoke<void>('dismiss', { itemId }),
  saveOffer: (itemId: string) => invoke<string>('save_offer', { itemId }),
  history: (q: { text?: string; source?: string; kind?: ItemKind; limit?: number }) =>
    invoke<HistoryEntry[]>('history', q),
  copyFromHistory: (id: string) => invoke<void>('copy_from_history', { id }),
  exportItem: (id: string) => invoke<string>('export_item', { id }),
  sendClipboardTo: (peer: string) => invoke<void>('send_clipboard_to', { peer }),
  sendFiles: (peer: string, paths: string[]) => invoke<string[]>('send_files', { peer, paths }),
  addAddress: (peer: string, addr: string) => invoke<void>('add_address', { peer, addr }),
  openWindow: (name: 'panel' | 'toast' | 'timeline' | 'settings' | 'onboarding') => invoke<void>('open_window', { name }),
  hideWindow: (name: 'panel' | 'toast' | 'timeline' | 'settings' | 'onboarding') => invoke<void>('hide_window', { name }),
  fitToast: (height: number) => invoke<void>('fit_toast', { height }),
}

export type { Offer }

export function onEngineEvent(cb: (e: EngineEvent) => void): Promise<UnlistenFn> {
  return listen<EngineEvent>('wisp://event', (msg) => cb(msg.payload))
}

export function onSettings(cb: (s: AppSettings) => void): Promise<UnlistenFn> {
  return listen<AppSettings>('wisp://settings', (msg) => cb(msg.payload))
}
```

(Tauri converts camelCase invoke argument keys to the Rust snake_case parameter names, so `itemId` reaches `item_id`.)

- [ ] **Step 6: Implement `appStore.ts`**

```ts
import { reactive, ref } from 'vue'
import { api, onEngineEvent, onSettings } from './api'
import { applySnapshot, initialState, reduce, type State } from './store'
import type { AppSettings } from './types'

let shared: { state: State; settings: ReturnType<typeof ref<AppSettings | null>>; resync: () => Promise<void> } | null = null

function applyTheme(settings: AppSettings | null) {
  document.documentElement.dataset.theme = settings?.theme ?? 'system'
}

export function useAppStore() {
  if (shared) return shared
  const state = reactive(initialState()) as State
  const settings = ref<AppSettings | null>(null)

  async function resync() {
    for (let attempt = 0; attempt < 40; attempt++) {
      try {
        applySnapshot(state, await api.snapshot())
        return
      } catch {
        await new Promise((r) => setTimeout(r, 250))
      }
    }
  }

  void api.getSettings().then((s) => {
    settings.value = s
    applyTheme(s)
  })
  void onSettings((s) => {
    const saveDirChanged = settings.value?.saveDir !== s.saveDir
    settings.value = s
    applyTheme(s)
    if (saveDirChanged) void resync()
  })
  void onEngineEvent((e) => {
    if (reduce(state, e)) void resync()
  })
  void resync()

  shared = { state, settings, resync }
  return shared
}
```

- [ ] **Step 7: Tokens and base styles in `styles.css`**

```css
@import 'tailwindcss';
@import '@fontsource-variable/geist';
@import '@fontsource-variable/geist-mono';

@theme {
  --font-sans: 'Geist Variable', ui-sans-serif, system-ui, sans-serif;
  --font-mono: 'Geist Mono Variable', ui-monospace, monospace;
}

:root {
  color-scheme: dark;
  --bg: #0b0d14;
  --surface: rgb(255 255 255 / 0.05);
  --surface-strong: rgb(255 255 255 / 0.1);
  --border: rgb(255 255 255 / 0.1);
  --text: #e8eaf2;
  --muted: #8a90a6;
  --accent-a: #2de2c5;
  --accent-b: #8b5cf6;
  --danger: #ff6b81;
  --glow: 0 0 28px rgb(45 226 197 / 0.35);
  --ease-spring: cubic-bezier(0.2, 0.9, 0.25, 1.2);
}

:root[data-theme='light'] {
  color-scheme: light;
  --bg: #f5f6fa;
  --surface: rgb(11 13 20 / 0.04);
  --surface-strong: rgb(11 13 20 / 0.08);
  --border: rgb(11 13 20 / 0.1);
  --text: #12141c;
  --muted: #5b6178;
  --glow: 0 0 22px rgb(139 92 246 / 0.25);
}

@media (prefers-color-scheme: light) {
  :root[data-theme='system'] {
    color-scheme: light;
    --bg: #f5f6fa;
    --surface: rgb(11 13 20 / 0.04);
    --surface-strong: rgb(11 13 20 / 0.08);
    --border: rgb(11 13 20 / 0.1);
    --text: #12141c;
    --muted: #5b6178;
    --glow: 0 0 22px rgb(139 92 246 / 0.25);
  }
}

html,
body {
  margin: 0;
  background: transparent;
  color: var(--text);
  font-family: var(--font-sans);
  -webkit-font-smoothing: antialiased;
  user-select: none;
}

:root[data-view='timeline'] body,
:root[data-view='settings'] body {
  background: var(--bg);
}

.glass {
  background: color-mix(in srgb, var(--bg) 70%, transparent);
  backdrop-filter: blur(24px) saturate(140%);
  border: 1px solid var(--border);
  border-radius: 18px;
}

.accent-text {
  background: linear-gradient(120deg, var(--accent-a), var(--accent-b));
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}

.hoverable {
  transition: background-color 140ms ease, border-color 140ms ease, color 140ms ease, opacity 140ms ease;
}

.move {
  transition: transform 420ms var(--ease-spring), opacity 200ms ease;
}

.pulse {
  animation: pulse 2.4s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { box-shadow: 0 0 0 0 rgb(45 226 197 / 0.0); }
  50% { box-shadow: var(--glow); }
}

/* Reduced motion keeps short fades and hover transitions; only large movement goes. */
@media (prefers-reduced-motion: reduce) {
  .move {
    transition: opacity 160ms ease;
    transform: none !important;
  }
  .pulse {
    animation: none;
    box-shadow: var(--glow);
  }
}
```

- [ ] **Step 8: Run tests and build**

Run: `cd apps/wisp && npm test && npm run build`
Expected: all tests pass (store 7, format 3, main 1), build succeeds.

- [ ] **Step 9: Commit**

```bash
git add apps/wisp/src
git commit -m "Add frontend tokens, types, API wrappers and the state reducer"
```

---

### Task 7: Shared components: orbs, constellation and previews

**Files:**
- Create: `apps/wisp/src/lib/constellation.ts`, `apps/wisp/src/components/IconGlyph.vue`, `DeviceOrb.vue`, `Constellation.vue`, `PreviewBody.vue`, `ItemCard.vue`, `TransferRow.vue`, `SearchField.vue`
- Test: `apps/wisp/src/__tests__/constellation.spec.ts`, `apps/wisp/src/__tests__/preview.spec.ts`

**Interfaces:**
- Consumes: `types.ts`, `format.ts` (Task 6)
- Produces:
  - `constellation.ts`: `interface OrbPlacement { id: string; x: number; y: number }`, `layoutOrbs(ids: string[], width: number, height: number): OrbPlacement[]` (this device sits at the center; peers on a ring, first peer on the left, second on the right, then alternating above and below; all orbs fully inside the box with a 36 px orb radius)
  - Components (props / emits):
    - `IconGlyph` `{ name: 'windows'|'mac'|'linux'|'link'|'file'|'image'|'text'|'code'|'catch'|'close'|'search'|'gear'|'timeline'|'plus'|'send'|'save' , size?: number }`
    - `DeviceOrb` `{ name, os, online, self?: boolean, mode?: DeliveryMode, pending?: number, dropActive?: boolean }`, emits `select`
    - `Constellation` `{ selfName, selfOs, peers: PeerStatus[], pendingByPeer: Record<string, number>, dropTarget: string | null, width?: number, height?: number }`, emits `select(peerId)`; each peer orb root element carries `data-peer-id`
    - `PreviewBody` `{ preview: Preview, compact?: boolean }`
    - `ItemCard` `{ preview: Preview, title: string, meta: string }` with a default slot for actions
    - `TransferRow` `{ transfer: Transfer, peerName: string }`
    - `SearchField` `v-model` string, `placeholder`

- [ ] **Step 1: Write the failing tests**

`apps/wisp/src/__tests__/constellation.spec.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { layoutOrbs } from '../lib/constellation'

describe('layoutOrbs', () => {
  const W = 320
  const H = 220

  it('puts the first peer left and the second right of the center', () => {
    const [a, b] = layoutOrbs(['a', 'b'], W, H)
    expect(a.x).toBeLessThan(W / 2)
    expect(b.x).toBeGreaterThan(W / 2)
    expect(Math.abs(a.y - H / 2)).toBeLessThan(1)
    expect(Math.abs(b.y - H / 2)).toBeLessThan(1)
  })

  it('keeps every orb inside the box and returns ids in order', () => {
    const ids = ['a', 'b', 'c', 'd', 'e', 'f']
    const placed = layoutOrbs(ids, W, H)
    expect(placed.map((p) => p.id)).toEqual(ids)
    for (const p of placed) {
      expect(p.x).toBeGreaterThanOrEqual(36)
      expect(p.x).toBeLessThanOrEqual(W - 36)
      expect(p.y).toBeGreaterThanOrEqual(36)
      expect(p.y).toBeLessThanOrEqual(H - 36)
    }
  })

  it('does not stack two orbs on the same spot', () => {
    const placed = layoutOrbs(['a', 'b', 'c', 'd'], W, H)
    for (let i = 0; i < placed.length; i++)
      for (let j = i + 1; j < placed.length; j++)
        expect(Math.hypot(placed[i].x - placed[j].x, placed[i].y - placed[j].y)).toBeGreaterThan(40)
  })

  it('handles no peers', () => {
    expect(layoutOrbs([], W, H)).toEqual([])
  })
})
```

`apps/wisp/src/__tests__/preview.spec.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import PreviewBody from '../components/PreviewBody.vue'

describe('PreviewBody', () => {
  it('shows a swatch for colors', () => {
    const w = mount(PreviewBody, { props: { preview: { kind: 'text', text: '#2DE2C5', smart: { type: 'color', value: '#2DE2C5' } } } })
    expect(w.find('[data-swatch]').attributes('style')).toContain('#2DE2C5')
  })
  it('highlights code', () => {
    const w = mount(PreviewBody, { props: { preview: { kind: 'text', text: 'const a = 1;\nconst b = 2;', smart: { type: 'code' } } } })
    expect(w.find('pre code').html()).toContain('hljs')
  })
  it('shows the domain for links', () => {
    const w = mount(PreviewBody, { props: { preview: { kind: 'text', text: 'https://example.org/a', smart: { type: 'link', domain: 'example.org' } } } })
    expect(w.text()).toContain('example.org')
  })
  it('renders plain text as text, never as html', () => {
    const w = mount(PreviewBody, { props: { preview: { kind: 'text', text: '<b>hi</b>', smart: { type: 'plain' } } } })
    expect(w.find('b').exists()).toBe(false)
    expect(w.text()).toContain('<b>hi</b>')
  })
  it('renders images and files', () => {
    const img = mount(PreviewBody, { props: { preview: { kind: 'image', dataUrl: 'data:image/png;base64,AQID', width: 10, height: 5 } } })
    expect(img.find('img').attributes('src')).toBe('data:image/png;base64,AQID')
    const file = mount(PreviewBody, { props: { preview: { kind: 'file', name: 'report.pdf' } } })
    expect(file.text()).toContain('report.pdf')
  })
})
```

Run: `npm test`
Expected: FAIL (modules not found).

- [ ] **Step 2: Implement `constellation.ts`**

```ts
export interface OrbPlacement {
  id: string
  x: number
  y: number
}

const ORB_RADIUS = 36

/** Angles in order: left, right, then alternating upper and lower slots on both sides. */
const SLOTS = [180, 0, 135, 45, 225, 315, 90, 270, 160, 20, 200, 340]

export function layoutOrbs(ids: string[], width: number, height: number): OrbPlacement[] {
  const cx = width / 2
  const cy = height / 2
  const rx = Math.max(0, width / 2 - ORB_RADIUS)
  const ry = Math.max(0, height / 2 - ORB_RADIUS)
  return ids.map((id, i) => {
    const ring = Math.floor(i / SLOTS.length)
    const scale = 1 - ring * 0.18
    const angle = ((SLOTS[i % SLOTS.length] + ring * 12) * Math.PI) / 180
    return {
      id,
      x: cx + Math.cos(angle) * rx * scale,
      y: cy - Math.sin(angle) * ry * scale,
    }
  })
}
```

- [ ] **Step 3: Implement `IconGlyph.vue`**

```vue
<script setup lang="ts">
const props = withDefaults(defineProps<{ name: string; size?: number }>(), { size: 16 })

const paths: Record<string, string> = {
  windows: 'M3 5l7-1v7H3zM11 4l10-1.5V11H11zM3 12h7v7l-7-1zM11 12h10v8.5L11 19z',
  mac: 'M16 3c0 2-1.5 3.5-3 3.5C13 4.5 14.5 3 16 3zM18.5 13c0-2.5 2-3.5 2-3.5-1-1.5-2.7-1.8-3.3-1.8-1.4-.1-2.7.8-3.4.8s-1.8-.8-3-.8C8.3 7.7 6 9.4 6 13c0 2.2.8 4.5 1.9 6 .9 1.3 1.7 2 2.7 2 1 0 1.4-.6 2.6-.6s1.6.6 2.6.6c1.1 0 1.8-1.1 2.6-2.2.5-.7.8-1.4 1-1.8-1.8-.8-2.9-2.3-2.9-4z',
  linux: 'M12 3c-2 0-3 1.8-3 4 0 1.4-1.6 3-2.6 5.2-.9 2-.4 3.8.9 4.6L6 19h12l-1.3-2.2c1.3-.8 1.8-2.6.9-4.6C16.6 10 15 8.4 15 7c0-2.2-1-4-3-4z',
  link: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
  file: 'M6 3h8l5 5v13H6zM14 3v5h5',
  image: 'M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15 9.5a1.5 1.5 0 1 0 0-.01',
  text: 'M5 6h14M5 11h14M5 16h9',
  code: 'M9 7l-5 5 5 5M15 7l5 5-5 5',
  catch: 'M12 4v11M7 11l5 5 5-5M5 20h14',
  close: 'M6 6l12 12M18 6L6 18',
  search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4',
  gear: 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1',
  timeline: 'M4 6h16M4 12h16M4 18h10',
  plus: 'M12 5v14M5 12h14',
  send: 'M4 12l16-8-6 16-2-7z',
  save: 'M5 4h11l3 3v13H5zM8 4v5h7V4M8 20v-6h8v6',
}
</script>

<template>
  <svg
    :width="props.size"
    :height="props.size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path :d="paths[props.name] ?? paths.file" />
  </svg>
</template>
```

- [ ] **Step 4: Implement `DeviceOrb.vue`**

```vue
<script setup lang="ts">
import { computed } from 'vue'
import IconGlyph from './IconGlyph.vue'
import type { DeliveryMode, Os } from '../lib/types'

const props = defineProps<{
  name: string
  os: Os
  online: boolean
  self?: boolean
  mode?: DeliveryMode
  pending?: number
  dropActive?: boolean
}>()
defineEmits<{ select: [] }>()

const glyph = computed(() => (props.os === 'MacOs' ? 'mac' : props.os === 'Windows' ? 'windows' : 'linux'))
</script>

<template>
  <button
    type="button"
    class="orb hoverable group relative flex flex-col items-center gap-1.5 outline-none"
    :class="{ 'opacity-45': !online && !self }"
    :title="name"
    @click="$emit('select')"
  >
    <span
      class="core grid size-[56px] place-items-center rounded-full"
      :class="{ pulse: self, 'is-drop': dropActive }"
    >
      <IconGlyph :name="glyph" :size="22" />
      <span
        v-if="pending"
        class="absolute -right-0.5 top-0 grid min-w-[18px] place-items-center rounded-full px-1 text-[10px] font-semibold text-[var(--bg)]"
        style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))"
      >{{ pending }}</span>
    </span>
    <span class="max-w-[92px] truncate text-[11px] font-medium">{{ name }}</span>
    <span v-if="mode === 'AutoSync'" class="text-[9px] uppercase tracking-wider text-[var(--muted)]">auto</span>
  </button>
</template>

<style scoped>
.core {
  background:
    radial-gradient(circle at 30% 25%, rgb(255 255 255 / 0.22), transparent 55%),
    linear-gradient(140deg, color-mix(in srgb, var(--accent-a) 55%, transparent), color-mix(in srgb, var(--accent-b) 55%, transparent));
  border: 1px solid var(--border);
  color: white;
  transition: box-shadow 160ms ease, transform 160ms ease;
}
.orb:hover .core,
.orb:focus-visible .core {
  box-shadow: var(--glow);
}
.is-drop {
  box-shadow: 0 0 0 3px var(--accent-a), var(--glow);
}
</style>
```

- [ ] **Step 5: Implement `Constellation.vue`**

```vue
<script setup lang="ts">
import { computed } from 'vue'
import DeviceOrb from './DeviceOrb.vue'
import { layoutOrbs } from '../lib/constellation'
import type { Os, PeerStatus } from '../lib/types'

const props = withDefaults(
  defineProps<{
    selfName: string
    selfOs: Os
    peers: PeerStatus[]
    pendingByPeer: Record<string, number>
    dropTarget: string | null
    width?: number
    height?: number
  }>(),
  { width: 328, height: 220 },
)
defineEmits<{ select: [peerId: string] }>()

const placed = computed(() => {
  const spots = layoutOrbs(props.peers.map((p) => p.peer.deviceId), props.width, props.height)
  return props.peers.map((p, i) => ({ status: p, ...spots[i] }))
})
</script>

<template>
  <div class="relative" :style="{ width: `${width}px`, height: `${height}px` }">
    <svg class="pointer-events-none absolute inset-0" :width="width" :height="height">
      <defs>
        <linearGradient id="wisp-link" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stop-color="var(--accent-a)" />
          <stop offset="1" stop-color="var(--accent-b)" />
        </linearGradient>
      </defs>
      <line
        v-for="p in placed"
        :key="p.id"
        :x1="width / 2"
        :y1="height / 2"
        :x2="p.x"
        :y2="p.y"
        :stroke="p.status.online ? 'url(#wisp-link)' : 'var(--border)'"
        :stroke-width="p.status.online ? 2 : 1"
        :stroke-dasharray="p.status.online ? undefined : '3 5'"
        class="hoverable"
      />
    </svg>
    <div class="move absolute -translate-x-1/2 -translate-y-1/2" :style="{ left: `${width / 2}px`, top: `${height / 2}px` }">
      <DeviceOrb :name="selfName" :os="selfOs" online self />
    </div>
    <div
      v-for="p in placed"
      :key="p.id"
      class="move absolute -translate-x-1/2 -translate-y-1/2"
      :style="{ left: `${p.x}px`, top: `${p.y}px` }"
      :data-peer-id="p.id"
    >
      <DeviceOrb
        :name="p.status.peer.name"
        :os="p.status.peer.os"
        :online="p.status.online"
        :mode="p.status.peer.mode"
        :pending="pendingByPeer[p.id] ?? 0"
        :drop-active="dropTarget === p.id"
        @select="$emit('select', p.id)"
      />
    </div>
  </div>
</template>
```

(Connection lines show online versus offline. The spec's cable versus Wi-Fi line styles need link-type information the core does not expose yet; that is deferred, see the ruling at the end of this plan.)

- [ ] **Step 6: Implement `PreviewBody.vue`**

```vue
<script setup lang="ts">
import { computed } from 'vue'
import hljs from 'highlight.js/lib/common'
import IconGlyph from './IconGlyph.vue'
import type { Preview } from '../lib/types'

const props = defineProps<{ preview: Preview; compact?: boolean }>()

const highlighted = computed(() =>
  props.preview.kind === 'text' && props.preview.smart.type === 'code'
    ? hljs.highlightAuto(props.preview.text).value
    : '',
)
</script>

<template>
  <div class="min-w-0 text-[13px] leading-snug">
    <template v-if="preview.kind === 'image'">
      <img
        :src="preview.dataUrl"
        :width="preview.width"
        :height="preview.height"
        class="max-h-40 w-auto max-w-full rounded-lg border border-[var(--border)] object-contain"
        alt="Image preview"
        draggable="false"
      />
    </template>
    <template v-else-if="preview.kind === 'file'">
      <div class="flex items-center gap-2"><IconGlyph name="file" /> <span class="truncate">{{ preview.name }}</span></div>
    </template>
    <template v-else-if="preview.smart.type === 'color'">
      <div class="flex items-center gap-3">
        <span data-swatch class="size-9 rounded-lg border border-[var(--border)]" :style="{ background: preview.smart.value }" />
        <span class="font-mono">{{ preview.smart.value }}</span>
      </div>
    </template>
    <template v-else-if="preview.smart.type === 'link'">
      <div class="flex items-center gap-2 text-[var(--accent-a)]"><IconGlyph name="link" /> <span class="font-medium">{{ preview.smart.domain }}</span></div>
      <p class="mt-1 truncate text-[var(--muted)]">{{ preview.text }}</p>
    </template>
    <template v-else-if="preview.smart.type === 'code'">
      <pre class="max-h-36 overflow-hidden rounded-lg bg-[var(--surface)] p-2 font-mono text-[11.5px]"><code class="hljs" v-html="highlighted" /></pre>
    </template>
    <p v-else class="whitespace-pre-wrap break-words" :class="compact ? 'line-clamp-2' : 'line-clamp-6'">{{ preview.text }}</p>
  </div>
</template>

<style>
.hljs-keyword, .hljs-built_in { color: var(--accent-b); }
.hljs-string, .hljs-attr { color: var(--accent-a); }
.hljs-comment { color: var(--muted); font-style: italic; }
.hljs-number, .hljs-literal { color: #f5b971; }
</style>
```

(`v-html` is used only for highlight.js output, which escapes its input.)

- [ ] **Step 7: Implement `ItemCard.vue`, `TransferRow.vue`, `SearchField.vue`**

`ItemCard.vue`:

```vue
<script setup lang="ts">
import PreviewBody from './PreviewBody.vue'
import type { Preview } from '../lib/types'

defineProps<{ preview: Preview; title: string; meta: string; compact?: boolean }>()
</script>

<template>
  <article class="hoverable rounded-xl border border-[var(--border)] bg-[var(--surface)] p-3 hover:bg-[var(--surface-strong)]">
    <header class="mb-2 flex items-baseline justify-between gap-2 text-[11px]">
      <span class="truncate font-medium">{{ title }}</span>
      <span class="shrink-0 text-[var(--muted)]">{{ meta }}</span>
    </header>
    <PreviewBody :preview="preview" :compact="compact" />
    <footer v-if="$slots.default" class="mt-3 flex gap-2"><slot /></footer>
  </article>
</template>
```

`TransferRow.vue`:

```vue
<script setup lang="ts">
import { computed } from 'vue'
import { formatBytes } from '../lib/format'
import type { Transfer } from '../lib/store'

const props = defineProps<{ transfer: Transfer; peerName: string }>()
const percent = computed(() => (props.transfer.total ? Math.round((props.transfer.done / props.transfer.total) * 100) : 0))
const label = computed(() => {
  const t = props.transfer
  if (t.status === 'paused') return `Paused, waiting for ${props.peerName}`
  if (t.status === 'failed') return t.reason ?? 'Failed'
  if (t.status === 'done') return t.outgoing ? `Sent to ${props.peerName}` : 'Saved'
  return `${formatBytes(t.done)} of ${formatBytes(t.total)}`
})
</script>

<template>
  <div class="text-[12px]">
    <div class="flex justify-between gap-2">
      <span class="truncate">{{ transfer.name || 'Incoming file' }}</span>
      <span class="shrink-0" :class="transfer.status === 'failed' ? 'text-[var(--danger)]' : 'text-[var(--muted)]'">{{ label }}</span>
    </div>
    <div class="mt-1 h-1 overflow-hidden rounded-full bg-[var(--surface-strong)]">
      <div
        class="h-full rounded-full"
        style="background: linear-gradient(90deg, var(--accent-a), var(--accent-b)); transition: width 200ms ease"
        :style="{ width: `${transfer.status === 'done' ? 100 : percent}%` }"
      />
    </div>
  </div>
</template>
```

`SearchField.vue`:

```vue
<script setup lang="ts">
import IconGlyph from './IconGlyph.vue'

const model = defineModel<string>({ required: true })
defineProps<{ placeholder?: string }>()
</script>

<template>
  <label class="hoverable flex items-center gap-2 rounded-xl border border-[var(--border)] bg-[var(--surface)] px-3 py-2 focus-within:border-[var(--accent-a)]">
    <IconGlyph name="search" class="text-[var(--muted)]" />
    <input
      v-model="model"
      class="w-full bg-transparent text-[13px] outline-none placeholder:text-[var(--muted)]"
      :placeholder="placeholder ?? 'Search'"
      spellcheck="false"
    />
  </label>
</template>
```

- [ ] **Step 8: Run tests and build**

Run: `npm test && npm run build`
Expected: all pass (constellation 4, preview 5 plus earlier), build succeeds.

- [ ] **Step 9: Commit**

```bash
git add apps/wisp/src
git commit -m "Add orb, constellation and preview components"
```

---

### Task 8: The tray panel

**Files:**
- Create: `apps/wisp/src/lib/drop.ts`
- Modify: `apps/wisp/src/views/PanelView.vue` (replace the placeholder)
- Test: `apps/wisp/src/__tests__/drop.spec.ts`, `apps/wisp/src/__tests__/panel.spec.ts`

**Interfaces:**
- Consumes: `useAppStore`, `api`, components from Task 7, `formatAge`, `osLabel`
- Produces:
  - `drop.ts`: `peerIdAt(el: Element | null): string | null` (closest `[data-peer-id]`), `toCssPoint(pos: { x: number; y: number }, dpr: number): { x: number; y: number }`
  - `PanelView.vue`: header (accent "Wisp", Timeline and Settings buttons), notices, constellation with orb selection sheet (mode toggle Catch / Auto-sync, "Send my clipboard", "Forget"), "Waiting to be caught" list, active transfers, search plus the 5 most recent timeline items (click copies), empty state with "Pair a device"

- [ ] **Step 1: Write the failing tests**

`apps/wisp/src/__tests__/drop.spec.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { peerIdAt, toCssPoint } from '../lib/drop'

describe('drop helpers', () => {
  it('finds the peer of the orb under the pointer', () => {
    document.body.innerHTML = '<div data-peer-id="abc"><span id="inner"></span></div><p id="other"></p>'
    expect(peerIdAt(document.getElementById('inner'))).toBe('abc')
    expect(peerIdAt(document.getElementById('other'))).toBeNull()
    expect(peerIdAt(null)).toBeNull()
  })
  it('converts physical drop positions to CSS pixels', () => {
    expect(toCssPoint({ x: 300, y: 150 }, 1.5)).toEqual({ x: 200, y: 100 })
  })
})
```

`apps/wisp/src/__tests__/panel.spec.ts` (Tauri modules are mocked; the view must render from store state alone):

```ts
import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { reactive, ref } from 'vue'
import { initialState } from '../lib/store'

const state = reactive(initialState())
vi.mock('../lib/appStore', () => ({ useAppStore: () => ({ state, settings: ref(null), resync: vi.fn() }) }))
vi.mock('../lib/api', () => ({
  api: { history: vi.fn().mockResolvedValue([]), openWindow: vi.fn(), hideWindow: vi.fn() },
}))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: vi.fn().mockResolvedValue(() => {}) }) }))

describe('PanelView', () => {
  it('invites pairing when there are no devices', async () => {
    Object.assign(state, { ready: true, deviceName: 'PC', platform: 'windows', peers: [] })
    const { default: PanelView } = await import('../views/PanelView.vue')
    const w = mount(PanelView)
    await flushPromises()
    expect(w.text()).toContain('Pair a device')
  })

  it('lists offers waiting to be caught', async () => {
    Object.assign(state, {
      ready: true,
      peers: [{ peer: { deviceId: 'm', name: 'MacBook', os: 'MacOs', fingerprint: 'x', addresses: [], mode: 'Catch' }, online: true, addr: null }],
      inbox: [{ peer: 'm', itemId: 'i1', kind: 'Text', size: 5, preview: { kind: 'text', text: 'hello', smart: { type: 'plain' } }, ageMs: 0 }],
    })
    const { default: PanelView } = await import('../views/PanelView.vue')
    const w = mount(PanelView)
    await flushPromises()
    expect(w.text()).toContain('Waiting to be caught')
    expect(w.text()).toContain('hello')
    expect(w.text()).toContain('MacBook')
  })
})
```

Run: `npm test`
Expected: FAIL.

- [ ] **Step 2: Implement `drop.ts`**

```ts
export function peerIdAt(el: Element | null): string | null {
  return el?.closest<HTMLElement>('[data-peer-id]')?.dataset.peerId ?? null
}

export function toCssPoint(pos: { x: number; y: number }, dpr: number): { x: number; y: number } {
  return { x: pos.x / dpr, y: pos.y / dpr }
}
```

- [ ] **Step 3: Implement `PanelView.vue`**

```vue
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import Constellation from '../components/Constellation.vue'
import ItemCard from '../components/ItemCard.vue'
import IconGlyph from '../components/IconGlyph.vue'
import SearchField from '../components/SearchField.vue'
import TransferRow from '../components/TransferRow.vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { peerIdAt, toCssPoint } from '../lib/drop'
import { formatAge } from '../lib/format'
import { dismissNotice } from '../lib/store'
import type { HistoryEntry, Os } from '../lib/types'

const { state } = useAppStore()
const selected = ref<string | null>(null)
const dropTarget = ref<string | null>(null)
const search = ref('')
const recent = ref<HistoryEntry[]>([])
const busy = ref(false)
const confirmForget = ref(false)

const selfOs = computed<Os>(() => (state.platform === 'macos' ? 'MacOs' : state.platform === 'windows' ? 'Windows' : 'Linux'))
const selectedPeer = computed(() => state.peers.find((p) => p.peer.deviceId === selected.value) ?? null)
const pendingByPeer = computed(() =>
  state.inbox.reduce<Record<string, number>>((acc, o) => ((acc[o.peer] = (acc[o.peer] ?? 0) + 1), acc), {}),
)
const activeTransfers = computed(() => Object.values(state.transfers).filter((t) => t.status === 'active' || t.status === 'paused'))

function nameOf(id: string): string {
  if (id === state.deviceId) return 'This device'
  return state.peers.find((p) => p.peer.deviceId === id)?.peer.name ?? 'Unknown device'
}

async function loadRecent() {
  try {
    recent.value = await api.history({ text: search.value || undefined, limit: 5 })
  } catch {
    recent.value = []
  }
}

let searchTimer: ReturnType<typeof setTimeout> | undefined
watch(search, () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(loadRecent, 150)
})
watch(() => [state.historyVersion, state.ready], loadRecent, { immediate: true })
watch(selected, () => (confirmForget.value = false))

async function run(task: () => Promise<unknown>) {
  busy.value = true
  try {
    await task()
  } catch (e) {
    state.notices.push({ id: Date.now(), kind: 'error', text: String(e) })
  } finally {
    busy.value = false
  }
}

function toggleMode() {
  const p = selectedPeer.value
  if (!p) return
  const next = p.peer.mode === 'Catch' ? 'AutoSync' : 'Catch'
  void run(async () => {
    await api.setMode(p.peer.deviceId, next)
    p.peer.mode = next
  })
}

function forget() {
  const p = selectedPeer.value
  if (!p) return
  if (!confirmForget.value) {
    confirmForget.value = true
    return
  }
  void run(async () => {
    await api.forget(p.peer.deviceId)
    state.peers = state.peers.filter((x) => x.peer.deviceId !== p.peer.deviceId)
    selected.value = null
  })
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') void api.hideWindow('panel')
}

let unlistenDrop: (() => void) | undefined
onMounted(async () => {
  window.addEventListener('keydown', onKey)
  unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
    const p = event.payload
    if (p.type === 'leave') {
      dropTarget.value = null
      return
    }
    const pt = toCssPoint(p.position, window.devicePixelRatio || 1)
    const peer = peerIdAt(document.elementFromPoint(pt.x, pt.y))
    if (p.type === 'drop') {
      dropTarget.value = null
      if (peer && p.paths.length) void run(() => api.sendFiles(peer, p.paths))
    } else {
      dropTarget.value = peer
    }
  })
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  unlistenDrop?.()
})
</script>

<template>
  <main class="glass flex h-screen flex-col gap-3 overflow-hidden p-4">
    <header class="flex items-center justify-between">
      <h1 class="accent-text text-lg font-semibold tracking-tight">Wisp</h1>
      <nav class="flex gap-1 text-[var(--muted)]">
        <button class="hoverable rounded-lg p-1.5 hover:bg-[var(--surface-strong)] hover:text-[var(--text)]" title="Timeline" @click="api.openWindow('timeline')">
          <IconGlyph name="timeline" />
        </button>
        <button class="hoverable rounded-lg p-1.5 hover:bg-[var(--surface-strong)] hover:text-[var(--text)]" title="Settings" @click="api.openWindow('settings')">
          <IconGlyph name="gear" />
        </button>
      </nav>
    </header>

    <div v-for="n in state.notices" :key="n.id" class="flex items-start justify-between gap-2 rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-[12px]" :class="n.kind === 'error' ? 'text-[var(--danger)]' : ''">
      <span>{{ n.text }}</span>
      <button class="text-[var(--muted)]" title="Dismiss" @click="dismissNotice(state, n.id)"><IconGlyph name="close" :size="12" /></button>
    </div>

    <section v-if="state.ready && !state.peers.length" class="flex flex-1 flex-col items-center justify-center gap-3 text-center">
      <p class="text-[13px] text-[var(--muted)]">No devices yet. Pair one to start sharing.</p>
      <button class="hoverable rounded-xl px-4 py-2 text-[13px] font-medium text-[var(--bg)]" style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))" @click="api.openWindow('settings')">
        Pair a device
      </button>
    </section>

    <template v-else>
      <div class="flex justify-center">
        <Constellation
          :self-name="state.deviceName || 'This device'"
          :self-os="selfOs"
          :peers="state.peers"
          :pending-by-peer="pendingByPeer"
          :drop-target="dropTarget"
          @select="(id) => (selected = selected === id ? null : id)"
        />
      </div>

      <section v-if="selectedPeer" class="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-3 text-[12px]">
        <div class="mb-2 flex items-center justify-between">
          <span class="font-medium">{{ selectedPeer.peer.name }}</span>
          <span class="text-[var(--muted)]">{{ selectedPeer.online ? 'Online' : 'Offline' }}</span>
        </div>
        <div class="flex flex-wrap gap-2">
          <button class="hoverable rounded-lg border border-[var(--border)] px-2.5 py-1.5 hover:bg-[var(--surface-strong)]" :disabled="busy" @click="toggleMode">
            {{ selectedPeer.peer.mode === 'Catch' ? 'Catch mode' : 'Auto-sync' }}
          </button>
          <button class="hoverable rounded-lg border border-[var(--border)] px-2.5 py-1.5 hover:bg-[var(--surface-strong)]" :disabled="busy || !selectedPeer.online" @click="run(() => api.sendClipboardTo(selectedPeer!.peer.deviceId))">
            Send my clipboard
          </button>
          <button class="hoverable rounded-lg px-2.5 py-1.5 text-[var(--danger)] hover:bg-[var(--surface-strong)]" :disabled="busy" @click="forget">
            {{ confirmForget ? 'Click again to forget' : 'Forget' }}
          </button>
        </div>
        <p class="mt-2 text-[11px] text-[var(--muted)]">Drop files on {{ selectedPeer.peer.name }} to send them.</p>
      </section>

      <section v-if="state.inbox.length" class="flex flex-col gap-2">
        <h2 class="text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">Waiting to be caught</h2>
        <ItemCard
          v-for="o in state.inbox.slice(0, 3)"
          :key="o.itemId"
          :preview="o.preview"
          :title="`from ${nameOf(o.peer)}`"
          :meta="formatAge(o.ageMs)"
          compact
        >
          <button class="hoverable rounded-lg px-2.5 py-1 text-[12px] font-medium text-[var(--bg)]" style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))" @click="run(() => api.catchItem(o.itemId))">Catch</button>
          <button class="hoverable rounded-lg px-2.5 py-1 text-[12px] text-[var(--muted)] hover:text-[var(--text)]" @click="run(() => api.dismiss(o.itemId).then(() => (state.inbox = state.inbox.filter((x) => x.itemId !== o.itemId))))">Dismiss</button>
        </ItemCard>
      </section>

      <section v-if="activeTransfers.length" class="flex flex-col gap-2">
        <TransferRow v-for="t in activeTransfers" :key="t.id" :transfer="t" :peer-name="nameOf(t.peer)" />
      </section>

      <section class="flex min-h-0 flex-1 flex-col gap-2">
        <SearchField v-model="search" placeholder="Search your timeline" />
        <div class="flex min-h-0 flex-col gap-1.5 overflow-y-auto">
          <button
            v-for="item in recent"
            :key="item.id"
            class="hoverable rounded-lg px-2 py-1.5 text-left hover:bg-[var(--surface-strong)]"
            title="Copy again"
            @click="run(() => api.copyFromHistory(item.id))"
          >
            <ItemCard :preview="item.preview" :title="nameOf(item.source)" :meta="formatAge(Date.now() - item.createdMs)" compact />
          </button>
          <p v-if="!recent.length" class="px-2 text-[12px] text-[var(--muted)]">Nothing here yet. Copy something.</p>
        </div>
      </section>
    </template>
  </main>
</template>
```

- [ ] **Step 4: Run tests and build**

Run: `npm test && npm run build`
Expected: pass.

- [ ] **Step 5: Manual check of drag and drop and focus**

Run the app (`npm run tauri dev`) with a paired device (or two app profiles on two machines). Drag a file from Explorer/Finder over the panel: the orb under the pointer gets the drop ring and the panel stays open while dragging (Review Focus 2); dropping sends the file. Record the result in the report.

- [ ] **Step 6: Commit**

```bash
git add apps/wisp/src
git commit -m "Build the tray panel"
```

---

### Task 9: The catch toast

**Files:**
- Create: `apps/wisp/src/lib/toasts.ts`
- Modify: `apps/wisp/src/views/ToastView.vue`
- Test: `apps/wisp/src/__tests__/toasts.spec.ts`

**Interfaces:**
- Consumes: `useAppStore`, `api` (`catchItem`, `saveOffer`, `dismiss`, `hideWindow`, `fitToast`), `PreviewBody`, `formatAge`
- Produces:
  - `toasts.ts`: `interface ToastGroup { peer: string; name: string; offers: Offer[] }`, `groupOffers(offers: Offer[], nameOf: (id: string) => string, max = 3): ToastGroup[]` (offers already newest first; groups ordered by their newest offer; at most `max` groups), `TOAST_LINGER_MS = 8000`, `shouldHide(now: number, lastChange: number, hovering: boolean, count: number): boolean`
  - `ToastView.vue`: one card per group: "from <name>" + age, the newest preview, "+N more" when stacked, buttons Catch (with the hotkey hint), Save as file, dismiss; hides after 8 s without changes unless hovered; hides at once when empty; reports its height through `fitToast`

- [ ] **Step 1: Write the failing tests**

```ts
import { describe, expect, it } from 'vitest'
import { groupOffers, shouldHide, TOAST_LINGER_MS } from '../lib/toasts'
import type { Offer } from '../lib/types'

const o = (peer: string, itemId: string): Offer => ({
  peer,
  itemId,
  kind: 'Text',
  size: 1,
  preview: { kind: 'text', text: itemId, smart: { type: 'plain' } },
  ageMs: 0,
})

describe('groupOffers', () => {
  const names: Record<string, string> = { m: 'MacBook', l: 'Laptop', p: 'PC', x: 'Other' }
  const nameOf = (id: string) => names[id]

  it('stacks offers per device, newest device first', () => {
    const groups = groupOffers([o('m', '3'), o('l', '2'), o('m', '1')], nameOf)
    expect(groups.map((g) => g.name)).toEqual(['MacBook', 'Laptop'])
    expect(groups[0].offers.map((x) => x.itemId)).toEqual(['3', '1'])
  })

  it('shows at most three groups', () => {
    expect(groupOffers([o('m', '1'), o('l', '2'), o('p', '3'), o('x', '4')], nameOf)).toHaveLength(3)
  })
})

describe('shouldHide', () => {
  it('hides when empty, lingers 8 s, and never while hovered', () => {
    expect(shouldHide(0, 0, false, 0)).toBe(true)
    expect(shouldHide(TOAST_LINGER_MS - 1, 0, false, 1)).toBe(false)
    expect(shouldHide(TOAST_LINGER_MS, 0, false, 1)).toBe(true)
    expect(shouldHide(TOAST_LINGER_MS * 2, 0, true, 1)).toBe(false)
  })
})
```

Save as `apps/wisp/src/__tests__/toasts.spec.ts`. Run `npm test`, expected FAIL.

- [ ] **Step 2: Implement `toasts.ts`**

```ts
import type { Offer } from './types'

export const TOAST_LINGER_MS = 8000

export interface ToastGroup {
  peer: string
  name: string
  offers: Offer[]
}

export function groupOffers(offers: Offer[], nameOf: (id: string) => string, max = 3): ToastGroup[] {
  const groups: ToastGroup[] = []
  for (const offer of offers) {
    let group = groups.find((g) => g.peer === offer.peer)
    if (!group) {
      group = { peer: offer.peer, name: nameOf(offer.peer), offers: [] }
      groups.push(group)
    }
    group.offers.push(offer)
  }
  return groups.slice(0, max)
}

export function shouldHide(now: number, lastChange: number, hovering: boolean, count: number): boolean {
  if (count === 0) return true
  if (hovering) return false
  return now - lastChange >= TOAST_LINGER_MS
}
```

- [ ] **Step 3: Implement `ToastView.vue`**

```vue
<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import IconGlyph from '../components/IconGlyph.vue'
import PreviewBody from '../components/PreviewBody.vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { formatAge } from '../lib/format'
import { groupOffers, shouldHide } from '../lib/toasts'

const { state, settings } = useAppStore()
const root = ref<HTMLElement | null>(null)
const hovering = ref(false)
const lastChange = ref(Date.now())
const working = ref<string | null>(null)

const isMac = computed(() => state.platform === 'macos')
const hotkeyHint = computed(() => {
  const combo = settings.value?.hotkey ?? 'Ctrl+Alt+V'
  return isMac.value ? combo.replace('Alt', 'Option') : combo
})

const groups = computed(() =>
  groupOffers(state.inbox, (id) => state.peers.find((p) => p.peer.deviceId === id)?.peer.name ?? 'A device'),
)

watch(
  () => state.inbox.map((o) => o.itemId).join(','),
  async () => {
    lastChange.value = Date.now()
    await nextTick()
    report()
  },
)

function report() {
  if (root.value) void api.fitToast(root.value.getBoundingClientRect().height + 8)
}

async function act(itemId: string, task: () => Promise<unknown>) {
  working.value = itemId
  try {
    await task()
    state.inbox = state.inbox.filter((o) => o.itemId !== itemId)
  } catch (e) {
    state.notices.push({ id: Date.now(), kind: 'error', text: String(e) })
  } finally {
    working.value = null
  }
}

let timer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  report()
  timer = setInterval(() => {
    if (shouldHide(Date.now(), lastChange.value, hovering.value, state.inbox.length)) void api.hideWindow('toast')
  }, 500)
})
onUnmounted(() => clearInterval(timer))
</script>

<template>
  <div
    ref="root"
    class="flex flex-col gap-2 p-1"
    :class="isMac ? '' : 'justify-end'"
    @mouseenter="hovering = true"
    @mouseleave="hovering = false; lastChange = Date.now()"
  >
    <TransitionGroup name="toast">
      <article v-for="g in groups" :key="g.peer" class="glass p-3 shadow-2xl">
        <header class="mb-2 flex items-center justify-between text-[11px]">
          <span class="font-medium">from {{ g.name }}</span>
          <span class="flex items-center gap-2 text-[var(--muted)]">
            {{ formatAge(g.offers[0].ageMs) }}
            <button title="Dismiss" class="hoverable hover:text-[var(--text)]" @click="act(g.offers[0].itemId, () => api.dismiss(g.offers[0].itemId))">
              <IconGlyph name="close" :size="12" />
            </button>
          </span>
        </header>
        <PreviewBody :preview="g.offers[0].preview" compact />
        <p v-if="g.offers.length > 1" class="mt-1 text-[11px] text-[var(--muted)]">+{{ g.offers.length - 1 }} more from {{ g.name }}</p>
        <footer class="mt-3 flex items-center gap-2">
          <button
            class="hoverable rounded-lg px-3 py-1.5 text-[12px] font-semibold text-[var(--bg)]"
            style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))"
            :disabled="working !== null"
            @click="act(g.offers[0].itemId, () => api.catchItem(g.offers[0].itemId))"
          >
            Catch
          </button>
          <button
            class="hoverable rounded-lg border border-[var(--border)] px-3 py-1.5 text-[12px] hover:bg-[var(--surface-strong)]"
            :disabled="working !== null"
            @click="act(g.offers[0].itemId, () => api.saveOffer(g.offers[0].itemId))"
          >
            Save as file
          </button>
          <span class="ml-auto font-mono text-[10px] text-[var(--muted)]">{{ hotkeyHint }}</span>
        </footer>
      </article>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: opacity 200ms ease, transform 320ms var(--ease-spring);
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(12px);
}
@media (prefers-reduced-motion: reduce) {
  .toast-enter-from,
  .toast-leave-to {
    transform: none;
  }
}
</style>
```

- [ ] **Step 4: Run tests and build**

Run: `npm test && npm run build`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add apps/wisp/src
git commit -m "Build the catch toast"
```

---

### Task 10: The timeline window

**Files:**
- Create: `apps/wisp/src/lib/timeline.ts`
- Modify: `apps/wisp/src/views/TimelineView.vue`
- Test: `apps/wisp/src/__tests__/timeline.spec.ts`

**Interfaces:**
- Consumes: `useAppStore`, `api.history`, `api.copyFromHistory`, `api.exportItem`, `ItemCard`, `SearchField`, `formatAge`
- Produces:
  - `timeline.ts`: `interface TimelineFilters { text: string; source: 'all' | string; kind: 'all' | ItemKind }`, `historyQuery(f: TimelineFilters): { text?: string; source?: string; kind?: ItemKind; limit: number }` (limit 200; empty text and `all` are omitted)
  - `TimelineView.vue`: search field, device chips (All, This device, each paired device), kind chips (All, Text, Rich text, Images), a responsive card grid; each card has Copy and Save as file; reloads on `historyVersion` changes and filter changes

- [ ] **Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { historyQuery } from '../lib/timeline'

describe('historyQuery', () => {
  it('omits empty filters and keeps the 200 item limit', () => {
    expect(historyQuery({ text: '  ', source: 'all', kind: 'all' })).toEqual({ limit: 200 })
    expect(historyQuery({ text: ' invoice ', source: 'abc', kind: 'Image' })).toEqual({
      text: 'invoice',
      source: 'abc',
      kind: 'Image',
      limit: 200,
    })
  })
})
```

Save as `apps/wisp/src/__tests__/timeline.spec.ts`; run `npm test`, expected FAIL.

- [ ] **Step 2: Implement `timeline.ts`**

```ts
import type { ItemKind } from './types'

export interface TimelineFilters {
  text: string
  source: 'all' | string
  kind: 'all' | ItemKind
}

export function historyQuery(f: TimelineFilters): { text?: string; source?: string; kind?: ItemKind; limit: number } {
  const q: { text?: string; source?: string; kind?: ItemKind; limit: number } = { limit: 200 }
  const text = f.text.trim()
  if (text) q.text = text
  if (f.source !== 'all') q.source = f.source
  if (f.kind !== 'all') q.kind = f.kind
  return q
}
```

- [ ] **Step 3: Implement `TimelineView.vue`**

```vue
<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import ItemCard from '../components/ItemCard.vue'
import SearchField from '../components/SearchField.vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { formatAge, formatBytes } from '../lib/format'
import { historyQuery, type TimelineFilters } from '../lib/timeline'
import type { HistoryEntry, ItemKind } from '../lib/types'

const { state } = useAppStore()
const filters = reactive<TimelineFilters>({ text: '', source: 'all', kind: 'all' })
const items = ref<HistoryEntry[]>([])
const status = ref('')

const sources = computed(() => [
  { id: 'all', name: 'All devices' },
  { id: state.deviceId, name: 'This device' },
  ...state.peers.map((p) => ({ id: p.peer.deviceId, name: p.peer.name })),
])
const kinds: { id: 'all' | ItemKind; name: string }[] = [
  { id: 'all', name: 'Everything' },
  { id: 'Text', name: 'Text' },
  { id: 'Html', name: 'Rich text' },
  { id: 'Image', name: 'Images' },
]

function nameOf(id: string) {
  return sources.value.find((s) => s.id === id)?.name ?? 'Unknown device'
}

let timer: ReturnType<typeof setTimeout> | undefined
async function load() {
  try {
    items.value = await api.history(historyQuery(filters))
  } catch {
    items.value = []
  }
}
watch(() => [filters.text, filters.source, filters.kind, state.historyVersion, state.ready], () => {
  clearTimeout(timer)
  timer = setTimeout(load, 120)
}, { immediate: true })

async function act(task: () => Promise<unknown>, done: string) {
  try {
    await task()
    status.value = done
  } catch (e) {
    status.value = String(e)
  }
  setTimeout(() => (status.value = ''), 2500)
}
</script>

<template>
  <main class="flex h-screen flex-col gap-4 p-6">
    <header class="flex flex-wrap items-center gap-4">
      <h1 class="accent-text text-xl font-semibold tracking-tight">Timeline</h1>
      <div class="w-80"><SearchField v-model="filters.text" placeholder="Search everything you copied" /></div>
      <span class="text-[12px] text-[var(--muted)]">{{ status }}</span>
    </header>
    <nav class="flex flex-wrap gap-2 text-[12px]">
      <button
        v-for="s in sources"
        :key="s.id"
        class="hoverable rounded-full border px-3 py-1"
        :class="filters.source === s.id ? 'border-[var(--accent-a)] text-[var(--text)]' : 'border-[var(--border)] text-[var(--muted)]'"
        @click="filters.source = s.id"
      >{{ s.name }}</button>
      <span class="mx-2 w-px bg-[var(--border)]" />
      <button
        v-for="k in kinds"
        :key="k.id"
        class="hoverable rounded-full border px-3 py-1"
        :class="filters.kind === k.id ? 'border-[var(--accent-b)] text-[var(--text)]' : 'border-[var(--border)] text-[var(--muted)]'"
        @click="filters.kind = k.id"
      >{{ k.name }}</button>
    </nav>
    <section class="grid min-h-0 flex-1 auto-rows-min grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-3 overflow-y-auto pr-1">
      <ItemCard
        v-for="item in items"
        :key="item.id"
        :preview="item.preview"
        :title="nameOf(item.source)"
        :meta="`${formatAge(Date.now() - item.createdMs)}, ${formatBytes(item.size)}`"
      >
        <button class="hoverable rounded-lg border border-[var(--border)] px-2.5 py-1 text-[12px] hover:bg-[var(--surface-strong)]" @click="act(() => api.copyFromHistory(item.id), 'Copied')">Copy</button>
        <button class="hoverable rounded-lg border border-[var(--border)] px-2.5 py-1 text-[12px] hover:bg-[var(--surface-strong)]" @click="act(() => api.exportItem(item.id), 'Saved')">Save as file</button>
      </ItemCard>
      <p v-if="!items.length" class="col-span-full py-16 text-center text-[13px] text-[var(--muted)]">Nothing matches. Copy something or loosen the filters.</p>
    </section>
  </main>
</template>
```

- [ ] **Step 4: Run tests and build**

Run: `npm test && npm run build`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add apps/wisp/src
git commit -m "Build the timeline window"
```

---

### Task 11: Settings, global hotkey, start at login and single instance

**Files:**
- Create: `apps/wisp/src-tauri/src/hotkey.rs`, `apps/wisp/src/lib/hotkeys.ts`
- Modify: `apps/wisp/src-tauri/src/lib.rs` (plugins, startup sync, `catch_from_hotkey`), `apps/wisp/src-tauri/src/commands.rs` (`update_settings` applies hotkey and autostart, `complete_onboarding` enables autostart), `apps/wisp/src/views/SettingsView.vue`
- Test: inline tests in `hotkey.rs`; `apps/wisp/src/__tests__/hotkeys.spec.ts`

**Interfaces:**
- Consumes: `AppSettings`, `windows`, `tray`, `AppState` (Tasks 3-5); `useAppStore`, `api` (Task 6)
- Produces:
  - `hotkey::parse(combo: &str) -> Result<Shortcut, String>`, `hotkey::apply(app, previous: Option<&str>, next: &str) -> Result<(), String>` (on failure the previous shortcut stays registered), `hotkey::plugin()`
  - `lib.rs`: `async fn catch_from_hotkey(app: &AppHandle)` (catches the newest offer; with nothing to catch it shows the panel)
  - `update_settings` rejects a hotkey that cannot be registered with a readable error and does not save it
  - `hotkeys.ts`: `comboFromEvent(e: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>): string | null` (needs at least one modifier plus a letter, digit, F-key or Space; order `Ctrl+Alt+Shift+Super+Key`), `displayCombo(combo: string, isMac: boolean): string` (`Alt` shows as `Option`, `Super` as `Cmd` on macOS)
  - `SettingsView.vue`: sections Pair a device (the `PairingCard` from Task 12; until then a placeholder line), Devices (mode, online, forget, add a known address), Shortcut (record and save), Start at login, Appearance (System / Dark / Light), Save folder (path, Change), Firewall hint (Windows only), About (version)

- [ ] **Step 1: Write the failing tests**

In a new `hotkey.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_default_and_rejects_garbage() {
        assert!(parse(crate::settings::DEFAULT_HOTKEY).is_ok());
        assert!(parse("Ctrl+Shift+Space").is_ok());
        assert!(parse("Ctrl+").is_err());
        assert!(parse("Banana+V").is_err());
        assert!(parse("").is_err());
    }
}
```

`apps/wisp/src/__tests__/hotkeys.spec.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { comboFromEvent, displayCombo } from '../lib/hotkeys'

const ev = (code: string, mods: Partial<Record<'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey', boolean>> = {}) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
})

describe('hotkeys', () => {
  it('builds combos in a fixed modifier order', () => {
    expect(comboFromEvent(ev('KeyV', { ctrlKey: true, altKey: true }))).toBe('Ctrl+Alt+V')
    expect(comboFromEvent(ev('Space', { metaKey: true, shiftKey: true }))).toBe('Shift+Super+Space')
    expect(comboFromEvent(ev('Digit5', { ctrlKey: true }))).toBe('Ctrl+5')
    expect(comboFromEvent(ev('F9', { altKey: true }))).toBe('Alt+F9')
  })
  it('needs a modifier and a real key', () => {
    expect(comboFromEvent(ev('KeyV'))).toBeNull()
    expect(comboFromEvent(ev('ControlLeft', { ctrlKey: true }))).toBeNull()
    expect(comboFromEvent(ev('Comma', { ctrlKey: true }))).toBeNull()
  })
  it('uses Mac names on macOS', () => {
    expect(displayCombo('Ctrl+Alt+V', true)).toBe('Ctrl+Option+V')
    expect(displayCombo('Super+Shift+V', true)).toBe('Cmd+Shift+V')
    expect(displayCombo('Ctrl+Alt+V', false)).toBe('Ctrl+Alt+V')
  })
})
```

Run: `cargo test -p wisp-app hotkey` and `npm test`
Expected: FAIL.

- [ ] **Step 2: Implement `hotkey.rs`** (above the tests)

```rust
use std::str::FromStr;

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub fn parse(combo: &str) -> Result<Shortcut, String> {
    let combo = combo.trim();
    if combo.is_empty() {
        return Err("The shortcut is empty".into());
    }
    Shortcut::from_str(combo).map_err(|e| format!("\"{combo}\" is not a valid shortcut ({e})"))
}

/// Replaces the registered catch shortcut. On failure the previous one stays active.
pub fn apply(app: &AppHandle, previous: Option<&str>, next: &str) -> Result<(), String> {
    let next_shortcut = parse(next)?;
    let shortcuts = app.global_shortcut();
    let previous_shortcut = previous.and_then(|p| parse(p).ok());
    if let Some(prev) = previous_shortcut {
        if prev == next_shortcut && shortcuts.is_registered(prev) {
            return Ok(());
        }
        let _ = shortcuts.unregister(prev);
    }
    if let Err(e) = shortcuts.register(next_shortcut) {
        if let Some(prev) = previous_shortcut {
            let _ = shortcuts.register(prev);
        }
        return Err(format!("{next} could not be registered, another app may use it ({e})"));
    }
    Ok(())
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { crate::catch_from_hotkey(&app).await });
            }
        })
        .build()
}
```

- [ ] **Step 3: Wire plugins and startup in `lib.rs`**

Add `mod hotkey;`. Plugins, in this order (single instance must be first):

```rust
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = windows::show(app, windows::View::Panel);
        }))
        .plugin(hotkey::plugin())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
```

Add:

```rust
pub(crate) async fn catch_from_hotkey(app: &AppHandle) {
    let Ok(node) = app.state::<AppState>().engine.node().await else { return };
    match node.catch_latest().await {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _ = windows::show(app, windows::View::Panel);
        }
        Err(e) => {
            let _ = app.emit("wisp://event", serde_json::json!({ "type": "error", "message": e.to_string() }));
        }
    }
    tray::refresh(app).await;
}

pub(crate) fn sync_system_integration(app: &AppHandle, settings: &AppSettings) {
    use tauri_plugin_autostart::ManagerExt;
    if let Err(message) = hotkey::apply(app, None, &settings.hotkey) {
        let _ = app.emit("wisp://event", serde_json::json!({ "type": "error", "message": format!("{message}. Change it in Settings.") }));
    }
    let launcher = app.autolaunch();
    let enabled = launcher.is_enabled().unwrap_or(false);
    if settings.autostart && !enabled {
        let _ = launcher.enable();
    } else if !settings.autostart && enabled {
        let _ = launcher.disable();
    }
}
```

In `setup`, inside `if onboarded { ... }` call `sync_system_integration(app.handle(), &settings_clone)` (clone the settings before moving them into `AppState`).

- [ ] **Step 4: Apply settings in `commands.rs`**

Replace `update_settings` with:

```rust
#[tauri::command]
pub async fn update_settings(app: AppHandle, state: State<'_, AppState>, next: AppSettings) -> Cmd<AppSettings> {
    use tauri_plugin_autostart::ManagerExt;
    let previous = state.settings.lock().unwrap().clone();
    if next.hotkey != previous.hotkey {
        crate::hotkey::apply(&app, Some(&previous.hotkey), &next.hotkey)?;
    }
    if next.autostart != previous.autostart {
        let launcher = app.autolaunch();
        let result = if next.autostart { launcher.enable() } else { launcher.disable() };
        result.map_err(|e| format!("Could not change start at login ({e})"))?;
    }
    let saved = save_settings(&app, &state, next)?;
    if saved.onboarded && saved.save_dir != previous.save_dir {
        crate::start_engine(&app).await;
    }
    Ok(saved)
}
```

In `complete_onboarding`, after `crate::start_engine(&app).await;` add `crate::sync_system_integration(&app, &state.settings.lock().unwrap().clone());`.

- [ ] **Step 5: Implement `hotkeys.ts`**

```ts
type KeyLike = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>

function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3)
  if (/^Digit[0-9]$/.test(code)) return code.slice(5)
  if (/^F([1-9]|1[0-2])$/.test(code)) return code
  if (code === 'Space') return 'Space'
  return null
}

export function comboFromEvent(e: KeyLike): string | null {
  const key = keyName(e.code)
  const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super'].filter(Boolean)
  if (!key || mods.length === 0) return null
  return [...mods, key].join('+')
}

export function displayCombo(combo: string, isMac: boolean): string {
  if (!isMac) return combo
  return combo
    .split('+')
    .map((part) => (part === 'Alt' ? 'Option' : part === 'Super' ? 'Cmd' : part))
    .join('+')
}
```

- [ ] **Step 6: Implement `SettingsView.vue`**

```vue
<script setup lang="ts">
import { computed, ref } from 'vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { comboFromEvent, displayCombo } from '../lib/hotkeys'
import { osLabel } from '../lib/format'
import type { AppSettings, Theme } from '../lib/types'

const { state, settings } = useAppStore()
const isMac = computed(() => state.platform === 'macos')
const recording = ref(false)
const message = ref('')
const addressInputs = ref<Record<string, string>>({})

async function save(patch: Partial<AppSettings>) {
  if (!settings.value) return
  try {
    settings.value = await api.updateSettings({ ...settings.value, ...patch })
    message.value = 'Saved'
  } catch (e) {
    message.value = String(e)
  }
  setTimeout(() => (message.value = ''), 3000)
}

function record(e: KeyboardEvent) {
  if (!recording.value) return
  e.preventDefault()
  if (e.key === 'Escape') {
    recording.value = false
    return
  }
  const combo = comboFromEvent(e)
  if (combo) {
    recording.value = false
    void save({ hotkey: combo })
  }
}

async function changeFolder() {
  const picked = await api.pickFolder()
  if (picked) await save({ saveDir: picked })
}

async function addAddress(peer: string) {
  const addr = addressInputs.value[peer]?.trim()
  if (!addr) return
  try {
    await api.addAddress(peer, addr)
    message.value = 'Address added, connecting'
    addressInputs.value[peer] = ''
  } catch (e) {
    message.value = String(e)
  }
}

const themes: { id: Theme; name: string }[] = [
  { id: 'system', name: 'System' },
  { id: 'dark', name: 'Dark' },
  { id: 'light', name: 'Light' },
]
</script>

<template>
  <main class="flex h-screen flex-col gap-6 overflow-y-auto p-6 text-[13px]" @keydown="record" tabindex="-1">
    <header class="flex items-baseline justify-between">
      <h1 class="accent-text text-xl font-semibold tracking-tight">Settings</h1>
      <span class="text-[12px] text-[var(--muted)]">{{ message }}</span>
    </header>

    <section class="flex flex-col gap-2">
      <h2 class="text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">Pair a device</h2>
      <slot name="pairing"><p class="text-[var(--muted)]">Pairing arrives in the next step.</p></slot>
    </section>

    <section class="flex flex-col gap-2">
      <h2 class="text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">Devices</h2>
      <p v-if="!state.peers.length" class="text-[var(--muted)]">No paired devices yet.</p>
      <article v-for="p in state.peers" :key="p.peer.deviceId" class="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-3">
        <div class="flex items-center justify-between">
          <span class="font-medium">{{ p.peer.name }} <span class="text-[var(--muted)]">{{ osLabel(p.peer.os) }}</span></span>
          <span :class="p.online ? 'text-[var(--accent-a)]' : 'text-[var(--muted)]'">{{ p.online ? 'Online' : 'Offline' }}</span>
        </div>
        <div class="mt-2 flex flex-wrap items-center gap-2">
          <select
            class="rounded-lg border border-[var(--border)] bg-[var(--surface)] px-2 py-1"
            :value="p.peer.mode"
            @change="api.setMode(p.peer.deviceId, ($event.target as HTMLSelectElement).value as 'Catch' | 'AutoSync').then(() => (p.peer.mode = ($event.target as HTMLSelectElement).value as 'Catch' | 'AutoSync'))"
          >
            <option value="Catch">Catch: ask me first</option>
            <option value="AutoSync">Auto-sync: put it on my clipboard</option>
          </select>
          <input v-model="addressInputs[p.peer.deviceId]" class="w-40 rounded-lg border border-[var(--border)] bg-transparent px-2 py-1" placeholder="Add IP address" />
          <button class="hoverable rounded-lg border border-[var(--border)] px-2 py-1 hover:bg-[var(--surface-strong)]" @click="addAddress(p.peer.deviceId)">Add</button>
          <button class="hoverable ml-auto rounded-lg px-2 py-1 text-[var(--danger)] hover:bg-[var(--surface-strong)]" @click="api.forget(p.peer.deviceId).then(() => (state.peers = state.peers.filter((x) => x.peer.deviceId !== p.peer.deviceId)))">Forget</button>
        </div>
      </article>
    </section>

    <section v-if="settings" class="flex flex-col gap-3">
      <h2 class="text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">Catch shortcut</h2>
      <div class="flex items-center gap-3">
        <kbd class="rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 font-mono">{{ recording ? 'Press keys, Esc to cancel' : displayCombo(settings.hotkey, isMac) }}</kbd>
        <button class="hoverable rounded-lg border border-[var(--border)] px-3 py-1.5 hover:bg-[var(--surface-strong)]" @click="recording = true">Change</button>
      </div>

      <h2 class="mt-2 text-[11px] font-medium uppercase tracking-wider text-[var(--muted)]">General</h2>
      <label class="flex items-center gap-2">
        <input type="checkbox" :checked="settings.autostart" @change="save({ autostart: ($event.target as HTMLInputElement).checked })" />
        Start Wisp when I log in
      </label>
      <div class="flex items-center gap-2">
        <span>Appearance</span>
        <button
          v-for="t in themes"
          :key="t.id"
          class="hoverable rounded-full border px-3 py-1"
          :class="settings.theme === t.id ? 'border-[var(--accent-a)]' : 'border-[var(--border)] text-[var(--muted)]'"
          @click="save({ theme: t.id })"
        >{{ t.name }}</button>
      </div>
      <div class="flex items-center gap-2">
        <span class="shrink-0">Save files to</span>
        <code class="truncate rounded bg-[var(--surface)] px-2 py-1 font-mono text-[12px]">{{ settings.saveDir ?? 'Downloads/Wisp' }}</code>
        <button class="hoverable rounded-lg border border-[var(--border)] px-3 py-1 hover:bg-[var(--surface-strong)]" @click="changeFolder">Change</button>
      </div>
    </section>

    <section v-if="state.platform === 'windows'" class="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-3 text-[12px]">
      <h2 class="mb-1 font-medium">Windows Firewall</h2>
      <p class="text-[var(--muted)]">When Windows asks whether Wisp may use the network, allow it on private networks. Without that, other devices cannot reach this one.</p>
    </section>

    <footer class="text-[11px] text-[var(--muted)]">Wisp 0.1.0, {{ state.deviceName }} on port {{ state.port }}</footer>
  </main>
</template>
```

- [ ] **Step 7: Run tests, build and clippy**

Run: `npm test && npm run build && cargo test -p wisp-app && cargo clippy -p wisp-app --all-targets -- -D warnings`
Expected: all pass and clean.

- [ ] **Step 8: Manual check of shortcut conflicts**

In Settings, record a shortcut another running app owns (for example one bound in a screenshot tool). Expected: a readable error, and the previous shortcut still catches (Review Focus 3). Record the result in the report.

- [ ] **Step 9: Commit**

```bash
git add apps/wisp
git commit -m "Add settings, the catch shortcut, start at login and single instance"
```

---

### Task 12: First run and pairing

**Files:**
- Create: `apps/wisp/src/lib/pairing.ts`, `apps/wisp/src/views/PairingCard.vue`
- Modify: `apps/wisp/src/views/OnboardingView.vue`, `apps/wisp/src/views/SettingsView.vue` (use `PairingCard` in the pairing section), `apps/wisp/src-tauri/src/commands.rs` (`default_device_name` command), `apps/wisp/src/lib/api.ts` (`defaultDeviceName`)
- Test: `apps/wisp/src/__tests__/pairing.spec.ts`

**Interfaces:**
- Consumes: `api.beginPairing`, `api.cancelPairing`, `api.pairWith`, `api.completeOnboarding`, `useAppStore` (`state.nearby`, `state.peers`)
- Produces:
  - `pairing.ts`: `normalizeCode(input: string): string | null` (keeps digits, returns them when exactly 6), `formatCode(code: string): string` (`"123456"` to `"123 456"`)
  - Command `default_device_name() -> String` (uses `engine::default_device_name`), registered in `generate_handler!`; `api.defaultDeviceName()`
  - `PairingCard.vue`: tabs "Show a code" (calls `beginPairing`, shows the code large, waits; success when a new peer appears; cancels on tab switch or unmount) and "Enter a code" (pick a nearby device or type an IP, enter the 6 digits, Pair)
  - `OnboardingView.vue`: step 1 names the device (prefilled with the host name), Continue calls `completeOnboarding`; step 2 shows `PairingCard`, the Windows firewall hint, and Done (hides onboarding, opens the panel)

- [ ] **Step 1: Write the failing tests**

```ts
import { describe, expect, it } from 'vitest'
import { formatCode, normalizeCode } from '../lib/pairing'

describe('pairing codes', () => {
  it('accepts six digits with any spacing', () => {
    expect(normalizeCode('123 456')).toBe('123456')
    expect(normalizeCode(' 12-34-56 ')).toBe('123456')
    expect(normalizeCode('12345')).toBeNull()
    expect(normalizeCode('1234567')).toBeNull()
    expect(normalizeCode('abcdef')).toBeNull()
  })
  it('groups codes for reading', () => {
    expect(formatCode('123456')).toBe('123 456')
  })
})
```

Save as `apps/wisp/src/__tests__/pairing.spec.ts`; run `npm test`, expected FAIL.

- [ ] **Step 2: Implement `pairing.ts`**

```ts
export function normalizeCode(input: string): string | null {
  const digits = input.replace(/\D/g, '')
  return digits.length === 6 ? digits : null
}

export function formatCode(code: string): string {
  return `${code.slice(0, 3)} ${code.slice(3)}`
}
```

- [ ] **Step 3: Add the `default_device_name` command**

In `commands.rs`:

```rust
#[tauri::command]
pub fn default_device_name() -> String {
    crate::engine::default_device_name()
}
```

Register it in `generate_handler!`. In `api.ts` add `defaultDeviceName: () => invoke<string>('default_device_name'),`.

In `complete_onboarding`, remove the lines that hide the onboarding window and show the panel (added in Task 5); the onboarding view now continues to its pairing step and closes itself.

- [ ] **Step 4: Implement `PairingCard.vue`**

```vue
<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { api } from '../lib/api'
import { useAppStore } from '../lib/appStore'
import { formatCode, normalizeCode } from '../lib/pairing'

const { state } = useAppStore()
const tab = ref<'show' | 'enter'>('show')
const code = ref<string | null>(null)
const startCount = ref(0)
const success = ref<string | null>(null)
const error = ref('')
const target = ref<string>('')
const typedAddress = ref('')
const typedCode = ref('')
const pairing = ref(false)

const address = computed(() => typedAddress.value.trim() || target.value)

async function showCode() {
  error.value = ''
  success.value = null
  startCount.value = state.peers.length
  try {
    code.value = await api.beginPairing()
  } catch (e) {
    error.value = String(e)
  }
}

watch(
  () => state.peers.length,
  (n) => {
    if (tab.value === 'show' && code.value && n > startCount.value) {
      success.value = state.peers[state.peers.length - 1]?.peer.name ?? 'a new device'
      code.value = null
    }
  },
)

watch(tab, (next, prev) => {
  if (prev === 'show' && code.value) void api.cancelPairing()
  code.value = null
  error.value = ''
  success.value = null
  if (next === 'show') void showCode()
}, { immediate: false })

async function pair() {
  const c = normalizeCode(typedCode.value)
  if (!c) {
    error.value = 'The code has 6 digits'
    return
  }
  if (!address.value) {
    error.value = 'Pick a device or type its IP address'
    return
  }
  pairing.value = true
  error.value = ''
  try {
    const peer = await api.pairWith(address.value, c)
    success.value = peer.name
    typedCode.value = ''
  } catch (e) {
    error.value = String(e)
  } finally {
    pairing.value = false
  }
}

void showCode()
onUnmounted(() => {
  if (code.value) void api.cancelPairing()
})
</script>

<template>
  <div class="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 text-[13px]">
    <div class="mb-4 flex gap-2">
      <button class="hoverable rounded-full border px-3 py-1" :class="tab === 'show' ? 'border-[var(--accent-a)]' : 'border-[var(--border)] text-[var(--muted)]'" @click="tab = 'show'">Show a code</button>
      <button class="hoverable rounded-full border px-3 py-1" :class="tab === 'enter' ? 'border-[var(--accent-a)]' : 'border-[var(--border)] text-[var(--muted)]'" @click="tab = 'enter'">Enter a code</button>
    </div>

    <p v-if="success" class="text-[var(--accent-a)]">Paired with {{ success }}. You are all set.</p>

    <template v-else-if="tab === 'show'">
      <p class="text-[var(--muted)]">On your other device, open Wisp, choose "Enter a code" and type:</p>
      <p v-if="code" class="accent-text my-4 text-center font-mono text-4xl font-semibold tracking-[0.2em]">{{ formatCode(code) }}</p>
      <p class="text-[11px] text-[var(--muted)]">Waiting for the other device. Three wrong tries end this code.</p>
      <button v-if="!code" class="hoverable mt-2 rounded-lg border border-[var(--border)] px-3 py-1.5" @click="showCode">New code</button>
    </template>

    <template v-else>
      <p class="mb-2 text-[var(--muted)]">Pick the device showing the code:</p>
      <div class="mb-3 flex flex-wrap gap-2">
        <button
          v-for="n in state.nearby"
          :key="n.device"
          class="hoverable rounded-full border px-3 py-1"
          :class="target === n.addresses[0] ? 'border-[var(--accent-a)]' : 'border-[var(--border)]'"
          :disabled="!n.addresses.length"
          @click="target = n.addresses[0] ?? ''; typedAddress = ''"
        >{{ n.name }}</button>
        <span v-if="!state.nearby.length" class="text-[12px] text-[var(--muted)]">No devices found nearby. Type the IP address instead.</span>
      </div>
      <div class="flex flex-wrap gap-2">
        <input v-model="typedAddress" class="w-44 rounded-lg border border-[var(--border)] bg-transparent px-2 py-1.5" placeholder="or IP address" />
        <input v-model="typedCode" inputmode="numeric" class="w-32 rounded-lg border border-[var(--border)] bg-transparent px-2 py-1.5 font-mono tracking-widest" placeholder="123 456" @keydown.enter="pair" />
        <button
          class="hoverable rounded-lg px-4 py-1.5 font-medium text-[var(--bg)]"
          style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))"
          :disabled="pairing"
          @click="pair"
        >{{ pairing ? 'Pairing' : 'Pair' }}</button>
      </div>
    </template>

    <p v-if="error" class="mt-3 text-[12px] text-[var(--danger)]">{{ error }}</p>
  </div>
</template>
```

- [ ] **Step 5: Use `PairingCard` in `SettingsView.vue`**

Replace the `<slot name="pairing">...</slot>` placeholder with `<PairingCard />` and import it (`import PairingCard from './PairingCard.vue'`).

- [ ] **Step 6: Implement `OnboardingView.vue`**

```vue
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import PairingCard from './PairingCard.vue'
import { api } from '../lib/api'

const step = ref<'name' | 'pair'>('name')
const name = ref('')
const error = ref('')
const busy = ref(false)
const isWindows = navigator.userAgent.includes('Windows')

onMounted(async () => {
  name.value = await api.defaultDeviceName()
})

async function next() {
  busy.value = true
  error.value = ''
  try {
    await api.completeOnboarding(name.value)
    step.value = 'pair'
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

async function done() {
  await api.openWindow('panel')
  await api.hideWindow('onboarding')
}
</script>

<template>
  <main class="glass flex h-screen flex-col gap-5 p-8">
    <header data-tauri-drag-region class="flex items-center justify-between">
      <h1 class="accent-text text-2xl font-semibold tracking-tight" data-tauri-drag-region>Welcome to Wisp</h1>
      <span class="text-[12px] text-[var(--muted)]">{{ step === 'name' ? '1 of 2' : '2 of 2' }}</span>
    </header>

    <section v-if="step === 'name'" class="flex flex-1 flex-col gap-4">
      <p class="text-[14px] text-[var(--muted)]">Your clipboard, on every computer you own. First, what should other devices call this one?</p>
      <input
        v-model="name"
        maxlength="64"
        class="rounded-xl border border-[var(--border)] bg-[var(--surface)] px-4 py-3 text-[15px] outline-none focus:border-[var(--accent-a)]"
        @keydown.enter="next"
      />
      <p v-if="error" class="text-[12px] text-[var(--danger)]">{{ error }}</p>
      <button
        class="hoverable mt-auto self-end rounded-xl px-5 py-2.5 font-medium text-[var(--bg)]"
        style="background: linear-gradient(120deg, var(--accent-a), var(--accent-b))"
        :disabled="busy || !name.trim()"
        @click="next"
      >Continue</button>
    </section>

    <section v-else class="flex flex-1 flex-col gap-4">
      <p class="text-[14px] text-[var(--muted)]">Now connect your other computer. Install Wisp there too, then pair with a code.</p>
      <PairingCard />
      <p v-if="isWindows" class="text-[12px] text-[var(--muted)]">If Windows asks whether Wisp may use the network, allow it on private networks.</p>
      <button class="hoverable mt-auto self-end rounded-xl border border-[var(--border)] px-5 py-2.5 hover:bg-[var(--surface-strong)]" @click="done">Done</button>
    </section>
  </main>
</template>
```

- [ ] **Step 7: Run tests, build and clippy**

Run: `npm test && npm run build && cargo test -p wisp-app && cargo clippy -p wisp-app --all-targets -- -D warnings`
Expected: all pass and clean.

- [ ] **Step 8: Commit**

```bash
git add apps/wisp
git commit -m "Add first run and pairing"
```

---

## Smoke checklist (run once after Task 12, record results in the final report)

On this Windows PC (and on the Mac and the second Windows machine when available):

1. Fresh start (delete `%APPDATA%/io.github.wl-lankin.wisp`): onboarding appears, name prefilled, Continue shows a pairing code.
2. Second device: Enter a code (nearby list or IP) pairs both; both panels show the other orb online.
3. `Cmd+Ctrl+Shift+4` on the Mac (or `Win+Shift+S` on Windows): the other device shows the toast with the thumbnail; Catch puts it on the clipboard; paste works.
4. Shortcut `Ctrl+Alt+V` catches the newest offer; with nothing waiting it opens the panel.
5. Auto-sync one direction: copies land without a toast; nothing bounces back.
6. Drag a file onto an orb: progress shows, the file arrives in Downloads/Wisp.
7. Timeline: search, filters, Copy and Save as file work.
8. Settings: theme switch, start at login toggle, shortcut change, save folder change (engine restarts, peers reconnect).
9. Second launch of the app shows the existing panel. Quit from the tray menu exits.
10. Windows with animations off: hover states still fade, nothing jumps.

## Rulings made while planning (deviations from the spec, for the human to review)

- Connection lines show online and offline only; the spec's cable versus Wi-Fi styles need link-type data the core does not have yet. Deferred.
- No QR code for pairing: there is no phone app, so a desktop cannot scan it. Codes only.
- Link previews show the domain and a link glyph, no favicon (no network fetches from the app).
- The tray shows a pending dot for uncaught offers; the extra "glow while receiving" is deferred.
- Renaming the device after first run and a history size setting are deferred (the core sets the name at first run and keeps 200 items).
- Dragging items out of the timeline is deferred; "Save as file" writes to the save folder and reveals the file instead.
- "Save as file" on a toast catches the item first, so it also lands on the clipboard.
- Windows firewall: a hint in onboarding and Settings; the installer rule comes in Plan 3.
- Timeline and Settings are normal windows (they get a taskbar button while open); the panel, toast and onboarding windows stay out of the taskbar.

## Done criteria for Plan 2

- `npm test` and `npm run build` pass in `apps/wisp`; `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- `npm run tauri dev` runs the app on Windows with the smoke checklist items 1, 4, 7, 8, 9 and 10 passing on one machine; items 2, 3, 5 and 6 pass between two machines (or are recorded as not yet run).
