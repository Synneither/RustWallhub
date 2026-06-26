mod config;
mod db;
mod downloader;
mod reddit;
mod thumbnail;
mod wallhaven;

use config::AppConfig;
use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

#[derive(Clone, Serialize)]
struct DownloadProgress {
    source: String,
    done: u32,
    total: u32,
    message: String,
}

#[derive(Clone, Serialize)]
struct DownloadComplete {
    source: String,
    success: u32,
    total: u32,
    message: String,
}

#[derive(Clone, Serialize)]
struct ImageDownloaded {
    source: String,
    name: String,
    path: String,
}

struct FileListCache {
    items: Vec<FileEntry>,
    total: usize,
    source: String,
    dir_path: String,
    cached_at: Instant,
}

#[derive(Clone)]
struct FileEntry {
    name: String,
    path: String,
    size: u64,
    is_orphan: bool,
}

struct AppState {
    config_path: Mutex<PathBuf>,
    file_cache: Mutex<Option<FileListCache>>,
    cancel_flag: Mutex<Option<Arc<AtomicBool>>>,
    http_client: reqwest::Client,
    config_cache: Mutex<Option<AppConfig>>,
}

#[derive(thiserror::Error, Debug)]
enum AppError {
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

const MAX_UPWARD_DEPTH: u32 = 100;

fn find_upward(
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
            log::warn!("[find_upward] exceeded max depth {} at {:?}", MAX_UPWARD_DEPTH, current);
            break;
        }
        if !current.pop() {
            break;
        }
        depth += 1;
    }
    None
}

fn database_score(path: &std::path::Path) -> Option<i64> {
    if !path.exists() {
        return None;
    }
    let conn = Connection::open(path).ok()?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))
        .ok()?;
    Some(count)
}

fn normalize_config_path(base_dir: &std::path::Path, value: String) -> String {
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
            if best.is_none() || score > best.as_ref().unwrap().1 {
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

fn load_config(state: &tauri::State<'_, AppState>) -> Result<AppConfig, AppError> {
    if let Ok(guard) = state.config_cache.lock() {
        if let Some(ref cached) = *guard {
            return Ok(cached.clone());
        }
    }

    let path = state
        .config_path
        .lock()
        .map_err(|e| AppError::Config(format!("锁定配置失败: {e}")))?
        .clone();
    let mut config = AppConfig::load(&path).map_err(AppError::Config)?;
    if let Some(base_dir) = path.parent() {
        config.wallhaven_db_path = normalize_config_path(base_dir, config.wallhaven_db_path);
        config.reddit_db_path = normalize_config_path(base_dir, config.reddit_db_path);
        config.wallhaven_save_dir = normalize_config_path(base_dir, config.wallhaven_save_dir);
        config.reddit_save_dir = normalize_config_path(base_dir, config.reddit_save_dir);
    }

    if let Ok(mut guard) = state.config_cache.lock() {
        *guard = Some(config.clone());
    }

    Ok(config)
}

fn save_config(state: &tauri::State<'_, AppState>, config: &AppConfig) -> Result<(), AppError> {
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

#[tauri::command]
async fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, AppError> {
    log::info!("[CMD] get_config called");
    let result = load_config(&state);
    log::info!("[CMD] get_config {}", if result.is_ok() { "ok" } else { "failed" });
    result
}

#[tauri::command]
async fn save_settings(
    state: tauri::State<'_, AppState>,
    config: AppConfig,
) -> Result<(), AppError> {
    log::info!("[CMD] save_settings called");
    if config.wallhaven_save_dir.is_empty() || config.reddit_save_dir.is_empty() {
        return Err(AppError::Config("保存目录不能为空".into()));
    }
    if config.wallhaven_db_path.is_empty() || config.reddit_db_path.is_empty() {
        return Err(AppError::Config("数据库路径不能为空".into()));
    }
    let result = save_config(&state, &config);
    log::info!("[CMD] save_settings {}", if result.is_ok() { "ok" } else { "failed" });
    result
}

#[derive(serde::Serialize)]
struct StatsResponse {
    wallhaven: db::DbStats,
    reddit: db::DbStats,
}

#[tauri::command]
async fn get_stats(state: tauri::State<'_, AppState>) -> Result<StatsResponse, AppError> {
    log::info!("[CMD] get_stats called");
    let config = load_config(&state)?;
    let wh_db_path = config.wallhaven_db_path.clone();
    let rd_db_path = config.reddit_db_path.clone();
    log::info!(
        "[CMD] get_stats: resolving db paths wh={}, rd={}",
        wh_db_path, rd_db_path
    );
    let wh_stats = db::get_db_stats(&wh_db_path)?;
    let rd_stats = db::get_db_stats(&rd_db_path)?;
    log::info!(
        "[CMD] get_stats: wh={:?}, rd={:?}",
        wh_stats, rd_stats
    );
    Ok(StatsResponse {
        wallhaven: wh_stats,
        reddit: rd_stats,
    })
}

fn setup_cancel_flag(state: &AppState) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut guard) = state.cancel_flag.lock() {
        *guard = Some(flag.clone());
    }
    flag
}

#[tauri::command]
async fn start_wallhaven_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    log::info!("[CMD] start_wallhaven_download called");
    let config = load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let app_clone = app.clone();
    let client = state.http_client.clone();

    tokio::spawn(async move {
        let wh_client = wallhaven::WallhavenClient::new(client.clone(), config.wallhaven_api_key.clone());

        let _ = tokio::fs::create_dir_all(&config.wallhaven_save_dir).await;

        let existing_ids = match db::get_existing_wallhaven_ids(&config.wallhaven_db_path) {
            Ok(ids) => ids,
            Err(e) => {
                log::error!("[wallhaven] 获取已有ID失败: {e}");
                return;
            }
        };
        let existing_set: std::collections::HashSet<String> = existing_ids.into_iter().collect();

        let mut collected: Vec<wallhaven::WallhavenImage> = Vec::new();
        let target = config.wallhaven_max_images;
        let mut page = 1u32;
        let max_pages = 100u32;

        while (collected.len() as u32) < target && page <= max_pages {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: "wallhaven".into(),
                    done: collected.len() as u32,
                    total: target,
                    message: format!("正在获取第 {page} 页..."),
                },
            );

            let resp = wh_client.search(
                page,
                &config.wallhaven_categories,
                &config.wallhaven_purity,
                &config.wallhaven_sorting,
                &config.wallhaven_order,
                &config.wallhaven_top_range,
                &config.wallhaven_atleast,
                &config.wallhaven_ratios,
                &config.wallhaven_q,
            ).await;

            match resp {
                Ok(data) => {
                    if data.data.is_empty() {
                        break;
                    }
                    for img in data.data {
                        if (collected.len() as u32) >= target {
                            break;
                        }
                        if !existing_set.contains(&img.id) {
                            collected.push(img);
                        }
                    }
                }
                Err(e) => {
                    let _ = app_clone.emit(
                        "download-progress",
                        DownloadProgress {
                            source: "wallhaven".into(),
                            done: collected.len() as u32,
                            total: target,
                            message: format!("获取第 {page} 页失败: {e}"),
                        },
                    );
                    break;
                }
            }
            page += 1;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        let total = collected.len() as u32;
        let mut success = 0u32;

        let urls: Vec<String> = collected.iter().map(|img| img.path.clone()).collect();
        let download_results =
            downloader::download_urls_concurrent(&client, &urls, cancel.clone(), 3).await;

        for (i, img) in collected.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                log::info!("[wallhaven] download cancelled (success={}/{})", success, total);
                let _ = app_clone.emit(
                    "download-complete",
                    DownloadComplete {
                        source: "wallhaven".into(),
                        success, total,
                        message: "下载已取消".to_string(),
                    },
                );
                return;
            }

            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: "wallhaven".into(),
                    done: i as u32,
                    total,
                    message: format!("正在处理 {} ({}/{})", img.id, i + 1, total),
                },
            );

            match &download_results[i] {
                Ok((bytes, content_type)) => {
                    let ext = downloader::get_file_extension(content_type, &img.path);
                    let safe_id = img
                        .id
                        .chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>();
                    let filename = format!("wallhaven_{safe_id}.{ext}");
                    let save_path =
                        std::path::Path::new(&config.wallhaven_save_dir).join(&filename);
                    let hash = downloader::compute_md5(bytes);

                    if tokio::fs::write(&save_path, bytes).await.is_ok() {
                        let thumb_dir = config.wallhaven_thumb_dir();
                        let bytes_clone = bytes.clone();
                        let filename_clone = filename.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            thumbnail::save_thumbnail_from_bytes(
                                &thumb_dir,
                                &filename_clone,
                                &bytes_clone,
                                2,
                            )
                        }).await;

                        let db_path = config.wallhaven_db_path.clone();
                        let img_id = img.id.clone();
                        let filename_clone = filename.clone();
                        let hash_clone = hash.clone();
                        let img_path = img.path.clone();
                        let img_url = img.short_url.clone();
                        let img_res = img.resolution.clone();
                        let inserted = tokio::task::spawn_blocking(move || {
                            db::insert_wallhaven_image(
                                &db_path,
                                &img_id,
                                &filename_clone,
                                &hash_clone,
                                &img_path,
                                &img_url,
                                &img_res,
                            )
                            .unwrap_or(false)
                        }).await.unwrap_or(false);

                        if inserted {
                            success += 1;
                            let _ = app_clone.emit(
                                "image-downloaded",
                                ImageDownloaded {
                                    source: "wallhaven".into(),
                                    name: filename.clone(),
                                    path: save_path.to_string_lossy().to_string(),
                                },
                            );
                        }
                    }
                }
                Err(e) => {
                    log::error!("[wallhaven] download failed {}: {}", img.id, e);
                }
            }
        }

        log::info!("[wallhaven] download complete (success={}/{})", success, total);
        let _ = app_clone.emit(
            "download-complete",
            DownloadComplete {
                source: "wallhaven".into(),
                success,
                total,
                message: format!("Wallhaven 下载完成: 成功 {success}/{total}"),
            },
        );
    });

    Ok("Wallhaven 下载已启动".to_string())
}

