//! 数据同步命令：快照导出 / 导入合并 / OSS 上传 / OSS 拉取。
//!
//! 同步模型：云盘/OSS 只是传输通道，同步的是 `VACUUM INTO` 单文件快照；
//! 合并发生在记录级（见 `db::import_*_snapshot`），绝不整库替换。

use crate::db::{self, ImportStats};
use crate::oss::{self, OssConfig};
use crate::state::{load_config, AppError, AppState};
use serde::Serialize;
use tauri::{Emitter, Manager};

/// 数据库命令统一放到阻塞线程池，避免 rusqlite 占用 tokio worker。
async fn run_blocking<F, T>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Other(format!("同步任务异常: {e}")))?
}

#[derive(Serialize)]
pub struct SyncImportResult {
    pub wallhaven: Option<ImportStats>,
    pub reddit: Option<ImportStats>,
}

#[derive(Serialize)]
pub struct SyncExportResult {
    /// 导出的快照文件路径（未导出的来源为 None）
    pub wallhaven: Option<String>,
    pub reddit: Option<String>,
}

/// 上传到 OSS 前在内存中持有的两个快照字节
struct SnapshotBytes {
    wallhaven: Option<Vec<u8>>,
    reddit: Option<Vec<u8>>,
}

/// 导出单个库的快照到临时目录并读回字节；库不存在时返回 Ok(None)。
fn export_snapshot_bytes(
    db_path: &str,
    file_name: &str,
    temp_dir: &std::path::Path,
) -> Result<Option<Vec<u8>>, AppError> {
    if !db::db_exists(db_path) {
        return Ok(None);
    }
    let path = temp_dir.join(file_name);
    db::export_snapshot(db_path, &path.to_string_lossy())
        .map_err(|e| AppError::Other(format!("导出快照失败 ({file_name}): {e}")))?;
    let bytes = std::fs::read(&path)
        .map_err(|e| AppError::Other(format!("读取快照失败 ({file_name}): {e}")))?;
    Ok(Some(bytes))
}

/// 导出两个数据库的快照到指定目录。
/// 文件名固定为 `wallhaven_images.db` / `reddit_images.db`（导入端按同名配对）。
#[tauri::command]
pub async fn export_snapshots(
    state: tauri::State<'_, AppState>,
    dir: String,
) -> Result<SyncExportResult, AppError> {
    log::info!("[CMD] export_snapshots: dir={}", dir);
    let config = load_config(&state)?;

    let wh_db = config.wallhaven_db_path.clone();
    let rd_db = config.reddit_db_path.clone();
    run_blocking(move || {
        let wh_target = std::path::Path::new(&dir).join("wallhaven_images.db");
        let rd_target = std::path::Path::new(&dir).join("reddit_images.db");

        let wallhaven = if db::db_exists(&wh_db) {
            db::export_snapshot(&wh_db, &wh_target.to_string_lossy())
                .map_err(|e| AppError::Other(format!("导出 Wallhaven 快照失败: {e}")))?;
            Some(wh_target.to_string_lossy().to_string())
        } else {
            None
        };
        let reddit = if db::db_exists(&rd_db) {
            db::export_snapshot(&rd_db, &rd_target.to_string_lossy())
                .map_err(|e| AppError::Other(format!("导出 Reddit 快照失败: {e}")))?;
            Some(rd_target.to_string_lossy().to_string())
        } else {
            None
        };

        if wallhaven.is_none() && reddit.is_none() {
            return Err(AppError::Other("两个数据库都不存在，无可导出内容".into()));
        }
        Ok(SyncExportResult { wallhaven, reddit })
    })
    .await
}

/// 从快照文件合并导入。两个路径都可选，但至少提供一个。
#[tauri::command]
pub async fn import_snapshots(
    state: tauri::State<'_, AppState>,
    wallhaven_path: Option<String>,
    reddit_path: Option<String>,
) -> Result<SyncImportResult, AppError> {
    log::info!(
        "[CMD] import_snapshots: wallhaven={:?}, reddit={:?}",
        wallhaven_path,
        reddit_path
    );
    if wallhaven_path.is_none() && reddit_path.is_none() {
        return Err(AppError::Other("未指定任何快照文件".into()));
    }
    let config = load_config(&state)?;

    let wh_db = config.wallhaven_db_path.clone();
    let rd_db = config.reddit_db_path.clone();
    run_blocking(move || {
        // 快照文件不存在的来源直接跳过（容错：用户选的目录里可能只有一个库的快照）
        let wallhaven = match wallhaven_path.filter(|p| std::path::Path::new(p).exists()) {
            Some(path) if db::db_exists(&wh_db) => {
                let stats = db::import_wallhaven_snapshot(&wh_db, &path)
                    .map_err(|e| AppError::Other(format!("导入 Wallhaven 快照失败: {e}")))?;
                db::invalidate_stats(&wh_db);
                Some(stats)
            }
            Some(_) => {
                return Err(AppError::Other(
                    "本地 Wallhaven 数据库不存在，请先初始化再导入".into(),
                ))
            }
            None => None,
        };
        let reddit = match reddit_path.filter(|p| std::path::Path::new(p).exists()) {
            Some(path) if db::db_exists(&rd_db) => {
                let stats = db::import_reddit_snapshot(&rd_db, &path)
                    .map_err(|e| AppError::Other(format!("导入 Reddit 快照失败: {e}")))?;
                db::invalidate_stats(&rd_db);
                Some(stats)
            }
            Some(_) => {
                return Err(AppError::Other(
                    "本地 Reddit 数据库不存在，请先初始化再导入".into(),
                ))
            }
            None => None,
        };
        if wallhaven.is_none() && reddit.is_none() {
            return Err(AppError::Other(
                "所选目录下没有找到快照文件（wallhaven_images.db / reddit_images.db）".into(),
            ));
        }
        Ok(SyncImportResult { wallhaven, reddit })
    })
    .await
}

