# AGENTS.md

## Project

**RustWallhub** — Tauri 2 desktop wallpaper manager (Wallhaven + Reddit).

**Scope**: the app *manages* wallpapers (download, browse, prune, DB upkeep) — it **does not set
the wallpaper**. The only wallpaper-related system integration is read-only: `get_active_wallpaper`
identifies the current one so the gallery can highlight it. Don't reintroduce desktop-environment
wallpaper setters, per-monitor pickers or the slideshow.

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
| Backend tests | `cargo test` (in `src-tauri/`) |
| Frontend typecheck only | `npx vue-tsc --noEmit` |
| Frontend e2e (optional) | `npx playwright test` |

There is no lint or formatter config in the repo. `cargo test` and `vue-tsc --noEmit` are the
two checks that must pass before committing.

## Architecture

- **Entry**: `src-tauri/src/lib.rs::run()` registers all Tauri commands and manages `AppState`.
  Commands are grouped by domain under `src-tauri/src/commands/` (gallery, download, settings,
  database, wallhaven, reddit, sync, system). `commands/gallery/` is further split by concern
  (`browse` / `thumbs` / `delete` / `orphan` / `info`) and re-exported from its `mod.rs`, so
  `commands::*` still resolves every command name.
- **Cross-cutting top-level modules** (keep these boundaries when adding code):
  | module | 职责 |
  |---|---|
  | `state.rs` | `AppState`、跨 IPC 事件结构、配置读写、asset 授权 |
  | `error.rs` | `AppError`（+ 序列化成字符串给前端） |
  | `safe_path.rs` | 文件名/路径安全校验：`ensure_plain_filename` / `safe_join` / `safe_join_all` |
  | `current_wallpaper.rs` | **只读**地查当前壁纸（Windows 走 `IDesktopWallpaper` COM） |
  | `winpath.rs` | Windows verbatim 路径（`\\?\`）降级，shell/Win32 API 的适配层 |
  | `trash.rs` | 移入回收站（Windows `SHFileOperationW` / Linux `gio`+XDG Trash） |
  | `exec.rs` | 外部命令查找（PATH + `~/.local/bin`、Nix profile 等） |
- **Frontend routing**: `src/App.vue` uses a `ref`-based view switcher (no vue-router, no pinia).
  **6 views**: Dashboard, Wallhaven, Reddit, Gallery, Database (`DbSettingsView.vue`), Settings.
  Views are wrapped in `<KeepAlive>`, so in-view `onMounted` runs once and refreshes go through
  watchers (e.g. `appState.galleryEpoch`).
- **Two image sources**, each with separate save dir, SQLite DB, and download logic:
  - **Wallhaven**: API client (`src-tauri/src/wallhaven.rs`) → `wallhaven_images.db`
  - **Reddit**: JSON scraping (`src-tauri/src/reddit.rs`) → `reddit_images.db`
- **Config**: `<config_dir>/rustwallhub/config.json` (auto-created with defaults). See
  `src-tauri/src/config.rs`. `db_dir` is the single source of truth for the two DB paths —
  `sync_db_dir()` derives them; an empty `db_dir` means legacy per-path config.
- **Events**: 9 backend→frontend events, all listened to in `src/stores/app.ts`:
  `download-progress`, `download-complete`, `image-downloaded`, `settings-changed`,
  `sync-completed`, `sync-failed`, `update-available`, `update-installing`,
  `update-progress`.

## Key details

- **Gallery listing**: `browse_image_files` (`commands/gallery/browse.rs`) scans the save directory
  on the filesystem — it does NOT query the database. It returns `thumb_path: null`; thumbnails
  are resolved in a second pass via `resolve_thumbnails`. Custom-directory mode passes
  `custom_dir` and then hides the DB-dependent actions in the UI.
- **Local image access**: Requires the Tauri asset protocol (`protocol-asset` feature in
  `Cargo.toml`). `tauri.conf.json` only whitelists `$CACHE/rustwallhub/**` statically; the
  configured save dirs are granted at startup via `allow_config_asset_dirs()`, and a
  user-picked custom directory is granted when `browse_image_files` receives it. Frontend turns
  paths into URLs with `convertFileSrc()`.
- **Path safety**: every filename arriving over IPC must go through `safe_path::safe_join()` (single)
  or `safe_join_all()` (batch, skips bad entries instead of aborting the whole batch). These are
  the only barrier between IPC input and disk paths — do not bypass them.
- **Image validation**: `downloader.rs` checks magic bytes (JPEG/PNG/GIF/WebP) on download;
  `file_is_image()` checks the extension only, for local directory scans.
- **Download flow**: start-style commands return immediately and spawn a background task. The
  per-image download loop runs on `tokio::spawn`; SQLite work runs on `spawn_blocking` so it
  does not occupy a tokio worker. Progress arrives only through events.
- **Two DBs are separate SQLite files**: cross-source paging (`db::get_all_images_paged`) reads each
  side through its own `idx_images_created_at`-ordered page query and streaming-merges the two
  sorted streams in Rust, then slices `[offset, offset + limit)` — cost is O(limit), not O(offset).
  Do **not** go back to `ATTACH` + `UNION ALL` + `ORDER BY`: SQLite cannot use an index for a
  cross-database sort, so it builds a TEMP B-TREE over both tables on every page (deep paging then
  costs hundreds of ms and grows with table size).
- **Missing/unliked semantics**: `love = 0` marks a record as disliked or missing. A record is
  counted as *missing* only when `love = 1` **and** the file is absent from the save dir;
  manually disliked records are not counted as missing.

## Conventions

- UI is entirely in Chinese (target audience).
- Vue components use `<script setup lang="ts">` with Composition API.
- Rust error type: `AppError` (`state.rs`) using `thiserror`, serialized as a string to the
  frontend. Frontend maps raw messages to Chinese via `src/utils/errors.ts::friendlyError`.
- All Tauri commands are `async` (even when blocking internally).
- Reusable frontend patterns live in `src/composables/` — see the table in
  `docs/FRONTEND.md` §4.1 (config draft, async buttons, selection, grid density, thumb cache,
  Wallhaven preview buffer, gallery viewer/detail). Prefer these over hand-rolled
  `loading = true / try / catch / finally`.
- Big views are decomposed into presentational components (`GalleryCard`, `GalleryBatchBar`,
  `GridSizeBar`, `WallhavenSearchForm`, `DbSyncPanel`, `RecordBrowserPanel`). **When moving
  markup into a child, move its styles too** — scoped CSS cannot reach inside a child component,
  and shared geometry belongs in `src/assets/style.css` (§4.2 of FRONTEND.md).
- Backend repetition between command modules goes into `commands/<domain>_common.rs`
  (see `download_common.rs`). Such modules are declared `pub mod` but must NOT be added to the
  `pub use` list in `commands/mod.rs`, since they export no commands.
- Comments explain *why*, not *what* — several non-obvious decisions are documented inline.

## Gotchas

- `tauri.conf.json` build commands use `deno task` — do not switch to `npm run` without updating
  the config.
- `cargo build` from scratch takes 8–15 minutes; changing `Cargo.toml` features triggers a full
  rebuild. Run it in the background and work on the frontend in parallel.
- A full `vite build` wipes `dist/`. To verify a build without touching `dist/`, use
  `npx vite build --outDir dist-verify` and delete it afterwards.
- `.gitignore` excludes `*.db` and `src-tauri/tauri-private.key`. Only the **public** key
  (`tauri-private.key.pub`) is tracked — never commit the private key.
- `src-tauri/target/debug/.cargo-lock` can be left behind by an interrupted `cargo build`,
  which then fails with a lock error; delete it before retrying.