#[tauri::command]
async fn start_reddit_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    log::info!("[CMD] start_reddit_download called");
    let config = load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let app_clone = app.clone();
    let client = state.http_client.clone();

    tokio::spawn(async move {
        let reddit_client = reddit::RedditClient::new(client.clone());

        let _ = tokio::fs::create_dir_all(&config.reddit_save_dir).await;

        let existing_urls = match db::get_existing_reddit_urls(&config.reddit_db_path) {
            Ok(urls) => urls,
            Err(e) => {
                log::error!("[reddit] 获取已有URL失败: {e}");
                return;
            }
        };
        let existing_set: std::collections::HashSet<String> = existing_urls.into_iter().collect();

        let target = config.reddit_max_images;
        let mut collected: Vec<reddit::RedditImage> = Vec::new();
        let mut after: Option<String> = None;
        let mut empty_batches = 0u32;

        while (collected.len() as u32) < target {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: "reddit".into(),
                    done: collected.len() as u32,
                    total: target,
                    message: format!("正在获取帖子... (已找到 {} 张)", collected.len()),
                },
            );

            let result =
                reddit_client.fetch_posts(after.as_deref(), config.reddit_max_posts).await;

            match result {
                Ok((images, next_after)) => {
                    let prev_len = collected.len();
                    for img in images {
                        if (collected.len() as u32) >= target {
                            break;
                        }
                        if !existing_set.contains(&img.image_url) {
                            collected.push(img);
                        }
                    }
                    if collected.len() == prev_len {
                        empty_batches += 1;
                        if empty_batches >= 3 {
                            break;
                        }
                    } else {
                        empty_batches = 0;
                    }
                    after = next_after;
                    if after.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = app_clone.emit(
                        "download-progress",
                        DownloadProgress {
                            source: "reddit".into(),
                            done: collected.len() as u32,
                            total: target,
                            message: format!("获取帖子失败: {e}"),
                        },
                    );
                    break;
                }
            }
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        let total = collected.len() as u32;
        let mut success = 0u32;

        let urls: Vec<String> = collected.iter().map(|img| img.image_url.clone()).collect();
        let download_results =
            downloader::download_urls_concurrent(&client, &urls, cancel.clone(), 3).await;

        for (i, img) in collected.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                log::info!("[reddit] download cancelled (success={}/{})", success, total);
                let _ = app_clone.emit(
                    "download-complete",
                    DownloadComplete {
                        source: "reddit".into(),
                        success, total,
                        message: "下载已取消".to_string(),
                    },
                );
                return;
            }

            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: "reddit".into(),
                    done: i as u32,
                    total,
                    message: format!("正在下载 ({}/{})", i + 1, total),
                },
            );

            match &download_results[i] {
                Ok((bytes, content_type)) => {
                    let ext = downloader::get_file_extension(content_type, &img.image_url);
                    let hash = downloader::compute_md5(bytes);
                    let filename = format!("{hash}.{ext}");
                    let save_path = std::path::Path::new(&config.reddit_save_dir).join(&filename);

                    if tokio::fs::write(&save_path, bytes).await.is_ok() {
                        let thumb_dir = config.reddit_thumb_dir();
                        let bytes_clone = bytes.clone();
                        let filename_clone = filename.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            thumbnail::save_thumbnail_from_bytes(
                                &thumb_dir,
                                &filename_clone,
                                &bytes_clone,
                                2,
                            )
                        }).await;

                        let db_path = config.reddit_db_path.clone();
                        let filename_clone = filename.clone();
                        let hash_clone = hash.clone();
                        let image_url = img.image_url.clone();
                        let title = img.title.clone();
                        let permalink = img.permalink.clone();
                        let inserted = tokio::task::spawn_blocking(move || {
                            db::insert_reddit_image(
                                &db_path,
                                &filename_clone,
                                &hash_clone,
                                &image_url,
                                &title,
                                &permalink,
                            )
                            .unwrap_or(false)
                        }).await.unwrap_or(false);

                        if inserted {
                            success += 1;
                            let _ = app_clone.emit(
                                "image-downloaded",
                                ImageDownloaded {
                                    source: "reddit".into(),
                                    name: filename.clone(),
                                    path: save_path.to_string_lossy().to_string(),
                                },
                            );
                        }
                    }
                }
                Err(e) => {
                    log::error!("[reddit] download failed {}: {}", img.title, e);
                }
            }
        }

        log::info!("[reddit] download complete (success={}/{})", success, total);
        let _ = app_clone.emit(
            "download-complete",
            DownloadComplete {
                source: "reddit".into(),
                success,
                total,
                message: format!("Reddit 下载完成: 成功 {success}/{total}"),
            },
        );
    });

    Ok("Reddit 下载已启动".to_string())
}

