# Gooliya Port HQ

A lightweight macOS menu bar app that scans your locally listening ports — npm/Vite/Next/Astro dev servers and Docker containers — and shows them in one popover, with pin, rename, and one-click open in browser.

繁體中文說明: [README.zh-TW.md](./README.zh-TW.md)

## Features

- Lists all locally listening ports from Node dev servers (`vite`, `astro`, `next`, `tsx`/`ts-node`, ...) and Docker containers
- Click a port to open it in your default browser
- Pin favorites to keep them at the top of the list
- Rename any entry with a custom label
- Auto-refreshes when the popover regains focus, or manually via the refresh button
- Optional launch-at-login
- Lives entirely in the menu bar — no Dock icon

## Install

Download the latest `.dmg` from [Releases](https://github.com/imsyuan/gooliya-port-hq/releases), open it, and drag **Gooliya Port HQ** into Applications.

> The app isn't signed with an Apple Developer ID, so on first launch Gatekeeper will refuse to open it. Right-click the app → **Open** to bypass this once. Builds are currently Apple Silicon (M1/M2/M3/M4) only.

## Tech Stack

- [Tauri v2](https://tauri.app/) (Rust) for the native shell, tray icon, and window management
- [SvelteKit](https://svelte.dev/) + [Svelte 5](https://svelte.dev/) (runes) for the UI, built as a static SPA
- Port scanning via `lsof` and process inspection via `ps` / `docker ps` on the Rust side

## Development

Requires Node.js and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Rust toolchain, etc.) for your platform.

```bash
npm install

# Full app (Rust + native window) — required to exercise Tauri commands
npm run tauri dev

# Frontend only, in a regular browser tab (no Tauri backend, invoke() calls will fail)
npm run dev

# Type-check the frontend
npm run check
```

## Build

```bash
npm run tauri build
```

Rust unit tests live in `src-tauri/src/lib.rs`:

```bash
cd src-tauri
cargo test
```

## License

[MIT](./LICENSE)
