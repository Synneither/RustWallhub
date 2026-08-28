//! 缩略图相关：反向解析缩略图名、清理失效缩略图。

use crate::downloader;
use std::path::Path;

/// 反向解析缩略图名对应的原图是否存在。
/// 兼容旧格式（缩略图名 = 原图名）与 DPR 新格式（`stem__w480.webp`）。
fn thumbnail_source_exists(save_dir: &str, thumb_name: &str) -> bool {
    if Path::new(save_dir).join(thumb_name).exists() {
        return true;
    }

    if let Some(rest) = thumb_name.strip_suffix(".webp") {
        if let Some((stem, width)) = rest.rsplit_once("__w") {
            if !stem.is_empty() && !width.is_empty() && width.chars().all(|c| c.is_ascii_digit()) {
                return downloader::IMAGE_EXTENSIONS
                    .iter()
                    .any(|ext| Path::new(save_dir).join(format!("{stem}.{ext}")).exists());
            }
        }
    }
    false
}

pub fn clean_stale_thumbnails(thumbnail_dir: &str, save_dir: &str) -> u64 {
    let thumb_dir_path = Path::new(thumbnail_dir);
    if !thumb_dir_path.is_dir() {
        return 0;
    }
    let mut cleaned = 0u64;
    if let Ok(entries) = std::fs::read_dir(thumb_dir_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && downloader::file_is_image(&path) {
                let name = entry.file_name().to_string_lossy().to_string();
                if !thumbnail_source_exists(save_dir, &name) {
                    std::fs::remove_file(&path).ok();
                    cleaned += 1;
                }
            }
        }
    }
    log::info!("[DB] clean_stale_thumbnails: cleaned={}", cleaned);
    cleaned
}
