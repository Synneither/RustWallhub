//! Download orchestration commands: recover_database_files, download_missing_images, cancel_downloads.

use crate::config::Source;
use crate::db;
use crate::downloader;
use crate::state::{
    save_image, setup_cancel_flag, AppError, AppState, DownloadComplete, DownloadProgress,
    ProgressThrottle,
};
use std::path::Path;
use std::sync::atomic::Ordering;
use tauri::Emitter;

#[tauri::command]
pub async fn recover_database_files(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    source: Source,
) -> Result<String, AppError> {
    log::info!("[CMD] recover_database_files: source={:?}", source);
    if matches!(source, Source::All) {
        return Err(AppError::Other(
            "全量恢复请分别调用 wallhaven / reddit".into(),
        ));
    }
    let config = crate::state::load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁获取失败: {e}")))?
        .clone();

    let save_dir = config.save_dir_for(source).to_string();
    let db_path = config.db_path_for(source).to_string();
    let is_wallhaven = Source::is_wallhaven(source);
    let source_str = source.to_string();
    let src_inner = source_str.clone();

    tokio::spawn(async move {
        let _ = tokio::fs::create_dir_all(&save_dir).await;

        // 查库 + 文件存在性过滤都是阻塞操作，放入 spawn_blocking。
        let filter_save_dir = save_dir.clone();
        let filter_db_path = db_path.clone();
        let images = match tokio::task::spawn_blocking(move || {
            let images = if is_wallhaven {
                db::get_wallhaven_missing_love(&filter_db_path)?
            } else {
                db::get_reddit_missing_love(&filter_db_path)?
            };
            Ok::<Vec<db::ImageRecord>, rusqlite::Error>(
                images
                    .into_iter()
                    .filter(|img| !Path::new(&filter_save_dir).join(&img.name).exists())
                    .collect(),
            )
        })
        .await
        {
            Ok(Ok(images)) => images,
            Ok(Err(e)) => {
                log::error!("[recover] 获取缺失图片失败: {e}");
                return;
            }
            Err(e) => {
                log::error!("[recover] 获取缺失图片任务异常: {e}");
                return;
            }
        };

        let to_download: Vec<&db::ImageRecord> = images.iter().collect();
        let total = to_download.len() as u32;
        let mut success = 0u32;

        // 分批下载 + 分批落盘：避免把所有原图 bytes 同时囤在内存里。
        let chunk_size = (config.download_concurrency.max(1) as usize)
            .saturating_mul(2)
            .max(1);
        let mut progress_throttle = ProgressThrottle::new();
        let mut processed = 0usize;
        for chunk in to_download.chunks(chunk_size) {
            let urls: Vec<String> = chunk.iter().map(|img| img.url.clone()).collect();
            let download_results = downloader::download_urls_concurrent(
                &client,
                &urls,
                cancel.clone(),
                config.download_concurrency,
                3,
            )
            .await;

            for (local_i, img) in chunk.iter().enumerate() {
                let i = processed + local_i;
                let file_path = Path::new(&save_dir).join(&img.name);

                if progress_throttle.should_emit(i + 1 == total as usize) {
                    let _ = app.emit(
                        "download-progress",
                        DownloadProgress {
                            source: src_inner.clone(),
                            done: i as u32,
                            total,
                            message: format!("正在下载 {} ({}/{})", img.name, i + 1, total),
                        },
                    );
                }

                if cancel.load(Ordering::Relaxed) {
                    log::info!(
                        "[recover] cancelled: source={} (success={}/{})",
                        src_inner,
                        success,
                        total
                    );
                    let _ = app.emit(
                        "download-complete",
                        DownloadComplete {
                            source: src_inner.clone(),
                            success,
                            total,
                            message: "下载已取消".to_string(),
                        },
                    );
                    return;
                }

                match &download_results[local_i] {
                    Ok((bytes, _content_type)) => match save_image(&file_path, bytes).await {
                        Ok(()) => {
                            success += 1;
                            db::invalidate_stats(&db_path);
                        }
                        Err(e) => log::error!("[recover] {}", e),
                    },
                    Err(e) => {
                        log::error!("[recover] download failed {}: {}", img.name, e);
                    }
                }
            }
            processed += chunk.len();
        }

        log::info!("[recover] complete: success={}/{}", success, total);
        let _ = app.emit(
            "download-complete",
            DownloadComplete {
                source: src_inner,
                success,
                total,
                message: format!("数据库下载完成: 成功 {success}/{total}"),
            },
        );
    });

    Ok(format!("{source_str} 数据库下载已启动"))
}

