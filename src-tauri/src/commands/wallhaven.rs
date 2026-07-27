//! Wallhaven commands: search_wallhaven, start_wallhaven_download, download_wallhaven_selected.

use crate::db;
use crate::downloader;
use crate::state::{
    save_image, setup_cancel_flag, AppError, AppState, DownloadComplete, DownloadProgress,
    ImageDownloaded,
};
use crate::wallhaven;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering;
use tauri::Emitter;

#[derive(Serialize)]
pub struct WallhavenSearchResult {
    pub images: Vec<WallhavenImageEntry>,
    pub page: u32,
    pub total_pages: u32,
    pub total: u32,
}

#[derive(Serialize)]
pub struct WallhavenImageEntry {
    pub id: String,
    pub thumbnail_url: String,
    pub path: String,
    pub resolution: String,
    pub short_url: String,
    pub file_size: u64,
    pub file_type: String,
}

#[derive(Deserialize)]
pub struct WallhavenSelected {
    id: String,
    path: String,
    resolution: String,
    short_url: String,
}

#[tauri::command]
pub async fn search_wallhaven(
    state: tauri::State<'_, AppState>,
    page: Option<u32>,
) -> Result<WallhavenSearchResult, AppError> {
    let page = page.unwrap_or(1);
    log::info!("[CMD] search_wallhaven: page={}", page);
    let config = crate::state::load_config(&state)?;
    let client = wallhaven::WallhavenClient::new(
        state
            .http_client
            .lock()
            .map_err(|e| AppError::Other(format!("锁获取失败: {e}")))?
            .clone(),
        config.wallhaven_api_key.clone(),
    );

    let resp = client
        .search(&wallhaven::WallhavenSearchParams {
            page,
            categories: config.wallhaven_categories.clone(),
            purity: config.wallhaven_purity.clone(),
            sorting: config.wallhaven_sorting.clone(),
            order: config.wallhaven_order.clone(),
            top_range: config.wallhaven_top_range.clone(),
            atleast: config.wallhaven_atleast.clone(),
            ratios: config.wallhaven_ratios.clone(),
            q: config.wallhaven_q.clone(),
        })
        .await
        .map_err(AppError::Config)?;

    let meta = resp.meta.as_ref();
    let images = resp
        .data
        .iter()
        .map(|img| {
            let prefix = if img.id.len() >= 2 {
                &img.id[..2]
            } else {
                &img.id[..1]
            };
            let thumbnail_url = format!("https://th.wallhaven.cc/small/{prefix}/{}.jpg", img.id);
            WallhavenImageEntry {
                id: img.id.clone(),
                thumbnail_url,
                path: img.path.clone(),
                resolution: img.resolution.clone(),
                short_url: img.short_url.clone(),
                file_size: img.file_size,
                file_type: img.file_type.clone(),
            }
        })
        .collect();

    Ok(WallhavenSearchResult {
        images,
        page: meta.map_or(1, |m| m.current_page),
        total_pages: meta.and_then(|m| m.last_page).unwrap_or(1),
        total: meta.and_then(|m| m.total).unwrap_or(0),
    })
}