#[tauri::command]
async fn start_db_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    source: String,
) -> Result<String, AppError> {
    log::info!("[CMD] start_db_download called: source={}", source);
    let config = load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let app_clone = app.clone();
    let source_clone = source.clone();
    let client = state.http_client.clone();

    tokio::spawn(async move {

        let (save_dir, db_path, thumb_dir) = if source_clone == "wallhaven" {
            (
                config.wallhaven_save_dir.clone(),
                config.wallhaven_db_path.clone(),
                config.wallhaven_thumb_dir(),
            )
        } else if source_clone == "reddit" {
            (
                config.reddit_save_dir.clone(),
                config.reddit_db_path.clone(),
                config.reddit_thumb_dir(),
            )
        } else {
            log::error!("[db_download] 未知源: {}", source_clone);
            return;
        };

        let _ = tokio::fs::create_dir_all(&save_dir).await;

        let images = if source_clone == "wallhaven" {
            match db::get_wallhaven_missing_love(&db_path) {
                Ok(imgs) => imgs,
                Err(e) => {
                    log::error!("[db_download] 获取wallhaven缺失图片失败: {e}");
                    return;
                }
            }
        } else {
            match db::get_reddit_missing_love(&db_path) {
                Ok(imgs) => imgs,
                Err(e) => {
                    log::error!("[db_download] 获取reddit缺失图片失败: {e}");
                    return;
                }
            }
        };

        let total = images.len() as u32;
        let mut success = 0u32;

        let to_download: Vec<&db::ImageRecord> = images
            .iter()
            .filter(|img| !std::path::Path::new(&save_dir).join(&img.name).exists())
            .collect();
        let total_pending = to_download.len() as u32;

        let urls: Vec<String> = to_download.iter().map(|img| img.url.clone()).collect();
        let download_results =
            downloader::download_urls_concurrent(&client, &urls, cancel.clone(), 3).await;

        for (i, img) in to_download.iter().enumerate() {
            let file_path = std::path::Path::new(&save_dir).join(&img.name);

            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: source_clone.clone(),
                    done: i as u32,
                    total: total_pending,
                    message: format!("正在下载 {} ({}/{})", img.name, i + 1, total_pending),
                },
            );

            if cancel.load(Ordering::Relaxed) {
                log::info!("[db_download] cancelled: source={} (success={}/{})", source_clone, success, total_pending);
                let _ = app_clone.emit(
                    "download-complete",
                    DownloadComplete {
                        source: source_clone.clone(),
                        success, total,
                        message: "下载已取消".to_string(),
                    },
                );
                return;
            }

            match &download_results[i] {
                Ok((bytes, _content_type)) => {
                    if tokio::fs::write(&file_path, bytes).await.is_ok() {
                        let thumb_dir = thumb_dir.clone();
                        let img_name = img.name.clone();
                        let bytes_clone = bytes.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            thumbnail::save_thumbnail_from_bytes(
                                &thumb_dir,
                                &img_name,
                                &bytes_clone,
                                2,
                            )
                        }).await;
                        success += 1;
                    } else {
                        log::error!("[db_download] write failed {}", file_path.display());
                    }
                }
                Err(e) => {
                    log::error!("[db_download] download failed {}: {}", img.name, e);
                }
            }
        }

        log::info!("[db_download] complete: success={}/{}", success, total);
        let _ = app_clone.emit(
            "download-complete",
            DownloadComplete {
                source: source_clone,
                success,
                total,
                message: format!("数据库下载完成: 成功 {success}/{total}"),
            },
        );
    });

    Ok(format!("{source} 数据库下载已启动"))
}

#[tauri::command]
async fn mark_dislike(state: tauri::State<'_, AppState>, source: String) -> Result<u64, AppError> {
    log::info!("[CMD] mark_dislike called: source={}", source);
    let config = load_config(&state)?;
    if source == "wallhaven" {
        Ok(db::mark_missing_dislike_wallhaven(
            &config.wallhaven_db_path,
            &config.wallhaven_save_dir,
        )?)
    } else if source == "reddit" {
        Ok(db::mark_missing_dislike_reddit(
            &config.reddit_db_path,
            &config.reddit_save_dir,
        )?)
    } else {
        let w = db::mark_missing_dislike_wallhaven(
            &config.wallhaven_db_path,
            &config.wallhaven_save_dir,
        )?;
        let r = db::mark_missing_dislike_reddit(&config.reddit_db_path, &config.reddit_save_dir)?;
        Ok(w + r)
    }
}

#[tauri::command]
async fn count_missing_images(
    state: tauri::State<'_, AppState>,
    source: String,
) -> Result<u64, AppError> {
    log::info!("[CMD] count_missing_images called: source={}", source);
    let config = load_config(&state)?;
    if source == "wallhaven" {
        Ok(db::count_missing_wallhaven(
            &config.wallhaven_db_path,
            &config.wallhaven_save_dir,
        )?)
    } else if source == "reddit" {
        Ok(db::count_missing_reddit(
            &config.reddit_db_path,
            &config.reddit_save_dir,
        )?)
    } else {
        let w = db::count_missing_wallhaven(
            &config.wallhaven_db_path,
            &config.wallhaven_save_dir,
        )?;
        let r = db::count_missing_reddit(&config.reddit_db_path, &config.reddit_save_dir)?;
        Ok(w + r)
    }
}

#[tauri::command]
async fn restore_love(state: tauri::State<'_, AppState>, source: String) -> Result<u64, AppError> {
    log::info!("[CMD] restore_love called: source={}", source);
    let config = load_config(&state)?;
    if source == "wallhaven" {
        Ok(db::restore_love_db(&config.wallhaven_db_path)?)
    } else if source == "reddit" {
        Ok(db::restore_love_db(&config.reddit_db_path)?)
    } else {
        let w = db::restore_love_db(&config.wallhaven_db_path)?;
        let r = db::restore_love_db(&config.reddit_db_path)?;
        Ok(w + r)
    }
}

