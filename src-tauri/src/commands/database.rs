//! Database query commands: list_database_images, list_orphan_files,
//! mark_disliked_files, restore_all_files, list_missing_images,
//! delete_missing_records, refresh_file_caches.

use crate::config::Source;
use crate::db;
use crate::downloader;
use crate::state::{ensure_plain_filename, AppError, AppState};
use crate::thumbnail;
use serde::Serialize;
use std::collections::HashSet;

/// 清掉目录列表与统计缓存，让紧随其后的读取反映磁盘真实现状。
///
/// 「数据库」页刷新时会连发 `list_missing_images` / `list_orphan_files` / `get_stats`
/// 三条命令，它们共用目录列表缓存（因此只扫一次目录）。但在这之前必须先清一次：
/// 缓存有 5 秒 TTL，不清的话用户点了「刷新」可能看到几秒前的旧结果。
///
/// 清完之后三条命令仍然共享**同一次**重新扫描，所以既保证新鲜又不重复扫盘。
#[tauri::command]
pub async fn refresh_file_caches() -> Result<(), AppError> {
    log::info!("[CMD] refresh_file_caches");
    db::clear_dir_listings();
    db::clear_stats_caches();
    Ok(())
}

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
            // 两库合并分页：通过 ATTACH + UNION ALL 在数据库层合并排序分页。
            // 旧实现"各取 limit+offset 条再应用层切片"的代价随 offset 线性增长
            // （深翻页时要把近 2 万条记录读进内存再深拷贝），这里改成 O(limit)。
            db::get_all_images_paged(
                &config.wallhaven_db_path,
                &config.reddit_db_path,
                limit,
                offset,
            )
            .map_err(AppError::Db)
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

                // 走目录列表缓存：本页刷新时「缺失列表 / 孤儿列表 / 统计」三处
                // 共用同一次 read_dir，不再各扫一遍。大小也一并从快照取，
                // 不必再对每个文件 stat。
                let listing = db::dir_listing(save_dir);
                let mut orphans = Vec::new();
                for (name, size) in listing.iter() {
                    // file_is_image 只看扩展名，用文件名字符串即可，无需真实路径
                    if !downloader::file_is_image(std::path::Path::new(name)) {
                        continue;
                    }
                    if !db_names.contains(name) {
                        orphans.push(OrphanFile {
                            name: name.clone(),
                            path: dir.join(name).to_string_lossy().to_string(),
                            size: *size,
                            source: src.to_string(),
                        });
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

/// 永久删除缺失图片的数据库记录，并顺手清掉它们残留的缩略图。
///
/// 与 `mark_disliked_files`（只置 love=0，可用「恢复所有已标记」撤销）不同，
/// 这是真的删行，调用方必须先用确认框明确告知不可撤销。
///
/// 缩略图一并删除：原图已经不在保存目录，缩略图必然对不上任何文件，
/// 留着只能等「清理孤儿缩略图」再扫一遍。
#[tauri::command]
pub async fn delete_missing_records(
    state: tauri::State<'_, AppState>,
    source: Source,
    names: Vec<String>,
) -> Result<u64, AppError> {
    log::info!(
        "[CMD] delete_missing_records: source={:?}, count={}",
        source,
        names.len()
    );
    // 文件名来自前端，先逐个校验，避免任何路径穿越进到删文件/删缩略图环节。
    for name in &names {
        ensure_plain_filename(name)?;
    }
    let config = crate::state::load_config(&state)?;
    run_blocking(move || {
        // 显式展开 All：`thumb_dir_for(All)` 会回退到 Reddit 目录，
        // 直接用它会把 Wallhaven 的缩略图留在原地。
        let targets: Vec<(&str, std::path::PathBuf)> = match source {
            Source::Wallhaven => vec![(
                config.wallhaven_db_path.as_str(),
                config.wallhaven_thumb_dir(),
            )],
            Source::Reddit => vec![(config.reddit_db_path.as_str(), config.reddit_thumb_dir())],
            Source::All => vec![
                (
                    config.wallhaven_db_path.as_str(),
                    config.wallhaven_thumb_dir(),
                ),
                (config.reddit_db_path.as_str(), config.reddit_thumb_dir()),
            ],
        };

        let mut removed = 0u64;
        for (db_path, thumb_dir) in targets {
            removed += db::delete_records_by_names(db_path, &names).map_err(AppError::Db)?;
            for name in &names {
                thumbnail::remove_thumbnails(&thumb_dir, name);
            }
        }
        log::info!("[delete_missing_records] removed={}", removed);
        Ok(removed)
    })
    .await
}
