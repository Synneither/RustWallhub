//! 应用状态与跨 IPC 的事件载荷。
//!
//! 错误类型在 `error.rs`、路径安全校验在 `safe_path.rs`、当前壁纸在
//! `current_wallpaper.rs`——这里只留 AppState、事件结构、配置读写与 asset 授权。
//!
//! All Tauri commands in `commands/` depend on the types and helpers defined here.

use crate::config::{AppConfig, Source};
use crate::error::AppError;
use rusqlite::Connection;
use std::collections::HashMap;
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
    /// 按 source 独立的下载取消标志。旧实现是单个 `Option<Arc<AtomicBool>>` 槽位，
    /// 并发下载时后启动者会覆盖前者，导致 cancel 只能取消最后一个任务。
    pub cancel_flag: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub http_client: Mutex<reqwest::Client>,
    /// 配置缓存。用 `Arc` 而不是直接存 `AppConfig`：`load_config` 是每个命令都会走的
    /// 热路径，配置有 30+ 个 String 字段，按值返回等于每次命令都深拷贝一遍。
    pub config_cache: Mutex<Option<Arc<AppConfig>>>,
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
///
/// 返回 `Arc<AppConfig>`：调用方拿到的是一份共享引用，不需要为每次命令深拷贝配置。
pub fn load_config(state: &AppState) -> Result<Arc<AppConfig>, AppError> {
    if let Ok(guard) = state.config_cache.lock() {
        if let Some(ref cached) = *guard {
            return Ok(Arc::clone(cached));
        }
    }

    let path = state
        .config_path
        .lock()
        .map_err(|e| AppError::Config(format!("锁定配置失败: {e}")))?
        .clone();

    let mut config = AppConfig::load(&path).map_err(AppError::Config)?;
    config.sync_db_dir();
    if let Some(base_dir) = path.parent() {
        config.wallhaven_db_path = normalize_config_path(base_dir, config.wallhaven_db_path);
        config.reddit_db_path = normalize_config_path(base_dir, config.reddit_db_path);
        config.db_dir = normalize_config_path(base_dir, config.db_dir);
        config.wallhaven_save_dir = normalize_config_path(base_dir, config.wallhaven_save_dir);
        config.reddit_save_dir = normalize_config_path(base_dir, config.reddit_save_dir);
        // 缩略图目录同样要归一化：它会被拿去授权 asset 协议与生成缩略图路径，
        // 相对路径在不同工作目录下会解析到不同位置。
        config.thumbnails_dir = normalize_config_path(base_dir, config.thumbnails_dir);
    }

    let config = Arc::new(config);
    if let Ok(mut guard) = state.config_cache.lock() {
        *guard = Some(Arc::clone(&config));
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
        *guard = Some(Arc::new(config.clone()));
    }
    Ok(())
}

pub fn setup_cancel_flag(state: &AppState, source: Source) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut guard) = state.cancel_flag.lock() {
        // 按 source 存，避免并发下载时后启动者覆盖前者的取消标志。
        guard.insert(source.to_string(), flag.clone());
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
    // 原子写：直接 write 会在中途崩溃/断电/取消时留下截断的损坏图片（内容寻址的
    // Reddit 文件名 `{hash}.{ext}` 会「看似正确」但图片已坏）。先写同目录临时文件再 rename。
    let mut tmp_os = save_path.as_os_str().to_os_string();
    tmp_os.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp_os);
    tokio::fs::write(&tmp, bytes)
        .await
        .map_err(|e| format!("写入临时文件失败 {}: {e}", tmp.display()))?;
    tokio::fs::rename(&tmp, save_path)
        .await
        .map_err(|e| format!("重命名文件失败 {}: {e}", save_path.display()))
}

/// 把目录加进 asset 协议白名单，让前端能通过 `convertFileSrc` 显示其中的图片。
///
/// `tauri.conf.json` 的静态 scope 只保留缩略图缓存目录，其余目录在运行时按需授权：
/// 用户配置的保存目录在启动/保存设置时加入，自定义浏览目录在用户主动选择时加入。
/// 这样既保证图片能显示，又不必把整个 `$HOME` 暴露给 asset 协议。
///
/// 授权失败只记日志不中断流程——最坏情况是图片显示不出来，不该让启动或保存失败。
pub fn allow_asset_dir(app: &tauri::AppHandle, dir: &str) {
    use tauri::Manager;

    if dir.is_empty() {
        return;
    }
    // 注意：目录还不存在时也必须授权。首启时默认保存目录通常尚未创建，而
    // `allow_directory` 只是登记一条 glob 规则（不校验路径存在），文件随后下载进来就能命中。
    // 若在这里用 `is_dir()` 提前返回，首次下载的图片会因为目录"当时不存在"而永远显示不出来。
    match app
        .asset_protocol_scope()
        .allow_directory(std::path::Path::new(dir), true)
    {
        Ok(()) => log::info!("[asset] 已授权目录: {dir}"),
        Err(e) => log::warn!("[asset] 授权目录失败 {dir}: {e}"),
    }
}

/// 按当前配置授权所有需要给前端读取的图片目录。
pub fn allow_config_asset_dirs(app: &tauri::AppHandle, config: &AppConfig) {
    allow_asset_dir(app, &config.wallhaven_save_dir);
    allow_asset_dir(app, &config.reddit_save_dir);
    allow_asset_dir(app, &config.thumbnails_dir);
}

/// 把**单个文件**加进 asset 协议白名单。
///
/// 用于路径不受本应用配置管辖、但确实要显示出来的图片（目前是系统当前的壁纸文件）。
/// 只授权这一个文件而不是它所在的目录，避免为了显示一张图就放开整个目录。
pub fn allow_asset_file(app: &tauri::AppHandle, path: &str) {
    use tauri::Manager;

    if path.is_empty() {
        return;
    }
    if let Err(e) = app
        .asset_protocol_scope()
        .allow_file(std::path::Path::new(path))
    {
        log::warn!("[asset] 授权文件失败 {path}: {e}");
    }
}