#[tauri::command]
async fn list_missing_images(
    state: tauri::State<'_, AppState>,
    source: String,
) -> Result<Vec<db::ImageRecord>, AppError> {
    log::info!("[CMD] list_missing_images called: source={}", source);
    let config = load_config(&state)?;
    if source == "wallhaven" {
        Ok(db::get_wallhaven_missing_files(
            &config.wallhaven_db_path,
            &config.wallhaven_save_dir,
        )?)
    } else if source == "reddit" {
        Ok(db::get_reddit_missing_files(
            &config.reddit_db_path,
            &config.reddit_save_dir,
        )?)
    } else {
        let mut all = db::get_wallhaven_missing_files(
            &config.wallhaven_db_path,
            &config.wallhaven_save_dir,
        )?;
        all.extend(db::get_reddit_missing_files(
            &config.reddit_db_path,
            &config.reddit_save_dir,
        )?);
        Ok(all)
    }
}

#[derive(serde::Serialize)]
struct OrphanFile {
    name: String,
    path: String,
    size: u64,
    source: String,
}

#[tauri::command]
async fn list_orphan_files(
    state: tauri::State<'_, AppState>,
    source: String,
) -> Result<Vec<OrphanFile>, AppError> {
    log::info!("[CMD] list_orphan_files called: source={}", source);
    let config = load_config(&state)?;

    let check_source = |src: &str, save_dir: &str, db_path: &str| -> Result<Vec<OrphanFile>, AppError> {
        let dir = std::path::Path::new(save_dir);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let db_names: std::collections::HashSet<String> =
            db::get_all_filenames(db_path)?.into_iter().collect();

        let mut orphans = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if file_path.is_file() && downloader::file_is_image(&file_path) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !db_names.contains(&name) {
                        orphans.push(OrphanFile {
                            name,
                            path: file_path.to_string_lossy().to_string(),
                            size: entry.metadata().map(|m| m.len()).unwrap_or(0),
                            source: src.to_string(),
                        });
                    }
                }
            }
        }
        Ok(orphans)
    };

    if source == "wallhaven" {
        check_source("wallhaven", &config.wallhaven_save_dir, &config.wallhaven_db_path)
    } else if source == "reddit" {
        check_source("reddit", &config.reddit_save_dir, &config.reddit_db_path)
    } else {
        let mut all = check_source("wallhaven", &config.wallhaven_save_dir, &config.wallhaven_db_path)?;
        all.extend(check_source("reddit", &config.reddit_save_dir, &config.reddit_db_path)?);
        Ok(all)
    }
}

#[tauri::command]
async fn mark_dislike_image(
    state: tauri::State<'_, AppState>,
    source: String,
    name: String,
) -> Result<bool, AppError> {
    log::info!("[CMD] mark_dislike_image: source={}, name={}", source, name);
    let config = load_config(&state)?;
    let (save_dir, db_path, thumb_dir) = if source == "wallhaven" {
        (
            config.wallhaven_save_dir.clone(),
            config.wallhaven_db_path.clone(),
            config.wallhaven_thumb_dir(),
        )
    } else {
        (
            config.reddit_save_dir.clone(),
            config.reddit_db_path.clone(),
            config.reddit_thumb_dir(),
        )
    };

    // DB 标记为不喜欢
    let db_ok = db::mark_dislike_by_name(&db_path, &name)?;

    // 删除文件
    let file_path = std::path::Path::new(&save_dir).join(&name);
    if file_path.exists() {
        std::fs::remove_file(&file_path).map_err(|e| {
            log::error!("[mark_dislike_image] 删除文件失败 {}: {}", file_path.display(), e);
            AppError::Io(e)
        })?;
    }

    // 删除缩略图（兼容新旧格式）
    let thumb_old = thumb_dir.join(&name);
    if thumb_old.exists() { std::fs::remove_file(&thumb_old).ok(); }
    // 新格式：name__w240, name__w480, name__w720
    for dpr in [1u32, 2, 3] {
        let tp = thumb_dir.join(thumbnail::thumb_filename_for_dpr(&name, dpr));
        if tp.exists() { std::fs::remove_file(&tp).ok(); }
    }

    Ok(db_ok)
}

#[tauri::command]
async fn delete_orphan_file(
    state: tauri::State<'_, AppState>,
    source: String,
    name: String,
) -> Result<bool, AppError> {
    log::info!("[CMD] delete_orphan_file: source={}, name={}", source, name);
    let config = load_config(&state)?;
    let (save_dir, thumb_dir) = if source == "wallhaven" {
        (
            config.wallhaven_save_dir.clone(),
            config.wallhaven_thumb_dir(),
        )
    } else {
        (
            config.reddit_save_dir.clone(),
            config.reddit_thumb_dir(),
        )
    };

    // 删除文件
    let file_path = std::path::Path::new(&save_dir).join(&name);
    let existed = file_path.exists();
    if existed {
        std::fs::remove_file(&file_path).map_err(|e| {
            log::error!("[delete_orphan_file] 删除文件失败 {}: {}", file_path.display(), e);
            AppError::Io(e)
        })?;
    }

    // 删除缩略图（兼容新旧格式）
    let thumb_old = thumb_dir.join(&name);
    if thumb_old.exists() { std::fs::remove_file(&thumb_old).ok(); }
    for dpr in [1u32, 2, 3] {
        let tp = thumb_dir.join(thumbnail::thumb_filename_for_dpr(&name, dpr));
        if tp.exists() { std::fs::remove_file(&tp).ok(); }
    }

    Ok(existed)
}

#[tauri::command]
async fn add_orphan_entries(
    state: tauri::State<'_, AppState>,
    source: String,
    names: Vec<String>,
) -> Result<u64, AppError> {
    log::info!("[CMD] add_orphan_entries: source={}, count={}", source, names.len());
    let config = load_config(&state)?;
    let (save_dir, db_path) = if source == "wallhaven" {
        (config.wallhaven_save_dir.clone(), config.wallhaven_db_path.clone())
    } else {
        (config.reddit_save_dir.clone(), config.reddit_db_path.clone())
    };

    let mut wallhaven_batch: Vec<(String, String, String, String, String, String)> = Vec::new();
    let mut reddit_batch: Vec<(String, String, String, String, String)> = Vec::new();

    for name in &names {
        let file_path = std::path::Path::new(&save_dir).join(name);
        if !file_path.is_file() {
            log::warn!("[add_orphan_entries] file not found: {}", file_path.display());
            continue;
        }
        let bytes = std::fs::read(&file_path).map_err(AppError::Io)?;
        let hash = downloader::compute_md5(&bytes);

        if source == "wallhaven" {
            let wallhaven_id = name
                .strip_prefix("wallhaven_")
                .and_then(|s| s.split('.').next())
                .unwrap_or("");
            wallhaven_batch.push((
                wallhaven_id.to_string(),
                name.clone(),
                hash,
                String::new(),
                String::new(),
                "unknown".to_string(),
            ));
        } else {
            reddit_batch.push((name.clone(), hash, String::new(), String::new(), String::new()));
        }
    }

    let added = if source == "wallhaven" {
        db::insert_wallhaven_images_batch(&db_path, &wallhaven_batch)?.0
    } else {
        db::insert_reddit_images_batch(&db_path, &reddit_batch)?.0
    };

    log::info!("[add_orphan_entries] done: added={}/{}", added, names.len());
    Ok(added)
}

