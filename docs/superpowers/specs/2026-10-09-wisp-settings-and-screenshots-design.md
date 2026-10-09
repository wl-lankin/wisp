# Wisp: Device Rename, History Size and macOS Screenshot Watcher

**Date:** 2026-10-09
**Status:** Approved in chat, awaiting written review
**Builds on:** `2026-10-09-wisp-design.md` (v1 spec), Plans 1 to 3 (branch `release`)

## 1. Purpose

Three additions the owner asked for after v1, before testing Wisp on a Windows laptop and a MacBook Air:

1. Rename this device in Settings, with other devices seeing the new name immediately.
2. Choose how many items the timeline keeps.
3. On macOS, offer screenshots that are saved as files (`Cmd+Shift+3/4/5`) to the other devices, like a copy.

The screen-edge portal from the v1 "Later" list was considered and dropped by the owner.

### Success criteria

- After renaming, a connected peer shows the new name without reconnecting; peers that connect later see it too (mDNS and Hello).
- Changing the history size takes effect without restarting the engine; lowering it removes the oldest items after the user confirms.
- A file screenshot on the Mac appears as an offer (catch card or auto-sync) on the Windows laptop within a few seconds; other images saved to the same folder are ignored.

## 2. Device rename

### Core (`wisp-core`)

- The device name becomes mutable at runtime. `Inner` keeps the current name behind a lock (or the identity behind a lock); every reader (`device_name()`, the mDNS advert, `PairInfo`, `Hello`) uses the current value.
- New `Node::rename(&self, name: &str) -> Result<String>`:
  - Trims and caps the name with the existing `cap_device_name` (64 characters, 200 bytes); rejects an empty result with a clear error.
  - Persists it to `identity.json` with the existing atomic, owner-only write; the key and device ID are unchanged.
  - Re-advertises on mDNS: the old service registration is replaced by one with the new `name` TXT value (same instance name = device ID).
  - Sends `Hello` with the new name to every connected peer. Peers already update a pinned peer's name on `Hello`; no protocol change.
  - Returns the stored (capped) name.
- Rename while the engine is (re)starting fails with the existing "still starting" error.

### App

- Command `rename_device(name) -> String`: calls `Node::rename`, stores the name in `AppSettings.device_name`, emits a resync event so open windows refresh.
- Settings gets a "This device" section: editable name field, Save button (disabled while unchanged or empty), short status line. The port stays shown in the footer.
- Onboarding is unchanged (the name chosen there is the initial name).

## 3. History size

### Core

- `History` gets a runtime cap (default 200). `History::set_cap(n)` stores it and prunes immediately; inserts prune to the current cap.
- Allowed range 20 to 2,000; values outside are rejected with a clear error.
- `History::count()` returns the number of stored items (used for the confirmation text).
- `NodeConfig` gains `history_cap: usize`; `Node::set_history_cap(n)` applies it at runtime without a restart. `HISTORY_CAP` stays as the default constant.

### App

- `AppSettings.history_size` (default 200), applied at engine start (`NodeConfig.history_cap`) and on change via `Node::set_history_cap`.
- Settings offers 50, 200, 500 and 1,000 items. Lowering below the current item count first asks: "This removes N older items. Continue?" (N from a `history_count` command). Raising needs no confirmation.

## 4. macOS screenshot watcher

### Detection

- Folder: the macOS screenshot location setting (`com.apple.screencapture` `location`), read at start; tilde expanded; fallback `~/Desktop` if unset or missing (with a notice when a configured folder is missing).
- Watch that folder non-recursively with the `notify` crate for created and renamed files.
- A file is a screenshot when it carries the `com.apple.metadata:kMDItemIsScreenCapture` extended attribute. File names are not used (they are localized, for example "Bildschirmfoto").
- Accepted extensions: png, jpg, jpeg, heic (if the `image` crate cannot decode a format, the file is skipped with a log line). Non-PNG input is converted to PNG.
- Before reading, wait until the file size is stable for two checks 200 ms apart (maximum 10 s), to avoid half-written files. Temporary dot-files are ignored.
- Each file is offered once (dedupe by path and modification time for the session).

### Delivery

- New `Node::offer_local(payload)`: the same path as a local clipboard copy (timeline item, `LocalItem` event, live offers or offline queue per peer, auto-sync handled by the receivers) but without writing to this Mac's clipboard.
- Screenshots over 50 MB use the existing oversized path (offered as a file).

### Setting

- `AppSettings.share_screenshot_files` (default true), shown only on macOS: "Share screenshots saved as files". Toggling starts or stops the watcher without restarting the engine.
- On Windows the watcher is not built or started (`Win+Shift+S` already uses the clipboard).

### Errors

- Unreadable or undecodable file: skipped, logged, no notice (avoid noise from partial files).
- Watcher cannot start (folder unreadable): one error notice "Wisp can't watch your screenshot folder"; the rest of the app keeps working.

## 5. Testing

- Core unit tests: rename persists and caps the name; a second in-process node sees the new name via `Hello` (two-node integration test); history `set_cap` prunes and rejects out-of-range values; `offer_local` produces an offer at the peer without touching the local clipboard.
- Watcher logic split into pure, cross-platform testable parts (extension filter, dot-file filter, stable-size wait using temp files, dedupe) plus a macOS-only part (xattr check, location lookup) covered by `#[cfg(target_os = "macos")]` tests that run in the macOS CI job.
- App: settings defaults and round trip for the two new fields; Vitest tests for the Settings view (rename save, history confirmation flow, the macOS-only toggle hidden on Windows).
- Manual: on the MacBook Air, `Cmd+Shift+4` a region; the Windows laptop shows the offer. Save a non-screenshot image to the Desktop; nothing is offered.
