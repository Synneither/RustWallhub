# AGENTS.md

## Project

**RustWallhub** — Tauri 2 desktop wallpaper manager (Wallhaven + Reddit).

- **Frontend**: Vue 3 + TypeScript + Vuetify 4 + Vite
- **Backend**: Rust + Tauri 2 + rusqlite (SQLite)
- **Package manager**: Deno (`deno.lock` present; `deno task` runs npm scripts from `package.json`)

## Commands

| Action | Command |
|--------|---------|
| Frontend dev server | `deno task dev` (Vite on :1420) |
| Typecheck + build | `deno task build` |
| Full Tauri dev | `deno task tauri dev` |
| Build desktop app | `deno task tauri build` |

No separate test, lint, or formatter commands exist.

## Architecture

- **Entry**: `src-tauri/src/lib.rs::run()` registers all Tauri commands and manages `AppState` (config path).
- **Frontend routing**: `src/App.vue` uses a simple `ref`-based view switcher (no vue-router). 5 views: Dashboard, Wallhaven, Reddit, Gallery, Settings.
- **Two image sources**, each with separate save dir, SQLite DB, and download logic:
  - **Wallhaven**: API client (`src-tauri/src/wallhaven.rs`) → `wallhaven_images.db`
  - **Reddit**: JSON scraping (`src-tauri/src/reddit.rs`) → `reddit_images.db`
- **Config**: `~/.config/rustwallhub/config.json` (auto-created with defaults). See `src-tauri/src/config.rs`.
- **Default paths**:
  - Save dirs: `~/Pictures/背景/wallhaven` and `~/Pictures/背景/reddit`
  - DBs: `wallhaven_images.db` and `reddit_images.db` (relative to working dir)

## Key details

- **Gallery "暂无本地图片"**: `GalleryView.vue:146-150` shows this when `list_local_images` returns zero images. That command (`lib.rs:540-588`) scans the save directory on the filesystem — it does NOT query the database. If the directory doesn't exist or has no image files, gallery is empty.
- **Local image access**: Requires Tauri asset protocol. Enabled in `Cargo.toml` (`protocol-asset` feature) and scoped in `tauri.conf.json` (`"$HOME/**/*"`). Frontend uses `convertFileSrc(path)` to create URLs.
- **Image validation**: `downloader.rs` checks magic bytes (JPEG/PNG/GIF/WebP) on download. `file_is_image()` checks extension only for local scan.
- **Download flow**: Commands spawn `tokio::task::spawn_blocking` threads, emit `download-progress` and `download-complete` events. Frontend listens in `App.vue`.
- **DB "stable/unstable"**: Tracks whether files still exist on disk. `mark_unstable` checks filesystem; `restore_stable` resets all to stable.

## Conventions

- UI is entirely in Chinese (target audience).
- Vue components use `<script setup lang="ts">` with Composition API.
- Rust error type: `AppError` (lib.rs) with `thiserror`, serialized as string to frontend.
- All Tauri commands are `async` (even when blocking internally).
- No code comments in source — this is intentional.

## Gotchas

- `tauri.conf.json` build commands use `deno task` — do not switch to `npm run` without updating config.
- DB paths in config are relative by default. If app is launched from different directory, DBs won't be found.
- `.gitignore` does not exclude `*.db` — `wallhaven_images.db` at root is tracked in git.
