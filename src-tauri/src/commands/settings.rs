//! Settings & stats commands: get_config, save_settings, get_stats, check_update.

use crate::config::AppConfig;
use crate::db;
use crate::state::{rebuild_http_client, AppError, AppState};
use serde::Serialize;
use tauri::Emitter;
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize)]
pub struct StatsResponse {
    pub wallhaven: db::DbStats,
    pub reddit: db::DbStats,
}

#[derive(Serialize, Clone)]
pub struct UpdateInfo {
    pub has_update: bool,
    pub version: String,
    pub current_version: String,
    pub body: Option<String>,
    pub date: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

#[tauri::command]
pub async fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, AppError> {
    log::info!("[CMD] get_config called");
    let result = crate::state::load_config(&state);
    log::info!(
        "[CMD] get_config {}",
        if result.is_ok() { "ok" } else { "failed" }
    );
    result
}

#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    config: AppConfig,
) -> Result<(), AppError> {
    log::info!("[CMD] save_settings called");
    if config.wallhaven_save_dir.is_empty() || config.reddit_save_dir.is_empty() {
        return Err(AppError::Config("保存目录不能为空".into()));
    }
    if config.db_dir.is_empty() {
        return Err(AppError::Config("数据库目录不能为空".into()));
    }
    if (config.download_concurrency as usize) < 1 || config.download_concurrency > 100 {
        return Err(AppError::Config("并发下载数超出范围 (1-100)".into()));
    }
    if config.request_timeout < 5 || config.request_timeout > 120 {
        return Err(AppError::Config("请求超时超出范围 (5-120 秒)".into()));
    }
    if config.thumbnail_dpr < 1 || config.thumbnail_dpr > 3 {
        return Err(AppError::Config("缩略图 DPR 超出范围 (1-3)".into()));
    }
    crate::state::save_config(&state, &config)?;
    if let Ok(mut cache) = state.file_cache.lock() {
        *cache = None;
    }
    std::fs::create_dir_all(std::path::Path::new(&config.db_dir)).ok();
    db::init_wallhaven_db(&config.wallhaven_db_path).ok();
    db::init_reddit_db(&config.reddit_db_path).ok();
    let _ = rebuild_http_client(&state, config.request_timeout);
    let _ = app.emit("settings-changed", ());
    log::info!("[CMD] save_settings done");
    Ok(())
}

#[tauri::command]
pub async fn get_stats(state: tauri::State<'_, AppState>) -> Result<StatsResponse, AppError> {
    log::info!("[CMD] get_stats called");
    let config = crate::state::load_config(&state)?;
    let wh_db_path = config.wallhaven_db_path.clone();
    let rd_db_path = config.reddit_db_path.clone();
    log::info!(
        "[CMD] get_stats: resolving db paths wh={}, rd={}",
        wh_db_path,
        rd_db_path
    );
    let wh_stats = db::get_db_stats(&wh_db_path)?;
    let rd_stats = db::get_db_stats(&rd_db_path)?;
    log::info!("[CMD] get_stats: wh={:?}, rd={:?}", wh_stats, rd_stats);
    Ok(StatsResponse {
        wallhaven: wh_stats,
        reddit: rd_stats,
    })
}

async fn do_check_update(app: &tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app
        .updater()
        .map_err(|e| format!("初始化 updater 失败: {e}"))?;
    let current_version = app.package_info().version.to_string();
    match updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?
    {
        Some(update) => Ok(Some(UpdateInfo {
            has_update: true,
            version: update.version,
            current_version,
            body: update.body,
            date: update.date.map(|d| d.to_string()),
        })),
        None => Ok(Some(UpdateInfo {
            has_update: false,
            version: current_version.clone(),
            current_version,
            body: None,
            date: None,
        })),
    }
}

#[tauri::command]
pub async fn check_update(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    log::info!("[CMD] check_update called");
    do_check_update(&app).await.map(|opt| {
        opt.unwrap_or_else(|| UpdateInfo {
            has_update: false,
            version: app.package_info().version.to_string(),
            current_version: app.package_info().version.to_string(),
            body: None,
            date: None,
        })
    })
}

/// Downloads and installs the latest update, then restarts the app.
/// Emits `update-progress` (with `UpdateProgress`) during download and
/// `update-installing` when the download finishes and installation begins.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    log::info!("[CMD] install_update called");
    let updater = app
        .updater()
        .map_err(|e| format!("初始化 updater 失败: {e}"))?;
    match updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?
    {
        Some(update) => {
            log::info!("[updater] 开始下载安装更新: v{}", update.version);
            let app_for_progress = app.clone();
            let app_for_finish = app.clone();
            let mut downloaded: u64 = 0;

            update
                .download_and_install(
                    move |chunk_len, content_length| {
                        downloaded += chunk_len as u64;
                        let _ = app_for_progress.emit(
                            "update-progress",
                            UpdateProgress {
                                downloaded,
                                total: content_length,
                            },
                        );
                    },
                    move || {
                        let _ = app_for_finish.emit("update-installing", ());
                    },
                )
                .await
                .map_err(|e| format!("下载安装失败: {e}"))?;

            log::info!("[updater] 更新安装完成，正在重启...");
            app.restart();
            #[allow(unreachable_code)]
            Ok(())
        }
        None => Err("没有可用的更新".into()),
    }
}

/// Called from `lib.rs` setup to perform delayed auto-update check on startup.
pub async fn startup_check_update(app: &tauri::AppHandle) {
    match do_check_update(app).await {
        Ok(Some(info)) if info.has_update => {
            log::info!("[updater] 发现新版本: {}", info.version);
            let _ = app.emit("update-available", info);
        }
        Ok(Some(_)) => log::info!("[updater] 当前已是最新版本"),
        Ok(None) => {}
        Err(e) => log::warn!("[updater] 自动检查更新失败: {e}"),
    }
}
