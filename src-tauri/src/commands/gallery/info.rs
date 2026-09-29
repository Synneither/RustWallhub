//! 单张图片详情（数据库元数据 + 磁盘信息）。

use super::ImageInfo;
use crate::config::Source;
use crate::db;
use crate::error::AppError;
use crate::safe_path::{ensure_plain_filename, safe_join};
use crate::state::AppState;

#[tauri::command]
pub async fn get_image_info(
    state: tauri::State<'_, AppState>,
    source: Source,
    name: String,
) -> Result<ImageInfo, AppError> {
    log::info!("[CMD] get_image_info: source={:?}, name={}", source, name);
    ensure_plain_filename(&name)?;
    let config = crate::state::load_config(&state)?;

    // 先查数据库元数据；Source::All 需要分别查两个库并选择正确来源目录。
    let wh_db_path = config.wallhaven_db_path.clone();
    let rd_db_path = config.reddit_db_path.clone();
    let lookup_name = name.clone();
    let db_record = match source {
        Source::Wallhaven => {
            let wh = wh_db_path.clone();
            tokio::task::spawn_blocking(move || db::get_wallhaven_image_by_name(&wh, &lookup_name))
                .await
                .map_err(|e| AppError::Other(format!("数据库查询任务异常: {e}")))?
                .map_err(AppError::Db)?
        }
        Source::Reddit => {
            let rd = rd_db_path.clone();
            tokio::task::spawn_blocking(move || db::get_reddit_image_by_name(&rd, &lookup_name))
                .await
                .map_err(|e| AppError::Other(format!("数据库查询任务异常: {e}")))?
                .map_err(AppError::Db)?
        }
        Source::All => {
            let wh = wh_db_path.clone();
            let rd = rd_db_path.clone();
            let lookup_name = name.clone();
            tokio::task::spawn_blocking(move || {
                // 只有确认 wallhaven 库中确实没有（Ok(None)）才回退查 reddit；
                // 库损坏/锁死（Err）必须直接暴露，否则会被误判为孤儿文件。
                match db::get_wallhaven_image_by_name(&wh, &lookup_name) {
                    Ok(Some(rec)) => Ok(Some(rec)),
                    Ok(None) => db::get_reddit_image_by_name(&rd, &lookup_name),
                    Err(e) => Err(e),
                }
            })
            .await
            .map_err(|e| AppError::Other(format!("数据库查询任务异常: {e}")))?
            .map_err(AppError::Db)?
        }
    };

    // 决定文件所在目录：
    // - 有数据库记录时按记录来源（wallhaven 记录在 wallhaven 目录，其余在 reddit 目录）
    // - 孤儿文件（无记录）且 source = All 时，`save_dir_for(All)` 固定返回 reddit 目录，
    //   会把 wallhaven 目录下的孤儿文件拼错路径，导致 size/尺寸恒为空。
    //   这里两个目录各探一次，取真实存在的那个。
    let save_dir = match db_record.as_ref().map(|r| r.source.as_str()) {
        Some("wallhaven") => config.wallhaven_save_dir.clone(),
        Some(_) => config.reddit_save_dir.clone(),
        None if matches!(source, Source::All) => {
            let wh_dir = config.wallhaven_save_dir.clone();
            let rd_dir = config.reddit_save_dir.clone();
            let probe = |dir: &str| {
                safe_join(std::path::Path::new(dir), &name)
                    .ok()
                    .filter(|p| p.exists())
            };
            if probe(&wh_dir).is_some() {
                wh_dir
            } else {
                // 都不存在时退回 reddit 目录，保持"报错信息指向一个具体路径"的旧行为
                rd_dir
            }
        }
        None => config.save_dir_for(source).to_string(),
    };
    let file_path = safe_join(std::path::Path::new(&save_dir), &name)?;
    let file_path_for_task = file_path.clone();

    // 图片文件读取与尺寸解析是阻塞 IO/CPU 操作，放到 spawn_blocking；
    // 同时改成直接 open 文件读取头部，而不是把整张原图读进内存。
    let (size, width, height, format) = tokio::task::spawn_blocking(move || {
        let size = std::fs::metadata(&file_path_for_task)
            .map(|m| m.len())
            .unwrap_or(0);
        let (width, height, format) = match image::ImageReader::open(&file_path_for_task) {
            Ok(reader) => match reader.with_guessed_format() {
                Ok(reader) => {
                    let fmt = reader.format().map(|f| format!("{f:?}"));
                    match reader.into_dimensions() {
                        Ok((w, h)) => (Some(w), Some(h), fmt),
                        Err(e) => {
                            log::warn!("[get_image_info] failed to read dimensions: {}", e);
                            (None, None, fmt)
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[get_image_info] failed to guess format: {}", e);
                    (None, None, None)
                }
            },
            Err(e) => {
                log::warn!("[get_image_info] failed to open file: {}", e);
                (None, None, None)
            }
        };
        (size, width, height, format)
    })
    .await
    .map_err(|e| AppError::Other(format!("图片信息任务异常: {e}")))?;

    let info_source = db_record
        .as_ref()
        .map(|rec| Some(rec.source.clone()))
        .unwrap_or_else(|| Some(source.to_string()));

    let (source_url, download_url, title, permalink, created_at, resolution) = match db_record {
        Some(rec) => {
            let res = if rec.resolution.is_empty() || rec.resolution == "unknown" {
                if let (Some(w), Some(h)) = (width, height) {
                    Some(format!("{}x{}", w, h))
                } else {
                    None
                }
            } else {
                Some(rec.resolution)
            };
            (
                if rec.source_url.is_empty() {
                    None
                } else {
                    Some(rec.source_url)
                },
                Some(rec.url),
                rec.title,
                rec.permalink,
                if rec.created_at.is_empty() {
                    None
                } else {
                    Some(rec.created_at)
                },
                res,
            )
        }
        None => {
            // Orphan file — derive resolution from image dimensions
            let res = if let (Some(w), Some(h)) = (width, height) {
                Some(format!("{}x{}", w, h))
            } else {
                None
            };
            (None, None, None, None, None, res)
        }
    };

    Ok(ImageInfo {
        name: name.clone(),
        path: file_path.to_string_lossy().to_string(),
        size,
        resolution,
        format,
        width,
        height,
        source_url,
        download_url,
        title,
        permalink,
        source: info_source,
        created_at,
    })
}
