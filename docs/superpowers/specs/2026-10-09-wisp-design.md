# Wisp - Design Spec

**Date:** 2026-10-09
**Status:** Draft, awaiting review

## 1. Purpose

Wisp shares clipboard content and files between the computers on a home network (one MacBook, one Windows laptop, one Windows PC). The headline use case: take a screenshot on the Mac with `Cmd+Ctrl+Shift+4` and paste it on the Windows PC, or the other way round with `Win+Shift+S`.

It lives in the Windows tray and the macOS menu bar, works peer to peer with no cloud and no account, and has a distinctive, modern UI.

### Success criteria

- A screenshot copied on one machine can be pasted on another in under 2 seconds on a home network, with one click or one hotkey.
- Works across devices connected by Wi-Fi and by cable.
- New devices are paired once with a short code; everything afterwards is encrypted and automatic.
- Copying never silently overwrites another machine's clipboard unless auto-sync was explicitly enabled for that device.

## 2. Scope

### v1

- Tray app (Windows) and menu bar app (macOS), with start at login.
- Discovery via mDNS, remembered peer addresses, and manual add by IP.
- Pairing with a 6-digit code (also shown as a QR code), followed by pinned, end-to-end encrypted connections.
- Two delivery modes, set per peer:
  - **Catch** (default): the receiving device shows a toast card; content lands on its clipboard only when accepted.
  - **Auto-sync**: content lands on the receiving device's clipboard immediately.
- Content types: plain text, rich text/HTML, images, files and folders.
- Tray panel with the device constellation; drag a file onto a device orb to send it.
- Shared timeline: last 200 items per device, searchable, tagged with the source device. Stored locally, never in a cloud.
- Smart previews: images, links (domain and favicon), code (syntax highlighted), color values (swatch).
- Clipboard items up to 50 MB. Larger content becomes an explicit file transfer.

### Later (out of v1)

- Screen-edge portal (drag a file against the screen edge facing another machine).
- macOS screenshot folder watcher (for screenshots saved as files instead of copied).
- Full link previews with page thumbnails.
- Mobile apps.
- Access from outside the home network.

## 3. Architecture

**Stack:** Tauri 2. Rust core, Vue 3 + TypeScript + Tailwind CSS frontend, Vite build. Native window materials: Mica/Acrylic on Windows, vibrancy on macOS (`window-vibrancy` crate).

### Rust core modules

Each module has one job and a small public interface. The frontend never talks to the network directly; it calls Tauri commands and listens to Tauri events emitted by the core.

| Module | Responsibility | Key crate |
|---|---|---|
| `identity` | Device ID (UUID), device name, OS, and the device's long-lived TLS key pair and self-signed certificate | `rcgen` |
| `discovery` | Advertise and browse `_wisp._udp.local`; TXT record carries device ID, name, OS, port, certificate fingerprint. Also probes remembered addresses of paired peers. | `mdns-sd` |
| `transport` | QUIC connections between paired peers. Tries every known address of a peer in parallel, keeps the lowest-latency path, re-evaluates on network change. | `quinn`, `rustls` |
| `pairing` | Turns a 6-digit code into an authenticated exchange of certificate fingerprints (SPAKE2). After pairing, a peer's fingerprint is pinned. | `spake2` |
| `peers` | Store of paired peers: ID, name, OS, fingerprint, last known addresses, delivery mode | none |
| `clipboard` | Watch for clipboard changes; read and write text, HTML, images, file lists | `clipboard-rs` |
| `policy` | Decides per peer whether a new item is sent as an offer (catch) or as full content (auto-sync), and applies size limits | none |
| `history` | Timeline storage and full-text search. Metadata in SQLite, binary payloads as files in the app data folder. Pruned to 200 items. | `rusqlite` (FTS5) |
| `transfer` | Chunked streaming of large payloads with progress events, resume from last confirmed chunk, BLAKE3 checksum verification | `blake3` |
| `protocol` | Message types and framing shared by all of the above | `serde`, `postcard` |

### Protocol messages

All messages are length-prefixed `postcard` frames on QUIC streams.

- `Hello { device_id, name, os, version }` on connection open.
- `Offer { item_id, kind, size, preview }` where `preview` is a short text excerpt or a thumbnail of at most 256 px.
- `Push { item_id, kind, payload }` for auto-sync and for accepted offers under 50 MB.
- `Pull { item_id }` sent by a catching device when the user accepts an offer.
- `FileStart { transfer_id, name, size, hash }`, `FileChunk { transfer_id, offset, bytes }`, `FileAck { transfer_id, offset }`, `FileDone { transfer_id }`.
- `Ping` / `Pong` for liveness and latency measurement.

### Data flow: screenshot on Mac, paste on PC (catch mode)

