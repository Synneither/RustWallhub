//! 缩略图：按需生成、清理孤儿缓存。

use super::{CleanThumbnailsResult, ThumbnailBatch, ThumbnailItem};
use crate::config::Source;
use crate::db;
use crate::error::AppError;
use crate::safe_path::ensure_plain_filename;
use crate::state::AppState;
use crate::thumbnail;
use std::collections::HashSet;
use std::path::PathBuf;

/// 单次缩略图批量解析的文件数上限（一页最多 96 张，留余量；防止 IPC 传入超长列表）。
const MAX_THUMB_BATCH: usize = 200;

#[tauri::command]
pub async fn resolve_thumbnails(
    state: tauri::State<'_, AppState>,
    source: Source,
    filenames: Vec<String>,
    dpr: Option<u32>,
) -> Result<ThumbnailBatch, AppError> {
    let dpr = dpr.unwrap_or(1).max(1);
    log::info!(
        "[CMD] resolve_thumbnails: source={:?}, count={}, dpr={}",
        source,
        filenames.len(),
        dpr
    );
    // 文件名只能是一个普通文件名，拒绝任何 IPC 传入的路径穿越。
    // 同时给单次批量上限，防止前端传一个超长列表把 rayon 池跑满、卡住其他命令。
    let mut seen = HashSet::new();
    let mut safe_filenames = Vec::with_capacity(filenames.len().min(MAX_THUMB_BATCH));
    for name in filenames {
        if safe_filenames.len() >= MAX_THUMB_BATCH {
            break;
        }
        ensure_plain_filename(&name)?;
        if seen.insert(name.clone()) {
            safe_filenames.push(name);
        }
    }

    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);
    let image_dir = PathBuf::from(&save_dir);

    // 批量缩略图包含图片解码/缩放等 CPU 密集操作，放到阻塞线程池。
    let batch_result = tokio::task::spawn_blocking(move || {
        thumbnail::ensure_batch_thumbnails(&thumb_dir, &image_dir, &safe_filenames, dpr)
    })
    .await
    .map_err(|e| AppError::Other(format!("缩略图任务异常: {e}")))?;

    let items = batch_result
        .into_iter()
        .map(|(name, thumb_path)| ThumbnailItem {
            name,
            thumb_path: thumb_path.to_string_lossy().to_string(),
        })
        .collect();

    Ok(ThumbnailBatch { items })
}

#[tauri::command]
pub async fn clean_thumbnails(
    state: tauri::State<'_, AppState>,
) -> Result<CleanThumbnailsResult, AppError> {
    log::info!("[CMD] clean_thumbnails called");
    let config = crate::state::load_config(&state)?;
    let wh_thumb_dir = config.wallhaven_thumb_dir().to_string_lossy().to_string();
    let rd_thumb_dir = config.reddit_thumb_dir().to_string_lossy().to_string();
    let wh_save_dir = config.wallhaven_save_dir.clone();
    let rd_save_dir = config.reddit_save_dir.clone();
    // 用户主动清理：顺带回收低于当前最低档位的缩略图。`thumbnail_dpr` 是「最低档位」
    // 语义（见前端 thumbSize.ts 的 pickThumbDpr），所以 floor=3 时 w240/w480 用不上。
    // 启动时的自动清理走 `clean_stale_thumbnails`（不传档位），避免误删下一刻要用的档位。
    let keep_min_dpr = config.thumbnail_dpr;

    tokio::task::spawn_blocking(move || {
        let wallhaven =
            db::clean_stale_thumbnails_keeping(&wh_thumb_dir, &wh_save_dir, Some(keep_min_dpr));
        let reddit =
            db::clean_stale_thumbnails_keeping(&rd_thumb_dir, &rd_save_dir, Some(keep_min_dpr));
        CleanThumbnailsResult { wallhaven, reddit }
    })
    .await
    .map_err(|e| AppError::Other(format!("清理缩略图任务异常: {e}")))
}