/// 上传两个库的快照到 OSS（先导出到临时目录再 PUT）。
#[tauri::command]
pub async fn oss_sync_upload(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    log::info!("[CMD] oss_sync_upload");
    run_oss_upload(&state).await
}

/// 上传快照的实现，命令与退出钩子共用。
pub async fn run_oss_upload(state: &AppState) -> Result<String, AppError> {
    let config = load_config(state)?;
    let oss = OssConfig::from_config(&config)?;

    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁定 HTTP client 失败: {e}")))?
        .clone();

    let wh_db = config.wallhaven_db_path.clone();
    let rd_db = config.reddit_db_path.clone();
    let temp_dir = std::env::temp_dir().join("rustwallhub-sync");
    let snapshots = run_blocking(move || {
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| AppError::Other(format!("创建临时目录失败: {e}")))?;
        let wallhaven = export_snapshot_bytes(&wh_db, "wallhaven_images.db", &temp_dir)?;
        let reddit = export_snapshot_bytes(&rd_db, "reddit_images.db", &temp_dir)?;
        if wallhaven.is_none() && reddit.is_none() {
            return Err(AppError::Other("两个数据库都不存在，无可上传内容".into()));
        }
        Ok(SnapshotBytes { wallhaven, reddit })
    })
    .await?;

    let (wh_key, rd_key) = oss.snapshot_keys();
    let mut done = Vec::new();
    if let Some(bytes) = snapshots.wallhaven {
        oss::put_object(&client, &oss, &wh_key, bytes).await?;
        done.push("Wallhaven");
    }
    if let Some(bytes) = snapshots.reddit {
        oss::put_object(&client, &oss, &rd_key, bytes).await?;
        done.push("Reddit");
    }
    Ok(format!("已上传 {} 快照到 OSS", done.join("、")))
}

/// 从 OSS 拉取快照并合并导入到本地两个库。
#[tauri::command]
pub async fn oss_sync_download(state: tauri::State<'_, AppState>) -> Result<SyncImportResult, AppError> {
    log::info!("[CMD] oss_sync_download");
    run_oss_download(&state).await
}

/// 拉取并合并的实现，命令与启动钩子共用。
pub async fn run_oss_download(state: &AppState) -> Result<SyncImportResult, AppError> {
    let config = load_config(state)?;
    let oss = OssConfig::from_config(&config)?;

    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁定 HTTP client 失败: {e}")))?
        .clone();

    let (wh_key, rd_key) = oss.snapshot_keys();
    let wh_exists = oss::head_object(&client, &oss, &wh_key).await?;
    let rd_exists = oss::head_object(&client, &oss, &rd_key).await?;
    if !wh_exists && !rd_exists {
        return Err(AppError::Other("云端没有任何快照，请先在其他电脑上上传".into()));
    }

    let wh_bytes = if wh_exists {
        Some(oss::get_object(&client, &oss, &wh_key).await?)
    } else {
        None
    };
    let rd_bytes = if rd_exists {
        Some(oss::get_object(&client, &oss, &rd_key).await?)
    } else {
        None
    };

    let wh_db = config.wallhaven_db_path.clone();
    let rd_db = config.reddit_db_path.clone();
    let temp_dir = std::env::temp_dir().join("rustwallhub-sync");
    run_blocking(move || {
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| AppError::Other(format!("创建临时目录失败: {e}")))?;

        let wallhaven = match wh_bytes {
            Some(bytes) if db::db_exists(&wh_db) => {
                let path = temp_dir.join("wallhaven_images.db");
                std::fs::write(&path, &bytes)
                    .map_err(|e| AppError::Other(format!("写入临时快照失败: {e}")))?;
                let stats = db::import_wallhaven_snapshot(&wh_db, &path.to_string_lossy())
                    .map_err(|e| AppError::Other(format!("导入 Wallhaven 快照失败: {e}")))?;
                db::invalidate_stats(&wh_db);
                Some(stats)
            }
            Some(_) => {
                return Err(AppError::Other(
                    "本地 Wallhaven 数据库不存在，请先初始化再从云端拉取".into(),
                ))
            }
            None => None,
        };

        let reddit = match rd_bytes {
            Some(bytes) if db::db_exists(&rd_db) => {
                let path = temp_dir.join("reddit_images.db");
                std::fs::write(&path, &bytes)
                    .map_err(|e| AppError::Other(format!("写入临时快照失败: {e}")))?;
                let stats = db::import_reddit_snapshot(&rd_db, &path.to_string_lossy())
                    .map_err(|e| AppError::Other(format!("导入 Reddit 快照失败: {e}")))?;
                db::invalidate_stats(&rd_db);
                Some(stats)
            }
            Some(_) => {
                return Err(AppError::Other(
                    "本地 Reddit 数据库不存在，请先初始化再从云端拉取".into(),
                ))
            }
            None => None,
        };

        Ok(SyncImportResult { wallhaven, reddit })
    })
    .await
}

