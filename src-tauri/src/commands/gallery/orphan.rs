//! 孤儿文件入库（磁盘上有、数据库里没有的文件登记进库）。

use crate::config::Source;
use crate::db;
use crate::downloader;
use crate::error::AppError;
use crate::safe_path::{ensure_plain_filename, safe_join_all};
use crate::state::AppState;

#[tauri::command]
pub async fn adopt_orphan_files(
    state: tauri::State<'_, AppState>,
    source: Source,
    names: Vec<String>,
) -> Result<u64, AppError> {
    log::info!(
        "[CMD] adopt_orphan_files: source={:?}, count={}",
        source,
        names.len()
    );
    for name in &names {
        ensure_plain_filename(name)?;
    }
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let db_path = config.db_path_for(source).to_string();

    // 收养孤儿需要读取整张图片并计算 MD5，属于阻塞 IO/CPU 操作。
    tokio::task::spawn_blocking(move || {
        let mut wallhaven_batch: Vec<(String, String, String, String, String, String)> = Vec::new();
        let mut reddit_batch: Vec<(String, String, String, String, String)> = Vec::new();

        for (name, file_path) in safe_join_all(std::path::Path::new(&save_dir), &names) {
            if !file_path.is_file() {
                log::warn!(
                    "[adopt_orphan_files] file not found: {}",
                    file_path.display()
                );
                continue;
            }
            let size = file_path.metadata().map(|m| m.len()).unwrap_or(0);
            if size == 0 {
                log::warn!(
                    "[adopt_orphan_files] skipping empty file: {}",
                    file_path.display()
                );
                continue;
            }
            // 流式计算 MD5：4K 壁纸单张可达 50MB，整文件读入会在批量收养时撑高内存峰值。
            let hash = match downloader::compute_md5_file(&file_path) {
                Ok(h) => h,
                Err(e) => {
                    log::warn!(
                        "[adopt_orphan_files] 读取失败，跳过 {}: {}",
                        file_path.display(),
                        e
                    );
                    continue;
                }
            };

            if source.is_wallhaven() {
                let wallhaven_id = name
                    .strip_prefix("wallhaven_")
                    .and_then(|s| s.split('.').next())
                    .unwrap_or("");
                wallhaven_batch.push((
                    wallhaven_id.to_string(),
                    name.to_string(),
                    hash,
                    // url 列 UNIQUE，收养的孤儿没有真实下载 URL，若全写空串，
                    // 批量收养时第二条起会撞 UNIQUE 约束被静默 skip（只入 1 条）。
                    // 用 `orphan:{name}` 占位保证唯一，且不会与真实 https URL 冲突。
                    format!("orphan:{name}"),
                    String::new(),
                    "unknown".to_string(),
                ));
            } else {
                reddit_batch.push((
                    name.to_string(),
                    hash,
                    format!("orphan:{name}"),
                    String::new(),
                    String::new(),
                ));
            }
        }

        let added = if source.is_wallhaven() {
            db::insert_wallhaven_images_batch(&db_path, &wallhaven_batch)?.0
        } else {
            db::insert_reddit_images_batch(&db_path, &reddit_batch)?.0
        };

        log::info!("[adopt_orphan_files] done: added={}/{}", added, names.len());
        Ok(added)
    })
    .await
    .map_err(|e| AppError::Other(format!("收养孤儿文件任务异常: {e}")))?
}
