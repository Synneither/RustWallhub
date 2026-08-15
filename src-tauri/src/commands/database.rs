//! Database query commands: list_database_images, list_orphan_files,
//! mark_disliked_files, count_missing_images, restore_all_files, list_missing_images.

use crate::config::Source;
use crate::db;
use crate::downloader;
use crate::state::{AppError, AppState};
use serde::Serialize;
use std::collections::HashSet;

/// 数据库命令统一放到阻塞线程池，避免 rusqlite/文件扫描占用 tokio worker。
async fn run_blocking<F, T>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Other(format!("数据库任务异常: {e}")))?
}

#[derive(Serialize)]
pub struct OrphanFile {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub source: String,
}

#[tauri::command]
pub async fn list_database_images(
    state: tauri::State<'_, AppState>,
    source: Source,
    limit: i64,
    offset: i64,
) -> Result<Vec<db::ImageRecord>, AppError> {
    log::info!(
        "[CMD] list_database_images: source={:?}, limit={}, offset={}",
        source,
        limit,
        offset
    );
    let config = crate::state::load_config(&state)?;
    run_blocking(move || match source {
        Source::Wallhaven => {
            db::get_wallhaven_images(&config.wallhaven_db_path, limit, offset).map_err(AppError::Db)
        }
        Source::Reddit => {
            db::get_reddit_images(&config.reddit_db_path, limit, offset).map_err(AppError::Db)
        }
        Source::All => {
            // 两库合并分页：各取前 offset+limit 条，合并排序后再切页。
            let fetch = limit.saturating_add(offset).max(limit);
            let mut rows = db::get_wallhaven_images(&config.wallhaven_db_path, fetch, 0)?;
            rows.extend(db::get_reddit_images(&config.reddit_db_path, fetch, 0)?);
            rows.sort_by(|a, b| {
                b.created_at
                    .cmp(&a.created_at)
                    .then_with(|| b.id.cmp(&a.id))
            });
            let start = (offset as usize).min(rows.len());
            let end = start.saturating_add(limit as usize).min(rows.len());
            Ok(rows[start..end].to_vec())
        }
    })
    .await
}

#[tauri::command]
pub async fn list_orphan_files(
    state: tauri::State<'_, AppState>,
    source: Source,
) -> Result<Vec<OrphanFile>, AppError> {
    log::info!("[CMD] list_orphan_files: source={:?}", source);
    let config = crate::state::load_config(&state)?;
    run_blocking(move || {
        let check_source =
            |src: Source, save_dir: &str, db_path: &str| -> Result<Vec<OrphanFile>, AppError> {
                let dir = std::path::Path::new(save_dir);
                if !dir.is_dir() {
                    return Ok(Vec::new());
                }
                let db_names: HashSet<String> =
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
                                    size: entry.metadata().map_or(0, |m| m.len()),
                                    source: src.to_string(),
                                });
                            }
                        }
                    }
                }
                Ok(orphans)
            };

        match source {
            Source::Wallhaven => check_source(
                Source::Wallhaven,
                &config.wallhaven_save_dir,
                &config.wallhaven_db_path,
            ),
            Source::Reddit => check_source(
                Source::Reddit,
                &config.reddit_save_dir,
                &config.reddit_db_path,
            ),
            Source::All => {
                let mut all = check_source(
                    Source::Wallhaven,
                    &config.wallhaven_save_dir,
                    &config.wallhaven_db_path,
                )?;
                all.extend(check_source(
                    Source::Reddit,
                    &config.reddit_save_dir,
                    &config.reddit_db_path,
                )?);
                Ok(all)
            }
        }
    })
    .await
}

#[tauri::command]
pub async fn mark_disliked_files(
    state: tauri::State<'_, AppState>,
    source: Source,
) -> Result<u64, AppError> {
    log::info!("[CMD] mark_disliked_files: source={:?}", source);
    let config = crate::state::load_config(&state)?;
    run_blocking(move || match source {
        Source::Wallhaven => db::mark_missing_dislike_wallhaven(
            &config.wallhaven_db_path,
            &config.wallhaven_save_dir,
        )
        .map_err(AppError::Db),
        Source::Reddit => {
            db::mark_missing_dislike_reddit(&config.reddit_db_path, &config.reddit_save_dir)
                .map_err(AppError::Db)
        }
        Source::All => {
            let w = db::mark_missing_dislike_wallhaven(
                &config.wallhaven_db_path,
                &config.wallhaven_save_dir,
            )?;
            let r =
                db::mark_missing_dislike_reddit(&config.reddit_db_path, &config.reddit_save_dir)?;
            Ok(w + r)
        }
    })
    .await
}

#[tauri::command]
pub async fn restore_all_files(
    state: tauri::State<'_, AppState>,
    source: Source,
) -> Result<u64, AppError> {
    log::info!("[CMD] restore_all_files: source={:?}", source);
    let config = crate::state::load_config(&state)?;
    run_blocking(move || match source {
        Source::Wallhaven => db::restore_love_db(&config.wallhaven_db_path).map_err(AppError::Db),
        Source::Reddit => db::restore_love_db(&config.reddit_db_path).map_err(AppError::Db),
        Source::All => {
            let w = db::restore_love_db(&config.wallhaven_db_path)?;
            let r = db::restore_love_db(&config.reddit_db_path)?;
            Ok(w + r)
        }
    })
    .await
}

#[tauri::command]
pub async fn list_missing_images(
    state: tauri::State<'_, AppState>,
    source: Source,
) -> Result<Vec<db::ImageRecord>, AppError> {
    log::info!("[CMD] list_missing_images: source={:?}", source);
    let config = crate::state::load_config(&state)?;
    run_blocking(move || match source {
        Source::Wallhaven => {
            db::get_wallhaven_missing_files(&config.wallhaven_db_path, &config.wallhaven_save_dir)
                .map_err(AppError::Db)
        }
        Source::Reddit => {
            db::get_reddit_missing_files(&config.reddit_db_path, &config.reddit_save_dir)
                .map_err(AppError::Db)
        }
        Source::All => {
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
    })
    .await
}