#[tauri::command]
async fn download_missing_images(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    source: String,
    images: Vec<db::ImageRecord>,
) -> Result<String, AppError> {
    log::info!("[CMD] download_missing_images: source={}, count={}", source, images.len());
    let config = load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let client = state.http_client.clone();

    let (save_dir, thumb_dir) = if source == "wallhaven" {
        (
            config.wallhaven_save_dir.clone(),
            config.wallhaven_thumb_dir(),
        )
    } else {
        (
            config.reddit_save_dir.clone(),
            config.reddit_thumb_dir(),
        )
    };

    let total_images = images.len();

    tokio::spawn(async move {

        let _ = tokio::fs::create_dir_all(&save_dir).await;

        let total = images.len() as u32;
        let mut success = 0u32;

        let urls: Vec<String> = images.iter().map(|img| img.url.clone()).collect();
        let download_results =
            downloader::download_urls_concurrent(&client, &urls, cancel.clone(), 3).await;

        for (i, img) in images.iter().enumerate() {
            let file_path = std::path::Path::new(&save_dir).join(&img.name);

            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    source: source.clone(),
                    done: i as u32,
                    total,
                    message: format!("正在下载 {} ({}/{})", img.name, i + 1, total),
                },
            );

            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                log::info!("[download_missing] cancelled (success={}/{})", success, total);
                let _ = app.emit(
                    "download-complete",
                    DownloadComplete {
                        source: source.clone(),
                        success,
                        total,
                        message: "下载已取消".to_string(),
                    },
                );
                return;
            }

            match &download_results[i] {
                Ok((bytes, _content_type)) => {
                    if tokio::fs::write(&file_path, bytes).await.is_ok() {
                        let thumb_dir = thumb_dir.clone();
                        let img_name = img.name.clone();
                        let bytes_clone = bytes.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            thumbnail::save_thumbnail_from_bytes(&thumb_dir, &img_name, &bytes_clone, 2)
                        }).await;
                        success += 1;
                    } else {
                        log::error!("[download_missing] write failed {}", file_path.display());
                    }
                }
                Err(e) => {
                    log::error!("[download_missing] download failed {}: {}", img.name, e);
                }
            }
        }

        log::info!("[download_missing] complete: success={}/{}", success, total);
        let _ = app.emit(
            "download-complete",
            DownloadComplete {
                source,
                success,
                total,
                message: format!("补下载完成: 成功 {success}/{total}"),
            },
        );
    });

    Ok(format!("补下载已启动，共 {} 张", total_images))
}

#[tauri::command]
async fn cancel_download(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    log::info!("[CMD] cancel_download called");
    if let Ok(guard) = state.cancel_flag.lock() {
        if let Some(ref flag) = *guard {
            flag.store(true, Ordering::Relaxed);
        }
    }
    Ok(())
}

#[tauri::command]
async fn get_images(
    state: tauri::State<'_, AppState>,
    source: String,
    limit: i64,
    offset: i64,
) -> Result<Vec<db::ImageRecord>, AppError> {
    log::info!("[CMD] get_images called: source={}, limit={}, offset={}", source, limit, offset);
    let config = load_config(&state)?;
    if source == "wallhaven" {
        Ok(db::get_wallhaven_images(
            &config.wallhaven_db_path,
            limit,
            offset,
        )?)
    } else {
        Ok(db::get_reddit_images(
            &config.reddit_db_path,
            limit,
            offset,
        )?)
    }
}

#[tauri::command]
async fn list_local_images(
    state: tauri::State<'_, AppState>,
    source: String,
    offset: usize,
    limit: usize,
) -> Result<serde_json::Value, AppError> {
    log::info!("[CMD] list_local_images called: source={}, offset={}, limit={}", source, offset, limit);
    let config = load_config(&state)?;
    let dir = if source == "wallhaven" {
        config.wallhaven_save_dir.clone()
    } else {
        config.reddit_save_dir.clone()
    };

    let path = PathBuf::from(&dir);
    if !path.is_dir() {
        return Ok(serde_json::json!({ "images": [], "total": 0 }));
    }

    {
        if let Ok(cache) = state.file_cache.lock() {
            if let Some(ref cached) = *cache {
                if cached.source == source && cached.dir_path == dir && cached.cached_at.elapsed().as_secs() < 30 {
                    let page_start = offset.min(cached.total);
                    let page_end = (page_start + limit).min(cached.total);
                    let page: Vec<serde_json::Value> = cached.items[page_start..page_end]
                        .iter()
                        .map(|e| serde_json::json!({
                            "name": e.name,
                            "path": e.path,
                            "thumb_path": null,
                            "size": e.size,
                            "is_orphan": e.is_orphan,
                        }))
                        .collect();
                    return Ok(serde_json::json!({ "images": page, "total": cached.total }));
                }
            }
        }
    }

    let db_names: std::collections::HashSet<String> = if source == "wallhaven" {
        db::get_all_filenames(&config.wallhaven_db_path).unwrap_or_default().into_iter().collect()
    } else if source == "reddit" {
        db::get_all_filenames(&config.reddit_db_path).unwrap_or_default().into_iter().collect()
    } else {
        let mut names: std::collections::HashSet<String> = db::get_all_filenames(&config.wallhaven_db_path).unwrap_or_default().into_iter().collect();
        names.extend(db::get_all_filenames(&config.reddit_db_path).unwrap_or_default());
        names
    };

    let mut entries: Vec<FileEntry> = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&path) {
        for entry in read_dir.flatten() {
            let file_path = entry.path();
            if file_path.is_file() && downloader::file_is_image(&file_path) {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_orphan = !db_names.contains(&name);
                entries.push(FileEntry {
                    name,
                    path: file_path.to_string_lossy().to_string(),
                    size: entry.metadata().map(|m| m.len()).unwrap_or(0),
                    is_orphan,
                });
            }
        }
    }

    entries.sort_by(|a, b| {
        a.is_orphan.cmp(&b.is_orphan).reverse().then(b.name.cmp(&a.name))
    });
    let total = entries.len();

    let page_start = offset.min(total);
    let page_end = (page_start + limit).min(total);
    let page: Vec<serde_json::Value> = entries[page_start..page_end]
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "path": e.path,
                "thumb_path": null,
                "size": e.size,
                "is_orphan": e.is_orphan,
            })
        })
        .collect();

    {
        if let Ok(mut cache) = state.file_cache.lock() {
            *cache = Some(FileListCache {
                total,
                source: source.clone(),
                dir_path: dir,
                items: entries.clone(),
                cached_at: Instant::now(),
            });
        }
    }

    Ok(serde_json::json!({ "images": page, "total": total }))
}

