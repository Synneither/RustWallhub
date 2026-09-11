//! Reddit commands: start_reddit_download.

use crate::commands::download_common::{
    emit_complete, emit_downloaded_for_added, emit_progress, rollback_saved_files, SavedFile,
};
use crate::config::Source;
use crate::db;
use crate::downloader;
use crate::reddit;
use crate::state::{
    save_image, setup_cancel_flag, AppError, AppState, ProgressThrottle,
};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering;

#[tauri::command]
pub async fn start_reddit_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    log::info!("[CMD] start_reddit_download called");
    let config = crate::state::load_config(&state)?;
    let cancel = setup_cancel_flag(&state, Source::Reddit);
    let app_clone = app.clone();
    let client = state
        .http_client
        .lock()
        .map_err(|e| AppError::Other(format!("锁获取失败: {e}")))?
        .clone();

    tokio::spawn(async move {
        let reddit_client = reddit::RedditClient::new(client.clone(), config.reddit_url.clone());

        let _ = tokio::fs::create_dir_all(&config.reddit_save_dir).await;

        let existing_urls = {
            let db_path = config.reddit_db_path.clone();
            match tokio::task::spawn_blocking(move || db::get_existing_reddit_urls(&db_path)).await
            {
                Ok(Ok(urls)) => urls,
                Ok(Err(e)) => {
                    log::error!("[reddit] 获取已有URL失败: {e}");
                    return;
                }
                Err(e) => {
                    log::error!("[reddit] 获取已有URL任务异常: {e}");
                    return;
                }
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
            emit_progress(
                &app_clone,
                "reddit",
                collected.len() as u32,
                target,
                format!("正在获取帖子... (已找到 {} 张)", collected.len()),
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
                    emit_progress(
                        &app_clone,
                        "reddit",
                        collected.len() as u32,
                        target,
                        format!("获取帖子失败: {e}"),
                    );
                    break;
                }
            }
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if (collected.len() as u32) < target {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }

        let total = collected.len() as u32;
        let mut success = 0u32;

        // 分批下载，避免 Reddit 大批量任务把所有图片 bytes 同时驻留内存。
        let chunk_size = downloader::download_chunk_size(config.download_concurrency);
        let mut progress_throttle = ProgressThrottle::new();
        let mut processed = 0usize;
        for chunk in collected.chunks(chunk_size) {
            let urls: Vec<String> = chunk.iter().map(|img| img.image_url.clone()).collect();
            let download_results = downloader::download_urls_concurrent(
                &client,
                &urls,
                cancel.clone(),
                config.download_concurrency,
                3,
            )
            .await;

            let mut db_batch: Vec<(String, String, String, String, String)> = Vec::new();
            let mut saved_files: Vec<SavedFile> = Vec::new();

            for (local_i, img) in chunk.iter().enumerate() {
                let i = processed + local_i;
                if cancel.load(Ordering::Relaxed) {
                    log::info!(
                        "[reddit] download cancelled (success={}/{})",
                        success,
                        total
                    );
                    emit_complete(&app_clone, "reddit", success, total, "下载已取消".to_string());
                    return;
                }

                if progress_throttle.should_emit(i + 1 == total as usize) {
                    emit_progress(
                        &app_clone,
                        "reddit",
                        i as u32,
                        total,
                        format!("正在下载 ({}/{})", i + 1, total),
                    );
                }

                match &download_results[local_i] {
                    Ok((bytes, content_type)) => {
                        let ext = downloader::get_file_extension(content_type, &img.image_url);
                        let hash = downloader::compute_md5(bytes);
                        let filename = format!("{hash}.{ext}");
                        let save_path = Path::new(&config.reddit_save_dir).join(&filename);

                        match save_image(&save_path, bytes).await {
                            Ok(()) => {
                                db_batch.push((
                                    filename.clone(),
                                    hash,
                                    img.image_url.clone(),
                                    img.title.clone(),
                                    img.permalink.clone(),
                                ));
                                saved_files.push(SavedFile {
                                    name: filename.clone(),
                                    path: save_path.to_string_lossy().to_string(),
                                });
                            }
                            Err(e) => log::error!("[reddit] {}", e),
                        }
                    }
                    Err(e) => {
                        log::error!("[reddit] download failed {}: {}", img.title, e);
                    }
                }
            }

            if !db_batch.is_empty() {
                let db_path = config.reddit_db_path.clone();
                let batch_len = db_batch.len() as u64;
                let (added, skipped, added_names) = match tokio::task::spawn_blocking(move || {
                    db::insert_reddit_images_batch_detailed(&db_path, &db_batch)
                })
                .await
                {
                    Ok(Ok(res)) => res,
                    Ok(Err(e)) => {
                        log::error!("[reddit] 批量写入数据库失败: {e}");
                        // 回滚：删除已落盘文件，避免磁盘有文件但库无记录的孤儿状态。
                        rollback_saved_files(&saved_files);
                        (0, batch_len, Vec::new())
                    }
                    Err(e) => {
                        log::error!("[reddit] 批量写入数据库任务异常: {e}");
                        rollback_saved_files(&saved_files);
                        (0, batch_len, Vec::new())
                    }
                };
                success += added as u32;
                if skipped > 0 {
                    log::warn!("[reddit] 本批跳过重复记录 {} 条", skipped);
                }
                emit_downloaded_for_added(&app_clone, "reddit", &saved_files, &added_names);
            }

            processed += chunk.len();
        }

        log::info!("[reddit] download complete (success={}/{})", success, total);
        emit_complete(
            &app_clone,
            "reddit",
            success,
            total,
            format!("Reddit 下载完成: 成功 {success}/{total}"),
        );
    });

    Ok("Reddit 下载已启动".to_string())
}
