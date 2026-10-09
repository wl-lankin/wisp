# Wisp Brand, Installers and CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Wisp its final logo and tray icons, produce a Windows NSIS installer (with a firewall rule) and a universal macOS `.dmg`, and run tests plus release builds in GitHub Actions.

**Architecture:** Hand-made vector artwork in `apps/wisp/brand/` is the single source for every icon (`tauri icon` regenerates the bundle icons; the tray PNGs are rendered from their own SVGs). Bundling is configured in `tauri.conf.json` plus an NSIS hook file and a macOS `Info.plist`. A small Node script keeps the four version fields in sync. Two workflows: `ci.yml` (tests and lint on Windows and macOS for every push and PR) and `release.yml` (tag `v*` builds installers with `tauri-apps/tauri-action` into a draft GitHub release).

**Tech Stack:** Tauri 2 bundler (NSIS, dmg), GitHub Actions (`actions/checkout@v4`, `actions/setup-node@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `tauri-apps/tauri-action@v0`), Node 22+ script, Vitest.

**Spec:** `docs/superpowers/specs/2026-10-09-wisp-design.md` (sections 4 Brand, 5 firewall row, 7 Build and distribution).
**Builds on:** Plans 1 and 2. Branch `release`, created from `tray-ui`.
**Plan series:** 3 of 3.

**Version drift:** if an action, CLI flag or config key differs in the current Tauri 2 release, adapt and note it; keep the behavior this plan names.

## Global Constraints

- Logo: a single flowing swirl in the teal `#2DE2C5` to violet `#8B5CF6` gradient on ink `#0B0D14`; it must read clearly at 16 px.
- Tray icon: monochrome template image on macOS, color glyph on Windows; a pending variant of each.
- Windows: NSIS installer, per-machine install, adds an inbound firewall rule for Wisp on private networks and removes it on uninstall.
- macOS: universal `.dmg` (Apple silicon and Intel), unsigned in v1, no dock icon (`LSUIElement`), minimum macOS 11.
- Releases build on tag push `v*` into a draft GitHub release; nothing is published automatically.
- One version for the app: `apps/wisp/package.json`, `apps/wisp/src-tauri/Cargo.toml`, `apps/wisp/src-tauri/tauri.conf.json`, `crates/wisp-core/Cargo.toml` must always match.
- No em-dashes or en-dashes anywhere. No `#[allow]` / `@ts-ignore`. Plain sentence commit messages. Do not push (the controller pushes when the human approves).

## Review Focus

1. **Firewall rule scope:** the rule must target the installed exe path only, profile private only, and be removed on uninstall (no orphan allow rules). Pinned in Task 3.
2. **Tray icon legibility:** at 16 and 32 px the swirl must not turn into a blob; the pending badge must be distinguishable from the comet dot. Pinned in Task 2 (rendered previews committed).
3. **CI secrets:** workflows use only `GITHUB_TOKEN`, with the narrowest permissions per job. Pinned in Task 5.
4. **Release safety:** a tag build creates a draft release only; a CI failure on one OS must not publish a half release. Pinned in Task 5.
5. **Toast error visibility:** a clipboard write failure after a hotkey catch must surface even when the toast was hidden. Pinned in Task 1.

---

### Task 1: Show engine errors in the toast even when it was hidden

This closes the gap parked at the end of Plan 2 (`docs/superpowers/followups/2026-10-09-plan-2-deferred.md`).

**Files:**
- Modify: `apps/wisp/src-tauri/src/lib.rs` (`TauriSink::react`), `apps/wisp/src/views/ToastView.vue`
- Test: `apps/wisp/src/__tests__/toastview.spec.ts`

**Interfaces:**
- Produces: on `Event::Error` whose message is the clipboard write failure (`"Couldn't write the clipboard. Try again."`), Rust shows the toast window (without focus), like on `OfferReceived`. ToastView treats an incoming error notice like an offer wake-up: resets `hidden`, `lastChange`, opacity and the fade timer, and shows the error; the "stale error" filter only drops errors that were already shown or dismissed, never one that arrived while hidden.

- [ ] **Step 1: Write the failing test** (append to `toastview.spec.ts`, reusing its mocks and fake timers)

```ts
it('comes back to show a clipboard error that arrives while hidden', async () => {
  // Arrange: one offer, let the toast auto-hide after 8 s.
  // Act: push the error notice through the store the same way appStore does for an engine error event.
  // Assert: hidden is cleared (opacity restored), the error text is rendered,
  // and hideWindow is not called again while the error is shown.
})
```

Write the body with the helpers already in the file (mount with an inbox of one offer, `vi.advanceTimersByTime(9000)`, then `pushNotice(state, 'error', "Couldn't write the clipboard. Try again.")`, `await flushPromises()`), asserting `wrapper.text()` contains the message and the hidden flag/opacity reset (inspect the outer wrapper's style or the exposed state the existing tests use).

Run: `cd apps/wisp && npx vitest run src/__tests__/toastview.spec.ts`
Expected: FAIL (error not shown after hide).

- [ ] **Step 2: Implement**

`lib.rs`, in `TauriSink::react`, add an arm:

```rust
            Event::Error { message } if message.contains("clipboard") => {
                let _ = windows::show(&self.app, windows::View::Toast);
            }
```

(keep it before the catch-all arm; the toast window is created hidden on demand and is `focusable(false)`, so this never steals focus).

`ToastView.vue`: watch the store's notices for new error ids; when a new error arrives, run the same reset path the offer counter uses (clear the fade timer, `hidden = false`, `lastChange = Date.now()`, opacity back to 1) and mark it as shown; restrict the stale-error filter to ids already shown or dismissed.

- [ ] **Step 3: Verify**

Run: `npm test && npm run build && cargo test -p wisp-app && cargo clippy -p wisp-app --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add apps/wisp
git commit -m "Bring the toast back for clipboard errors"
```

---

### Task 2: Final logo and tray icons

**Files:**
- Create: `apps/wisp/brand/logo.svg`, `apps/wisp/brand/tray-mono.svg`, `apps/wisp/brand/tray-mono-pending.svg`, `apps/wisp/brand/tray-color.svg`, `apps/wisp/brand/tray-color-pending.svg`, `apps/wisp/brand/preview.html` (renders every icon at 16, 20, 24, 32, 64 px on light and dark backgrounds), `apps/wisp/brand/README.md` (one paragraph: palette, usage, regeneration commands)
- Delete: `apps/wisp/app-icon.svg`, `apps/wisp/tray-*.svg` (replaced by `brand/`)
- Regenerate: `apps/wisp/src-tauri/icons/**` (bundle icons and the four tray PNG folders)
- Modify: `apps/wisp/src-tauri/src/tray.rs` only if the tray PNG paths change (keep them the same)

**Interfaces:**
- Produces: same icon file paths as today (`icons/32x32.png` ... `icons/icon.ico`, `icons/tray-*/32x32.png`), new artwork.

- [ ] **Step 1: Create the artwork**

`apps/wisp/brand/logo.svg`:

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
  <defs>
    <linearGradient id="stroke" x1="0.1" y1="0.9" x2="0.9" y2="0.1">
      <stop offset="0" stop-color="#2DE2C5"/>
      <stop offset="1" stop-color="#8B5CF6"/>
    </linearGradient>
    <radialGradient id="glow" cx="0.5" cy="0.45" r="0.6">
      <stop offset="0" stop-color="#8B5CF6" stop-opacity="0.35"/>
      <stop offset="1" stop-color="#8B5CF6" stop-opacity="0"/>
    </radialGradient>
    <filter id="soft" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="10"/>
    </filter>
  </defs>
  <rect width="512" height="512" rx="116" fill="#0B0D14"/>
  <rect width="512" height="512" rx="116" fill="url(#glow)"/>
  <g fill="none" stroke-linecap="round" stroke-linejoin="round">
    <path d="M136 356C136 244 222 168 314 188C386 204 404 292 346 324C298 351 246 312 266 268C279 240 322 242 328 268"
          stroke="url(#stroke)" stroke-width="46" opacity="0.55" filter="url(#soft)"/>
    <path d="M136 356C136 244 222 168 314 188C386 204 404 292 346 324C298 351 246 312 266 268C279 240 322 242 328 268"
          stroke="url(#stroke)" stroke-width="42"/>
  </g>
  <circle cx="384" cy="132" r="26" fill="#2DE2C5"/>
  <circle cx="384" cy="132" r="44" fill="#2DE2C5" opacity="0.18"/>
</svg>
```

`apps/wisp/brand/tray-color.svg` (Windows, 64 viewBox, thicker stroke, no glow):

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="0.1" y1="0.9" x2="0.9" y2="0.1">
      <stop offset="0" stop-color="#2DE2C5"/>
      <stop offset="1" stop-color="#8B5CF6"/>
    </linearGradient>
  </defs>
  <path d="M12 48C12 31 25 20 39 23C50 25 53 38 44 43C37 47 29 41 32 34C34 30 40 30 41 34"
        fill="none" stroke="url(#g)" stroke-width="7.5" stroke-linecap="round" stroke-linejoin="round"/>
  <circle cx="50" cy="13" r="5" fill="#2DE2C5"/>
</svg>
```

`tray-color-pending.svg`: same plus a badge that cannot be confused with the comet dot: `<circle cx="52" cy="52" r="10" fill="#8B5CF6" stroke="#0B0D14" stroke-width="3"/>`.

`tray-mono.svg` / `tray-mono-pending.svg`: same geometry, all fills and strokes `#000` (template images), pending badge `<circle cx="52" cy="52" r="10" fill="#000"/>`.

`apps/wisp/brand/preview.html`: a static page (no scripts from the network) with `<img>` tags of the five SVGs at 16, 20, 24, 32 and 64 px on a `#0B0D14` row and a `#F5F6FA` row, plus the generated tray PNGs at 1x and 2x.

- [ ] **Step 2: Regenerate icons**

```bash
cd apps/wisp
npx tauri icon brand/logo.svg
npx tauri icon brand/tray-mono.svg -o src-tauri/icons/tray-mono --png 32
npx tauri icon brand/tray-mono-pending.svg -o src-tauri/icons/tray-mono-pending --png 32
npx tauri icon brand/tray-color.svg -o src-tauri/icons/tray-color --png 32
npx tauri icon brand/tray-color-pending.svg -o src-tauri/icons/tray-color-pending --png 32
```

Delete the generated `icons/android` and `icons/ios` folders (Wisp targets desktop only) and keep them out with `apps/wisp/src-tauri/icons/android/` and `.../ios/` lines in `.gitignore`. Remove the old `apps/wisp/app-icon.svg` and `apps/wisp/tray-*.svg`.

- [ ] **Step 3: Legibility check**

Open `apps/wisp/brand/preview.html` in a browser (or render it with Playwright if available) and screenshot it to `apps/wisp/brand/preview.png`. Check at 16 px: the swirl is still a curl, not a blob; the pending badge is clearly separate from the comet dot. If not, thicken the stroke or simplify the path and regenerate. Commit `preview.png`.

- [ ] **Step 4: Verify**

Run: `cargo build -p wisp-app && cargo test -p wisp-app`
Expected: build succeeds (the tray `include_bytes!` paths still resolve), tests pass.

- [ ] **Step 5: Commit**

```bash
git add apps/wisp .gitignore
git commit -m "Add the final Wisp logo and tray icons"
```

---

### Task 3: Bundle configuration, NSIS firewall hook and macOS Info.plist

**Files:**
- Modify: `apps/wisp/src-tauri/tauri.conf.json`
- Create: `apps/wisp/src-tauri/windows/hooks.nsh`, `apps/wisp/src-tauri/Info.plist`

**Interfaces:**
- Produces: `npm run tauri build` on Windows yields `src-tauri/target/release/bundle/nsis/Wisp_<version>_x64-setup.exe`; on macOS with `--target universal-apple-darwin` a universal `.dmg`.

- [ ] **Step 1: `tauri.conf.json`**

Set `"mainBinaryName": "Wisp"` at the top level and replace the `bundle` block with:

```json
  "bundle": {
    "active": true,
    "targets": ["nsis", "dmg", "app"],
    "publisher": "Wolfgang Linz",
    "category": "Utility",
    "shortDescription": "Share your clipboard and files between your computers",
    "longDescription": "Wisp shares clipboard content and files between the computers on your home network. Peer to peer, encrypted, no cloud.",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"],
    "windows": {
      "nsis": {
        "installMode": "perMachine",
        "installerHooks": "windows/hooks.nsh",
        "displayLanguageSelector": false
      }
    },
    "macOS": {
      "minimumSystemVersion": "11.0"
    }
  }
```

(Per-machine install is needed to add a firewall rule; it costs one UAC prompt at install time.)

- [ ] **Step 2: `windows/hooks.nsh`**

```nsis
!macro NSIS_HOOK_POSTINSTALL
  ; Let other Wisp devices on private networks reach this one.
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Wisp"'
  nsExec::ExecToLog 'netsh advfirewall firewall add rule name="Wisp" dir=in action=allow program="$INSTDIR\Wisp.exe" enable=yes profile=private protocol=UDP'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Wisp"'
!macroend
```

(QUIC and mDNS both use UDP. The delete before add keeps reinstalls from stacking duplicate rules.)

- [ ] **Step 3: `Info.plist`** (Tauri merges it into the app bundle)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>LSUIElement</key>
  <true/>
  <key>NSLocalNetworkUsageDescription</key>
  <string>Wisp finds and talks to your other computers on this network.</string>
  <key>NSBonjourServices</key>
  <array>
    <string>_wisp._udp</string>
  </array>
</dict>
</plist>
```

(`NSLocalNetworkUsageDescription` and `NSBonjourServices` are required on macOS for local network discovery.)

- [ ] **Step 4: Build the Windows installer locally**

Run: `cd apps/wisp && npm run tauri build`
Expected: the NSIS installer exists under `src-tauri/target/release/bundle/nsis/`, named with the product name and version; the main binary inside is `Wisp.exe` (check `$INSTDIR` naming matches the hook's `Wisp.exe`; if Tauri names it differently, fix the hook, not the config). Do not run the installer unattended (it needs UAC); record the produced file name and size in the report. If `mainBinaryName` is not supported by the installed CLI, remove it and point the hook at the actual exe name.

- [ ] **Step 5: Commit**

```bash
git add apps/wisp/src-tauri
git commit -m "Configure the installers with a firewall rule and a menu bar only macOS app"
```

---

### Task 4: One app version everywhere

**Files:**
- Create: `apps/wisp/scripts/versions.mjs` (pure functions + CLI), `apps/wisp/src/__tests__/versions.spec.ts`
- Modify: `apps/wisp/package.json` (scripts `check-versions`, `set-version`)

**Interfaces:**
- Produces: `readVersions(root): Record<string, string>` (reads the four files listed in Global Constraints), `mismatches(versions): string[]`, `setVersion(root, version)` (rewrites all four, Cargo `version = "x"` in the `[package]` section only, JSON `version` keys), CLI: `node scripts/versions.mjs check` (exit 1 with a readable list on mismatch) and `node scripts/versions.mjs set 0.2.0` (rejects anything not matching `^\d+\.\d+\.\d+$`).

- [ ] **Step 1: Failing tests** (`versions.spec.ts`): build a temp directory tree with the four files, assert `readVersions` finds all four, `mismatches` is empty when equal and names the odd file otherwise, `setVersion` updates all four and leaves dependency versions in Cargo.toml untouched (e.g. a `serde = "1"` line), and an invalid version throws.

Run: `cd apps/wisp && npx vitest run src/__tests__/versions.spec.ts` (add `scripts/**` handling to the Vitest include if needed by importing the module via a relative path from the test). Expected: FAIL.

- [ ] **Step 2: Implement `versions.mjs`** with Node built-ins only (`node:fs`, `node:path`, `node:url`); TOML handled by a line-based edit limited to the `[package]` table. Add a sibling `scripts/versions.d.mts` declaring the three exported functions so `vue-tsc` accepts the import from the test (no `allowJs`, no ignore comments).

- [ ] **Step 3: Scripts** in `apps/wisp/package.json`: `"check-versions": "node scripts/versions.mjs check"`, `"set-version": "node scripts/versions.mjs set"`.

- [ ] **Step 4: Verify** `npm test && npm run check-versions` (expected: pass, prints the shared version).

- [ ] **Step 5: Commit**

```bash
git add apps/wisp
git commit -m "Keep the app version in sync across all manifests"
```

---

### Task 5: GitHub Actions for CI and releases

**Files:**
- Create: `.github/workflows/ci.yml`, `.github/workflows/release.yml`

**Interfaces:**
- Produces: CI on every push and pull request (Windows and macOS); a draft release with installers for every `v*` tag.

- [ ] **Step 1: `ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main, core-engine, tray-ui, release]
  pull_request:

permissions:
  contents: read

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: apps/wisp/package-lock.json
      - name: Install frontend dependencies
        working-directory: apps/wisp
        run: npm ci
      - name: Check versions
        working-directory: apps/wisp
        run: npm run check-versions
      - name: Frontend tests
        working-directory: apps/wisp
        run: npm test
      - name: Frontend build
        working-directory: apps/wisp
        run: npm run build
      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Rust tests
        run: cargo test --workspace
```

(macOS runs the unix-only permission tests that never ran on Windows.)

- [ ] **Step 2: `release.yml`**

```yaml
name: Release

on:
  push:
    tags: ['v*']

permissions:
  contents: read

jobs:
  build:
    permissions:
      contents: write
    strategy:
      fail-fast: true
      matrix:
        include:
          - os: windows-latest
            args: ''
          - os: macos-latest
            args: '--target universal-apple-darwin'
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.os == 'macos-latest' && 'aarch64-apple-darwin,x86_64-apple-darwin' || '' }}
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: apps/wisp/package-lock.json
      - name: Install frontend dependencies
        working-directory: apps/wisp
        run: npm ci
      - name: Check the tag matches the app version
        working-directory: apps/wisp
        shell: bash
        run: |
          npm run check-versions
          test "v$(node -p "require('./package.json').version")" = "${GITHUB_REF_NAME}"
      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          projectPath: apps/wisp
          tagName: ${{ github.ref_name }}
          releaseName: Wisp ${{ github.ref_name }}
          releaseBody: See the README for install notes. macOS builds are unsigned; right-click the app and choose Open the first time.
          releaseDraft: true
          prerelease: false
          args: ${{ matrix.args }}
```

(Drafts are never visible publicly until the owner publishes them by hand.)

- [ ] **Step 3: Validate the YAML**

Run (any available tool): `npx --yes @action-validator/cli .github/workflows/ci.yml .github/workflows/release.yml` or, if unavailable, parse both files with a YAML parser (e.g. `python -c "import yaml,sys; [yaml.safe_load(open(f)) for f in sys.argv[1:]]" .github/workflows/*.yml`) and check the keys against the Actions schema by reading. Expected: valid. Real execution happens when the controller pushes after human approval.

- [ ] **Step 4: Commit**

```bash
git add .github
git commit -m "Run tests in CI and build installers for tagged releases"
```

---

### Task 6: README for users and developers

**Files:**
- Modify: `README.md`

**Interfaces:**
- Produces: sections Install (Windows: run the setup, allow it on private networks; macOS: open the dmg, drag to Applications, right-click and Open the first time, allow local network access), First run (name, pair with a code), Everyday use (catch card, `Ctrl+Alt+V` / `Ctrl+Option+V`, auto-sync per device, drag files onto a device, timeline), Privacy (peer to peer, encrypted, nothing leaves your network, history stays on each device), Develop (prerequisites Rust stable and Node 22+, `cd apps/wisp && npm install && npm run tauri dev`, tests, `npm run set-version 0.2.0`, tag `v0.2.0` to build a draft release), Status (v0.1, unsigned builds), with the logo at the top (`apps/wisp/brand/logo.svg`, 96 px).

- [ ] **Step 1: Write the README** in short, friendly English, no dashes as punctuation (use commas or periods), headings with sentence case.

- [ ] **Step 2: Check links and paths exist** (`ls` every referenced path).

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "Write the README"
```

---

## Done criteria for Plan 3

- `npm test`, `npm run build`, `npm run check-versions`, `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass locally.
- `npm run tauri build` produces the NSIS installer on Windows.
- Both workflows validate; after the human approves the push, CI is green on Windows and macOS for the `release` branch PR.
- A test tag (for example `v0.1.0`) produces a draft release with the Windows setup and the universal dmg, once the human chooses to push a tag.
