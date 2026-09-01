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

/// 下载进度事件节流器：大任务按时间间隔发事件，避免每张图都跨 IPC 更新前端。
pub struct ProgressThrottle {
    last_emit: Option<Instant>,
}

impl ProgressThrottle {
    pub fn new() -> Self {
        Self { last_emit: None }
    }

    /// `force=true` 时必定发送（用于最后一张/完成前），否则至少间隔 150ms。
    pub fn should_emit(&mut self, force: bool) -> bool {
        if force {
            self.last_emit = Some(Instant::now());
            return true;
        }
        let now = Instant::now();
        let ready = self
            .last_emit
            .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(150));
        if ready {
            self.last_emit = Some(now);
        }
        ready
    }
}

// ---------------------------------------------------------------------------
// File-list cache (used by browse_image_files)
// ---------------------------------------------------------------------------

pub struct FileListCache {
    /// 目录中的**全量**条目（不含任何搜索过滤）。搜索与排序只在读取时应用，
    /// 否则一次带搜索的扫描会把子集写进缓存，清空搜索框后图库会凭空少图。
    pub items: std::sync::Arc<[FileEntry]>,
    pub source: String,
    pub dir_path: String,
    pub cached_at: Instant,
    /// 缓存创建时目录的 mtime。目录有增删时 mtime 会变化，可用于快速失效。
    pub dir_modified: Option<std::time::SystemTime>,
    /// `items` 当前已按此排序键排好序。请求同一排序键且无搜索时可直接切片分页，
    /// 省掉每翻一页就做一次的 O(n log n) 全量重排。
    pub sorted_by: String,
}

#[derive(Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_orphan: bool,
    pub modified: Option<std::time::SystemTime>,
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
    pub slideshow_cancel: Mutex<Option<Arc<AtomicBool>>>,
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

/// 校验一个字符串是“纯文件名”而不是路径（拒绝 `/`、绝对路径与 `..` 组件）。
/// 所有从 IPC 接收、随后要拼接到目录后面的 filename 参数都应先经过此校验。
pub fn ensure_plain_filename(name: &str) -> Result<(), AppError> {
    use std::path::Component;

    let path = std::path::Path::new(name);
    let mut components = path.components();
    let valid = match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => !has_path_separator(name),
        _ => false,
    };
    if !valid {
        return Err(AppError::Other("非法的文件路径".into()));
    }
    Ok(())
}

/// Windows 上反斜杠是路径分隔符，出现在文件名里即视为穿越尝试；
/// Linux/macOS 上反斜杠是**合法**的文件名字符，不能因此拒绝该文件。
#[cfg(target_os = "windows")]
fn has_path_separator(name: &str) -> bool {
    name.contains('\\')
}

#[cfg(not(target_os = "windows"))]
fn has_path_separator(_name: &str) -> bool {
    false
}

/// Safely join `name` onto `base`, rejecting path-traversal attempts like `../`.
/// Returns the canonicalized path if it lies within `base`, otherwise an error.
pub fn safe_join(base: &std::path::Path, name: &str) -> Result<PathBuf, AppError> {
    let base_canonical = base
        .canonicalize()
        .map_err(|e| AppError::Other(format!("无法解析基础路径: {e}")))?;
    safe_join_with(base, &base_canonical, name)
}

/// 与 [`safe_join`] 相同的校验，但复用已 canonicalize 过的 base。
/// 单个文件在循环里反复 canonicalize base 是纯浪费（每个文件 2 次 syscall）。
fn safe_join_with(
    base: &std::path::Path,
    base_canonical: &std::path::Path,
    name: &str,
) -> Result<PathBuf, AppError> {
    ensure_plain_filename(name)?;
    let candidate = base.join(name);
    // If the file doesn't exist yet, canonicalize the parent and join the filename
    let resolved = if candidate.exists() {
        candidate.canonicalize()
    } else {
        Ok(base_canonical.join(name))
    };
    let resolved = resolved.map_err(|e| AppError::Other(format!("无法解析路径: {e}")))?;
    if !resolved.starts_with(base_canonical) {
        return Err(AppError::Other("非法的文件路径".into()));
    }
    Ok(resolved)
}

/// 批量解析文件名 → 安全路径。base 只 canonicalize 一次，且**单个失败不会中断整批**：
/// 失败项只记日志并跳过，返回成功解析的 `(文件名, 路径)` 列表。
///
/// 批量操作（删除/标记/收养）里若用 `safe_join(...)?`，第一个失败就会让剩余文件全部
/// 得不到处理，而用户点了「删除 20 个」往往期望尽力完成。
pub fn safe_join_all<'a>(base: &std::path::Path, names: &'a [String]) -> Vec<(&'a str, PathBuf)> {
    let base_canonical = match base.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[safe_join_all] 无法解析基础路径 {}: {}", base.display(), e);
            return Vec::new();
        }
    };
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        match safe_join_with(base, &base_canonical, name) {
            Ok(path) => out.push((name.as_str(), path)),
            Err(e) => log::warn!("[safe_join_all] 跳过 {}: {}", name, e),
        }
    }
    out
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

/// 读取配置。参数用 `&AppState`：传入 `&tauri::State<'_, AppState>` 时靠 Deref 自动转换，
/// 这样命令与后台钩子（拿到的可能是 `State` 也可能是 `&AppState`）都能复用。
pub fn load_config(state: &AppState) -> Result<AppConfig, AppError> {
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

pub fn rebuild_http_client(
    state: &AppState,
    timeout_secs: u64,
    proxy_url: &str,
) -> Result<(), String> {
    let mut builder = reqwest::Client::builder()
        .user_agent("RustWallhub/1.0")
        .timeout(Duration::from_secs(timeout_secs));
    if !proxy_url.is_empty() {
        builder = builder
            .proxy(reqwest::Proxy::all(proxy_url).map_err(|e| format!("代理设置失败: {e}"))?);
        log::info!("[http] 使用代理: {}", proxy_url);
    }
    let new_client = builder
        .build()
        .map_err(|e| format!("创建 HTTP client 失败: {e}"))?;
    if let Ok(mut client) = state.http_client.lock() {
        *client = new_client;
    }
    Ok(())
}

/// Save image bytes to disk. 缩略图不再随下载即时生成，而是由图库/新图预览条
/// 通过 `resolve_thumbnails` 惰性生成，避免下载链路同时持有原图副本和解码位图。
pub async fn save_image(
    save_path: impl AsRef<std::path::Path>,
    bytes: &[u8],
) -> Result<(), String> {
    let save_path = save_path.as_ref();
    tokio::fs::write(save_path, bytes)
        .await
        .map_err(|e| format!("写入文件失败 {}: {e}", save_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_plain_filename() {
        assert!(ensure_plain_filename("a.jpg").is_ok());
        assert!(ensure_plain_filename("a..b.jpg").is_ok());
        assert!(ensure_plain_filename("..").is_err());
        assert!(ensure_plain_filename("../a.jpg").is_err());
        assert!(ensure_plain_filename("sub/a.jpg").is_err());
        assert!(ensure_plain_filename("sub\\a.jpg").is_err());
        assert!(ensure_plain_filename("").is_err());
    }
}