#[tauri::command]
pub async fn download_missing_images(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    source: Source,
    images: Vec<db::ImageRecord>,
) -> Result<String, AppError> {
    log::info!(
        "[CMD] download_missing_images: source={:?}, count={}",
        source,
        images.len()
    );
    if matches!(source, Source::All) {
        return Err(AppError::Other(
            "补下载请分别按 wallhaven / reddit 调用".into(),
        ));
    }
    for img in &images {
        crate::state::ensure_plain_filename(&img.name)?;
    }
    let config = crate::state::load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁获取失败: {e}")))?
        .clone();

    let save_dir = config.save_dir_for(source).to_string();
    let db_path = config.db_path_for(source).to_string();
    let download_concurrency = config.download_concurrency;
    let source_str = source.to_string();
    let total_images = images.len();

    tokio::spawn(async move {
        let _ = tokio::fs::create_dir_all(&save_dir).await;

        let total = images.len() as u32;
        let mut success = 0u32;

        // 分批下载，防止大任务把所有图片同时放在内存里。
        let chunk_size = (download_concurrency.max(1) as usize)
            .saturating_mul(2)
            .max(1);
        let mut progress_throttle = ProgressThrottle::new();
        let mut processed = 0usize;
        for chunk in images.chunks(chunk_size) {
            let urls: Vec<String> = chunk.iter().map(|img| img.url.clone()).collect();
            let download_results = downloader::download_urls_concurrent(
                &client,
                &urls,
                cancel.clone(),
                download_concurrency,
                3,
            )
            .await;

            for (local_i, img) in chunk.iter().enumerate() {
                let i = processed + local_i;
                let file_path = Path::new(&save_dir).join(&img.name);

                if progress_throttle.should_emit(i + 1 == total as usize) {
                    let _ = app.emit(
                        "download-progress",
                        DownloadProgress {
                            source: source_str.clone(),
                            done: i as u32,
                            total,
                            message: format!("正在下载 {} ({}/{})", img.name, i + 1, total),
                        },
                    );
                }

                if cancel.load(Ordering::Relaxed) {
                    log::info!(
                        "[download_missing] cancelled (success={}/{})",
                        success,
                        total
                    );
                    let _ = app.emit(
                        "download-complete",
                        DownloadComplete {
                            source: source_str.clone(),
                            success,
                            total,
                            message: "下载已取消".to_string(),
                        },
                    );
                    return;
                }

                match &download_results[local_i] {
                    Ok((bytes, _content_type)) => match save_image(&file_path, bytes).await {
                        Ok(()) => {
                            success += 1;
                            db::invalidate_stats(&db_path);
                        }
                        Err(e) => log::error!("[download_missing] {}", e),
                    },
                    Err(e) => {
                        log::error!("[download_missing] download failed {}: {}", img.name, e);
                    }
                }
            }
            processed += chunk.len();
        }

        log::info!("[download_missing] complete: success={}/{}", success, total);
        let _ = app.emit(
            "download-complete",
            DownloadComplete {
                source: source_str.clone(),
                success,
                total,
                message: format!("补下载完成: 成功 {success}/{total}"),
            },
        );
    });

    Ok(format!("补下载已启动，共 {} 张", total_images))
}

#[tauri::command]
pub async fn cancel_downloads(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    log::info!("[CMD] cancel_downloads called");
    if let Ok(guard) = state.cancel_flag.lock() {
        if let Some(ref flag) = *guard {
            flag.store(true, Ordering::Relaxed);
        }
    }
    Ok(())
}