#[tauri::command]
async fn get_thumbnail_path(
    state: tauri::State<'_, AppState>,
    source: String,
    filename: String,
    dpr: Option<u32>,
) -> Result<String, AppError> {
    let dpr = dpr.unwrap_or(1).max(1);
    log::info!(
        "[CMD] get_thumbnail_path: source={}, file={}, dpr={}",
        source,
        filename,
        dpr
    );
    let config = load_config(&state)?;
    let dir = if source == "wallhaven" {
        &config.wallhaven_save_dir
    } else {
        &config.reddit_save_dir
    };
    let image_dir = PathBuf::from(dir);
    let thumb_dir = if source == "wallhaven" {
        config.wallhaven_thumb_dir()
    } else {
        config.reddit_thumb_dir()
    };

    // 确保缩略图存在（dpr 决定分辨率）
    let result = thumbnail::resolve_thumb_path(&thumb_dir, &image_dir, &filename, dpr);
    match result {
        Ok(thumb_path) => Ok(thumb_path.to_string_lossy().to_string()),
        Err(_) => {
            // 缩略图生成失败，回退到原图
            Ok(image_dir.join(&filename).to_string_lossy().to_string())
        }
    }
}

#[tauri::command]
async fn get_thumbnail_paths(
    state: tauri::State<'_, AppState>,
    source: String,
    filenames: Vec<String>,
    dpr: Option<u32>,
) -> Result<serde_json::Value, AppError> {
    let dpr = dpr.unwrap_or(1).max(1);
    log::info!(
        "[CMD] get_thumbnail_paths: source={}, count={}, dpr={}",
        source,
        filenames.len(),
        dpr
    );
    let config = load_config(&state)?;
    let dir = if source == "wallhaven" {
        &config.wallhaven_save_dir
    } else {
        &config.reddit_save_dir
    };
    let image_dir = PathBuf::from(dir);
    let thumb_dir = if source == "wallhaven" {
        config.wallhaven_thumb_dir()
    } else {
        config.reddit_thumb_dir()
    };

    // 并行生成缩略图（dpr 决定分辨率）
    let batch_result = thumbnail::ensure_batch_thumbnails(&thumb_dir, &image_dir, &filenames, dpr);
    let result: Vec<serde_json::Value> = batch_result
        .into_iter()
        .map(|(name, thumb_path)| {
            serde_json::json!({
                "name": name,
                "thumb_path": thumb_path.to_string_lossy().to_string(),
            })
        })
        .collect();

    Ok(serde_json::json!({ "items": result }))
}

/// Wallhaven 搜索预览（不下载）
#[tauri::command]
async fn search_wallhaven(
    state: tauri::State<'_, AppState>,
    page: Option<u32>,
) -> Result<serde_json::Value, AppError> {
    let page = page.unwrap_or(1);
    log::info!("[CMD] search_wallhaven called: page={}", page);
    let config = load_config(&state)?;
    let client = wallhaven::WallhavenClient::new(state.http_client.clone(), config.wallhaven_api_key.clone());

    let resp = client.search(
        page,
        &config.wallhaven_categories,
        &config.wallhaven_purity,
        &config.wallhaven_sorting,
        &config.wallhaven_order,
        &config.wallhaven_top_range,
        &config.wallhaven_atleast,
        &config.wallhaven_ratios,
        &config.wallhaven_q,
    )
    .await
    .map_err(AppError::Config)?;

    let meta = resp.meta.as_ref();
    let images: Vec<serde_json::Value> = resp.data.iter().map(|img| {
        let prefix = if img.id.len() >= 2 { &img.id[..2] } else { &img.id[..1] };
        let thumbnail_url = format!("https://th.wallhaven.cc/small/{prefix}/{}.jpg", img.id);
        serde_json::json!({
            "id": img.id,
            "thumbnail_url": thumbnail_url,
            "path": img.path,
            "resolution": img.resolution,
            "short_url": img.short_url,
            "file_size": img.file_size,
            "file_type": img.file_type,
        })
    }).collect();

    Ok(serde_json::json!({
        "images": images,
        "page": meta.map(|m| m.current_page).unwrap_or(1),
        "total_pages": meta.and_then(|m| m.last_page).unwrap_or(1),
        "total": meta.and_then(|m| m.total).unwrap_or(0),
    }))
}

/// 下载选中的 Wallhaven 图片
#[derive(serde::Deserialize)]
struct WallhavenSelected {
    id: String,
    path: String,
    resolution: String,
    short_url: String,
}

