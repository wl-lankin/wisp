<p align="center">
  <img src="apps/wisp/brand/logo.svg" alt="Wisp logo" width="96" height="96">
</p>

# Wisp

Share your clipboard and files between Mac and Windows on your local network. Peer to peer, encrypted, lives in your tray.

## Install

### Windows

1. Download the setup from the latest release and run it.
2. It asks for admin rights once, because Wisp installs for all users on the machine.
3. The installer adds a firewall rule so your other devices can reach Wisp on private networks.

The installer is not signed yet, so SmartScreen may warn you. Choose "More info", then "Run anyway".

### macOS

1. Open the dmg and drag Wisp to Applications.
2. The build is ad-hoc signed but not notarized, so macOS blocks it the first time. Open System Settings, go to Privacy & Security, and choose "Open Anyway".
3. macOS will ask for local network access. Allow it, otherwise Wisp can't see your other devices.

## First run

1. Give this device a name.
2. Pair it with another device using a code. Wisp shows the code on one device, you type it on the other.

## Everyday use

- **Catch card:** press `Ctrl+Alt+V` on Windows or `Ctrl+Option+V` on macOS to catch what is on your clipboard and send it. You can change the shortcut in Settings.
- **Auto-sync:** turn it on per device, so a paired device always gets your clipboard.
- **Files:** drag files onto a device to send them.
- **Timeline:** see everything you sent and received in one place.

## Privacy

- Devices talk directly to each other. There is no server in the middle.
- Everything is encrypted.
- Nothing leaves your network.
- History stays on each device.

## Develop

You need Rust stable and Node 22 or newer. On Windows you also need WebView2, which comes with Windows 11.

```bash
cd apps/wisp
npm install
npm run tauri dev
```

Run the tests:

```bash
cd apps/wisp && npm test
cargo test --workspace
```

### Release

1. In `apps/wisp`, run `npm run set-version 0.2.0`.
2. At the repo root, run `cargo check` so `Cargo.lock` picks up the new crate versions.
3. Commit the manifests and `Cargo.lock`.
4. Tag it with `git tag v0.2.0` and push the tag.

CI builds the Windows setup and the universal dmg into a draft release. Publish it by hand when you are happy.

## Status

Version 0.1. Builds are unsigned.

## Docs

- [Design spec](docs/superpowers/specs/2026-10-09-wisp-design.md)
- [Plan 1: core engine](docs/superpowers/plans/2026-10-09-wisp-1-core-engine.md)
- [Plan 2: tray UI](docs/superpowers/plans/2026-10-09-wisp-2-tray-ui.md)
- [Plan 3: brand and release](docs/superpowers/plans/2026-10-09-wisp-3-brand-and-release.md)
