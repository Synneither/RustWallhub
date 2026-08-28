//! Wallhaven commands: search_wallhaven, start_wallhaven_download, download_wallhaven_selected.

use crate::db;
use crate::downloader;
use crate::state::{
    save_image, setup_cancel_flag, AppError, AppState, DownloadComplete, DownloadProgress,
    ImageDownloaded, ProgressThrottle,
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

/// 选中下载的 URL 来自前端 IPC，必须校验为 Wallhaven 域名，避免被当作通用下载代理。
fn is_wallhaven_image_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    let host = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");
    host == "wallhaven.cc" || host.ends_with(".wallhaven.cc")
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
            // 优先用 large 缩略图（约 500px 宽），放大网格时不模糊；
            // 缺失时回退到 small 的固定 URL 规则。
            let thumbnail_url = img
                .thumbs
                .as_ref()
                .map(|t| t.large.clone())
                .unwrap_or_else(|| {
                    let prefix = if img.id.len() >= 2 {
                        &img.id[..2]
                    } else {
                        &img.id[..1]
                    };
                    format!("https://th.wallhaven.cc/small/{prefix}/{}.jpg", img.id)
                });
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

        let existing_ids = {
            let db_path = config.wallhaven_db_path.clone();
            match tokio::task::spawn_blocking(move || db::get_existing_wallhaven_ids(&db_path))
                .await
            {
                Ok(Ok(ids)) => ids,
                Ok(Err(e)) => {
                    log::error!("[wallhaven] 获取已有ID失败: {e}");
                    return;
                }
                Err(e) => {
                    log::error!("[wallhaven] 获取已有ID任务异常: {e}");
                    return;
                }
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
            // 只有还需要继续翻页时才做节流等待，避免最后一批白等 2 秒。
            if (collected.len() as u32) < target && page <= max_pages {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }

        let total = collected.len() as u32;
        let mut success = 0u32;

        // 分批下载，限制同时驻留内存的原图数量；每批落盘后统一事务入库。
        let chunk_size = (config.download_concurrency.max(1) as usize)
            .saturating_mul(2)
            .max(1);
        let mut progress_throttle = ProgressThrottle::new();
        let mut processed = 0usize;
        for chunk in collected.chunks(chunk_size) {
            let urls: Vec<String> = chunk.iter().map(|img| img.path.clone()).collect();
            let download_results = downloader::download_urls_concurrent(
                &client,
                &urls,
                cancel.clone(),
                config.download_concurrency,
                3,
            )
            .await;

            let mut db_batch: Vec<(String, String, String, String, String, String)> = Vec::new();
            let mut saved_files: Vec<(String, String)> = Vec::new();

            for (local_i, img) in chunk.iter().enumerate() {
                let i = processed + local_i;
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

                if progress_throttle.should_emit(i + 1 == total as usize) {
                    let _ = app_clone.emit(
                        "download-progress",
                        DownloadProgress {
                            source: "wallhaven".into(),
                            done: i as u32,
                            total,
                            message: format!("正在处理 {} ({}/{})", img.id, i + 1, total),
                        },
                    );
                }

                match &download_results[local_i] {
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

                        match save_image(&save_path, bytes).await {
                            Ok(()) => {
                                db_batch.push((
                                    img.id.clone(),
                                    filename.clone(),
                                    hash,
                                    img.path.clone(),
                                    img.short_url.clone(),
                                    img.resolution.clone(),
                                ));
                                saved_files.push((
                                    filename.clone(),
                                    save_path.to_string_lossy().to_string(),
                                ));
                            }
                            Err(e) => log::error!("[wallhaven] {}", e),
                        }
                    }
                    Err(e) => {
                        log::error!("[wallhaven] download failed {}: {}", img.id, e);
                    }
                }
            }

            if !db_batch.is_empty() {
                let db_path = config.wallhaven_db_path.clone();
                let batch_len = db_batch.len() as u64;
                let (added, skipped, added_names) = match tokio::task::spawn_blocking(move || {
                    db::insert_wallhaven_images_batch_detailed(&db_path, &db_batch)
                })
                .await
                {
                    Ok(Ok(res)) => res,
                    Ok(Err(e)) => {
                        log::error!("[wallhaven] 批量写入数据库失败: {e}");
                        (0, batch_len, Vec::new())
                    }
                    Err(e) => {
                        log::error!("[wallhaven] 批量写入数据库任务异常: {e}");
                        (0, batch_len, Vec::new())
                    }
                };
                success += added as u32;
                if skipped > 0 {
                    log::warn!("[wallhaven] 本批跳过重复记录 {} 条", skipped);
                }
                let added_names: HashSet<String> = added_names.into_iter().collect();
                for (name, path) in saved_files {
                    if added_names.contains(&name) {
                        let _ = app_clone.emit(
                            "image-downloaded",
                            ImageDownloaded {
                                source: "wallhaven".into(),
                                name,
                                path,
                            },
                        );
                    }
                }
            }

            processed += chunk.len();
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
    if let Some(img) = images.iter().find(|img| !is_wallhaven_image_url(&img.path)) {
        return Err(AppError::Other(format!(
            "拒绝下载非 Wallhaven 域名: {}",
            img.path
        )));
    }
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

        let existing_ids = {
            let db_path = config.wallhaven_db_path.clone();
            match tokio::task::spawn_blocking(move || db::get_existing_wallhaven_ids(&db_path))
                .await
            {
                Ok(Ok(ids)) => ids,
                Ok(Err(e)) => {
                    log::error!("[wallhaven] 获取已有ID失败: {e}");
                    return;
                }
                Err(e) => {
                    log::error!("[wallhaven] 获取已有ID任务异常: {e}");
                    return;
                }
            }
        };
        let existing_set: HashSet<String> = existing_ids.into_iter().collect();
        // 先把已存在的 ID 过滤掉，避免下载完成后再丢弃。
        let pending: Vec<&WallhavenSelected> = images
            .iter()
            .filter(|img| !existing_set.contains(&img.id))
            .collect();
        let total = pending.len() as u32;
        let mut success = 0u32;

        let chunk_size = (config.download_concurrency.max(1) as usize)
            .saturating_mul(2)
            .max(1);
        let mut progress_throttle = ProgressThrottle::new();
        let mut processed = 0usize;
        for chunk in pending.chunks(chunk_size) {
            let urls: Vec<String> = chunk.iter().map(|img| img.path.clone()).collect();
            let download_results = downloader::download_urls_concurrent(
                &client,
                &urls,
                cancel.clone(),
                config.download_concurrency,
                3,
            )
            .await;

            let mut db_batch: Vec<(String, String, String, String, String, String)> = Vec::new();
            let mut saved_files: Vec<(String, String)> = Vec::new();

            for (local_i, img) in chunk.iter().enumerate() {
                let i = processed + local_i;
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

                if progress_throttle.should_emit(i + 1 == total as usize) {
                    let _ = app_clone.emit(
                        "download-progress",
                        DownloadProgress {
                            source: "wallhaven".into(),
                            done: i as u32,
                            total,
                            message: format!("正在下载 {} ({}/{})", img.id, i + 1, total),
                        },
                    );
                }

                if let Ok((bytes, content_type)) = &download_results[local_i] {
                    let ext = downloader::get_file_extension(content_type, &img.path);
                    let safe_id: String = img.id.chars().filter(|c| c.is_alphanumeric()).collect();
                    let filename = format!("wallhaven_{safe_id}.{ext}");
                    let save_path = Path::new(&config.wallhaven_save_dir).join(&filename);
                    let hash = downloader::compute_md5(bytes);

                    match save_image(&save_path, bytes).await {
                        Ok(()) => {
                            db_batch.push((
                                img.id.clone(),
                                filename.clone(),
                                hash,
                                img.path.clone(),
                                img.short_url.clone(),
                                img.resolution.clone(),
                            ));
                            saved_files
                                .push((filename.clone(), save_path.to_string_lossy().to_string()));
                        }
                        Err(e) => log::error!("[wallhaven] {}", e),
                    }
                }
            }

            if !db_batch.is_empty() {
                let db_path = config.wallhaven_db_path.clone();
                let batch_len = db_batch.len() as u64;
                let (added, skipped, added_names) = match tokio::task::spawn_blocking(move || {
                    db::insert_wallhaven_images_batch_detailed(&db_path, &db_batch)
                })
                .await
                {
                    Ok(Ok(res)) => res,
                    Ok(Err(e)) => {
                        log::error!("[wallhaven] 批量写入数据库失败: {e}");
                        (0, batch_len, Vec::new())
                    }
                    Err(e) => {
                        log::error!("[wallhaven] 批量写入数据库任务异常: {e}");
                        (0, batch_len, Vec::new())
                    }
                };
                success += added as u32;
                if skipped > 0 {
                    log::warn!("[wallhaven] 本批跳过重复记录 {} 条", skipped);
                }
                let added_names: HashSet<String> = added_names.into_iter().collect();
                for (name, path) in saved_files {
                    if added_names.contains(&name) {
                        let _ = app_clone.emit(
                            "image-downloaded",
                            ImageDownloaded {
                                source: "wallhaven".into(),
                                name,
                                path,
                            },
                        );
                    }
                }
            }

            processed += chunk.len();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_wallhaven_image_url() {
        assert!(is_wallhaven_image_url(
            "https://wallhaven.cc/images/abc.jpg"
        ));
        assert!(is_wallhaven_image_url(
            "https://w.wallhaven.cc/full/ab/abc.jpg"
        ));
        assert!(is_wallhaven_image_url("http://wallhaven.cc/images/abc.jpg"));
        assert!(!is_wallhaven_image_url(
            "https://evil.com/wallhaven.cc/x.jpg"
        ));
        assert!(!is_wallhaven_image_url("ftp://wallhaven.cc/x.jpg"));
    }
}