/// 测试 OSS 配置连通性（HEAD 探测快照对象）。
/// 返回人类可读的结果描述，不区分"无快照"与"有快照"为错误。
#[tauri::command]
pub async fn test_oss_config(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    log::info!("[CMD] test_oss_config");
    let config = load_config(&state)?;
    let oss = OssConfig::from_config(&config)?;

    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁定 HTTP client 失败: {e}")))?
        .clone();

    let (wh_key, _) = oss.snapshot_keys();
    let exists = oss::head_object(&client, &oss, &wh_key).await?;
    Ok(if exists {
        "连接成功，云端已有快照".into()
    } else {
        "连接成功，云端暂无快照（上传后可在此拉取）".into()
    })
}

// ---------------------------------------------------------------------------
// 自动同步钩子（启动拉取 / 退出上传）
// ---------------------------------------------------------------------------

/// 退出自动上传只做一次：否则 `exit()` 触发的第二次 ExitRequested 会再次拦截，形成死循环。
static EXIT_SYNC_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 把合并结果转成一句人类可读的摘要（事件与日志共用）
pub fn format_import_result(r: &SyncImportResult) -> String {
    let mut parts = Vec::new();
    if let Some(s) = r.wallhaven {
        parts.push(format!(
            "Wallhaven 新增 {} 条、恢复 {} 条",
            s.inserted, s.loved
        ));
    }
    if let Some(s) = r.reddit {
        parts.push(format!("Reddit 新增 {} 条、恢复 {} 条", s.inserted, s.loved));
    }
    if parts.is_empty() {
        "没有可导入的内容".to_string()
    } else {
        parts.join("；")
    }
}

/// 退出前是否需要自动上传（只返回是否需要，实际执行在 `auto_sync_on_exit`）。
/// 用 `EXIT_SYNC_DONE` 保证整个进程生命周期内只拦一次退出。
pub fn should_auto_upload_on_exit(state: &AppState) -> bool {
    if EXIT_SYNC_DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return false;
    }
    load_config(state)
        .map(|c| c.oss_auto_upload_on_exit)
        .unwrap_or(false)
}

/// 退出时的自动上传。带整体超时，避免网络卡死导致应用关不掉。
pub async fn auto_sync_on_exit(handle: tauri::AppHandle) {
    log::info!("[sync] 退出自动上传开始");
    let state = handle.state::<AppState>();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        run_oss_upload(&state),
    )
    .await;
    match result {
        Ok(Ok(msg)) => log::info!("[sync] 退出自动上传完成：{msg}"),
        Ok(Err(e)) => log::warn!("[sync] 退出自动上传失败：{e}"),
        Err(_) => log::warn!("[sync] 退出自动上传超时（15 秒），已放弃"),
    }
}

/// 启动时的自动拉取。失败只记日志，不打断应用启动；
/// 本地库尚未初始化时静默跳过（初始化由前端引导）。
pub async fn auto_sync_on_startup(handle: &tauri::AppHandle) {
    let state = handle.state::<AppState>();
    let config = match load_config(&state) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[sync] 启动自动拉取：读取配置失败 {e}");
            return;
        }
    };
    if !config.oss_auto_download_on_start {
        return;
    }
    if !db::db_exists(&config.wallhaven_db_path) && !db::db_exists(&config.reddit_db_path) {
        log::info!("[sync] 启动自动拉取：本地数据库尚未初始化，跳过");
        return;
    }

    log::info!("[sync] 启动自动拉取开始");
    match run_oss_download(&state).await {
        Ok(r) => {
            let msg = format_import_result(&r);
            log::info!("[sync] 启动自动拉取完成：{msg}");
            let _ = handle.emit("sync-completed", msg);
        }
        Err(e) => {
            log::warn!("[sync] 启动自动拉取失败：{e}");
            let _ = handle.emit("sync-failed", e.to_string());
        }
    }
}
