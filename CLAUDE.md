# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

"Gooliya Port Bar" — a macOS menu-bar (tray) Tauri app that scans locally listening ports (Node/npm dev servers and Docker containers) and shows them in a small popover with pin/rename/open-in-browser actions. Frontend is SvelteKit (Svelte 5, static SPA), backend is Rust (Tauri v2).

## Commands

Frontend only (fast iteration on UI, runs in a regular browser tab, no Rust backend/`invoke` calls will work):
```
npm run dev       # vite dev server on :1420
npm run build     # vite build -> ./build (static SPA, adapter-static + fallback index.html)
npm run preview
npm run check      # svelte-kit sync && svelte-check (TypeScript/Svelte type checking)
npm run check:watch
```

Full app (Rust + WebView, required to exercise `invoke()` calls like `get_ports`, `get_prefs`, tray/menu behavior):
```
npm run tauri dev    # or: npx tauri dev
npm run tauri build  # or: npx tauri build
```

Rust-side checks (run from `src-tauri/`):
```
cargo test    # runs unit tests in src-tauri/src/lib.rs (infer_cmd_label, infer_project_name, dedup/sort)
cargo check
```

There is no JS test runner configured — Svelte-side correctness is verified via `npm run check` (types) and manual `tauri dev` runs.

## Architecture

Single-window, single-page app — there is no routing beyond the one page (`src/routes/+page.svelte`), and SSR is explicitly disabled (`src/routes/+layout.ts`) because Tauri has no Node server; the SvelteKit adapter is `adapter-static` with `fallback: index.html` (SPA mode).

**Rust backend (`src-tauri/src/lib.rs`)** exposes four `#[tauri::command]`s invoked from the frontend via `@tauri-apps/api/core`'s `invoke()`:
- `get_ports` — shells out to `/usr/sbin/lsof -i -P -n`, filters `LISTEN` lines, and for each port classifies the owning process as `npm` (process name `node`/`node.js`, then infers a project name from the full `ps` command by regex-matching `/workspace/<name>/`, and a friendly command label like `vite dev` via substring sniffing) or `docker` (process name starts with `docker`/`com.docker`, container name resolved by cross-referencing `docker ps --format '{{.Names}}\t{{.Ports}}'` with a 2s timeout via a background thread + channel). Ports are deduped and sorted.
- `get_prefs` / `save_prefs` — read/write a `HashMap<port, PortPref>` (custom display name + pinned flag) as JSON to `app_config_dir()/prefs.json`. This is the only persisted state; there is no database.
- `quit_app` — exits the process.

App setup (in `run()`) wires up macOS-specific tray/menu behavior: an `Accessory` activation policy (no Dock icon), a vibrancy effect on the window, a tray icon with a left-click handler that toggles/positions the popover window directly under the tray icon (physical-pixel math accounts for display scale factor), and window auto-hide on focus loss. This tray-popover behavior is macOS-centric — if extending to other platforms, expect to touch this code.

**Frontend (`src/routes/+page.svelte`)** is a single Svelte 5 component (uses runes: `$state`, `$derived.by`, `$effect`) that owns all UI state: raw port list from `get_ports`, prefs from `get_prefs`/`save_prefs`, inline rename editing, pin toggling (pinned entries sort first), and an autostart toggle via `@tauri-apps/plugin-autostart`. A `$effect` re-fits the actual OS window to content height (`fitWindow`, via `getCurrentWindow().setSize`, measuring `.app`'s own `getBoundingClientRect().height` — not `document.documentElement.scrollHeight`, which over-measures) any time `portsLoading`/`scanError`/`displayPorts` change, since the window is a fixed-width (360px) popover with no natural scroll chrome. Ports are re-scanned automatically when the window regains focus.

**Permissions** are scoped in `src-tauri/capabilities/default.json` (Tauri v2 capability system) — when adding a new plugin or a command that needs a new permission, it must be added there or the frontend call will be silently denied at runtime with a rejected promise (not a compile error, and easy to miss if the call site doesn't catch it). Notably, `core:default` only covers read-only window queries (`allow-inner-size`, `allow-scale-factor`, etc.) — any state-mutating window call from JS (`setSize`, `setPosition`, `hide`, `show`, `setFocus`, ...) needs its own explicit `core:window:allow-*` permission; `core:window:allow-set-size` is already added for `fitWindow`.

## Notes

- UI strings and comments in the Svelte component are in Traditional Chinese (繁體中文) — keep new user-facing strings consistent with that.
- `infer_project_name` assumes projects live under a `/workspace/<name>/` path segment; `infer_cmd_label` does substring sniffing for `vite`/`astro`/`next`/`tsx`/`ts-node`/a hardcoded `hipki` project. Both are intentionally simple heuristics with Rust unit tests in `src-tauri/src/lib.rs` — extend the tests alongside any change to this matching logic.