#[tauri::command]
async fn download_wallhaven_selected(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    images: Vec<WallhavenSelected>,
) -> Result<String, AppError> {
    log::info!("[CMD] download_wallhaven_selected: count={}", images.len());
    let config = load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let app_clone = app.clone();
    let client = state.http_client.clone();
    let total = images.len() as u32;
    let count = total;

    tokio::spawn(async move {
        let _ = tokio::fs::create_dir_all(&config.wallhaven_save_dir).await;

        let existing_ids = match db::get_existing_wallhaven_ids(&config.wallhaven_db_path) {
            Ok(ids) => ids,
            Err(e) => {
                log::error!("[wallhaven] 获取已有ID失败: {e}");
                return;
            }
        };
        let existing_set: std::collections::HashSet<String> = existing_ids.into_iter().collect();
        let mut success = 0u32;

        let urls: Vec<String> = images.iter().map(|img| img.path.clone()).collect();
        let download_results = downloader::download_urls_concurrent(&client, &urls, cancel.clone(), 3).await;

        for (i, img) in images.iter().enumerate() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                log::info!("[wallhaven] download cancelled (success={}/{})", success, total);
                let _ = app_clone.emit("download-complete", DownloadComplete {
                    source: "wallhaven".into(), success, total,
                    message: "\u{4e0b}\u{8f7d}\u{5df2}\u{53d6}\u{6d88}".to_string(),
                });
                return;
            }

            if existing_set.contains(&img.id) {
                continue;
            }

            let _ = app_clone.emit("download-progress", DownloadProgress {
                source: "wallhaven".into(), done: i as u32, total,
                message: format!("\u{6b63}\u{5728}\u{4e0b}\u{8f7d} {} ({}/{})", img.id, i + 1, total),
            });

            if let Ok((bytes, content_type)) = &download_results[i] {
                let ext = downloader::get_file_extension(content_type, &img.path);
                let safe_id: String = img.id.chars().filter(|c| c.is_alphanumeric()).collect();
                let filename = format!("wallhaven_{safe_id}.{ext}");
                let save_path = std::path::Path::new(&config.wallhaven_save_dir).join(&filename);
                let hash = downloader::compute_md5(bytes);

                if tokio::fs::write(&save_path, bytes).await.is_ok() {
                    let thumb_dir = config.wallhaven_thumb_dir();
                    let filename_clone = filename.clone();
                    let bytes_clone = bytes.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        thumbnail::save_thumbnail_from_bytes(&thumb_dir, &filename_clone, &bytes_clone, 2)
                    }).await;

                    let db_path = config.wallhaven_db_path.clone();
                    let img_id = img.id.clone();
                    let filename_clone = filename.clone();
                    let hash_clone = hash.clone();
                    let img_path = img.path.clone();
                    let img_short_url = img.short_url.clone();
                    let img_resolution = img.resolution.clone();
                    let inserted = tokio::task::spawn_blocking(move || {
                        db::insert_wallhaven_image(
                            &db_path, &img_id, &filename_clone, &hash_clone,
                            &img_path, &img_short_url, &img_resolution,
                        ).unwrap_or(false)
                    }).await.unwrap_or(false);

                    if inserted {
                        success += 1;
                        let _ = app_clone.emit("image-downloaded", ImageDownloaded {
                            source: "wallhaven".into(),
                            name: filename,
                            path: save_path.to_string_lossy().to_string(),
                        });
                    }
                }
            }
        }

        log::info!("[wallhaven] download complete (success={}/{})", success, total);
        let _ = app_clone.emit("download-complete", DownloadComplete {
            source: "wallhaven".into(), success, total,
            message: format!("Wallhaven \u{4e0b}\u{8f7d}\u{5b8c}\u{6210}: \u{6210}\u{529f} {success}/{total}"),
        });
    });

    Ok(format!("\u{5373}\u{5c06}\u{4e0b}\u{8f7d} {count} \u{5f20}\u{58c1}\u{7eb8}"))
}

// ---------------------------------------------------------------------------
// \u{58c1}\u{7eb8}\u{8bbe}\u{7f6e} — \u{6bcf}\u{4e2a}\u{684c}\u{9762}\u{73af}\u{5883}\u{4e00}\u{4e2a}\u{72ec}\u{7acb}\u{7684}\u{68c0}\u{6d4b}\u{51fd}\u{6570}
// ---------------------------------------------------------------------------

/// GNOME (gsettings)
fn set_gnome_wallpaper(path_str: &str) -> Option<String> {
    if !Command::new("gsettings")
        .args(["get", "org.gnome.desktop.background", "picture-uri"])
        .output()
        .is_ok()
    {
        return None;
    }
    let uri = format!("file://{path_str}");
    if let Ok(output) = Command::new("gsettings")
        .args(["set", "org.gnome.desktop.background", "picture-uri", &uri])
        .output()
    {
        if output.status.success() {
            Command::new("gsettings")
                .args(["set", "org.gnome.desktop.background", "picture-uri-dark", &uri])
                .output()
                .ok();
            return Some("\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (GNOME)".to_string());
        }
    }
    None
}

