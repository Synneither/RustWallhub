//! 图库（本地文件浏览、缩略图、删除/收养、详情）。
//!
//! 按职责拆成子模块，命令名与前端 `utils/api.ts` 一一对应：
//!
//! | 子模块 | 命令 |
//! |---|---|
//! | [`browse`] | `browse_image_files` |
//! | [`thumbs`] | `resolve_thumbnails` / `clean_thumbnails` |
//! | [`delete`] | `dislike_file(s)` / `delete_orphan_file(s)` |
//! | [`orphan`] | `adopt_orphan_files` |
//! | [`info`] | `get_image_info` |
//!
//! 跨子模块共用的只有下面这些响应类型（前端 `types.ts` 有对应声明）。

use serde::Serialize;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LocalImageList {
    pub images: Vec<LocalImageEntry>,
    pub total: usize,
}

#[derive(Serialize)]
pub struct LocalImageEntry {
    pub name: String,
    pub path: String,
    pub thumb_path: Option<String>,
    pub size: u64,
    pub is_orphan: bool,
    pub modified_date: Option<String>,
}

#[derive(Serialize)]
pub struct ThumbnailBatch {
    pub items: Vec<ThumbnailItem>,
}

#[derive(Serialize)]
pub struct ThumbnailItem {
    pub name: String,
    pub thumb_path: String,
}

#[derive(Serialize)]
pub struct CleanThumbnailsResult {
    pub wallhaven: u64,
    pub reddit: u64,
}

#[derive(Serialize)]
pub struct ImageInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub resolution: Option<String>,
    pub format: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub source_url: Option<String>,
    pub download_url: Option<String>,
    pub title: Option<String>,
    pub permalink: Option<String>,
    pub source: Option<String>,
    pub created_at: Option<String>,
}

mod browse;
mod delete;
mod info;
mod orphan;
mod thumbs;

pub use browse::*;
pub use delete::*;
pub use info::*;
pub use orphan::*;
pub use thumbs::*;
