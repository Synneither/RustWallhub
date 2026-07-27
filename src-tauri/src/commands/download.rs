//! Download orchestration commands: recover_database_files, download_missing_images, cancel_downloads.

use crate::config::Source;
use crate::db;
use crate::downloader;
use crate::state::{
    save_image, setup_cancel_flag, AppError, AppState, DownloadComplete, DownloadProgress,
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
    let config = crate::state::load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁获取失败: {e}")))?
        .clone();

    let save_dir = config.save_dir_for(source).to_string();
    let db_path = config.db_path_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);
    let is_wallhaven = Source::is_wallhaven(source);
    let source_str = source.to_string();
    let src_inner = source_str.clone();

    tokio::spawn(async move {
        let _ = tokio::fs::create_dir_all(&save_dir).await;

        let images = if is_wallhaven {
            match db::get_wallhaven_missing_love(&db_path) {
                Ok(imgs) => imgs,
                Err(e) => {
                    log::error!("[recover] 获取wallhaven缺失图片失败: {e}");
                    return;
                }
            }
        } else {
            match db::get_reddit_missing_love(&db_path) {
                Ok(imgs) => imgs,
                Err(e) => {
                    log::error!("[recover] 获取reddit缺失图片失败: {e}");
                    return;
                }
            }
        };

        let total = images.len() as u32;
        let mut success = 0u32;

        let to_download: Vec<&db::ImageRecord> = images
            .iter()
            .filter(|img| !Path::new(&save_dir).join(&img.name).exists())
            .collect();
        let total_pending = to_download.len() as u32;

        let urls: Vec<String> = to_download.iter().map(|img| img.url.clone()).collect();
        let download_results = downloader::download_urls_concurrent(
            &client,
            &urls,
            cancel.clone(),
            config.download_concurrency,
            3,
        )
        .await;

        for (i, img) in to_download.iter().enumerate() {
            let file_path = Path::new(&save_dir).join(&img.name);

            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    source: src_inner.clone(),
                    done: i as u32,
                    total: total_pending,
                    message: format!("正在下载 {} ({}/{})", img.name, i + 1, total_pending),
                },
            );

            if cancel.load(Ordering::Relaxed) {
                log::info!(
                    "[recover] cancelled: source={} (success={}/{})",
                    src_inner,
                    success,
                    total_pending
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

            match &download_results[i] {
                Ok((bytes, _content_type)) => {
                    if let Some(thumb_handle) = save_image(
                        &file_path,
                        bytes,
                        &thumb_dir,
                        &img.name,
                        config.thumbnail_dpr,
                    )
                    .await
                    {
                        let _ = thumb_handle.await;
                        success += 1;
                    } else {
                        log::error!("[recover] write failed {}", file_path.display());
                    }
                }
                Err(e) => {
                    log::error!("[recover] download failed {}: {}", img.name, e);
                }
            }
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
    let config = crate::state::load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁获取失败: {e}")))?
        .clone();

    let save_dir = config.save_dir_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);
    let download_concurrency = config.download_concurrency;
    let thumbnail_dpr = config.thumbnail_dpr;
    let source_str = source.to_string();
    let total_images = images.len();

    tokio::spawn(async move {
        let _ = tokio::fs::create_dir_all(&save_dir).await;

        let total = images.len() as u32;
        let mut success = 0u32;

        let urls: Vec<String> = images.iter().map(|img| img.url.clone()).collect();
        let download_results = downloader::download_urls_concurrent(
            &client,
            &urls,
            cancel.clone(),
            download_concurrency,
            3,
        )
        .await;

        for (i, img) in images.iter().enumerate() {
            let file_path = Path::new(&save_dir).join(&img.name);

            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    source: source_str.clone(),
                    done: i as u32,
                    total,
                    message: format!("正在下载 {} ({}/{})", img.name, i + 1, total),
                },
            );

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

            match &download_results[i] {
                Ok((bytes, _content_type)) => {
                    if let Some(thumb_handle) =
                        save_image(&file_path, bytes, &thumb_dir, &img.name, thumbnail_dpr).await
                    {
                        let _ = thumb_handle.await;
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
