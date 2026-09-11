//! 下载流程的公共部分。
//!
//! 四个下载命令（wallhaven 搜索下载、wallhaven 补下载、reddit 搜索下载、数据库恢复下载）
//! 的收尾逻辑完全一致：分批落盘 → 统一事务入库 → 回滚孤儿文件 → 发 `image-downloaded` 事件。
//! 这里把这段逻辑和它用到的类型收拢到一处，避免四份拷贝各自演化。

use std::collections::HashSet;

use tauri::{AppHandle, Emitter};

use crate::state::{DownloadComplete, DownloadProgress, ImageDownloaded};

/// 落盘成功、等待入库的一条记录。
pub struct SavedFile {
    pub name: String,
    pub path: String,
}

/// 发一次下载进度事件。`done` 是已完成数量，`total` 是本轮总数。
pub fn emit_progress(
    app: &AppHandle,
    source: &str,
    done: u32,
    total: u32,
    message: String,
) {
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            source: source.to_string(),
            done,
            total,
            message,
        },
    );
}

/// 发一次下载结束事件（含取消场景）。
pub fn emit_complete(app: &AppHandle, source: &str, success: u32, total: u32, message: String) {
    let _ = app.emit(
        "download-complete",
        DownloadComplete {
            source: source.to_string(),
            success,
            total,
            message,
        },
    );
}

/// DB 写入失败时回滚已落盘的文件，避免「磁盘有文件、库无记录」的孤儿状态。
pub fn rollback_saved_files(saved_files: &[SavedFile]) {
    for file in saved_files {
        if let Err(e) = std::fs::remove_file(&file.path) {
            log::warn!("[download] 回滚文件失败 {}: {e}", file.name);
        } else {
            log::warn!("[download] DB 写入失败，已回滚文件 {}", file.name);
        }
    }
}

/// 只对真正新增进库的记录发 `image-downloaded`，跳过因重复而被 DB 忽略的文件。
///
/// 逐个判断而不是全发，是为了让前端"本次新图"列表不出现重复项。
pub fn emit_downloaded_for_added(
    app: &AppHandle,
    source: &str,
    saved_files: &[SavedFile],
    added_names: &[String],
) {
    let added: HashSet<&String> = added_names.iter().collect();
    for file in saved_files {
        if added.contains(&file.name) {
            let _ = app.emit(
                "image-downloaded",
                ImageDownloaded {
                    source: source.to_string(),
                    name: file.name.clone(),
                    path: file.path.clone(),
                },
            );
        }
    }
}