/// XFCE (xfconf-query)
fn set_xfce_wallpaper(path_str: &str) -> Option<String> {
    let output = Command::new("xfconf-query")
        .args(["-c", "xfce4-desktop", "-lv"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() == 2 && parts[0].contains("last-image") {
            if let Ok(output) = Command::new("xfconf-query")
                .args(["-c", "xfce4-desktop", "-p", parts[0].trim(), "-s", path_str])
                .output()
            {
                if output.status.success() {
                    return Some("\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (XFCE)".to_string());
                }
            }
        }
    }
    None
}

/// KDE Plasma (qdbus)
fn set_kde_wallpaper(path_str: &str) -> Option<String> {
    let has_kde = Command::new("kwriteconfig5").args(["--help"]).output().is_ok()
        || Command::new("kwriteconfig6").args(["--help"]).output().is_ok();
    if !has_kde {
        return None;
    }
    log::info!("[set_wallpaper] detected KDE Plasma");
    let script = format!(
        "var allDesktops = desktops();
for (var i = 0; i < allDesktops.length; i++) {{
    var d = allDesktops[i];
    d.wallpaperPlugin = 'org.kde.image';
    d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General'];
    d.writeConfig('Image', 'file://{path_str}');
}}"
    );
    let output = Command::new("qdbus")
        .args(["org.kde.plasmashell", "/PlasmaShell", "org.kde.PlasmaShell.evaluateScript", &script])
        .output()
        .ok()?;
    if output.status.success() {
        return Some("\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (KDE)".to_string());
    }
    None
}

/// sway (swaymsg)
fn set_sway_wallpaper(path_str: &str) -> Option<String> {
    let output = Command::new("swaymsg").args(["-t", "get_outputs"]).output().ok()?;
    if output.status.success() {
        Command::new("swaymsg")
            .args(["output", "*", "bg", path_str, "fill"])
            .output()
            .ok()?;
        return Some("\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (sway)".to_string());
    }
    None
}

/// Hyprland (hyprpaper)
fn set_hyprland_wallpaper(path_str: &str) -> Option<String> {
    if !Command::new("hyprctl").arg("--version").output().is_ok() {
        return None;
    }
    let monitors = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok()
        .map(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.lines()
                .filter(|l| l.contains("\"name\":"))
                .filter_map(|l| {
                    let parts: Vec<&str> = l.splitn(2, ':').collect();
                    (parts.len() == 2).then(|| parts[1].trim().trim_matches('"').trim_matches(',').to_string())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Command::new("hyprctl")
        .args(["hyprpaper", "preload", path_str])
        .output()
        .ok();

    let ok = if monitors.is_empty() {
        Command::new("hyprctl")
            .args(["hyprpaper", "wallpaper", &format!(",{path_str}")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        monitors.iter().all(|monitor| {
            Command::new("hyprctl")
                .args(["hyprpaper", "wallpaper", &format!("{monitor},{path_str}")])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
    };

    ok.then(|| "\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (Hyprland)".to_string())
}

/// swww
fn set_swww_wallpaper(path_str: &str) -> Option<String> {
    if !Command::new("swww").arg("--version").output().is_ok() {
        return None;
    }
    let output = Command::new("swww")
        .args(["img", "--transition-type", "fade", "--transition-step", "60", path_str])
        .output()
        .ok()?;
    output.status.success().then(|| "\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (swww)".to_string())
}


/// feh（最后回退）
fn set_feh_wallpaper(path_str: &str) -> Option<String> {
    let output = Command::new("feh").args(["--bg-fill", path_str]).output().ok()?;
    output.status.success().then(|| "\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (feh)".to_string())
}


#[tauri::command]
async fn set_wallpaper(file_path: String) -> Result<String, AppError> {
    log::info!("[CMD] set_wallpaper: file={}", file_path);
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err(AppError::Other(format!("文件不存在: {}", file_path)));
    }
    let absolute_path = path
        .canonicalize()
        .map_err(|e| AppError::Other(format!("获取绝对路径失败: {e}")))?;
    let path_str = absolute_path.to_string_lossy().to_string();
    set_gnome_wallpaper(&path_str)
        .or_else(|| set_xfce_wallpaper(&path_str))
        .or_else(|| set_kde_wallpaper(&path_str))
        .or_else(|| set_sway_wallpaper(&path_str))
        .or_else(|| set_hyprland_wallpaper(&path_str))
        .or_else(|| set_swww_wallpaper(&path_str))
        .or_else(|| set_feh_wallpaper(&path_str))
        .ok_or_else(|| AppError::Other(
            "未检测到支持的桌面环境。支持: GNOME, KDE, XFCE, sway, Hyprland, niri(swww), swww, feh".to_string(),
        ))
}


#[tauri::command]
async fn delete_image(
    state: tauri::State<'_, AppState>,
    source: String,
    name: String,
) -> Result<bool, AppError> {
    log::info!("[CMD] delete_image: source={}, name={}", source, name);
    let config = load_config(&state)?;
    let (save_dir, db_path, thumb_dir) = if source == "wallhaven" {
        (
            config.wallhaven_save_dir.clone(),
            config.wallhaven_db_path.clone(),
            config.wallhaven_thumb_dir(),
        )
    } else {
        (
            config.reddit_save_dir.clone(),
            config.reddit_db_path.clone(),
            config.reddit_thumb_dir(),
        )
    };

    // 先删数据库记录，再删文件（防止 DB 记录成孤儿）
    let db_ok = if source == "wallhaven" {
        db::delete_wallhaven_image(&db_path, &name).map_err(AppError::Db)?;
        true
    } else {
        db::delete_reddit_image(&db_path, &name).map_err(AppError::Db)?;
        true
    };

    // 删除文件（文件删除失败不阻止流程，只记日志）
    let file_path = std::path::Path::new(&save_dir).join(&name);
    if file_path.exists() {
        if let Err(e) = std::fs::remove_file(&file_path) {
            log::warn!("[delete_image] 删除文件失败 {}: {e}", file_path.display());
        }
    }

    // 删除缩略图（兼容新旧格式）
    let thumb_old = thumb_dir.join(&name);
    if thumb_old.exists() { std::fs::remove_file(&thumb_old).ok(); }
    for dpr in [1u32, 2, 3] {
        let tp = thumb_dir.join(thumbnail::thumb_filename_for_dpr(&name, dpr));
        if tp.exists() { std::fs::remove_file(&tp).ok(); }
    }

    Ok(db_ok)
}

#[tauri::command]
async fn clean_thumbnails(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, AppError> {
    log::info!("[CMD] clean_thumbnails called");
    let config = load_config(&state)?;
    let wh_thumb_dir = config.wallhaven_thumb_dir();
    let wh_cleaned = db::clean_stale_thumbnails(
        &wh_thumb_dir.to_string_lossy(),
        &config.wallhaven_save_dir,
    )
    .unwrap_or(0);
    let rd_thumb_dir = config.reddit_thumb_dir();
    let rd_cleaned = db::clean_stale_thumbnails(
        &rd_thumb_dir.to_string_lossy(),
        &config.reddit_save_dir,
    )
    .unwrap_or(0);
    Ok(serde_json::json!({
        "wallhaven": wh_cleaned,
        "reddit": rd_cleaned,
    }))
}
/// 获取当前桌面壁纸路径（从 noctalia 缓存）
#[tauri::command]
async fn get_current_wallpaper() -> Result<serde_json::Value, AppError> {
    let noc_path = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("noctalia")
        .join("wallpapers.json");

    if !noc_path.exists() {
        return Ok(serde_json::json!({ "path": null }));
    }

    let content = std::fs::read_to_string(&noc_path)
        .map_err(|e| AppError::Other(format!("读取 noctalia 配置失败: {e}")))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Other(format!("解析 noctalia 配置失败: {e}")))?;

    let is_dark = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("dark"))
        .unwrap_or(true);

    let key = if is_dark { "dark" } else { "light" };
    let path = json
        .pointer(&format!("/wallpapers/eDP-1/{key}"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(serde_json::json!({ "path": path }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    log::info!("RustWallhub 启动");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let config_dir = app
                .path()
                .config_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let config_path = config_dir.join("rustwallhub").join("config.json");

            let mut config = AppConfig::load(&config_path).unwrap_or_default();
            // 统一归一化路径（与 load_config 一致）
            if let Some(base_dir) = config_path.parent() {
                config.wallhaven_db_path = normalize_config_path(base_dir, config.wallhaven_db_path);
                config.reddit_db_path = normalize_config_path(base_dir, config.reddit_db_path);
                config.wallhaven_save_dir = normalize_config_path(base_dir, config.wallhaven_save_dir);
                config.reddit_save_dir = normalize_config_path(base_dir, config.reddit_save_dir);
            }

            let wh_db = config.wallhaven_db_path.clone();
            let rd_db = config.reddit_db_path.clone();
            std::fs::create_dir_all(&config.wallhaven_save_dir).ok();
            std::fs::create_dir_all(&config.reddit_save_dir).ok();
            // 确保数据库目录存在
            if let Some(wh_parent) = std::path::Path::new(&wh_db).parent() {
                if !wh_parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(wh_parent).ok();
                }
            }
            if let Some(rd_parent) = std::path::Path::new(&rd_db).parent() {
                if !rd_parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(rd_parent).ok();
                }
            }

            db::init_wallhaven_db(&wh_db).ok();
            db::init_reddit_db(&rd_db).ok();

            let client = reqwest::Client::builder()
                .user_agent("RustWallhub/1.0")
                .timeout(Duration::from_secs(30))
                .build()
                .expect("创建 HTTP client 失败");

            app.manage(AppState {
                config_path: Mutex::new(config_path),
                file_cache: Mutex::new(None),
                cancel_flag: Mutex::new(None),
                http_client: client,
                config_cache: Mutex::new(Some(config)),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_settings,
            get_stats,
            start_wallhaven_download,
            start_reddit_download,
            start_db_download,
            mark_dislike,
            cancel_download,
            count_missing_images,
            restore_love,
            list_missing_images,
            download_missing_images,
            delete_orphan_file,
            list_orphan_files,
            mark_dislike_image,
            add_orphan_entries,
            get_images,
            list_local_images,
            get_thumbnail_path,
            get_thumbnail_paths,
            search_wallhaven,
            download_wallhaven_selected,
            set_wallpaper,
            delete_image,
            clean_thumbnails,
            get_current_wallpaper,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时出错");
}
