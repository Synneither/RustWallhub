//! Reddit commands: start_reddit_download.

use crate::db;
use crate::downloader;
use crate::reddit;
use crate::state::{
    save_image, setup_cancel_flag, AppError, AppState, DownloadComplete, DownloadProgress,
    ImageDownloaded,
};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering;
use tauri::Emitter;

#[tauri::command]
pub async fn start_reddit_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    log::info!("[CMD] start_reddit_download called");
    let config = crate::state::load_config(&state)?;
    let cancel = setup_cancel_flag(&state);
    let app_clone = app.clone();
    let client = state.http_client.lock().unwrap().clone();

    tokio::spawn(async move {
        let reddit_client = reddit::RedditClient::new(client.clone(), config.reddit_url.clone());

        let _ = tokio::fs::create_dir_all(&config.reddit_save_dir).await;

        let existing_urls = match db::get_existing_reddit_urls(&config.reddit_db_path) {
            Ok(urls) => urls,
            Err(e) => {
                log::error!("[reddit] 获取已有URL失败: {e}");
                return;
            }
        };
        let existing_set: HashSet<String> = existing_urls.into_iter().collect();

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

            let result = reddit_client
                .fetch_posts(after.as_deref(), config.reddit_max_posts)
                .await;

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
                    "[reddit] download cancelled (success={}/{})",
                    success,
                    total
                );
                let _ = app_clone.emit(
                    "download-complete",
                    DownloadComplete {
                        source: "reddit".into(),
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
                    let save_path = Path::new(&config.reddit_save_dir).join(&filename);

                    let thumb_dir = config.reddit_thumb_dir();
                    if let Some(thumb_handle) = save_image(
                        &save_path,
                        bytes,
                        &thumb_dir,
                        &filename,
                        config.thumbnail_dpr,
                    )
                    .await
                    {
                        let db_path = config.reddit_db_path.clone();
                        let filename_for_db = filename.clone();
                        let hash_for_db = hash.clone();
                        let image_url = img.image_url.clone();
                        let title = img.title.clone();
                        let permalink = img.permalink.clone();

                        let db_handle = tokio::task::spawn_blocking(move || {
                            db::insert_reddit_image(
                                &db_path,
                                &filename_for_db,
                                &hash_for_db,
                                &image_url,
                                &title,
                                &permalink,
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
