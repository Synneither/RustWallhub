//! Shared application state, error types, event payloads, and helper functions.
//!
//! All Tauri commands in `commands/` depend on the types and helpers defined here.

use crate::config::AppConfig;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Event payloads (emitted to the frontend)
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub source: String,
    pub done: u32,
    pub total: u32,
    pub message: String,
}

#[derive(Clone, serde::Serialize)]
pub struct DownloadComplete {
    pub source: String,
    pub success: u32,
    pub total: u32,
    pub message: String,
}

#[derive(Clone, serde::Serialize)]
pub struct ImageDownloaded {
    pub source: String,
    pub name: String,
    pub path: String,
}

// ---------------------------------------------------------------------------
// File-list cache (used by browse_image_files)
// ---------------------------------------------------------------------------

pub struct FileListCache {
    pub items: Vec<FileEntry>,
    pub total: usize,
    pub source: String,
    pub dir_path: String,
    pub cached_at: Instant,
}

#[derive(Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_orphan: bool,
}

// ---------------------------------------------------------------------------
// Application state (managed by Tauri)
// ---------------------------------------------------------------------------

pub struct AppState {
    pub config_path: Mutex<PathBuf>,
    pub file_cache: Mutex<Option<FileListCache>>,
    pub cancel_flag: Mutex<Option<Arc<AtomicBool>>>,
    pub http_client: Mutex<reqwest::Client>,
    pub config_cache: Mutex<Option<AppConfig>>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("{0}")]
    Db(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Other(String),
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

const MAX_UPWARD_DEPTH: u32 = 100;

pub fn find_upward(
    base_dir: &std::path::Path,
    relative: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let mut current = base_dir.to_path_buf();
    let mut depth = 0u32;
    loop {
        let candidate = current.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
        if depth >= MAX_UPWARD_DEPTH {
            log::warn!(
                "[find_upward] exceeded max depth {} at {}",
                MAX_UPWARD_DEPTH,
                current.display()
            );
            break;
        }
        if !current.pop() {
            break;
        }
        depth += 1;
    }
    None
}

pub fn database_score(path: &std::path::Path) -> Option<i64> {
    if !path.exists() {
        return None;
    }
    let conn = Connection::open(path).ok()?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))
        .ok()?;
    Some(count)
}

pub fn normalize_config_path(base_dir: &std::path::Path, value: String) -> String {
    let path = std::path::PathBuf::from(&value);
    if path.is_absolute() {
        return value;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| base_dir.to_path_buf());
    let cwd_resolved = cwd.join(&path);
    let config_resolved = base_dir.join(&path);

    let mut candidates = Vec::new();
    candidates.push(cwd_resolved.clone());
    if let Some(found) = find_upward(&cwd, &path) {
        if found != cwd_resolved {
            candidates.push(found);
        }
    }
    candidates.push(config_resolved.clone());
    if let Some(found) = find_upward(base_dir, &path) {
        if found != config_resolved {
            candidates.push(found);
        }
    }

    let mut best: Option<(&std::path::PathBuf, i64)> = None;
    for candidate in &candidates {
        if let Some(score) = database_score(candidate) {
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((candidate, score));
            }
        }
    }

    if let Some((best_path, _)) = best {
        return best_path.to_string_lossy().to_string();
    }

    if cwd_resolved.exists() {
        return cwd_resolved.to_string_lossy().to_string();
    }
    if config_resolved.exists() {
        return config_resolved.to_string_lossy().to_string();
    }
    candidates
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or(config_resolved)
        .to_string_lossy()
        .to_string()
}

pub fn load_config(state: &tauri::State<'_, AppState>) -> Result<AppConfig, AppError> {
    let path = state
        .config_path
        .lock()
        .map_err(|e| AppError::Config(format!("锁定配置失败: {e}")))?
        .clone();

    if let Ok(guard) = state.config_cache.lock() {
        if let Some(ref cached) = *guard {
            return Ok(cached.clone());
        }
    }

    let mut config = AppConfig::load(&path).map_err(AppError::Config)?;
    config.sync_db_dir();
    if let Some(base_dir) = path.parent() {
        config.wallhaven_db_path = normalize_config_path(base_dir, config.wallhaven_db_path);
        config.reddit_db_path = normalize_config_path(base_dir, config.reddit_db_path);
        config.db_dir = normalize_config_path(base_dir, config.db_dir);
        config.wallhaven_save_dir = normalize_config_path(base_dir, config.wallhaven_save_dir);
        config.reddit_save_dir = normalize_config_path(base_dir, config.reddit_save_dir);
    }

    if let Ok(mut guard) = state.config_cache.lock() {
        *guard = Some(config.clone());
    }

    Ok(config)
}

pub fn save_config(state: &tauri::State<'_, AppState>, config: &AppConfig) -> Result<(), AppError> {
    let path = state
        .config_path
        .lock()
        .map_err(|e| AppError::Config(format!("锁定配置失败: {e}")))?
        .clone();
    config.save(&path).map_err(AppError::Config)?;
    if let Ok(mut guard) = state.config_cache.lock() {
        *guard = Some(config.clone());
    }
    Ok(())
}

pub fn setup_cancel_flag(state: &AppState) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut guard) = state.cancel_flag.lock() {
        *guard = Some(flag.clone());
    }
    flag
}

pub fn rebuild_http_client(state: &AppState, timeout_secs: u64) -> Result<(), String> {
    let new_client = reqwest::Client::builder()
        .user_agent("RustWallhub/1.0")
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("创建 HTTP client 失败: {e}"))?;
    if let Ok(mut client) = state.http_client.lock() {
        *client = new_client;
    }
    Ok(())
}

/// Save image bytes to disk and spawn thumbnail generation.
/// Returns a `JoinHandle` for the thumbnail task so callers can await it
/// concurrently with other work (e.g., DB insertion).
pub async fn save_image(
    save_path: impl AsRef<std::path::Path>,
    bytes: &[u8],
    thumb_dir: impl AsRef<std::path::Path>,
    filename: &str,
    dpr: u32,
) -> Option<tokio::task::JoinHandle<()>> {
    if tokio::fs::write(save_path.as_ref(), bytes).await.is_err() {
        return None;
    }
    let thumb_dir = thumb_dir.as_ref().to_path_buf();
    let filename = filename.to_string();
    let bytes = bytes.to_vec();
    Some(tokio::task::spawn_blocking(move || {
        let _ = crate::thumbnail::save_thumbnail_from_bytes(&thumb_dir, &filename, &bytes, dpr);
    }))
}
