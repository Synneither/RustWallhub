//! 缩略图相关：反向解析缩略图名、清理失效缩略图。

use crate::downloader;
use std::collections::HashSet;
use std::path::Path;

/// 把缩略图名还原成它对应的原图文件名集合（用于判断缩略图是否还有效）。
///
/// 兼容两种格式：
/// - 旧格式：缩略图名 == 原图名
/// - DPR 新格式：`stem__w480.webp` → `stem.{jpg,png,...}`
fn candidate_source_names(thumb_name: &str) -> Vec<String> {
    let mut names = vec![thumb_name.to_string()];
    if let Some(rest) = thumb_name.strip_suffix(".webp") {
        if let Some((stem, width)) = rest.rsplit_once("__w") {
            if !stem.is_empty() && !width.is_empty() && width.chars().all(|c| c.is_ascii_digit()) {
                names.extend(
                    downloader::IMAGE_EXTENSIONS
                        .iter()
                        .map(|ext| format!("{stem}.{ext}")),
                );
            }
        }
    }
    names
}

/// 清理原图已不存在的缩略图。
///
/// 判定改为对**保存目录的文件名快照**做集合查找：原来是每个缩略图最多 8 次
/// `Path::exists()`（1 次原名 + 7 种扩展名），几千张缩略图就是上万次 stat。
/// 现在整个保存目录只扫一次，判定变成纯内存查找。
pub fn clean_stale_thumbnails(thumbnail_dir: &str, save_dir: &str) -> u64 {
    let thumb_dir_path = Path::new(thumbnail_dir);
    if !thumb_dir_path.is_dir() {
        return 0;
    }

    // 先取保存目录快照（带短缓存，与缺失/孤儿/统计共用同一次扫描）。
    let existing: HashSet<String> = super::dir_listing(save_dir).keys().cloned().collect();

    let mut cleaned = 0u64;
    if let Ok(entries) = std::fs::read_dir(thumb_dir_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !downloader::file_is_image(&path) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let still_valid = candidate_source_names(&name)
                .iter()
                .any(|candidate| existing.contains(candidate));
            if !still_valid {
                std::fs::remove_file(&path).ok();
                cleaned += 1;
            }
        }
    }
    log::info!("[DB] clean_stale_thumbnails: cleaned={}", cleaned);
    cleaned
}
