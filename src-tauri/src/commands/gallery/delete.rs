//! 删除：标记不喜欢 / 删孤儿文件。
//!
//! 两者都用 [`crate::trash::move_to_trash`] 移入回收站，**失败即报错，绝不退化成永久删除**；
//! 缩略图缓存是可再生物，直接删。

use crate::config::Source;
use crate::db;
use crate::error::AppError;
use crate::safe_path::{ensure_plain_filename, safe_join, safe_join_all};
use crate::state::AppState;
use crate::thumbnail;

#[tauri::command]
pub async fn dislike_file(
    state: tauri::State<'_, AppState>,
    source: Source,
    name: String,
) -> Result<bool, AppError> {
    log::info!("[CMD] dislike_file: source={:?}, name={}", source, name);
    ensure_plain_filename(&name)?;
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let db_path = config.db_path_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);

    // 数据库写入与文件删除都是阻塞操作，与批量版 dislike_files 保持一致放进阻塞线程池，
    // 避免卡住 Tauri 的 async runtime。
    tokio::task::spawn_blocking(move || {
        let db_ok = db::mark_dislike_by_name(&db_path, &name)?;

        let file_path = safe_join(std::path::Path::new(&save_dir), &name)?;
        if file_path.exists() {
            // 用 inspect_err 而非 map_err：错误类型已经是 AppError，这里只加日志不转换。
            crate::trash::move_to_trash(&file_path).inspect_err(|e| {
                log::error!(
                    "[dislike_file] 移入回收站失败 {}: {}",
                    file_path.display(),
                    e
                );
            })?;
        }

        thumbnail::remove_thumbnails(&thumb_dir, &name);

        Ok(db_ok)
    })
    .await
    .map_err(|e| AppError::Other(format!("删除文件任务异常: {e}")))?
}

#[tauri::command]
pub async fn dislike_files(
    state: tauri::State<'_, AppState>,
    source: Source,
    names: Vec<String>,
) -> Result<u64, AppError> {
    log::info!(
        "[CMD] dislike_files: source={:?}, count={}",
        source,
        names.len()
    );
    for name in &names {
        ensure_plain_filename(name)?;
    }
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let db_path = config.db_path_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);

    tokio::task::spawn_blocking(move || {
        let marked = db::mark_dislike_by_names(&db_path, &names)?;
        let mut removed = 0u64;
        // base 只 canonicalize 一次；单个路径解析失败只跳过该文件，不中断整批，
        // 否则用户点了「删除 20 个」会因第一个失败而一个都删不掉。
        for (name, file_path) in safe_join_all(std::path::Path::new(&save_dir), &names) {
            if file_path.exists() {
                if let Err(e) = crate::trash::move_to_trash(&file_path) {
                    log::error!(
                        "[dislike_files] 移入回收站失败 {}: {}",
                        file_path.display(),
                        e
                    );
                } else {
                    removed += 1;
                }
            }
            thumbnail::remove_thumbnails(&thumb_dir, name);
        }
        log::info!(
            "[dislike_files] marked={} removed={}/{}",
            marked,
            removed,
            names.len()
        );
        Ok(marked.max(removed))
    })
    .await
    .map_err(|e| AppError::Other(format!("批量删除任务异常: {e}")))?
}

#[tauri::command]
pub async fn delete_orphan_file(
    state: tauri::State<'_, AppState>,
    source: Source,
    name: String,
) -> Result<bool, AppError> {
    log::info!(
        "[CMD] delete_orphan_file: source={:?}, name={}",
        source,
        name
    );
    ensure_plain_filename(&name)?;
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);

    // 与批量版 delete_orphan_files 保持一致，文件 IO 放进阻塞线程池。
    tokio::task::spawn_blocking(move || {
        let file_path = safe_join(std::path::Path::new(&save_dir), &name)?;
        let existed = file_path.exists();
        if existed {
            crate::trash::move_to_trash(&file_path).inspect_err(|e| {
                log::error!(
                    "[delete_orphan_file] 移入回收站失败 {}: {}",
                    file_path.display(),
                    e
                );
            })?;
        }

        thumbnail::remove_thumbnails(&thumb_dir, &name);

        Ok(existed)
    })
    .await
    .map_err(|e| AppError::Other(format!("删除孤儿文件任务异常: {e}")))?
}

#[tauri::command]
pub async fn delete_orphan_files(
    state: tauri::State<'_, AppState>,
    source: Source,
    names: Vec<String>,
) -> Result<u64, AppError> {
    log::info!(
        "[CMD] delete_orphan_files: source={:?}, count={}",
        source,
        names.len()
    );
    for name in &names {
        ensure_plain_filename(name)?;
    }
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);

    tokio::task::spawn_blocking(move || {
        let mut removed = 0u64;
        // 同 dislike_files：base 只解析一次，单个失败只跳过，不中断整批。
        for (name, file_path) in safe_join_all(std::path::Path::new(&save_dir), &names) {
            if file_path.exists() {
                if let Err(e) = crate::trash::move_to_trash(&file_path) {
                    log::error!(
                        "[delete_orphan_files] 移入回收站失败 {}: {}",
                        file_path.display(),
                        e
                    );
                } else {
                    removed += 1;
                }
            }
            thumbnail::remove_thumbnails(&thumb_dir, name);
        }
        Ok(removed)
    })
    .await
    .map_err(|e| AppError::Other(format!("批量删除孤儿文件任务异常: {e}")))?
}