#[tauri::command]
pub async fn start_wallhaven_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    log::info!("[CMD] start_wallhaven_download called");
    let config = crate::state::load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let app_clone = app.clone();
    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁获取失败: {e}")))?
        .clone();

    tokio::spawn(async move {
        let wh_client =
            wallhaven::WallhavenClient::new(client.clone(), config.wallhaven_api_key.clone());

        let _ = tokio::fs::create_dir_all(&config.wallhaven_save_dir).await;

        let existing_ids = match db::get_existing_wallhaven_ids(&config.wallhaven_db_path) {
            Ok(ids) => ids,
            Err(e) => {
                log::error!("[wallhaven] 获取已有ID失败: {e}");
                return;
            }
        };
        let existing_set: HashSet<String> = existing_ids.into_iter().collect();

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

            let resp = wh_client
                .search(&wallhaven::WallhavenSearchParams {
                    page,
                    categories: config.wallhaven_categories.clone(),
                    purity: config.wallhaven_purity.clone(),
                    sorting: config.wallhaven_sorting.clone(),
                    order: config.wallhaven_order.clone(),
                    top_range: config.wallhaven_top_range.clone(),
                    atleast: config.wallhaven_atleast.clone(),
                    ratios: config.wallhaven_ratios.clone(),
                    q: config.wallhaven_q.clone(),
                })
                .await;

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
        let download_results = downloader::download_urls_concurrent(
            &client,
            &urls,
            cancel.clone(),
            config.download_concurrency,
            3,
        )
        .await;

        for (i, img) in collected.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                log::info!(
                    "[wallhaven] download cancelled (success={}/{})",
                    success,
                    total
                );
                let _ = app_clone.emit(
                    "download-complete",
                    DownloadComplete {
                        source: "wallhaven".into(),
                        success,
                        total,
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
                    let save_path = Path::new(&config.wallhaven_save_dir).join(&filename);
                    let hash = downloader::compute_md5(bytes);

                    let thumb_dir = config.wallhaven_thumb_dir();
                    if let Some(thumb_handle) = save_image(
                        &save_path,
                        bytes,
                        &thumb_dir,
                        &filename,
                        config.thumbnail_dpr,
                    )
                    .await
                    {
                        let db_path = config.wallhaven_db_path.clone();
                        let img_id = img.id.clone();
                        let filename_for_db = filename.clone();
                        let hash_for_db = hash.clone();
                        let img_path = img.path.clone();
                        let img_url = img.short_url.clone();
                        let img_res = img.resolution.clone();

                        let db_handle = tokio::task::spawn_blocking(move || {
                            db::insert_wallhaven_image(
                                &db_path,
                                &img_id,
                                &filename_for_db,
                                &hash_for_db,
                                &img_path,
                                &img_url,
                                &img_res,
                            )
                            .unwrap_or(false)
                        });

                        let (_, inserted) = tokio::join!(thumb_handle, db_handle);
                        let inserted = inserted.unwrap_or(false);

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

        log::info!(
            "[wallhaven] download complete (success={}/{})",
            success,
            total
        );
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
pub async fn download_wallhaven_selected(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    images: Vec<WallhavenSelected>,
) -> Result<String, AppError> {
    log::info!("[CMD] download_wallhaven_selected: count={}", images.len());
    let config = crate::state::load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let app_clone = app.clone();
    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁获取失败: {e}")))?
        .clone();
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
        let existing_set: HashSet<String> = existing_ids.into_iter().collect();
        let mut success = 0u32;

        let urls: Vec<String> = images.iter().map(|img| img.path.clone()).collect();
        let download_results = downloader::download_urls_concurrent(
            &client,
            &urls,
            cancel.clone(),
            config.download_concurrency,
            3,
        )
        .await;

        for (i, img) in images.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                log::info!(
                    "[wallhaven] download cancelled (success={}/{})",
                    success,
                    total
                );
                let _ = app_clone.emit(
                    "download-complete",
                    DownloadComplete {
                        source: "wallhaven".into(),
                        success,
                        total,
                        message: "下载已取消".to_string(),
                    },
                );
                return;
            }

            if existing_set.contains(&img.id) {
                continue;
            }

            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: "wallhaven".into(),
                    done: i as u32,
                    total,
                    message: format!("正在下载 {} ({}/{})", img.id, i + 1, total),
                },
            );

            if let Ok((bytes, content_type)) = &download_results[i] {
                let ext = downloader::get_file_extension(content_type, &img.path);
                let safe_id: String = img.id.chars().filter(|c| c.is_alphanumeric()).collect();
                let filename = format!("wallhaven_{safe_id}.{ext}");
                let save_path = Path::new(&config.wallhaven_save_dir).join(&filename);
                let hash = downloader::compute_md5(bytes);

                let thumb_dir = config.wallhaven_thumb_dir();
                if let Some(thumb_handle) = save_image(
                    &save_path,
                    bytes,
                    &thumb_dir,
                    &filename,
                    config.thumbnail_dpr,
                )
                .await
                {
                    let db_path = config.wallhaven_db_path.clone();
                    let img_id = img.id.clone();
                    let filename_for_db = filename.clone();
                    let hash_for_db = hash.clone();
                    let img_path = img.path.clone();
                    let img_short_url = img.short_url.clone();
                    let img_resolution = img.resolution.clone();

                    let db_handle = tokio::task::spawn_blocking(move || {
                        db::insert_wallhaven_image(
                            &db_path,
                            &img_id,
                            &filename_for_db,
                            &hash_for_db,
                            &img_path,
                            &img_short_url,
                            &img_resolution,
                        )
                        .unwrap_or(false)
                    });

                    let (_, inserted) = tokio::join!(thumb_handle, db_handle);
                    let inserted = inserted.unwrap_or(false);

                    if inserted {
                        success += 1;
                        let _ = app_clone.emit(
                            "image-downloaded",
                            ImageDownloaded {
                                source: "wallhaven".into(),
                                name: filename,
                                path: save_path.to_string_lossy().to_string(),
                            },
                        );
                    }
                }
            }
        }

        log::info!(
            "[wallhaven] download complete (success={}/{})",
            success,
            total
        );
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

    Ok(format!("即将下载 {count} 张壁纸"))
}