1. The user presses `Cmd+Ctrl+Shift+4`; macOS puts the image on the clipboard.
2. The Mac's `clipboard` watcher sees the change, `history` stores the item and computes its BLAKE3 hash.
3. `policy` decides per connected peer: catch peers get an `Offer` with a thumbnail, auto-sync peers get a `Push`.
4. The PC shows the toast card with the thumbnail.
5. The user clicks **Catch** or presses the hotkey. The PC sends `Pull`, receives the image, writes it to its clipboard, and records the hash as "written by Wisp".
6. The PC's own clipboard watcher sees the change, matches the hash, and does not re-broadcast it (echo-loop prevention).

### Hotkey

Default **`Ctrl+Alt+V`** on Windows and **`Ctrl+Option+V`** on macOS (`Ctrl+Shift+V` is avoided because it is "paste as plain text" in many apps). Configurable. It catches the most recent pending offer.

## 4. UI and brand

### Brand

- **Name:** Wisp.
- **Palette "aurora on ink":** ink background `#0B0D14`, frosted glass panels, accent gradient teal `#2DE2C5` to violet `#8B5CF6`. A light theme follows the OS setting.
- **Type:** Geist and Geist Mono, bundled locally.
- **Logo:** a single flowing swirl in the teal-violet gradient. Drafts generated with Gemini after spec approval.
- **Tray icon:** monochrome swirl (macOS template image; light and dark variants on Windows). Soft glow while receiving; small dot while uncaught offers are waiting.

### Tray panel

Opens on tray icon click, about 360 x 480 px, glass material.

- **Constellation:** this device in the center, paired devices as orbs around it. Connection line style shows the path: solid glow for cable, soft shimmer for Wi-Fi. Offline devices are dimmed.
- Drag a file onto an orb to send it. Click an orb for: delivery mode toggle (catch / auto-sync), "send current clipboard", "open timeline filtered to this device".
- Below: search field and the last 5 timeline items. Clicking an item copies it again.

### Toast card

Bottom right on Windows, top right on macOS.

- Preview, source device and age ("from MacBook, 2s ago").
- Actions: **Catch** (with hotkey hint), **Save as file**, dismiss.
- Fades into the tray after 8 seconds; still catchable from the panel.
- Several offers from the same device collapse into one stack ("3 items from MacBook").

### Timeline window

Full window with search and filters by device and type. Items shown as a card grid. Click to copy, drag out to save to disk.

### Settings

Devices and pairing, add device by IP, hotkey, start at login, history size, file save location (default `Downloads/Wisp`), theme.

### First run

1. Name this device (prefilled with the OS hostname).
2. Pairing screen: either show a code and QR for another device, or enter the code shown elsewhere.
3. The constellation, waiting for other devices to appear.

### Motion

Calm and smooth: spring-based orb movement, soft toast slide-in, glow pulses. Under reduced motion (the owner runs Windows with animations off), large movements are removed but short fades and hover transitions remain, so the UI never feels choppy.

## 5. Error handling

| Situation | Behaviour |
|---|---|
| Peer goes offline during a transfer | Orb dims; transfer shows "paused, waiting for <device>"; resumes from the last acknowledged chunk if the peer returns within 10 minutes, otherwise marked failed with a retry button |
| Peer is offline when an item is created | Offers for that peer are queued (latest 20) and delivered when it reconnects |
| Discovery finds nothing (separate network segments) | Probe remembered addresses; if a paired peer stays unreachable, the panel suggests adding it by IP |
| Unknown device or changed fingerprint | Connection refused; warning "This device's identity changed. Re-pair?" Never accepted silently |
| Wrong pairing code three times | Pairing session ends; a new code is required |
| Clipboard locked by another app | Retry with backoff for up to 2 seconds, then toast "Couldn't write the clipboard. Try again." |
| Unsupported clipboard format | Fall back to the richest supported format (for example HTML to plain text); the timeline item shows which format was sent |
| Content over 50 MB | Sent as a file offer instead of a clipboard item |
| Firewall blocks inbound connections | Detected on first run; panel shows an explanation of how to allow Wisp |

## 6. Testing

- **Rust unit tests:** protocol framing round trips, pairing (correct and wrong codes), policy decisions per mode and size, echo-loop prevention, history store and pruning, chunking and checksum verification, transfer resume.
- **Integration tests:** two Wisp cores in one process on localhost with an in-memory clipboard. Pair, send text, image and file, kill the connection mid-transfer and resume, verify catch vs auto-sync behaviour.
- **Frontend tests (Vitest):** constellation layout and states, toast stacking, timeline search and filters.
- **Manual smoke checklist** on the real devices: Mac to PC screenshot, PC to Mac screenshot, Wi-Fi to cable, auto-sync between the two Windows machines, a file over 1 GB.

## 7. Build and distribution

- Windows installer (NSIS) and macOS `.dmg` (universal) built by GitHub Actions on tag push.
- macOS build is ad-hoc signed, not notarized, in v1; first launch is allowed via System Settings > Privacy & Security > Open Anyway.
- Public repository: `wl-lankin/wisp`.
