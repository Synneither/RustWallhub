//! Gallery commands: browse_image_files, resolve_thumbnail(s), clean_thumbnails,
//! delete_image, dislike_file, delete_orphan_file, adopt_orphan_files.

use crate::config::Source;
use crate::db;
use crate::downloader;
use crate::state::{AppError, AppState, FileEntry, FileListCache};
use crate::thumbnail;
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

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

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn browse_image_files(
    state: tauri::State<'_, AppState>,
    source: Source,
    offset: usize,
    limit: usize,
    custom_dir: Option<String>,
) -> Result<LocalImageList, AppError> {
    log::info!(
        "[CMD] browse_image_files: source={:?}, offset={}, limit={}, custom_dir={:?}",
        source,
        offset,
        limit,
        custom_dir
    );
    let config = crate::state::load_config(&state)?;
    let dir = if let Some(ref custom) = custom_dir {
        custom.clone()
    } else {
        config.save_dir_for(source).to_string()
    };

    let path = PathBuf::from(&dir);
    if !path.is_dir() {
        return Ok(LocalImageList {
            images: Vec::new(),
            total: 0,
        });
    }

    {
        if let Ok(cache) = state.file_cache.lock() {
            if let Some(ref cached) = *cache {
                let src_str = source.to_string();
                if cached.source == src_str
                    && cached.dir_path == dir
                    && cached.cached_at.elapsed().as_secs() < 30
                {
                    let page_start = offset.min(cached.total);
                    let page_end = (page_start + limit).min(cached.total);
                    let images = cached.items[page_start..page_end]
                        .iter()
                        .map(|e| LocalImageEntry {
                            name: e.name.clone(),
                            path: e.path.clone(),
                            thumb_path: None,
                            size: e.size,
                            is_orphan: e.is_orphan,
                        })
                        .collect();
                    return Ok(LocalImageList {
                        images,
                        total: cached.total,
                    });
                }
            }
        }
    }

    let db_names: HashSet<String> = match source {
        Source::Wallhaven => db::get_all_filenames(&config.wallhaven_db_path)
            .unwrap_or_default()
            .into_iter()
            .collect(),
        Source::Reddit => db::get_all_filenames(&config.reddit_db_path)
            .unwrap_or_default()
            .into_iter()
            .collect(),
        Source::All => {
            let mut names: HashSet<String> = db::get_all_filenames(&config.wallhaven_db_path)
                .unwrap_or_default()
                .into_iter()
                .collect();
            names.extend(db::get_all_filenames(&config.reddit_db_path).unwrap_or_default());
            names
        }
    };

    let mut entries: Vec<FileEntry> = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&path) {
        for entry in read_dir.flatten() {
            let file_path = entry.path();
            if file_path.is_file() && downloader::file_is_image(&file_path) {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_orphan = !db_names.contains(&name);
                entries.push(FileEntry {
                    name,
                    path: file_path.to_string_lossy().to_string(),
                    size: entry.metadata().map_or(0, |m| m.len()),
                    is_orphan,
                });
            }
        }
    }

    entries.sort_by(|a, b| {
        a.is_orphan
            .cmp(&b.is_orphan)
            .reverse()
            .then(b.name.cmp(&a.name))
    });
    let total = entries.len();

    let page_start = offset.min(total);
    let page_end = (page_start + limit).min(total);
    let images = entries[page_start..page_end]
        .iter()
        .map(|e| LocalImageEntry {
            name: e.name.clone(),
            path: e.path.clone(),
            thumb_path: None,
            size: e.size,
            is_orphan: e.is_orphan,
        })
        .collect();

    {
        if let Ok(mut cache) = state.file_cache.lock() {
            *cache = Some(FileListCache {
                total,
                source: source.to_string(),
                dir_path: dir,
                items: entries,
                cached_at: Instant::now(),
            });
        }
    }

    Ok(LocalImageList { images, total })
}

#[tauri::command]
pub async fn resolve_thumbnail(
    state: tauri::State<'_, AppState>,
    source: Source,
    filename: String,
    dpr: Option<u32>,
) -> Result<String, AppError> {
    let dpr = dpr.unwrap_or(1).max(1);
    log::info!(
        "[CMD] resolve_thumbnail: source={:?}, file={}, dpr={}",
        source,
        filename,
        dpr
    );
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source);
    let thumb_dir = config.thumb_dir_for(source);
    let image_dir = PathBuf::from(&save_dir);

    let result = thumbnail::resolve_thumb_path(&thumb_dir, &image_dir, &filename, dpr);
    match result {
        Ok(thumb_path) => Ok(thumb_path.to_string_lossy().to_string()),
        Err(_) => Ok(image_dir.join(&filename).to_string_lossy().to_string()),
    }
}

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
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source);
    let thumb_dir = config.thumb_dir_for(source);
    let image_dir = PathBuf::from(&save_dir);

    let batch_result = thumbnail::ensure_batch_thumbnails(&thumb_dir, &image_dir, &filenames, dpr);
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
pub async fn delete_image(
    state: tauri::State<'_, AppState>,
    source: Source,
    name: String,
) -> Result<bool, AppError> {
    log::info!("[CMD] delete_image: source={:?}, name={}", source, name);
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source);
    let db_path = config.db_path_for(source);
    let thumb_dir = config.thumb_dir_for(source);

    let marked = db::mark_dislike_by_name(db_path, &name)?;

    let file_path = std::path::Path::new(&save_dir).join(&name);
    if file_path.exists() {
        if let Err(e) = std::fs::remove_file(&file_path) {
            log::warn!("[delete_image] 删除文件失败 {}: {e}", file_path.display());
        }
    }

    thumbnail::remove_thumbnails(&thumb_dir, &name);

    Ok(marked)
}

#[tauri::command]
pub async fn dislike_file(
    state: tauri::State<'_, AppState>,
    source: Source,
    name: String,
) -> Result<bool, AppError> {
    log::info!("[CMD] dislike_file: source={:?}, name={}", source, name);
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source);
    let db_path = config.db_path_for(source);
    let thumb_dir = config.thumb_dir_for(source);

    let db_ok = db::mark_dislike_by_name(db_path, &name)?;

    let file_path = std::path::Path::new(&save_dir).join(&name);
    if file_path.exists() {
        std::fs::remove_file(&file_path).map_err(|e| {
            log::error!("[dislike_file] 删除文件失败 {}: {}", file_path.display(), e);
            AppError::Io(e)
        })?;
    }

    thumbnail::remove_thumbnails(&thumb_dir, &name);

    Ok(db_ok)
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
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source);
    let thumb_dir = config.thumb_dir_for(source);

    let file_path = std::path::Path::new(&save_dir).join(&name);
    let existed = file_path.exists();
    if existed {
        std::fs::remove_file(&file_path).map_err(|e| {
            log::error!(
                "[delete_orphan_file] 删除文件失败 {}: {}",
                file_path.display(),
                e
            );
            AppError::Io(e)
        })?;
    }

    thumbnail::remove_thumbnails(&thumb_dir, &name);

    Ok(existed)
}

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
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source);
    let db_path = config.db_path_for(source);

    let mut wallhaven_batch: Vec<(String, String, String, String, String, String)> = Vec::new();
    let mut reddit_batch: Vec<(String, String, String, String, String)> = Vec::new();

    for name in &names {
        let file_path = std::path::Path::new(&save_dir).join(name);
        if !file_path.is_file() {
            log::warn!(
                "[adopt_orphan_files] file not found: {}",
                file_path.display()
            );
            continue;
        }
        let bytes = std::fs::read(&file_path).map_err(AppError::Io)?;
        let hash = downloader::compute_md5(&bytes);

        if source.is_wallhaven() {
            let wallhaven_id = name
                .strip_prefix("wallhaven_")
                .and_then(|s| s.split('.').next())
                .unwrap_or("");
            wallhaven_batch.push((
                wallhaven_id.to_string(),
                name.clone(),
                hash,
                String::new(),
                String::new(),
                "unknown".to_string(),
            ));
        } else {
            reddit_batch.push((
                name.clone(),
                hash,
                String::new(),
                String::new(),
                String::new(),
            ));
        }
    }

    let added = if source.is_wallhaven() {
        db::insert_wallhaven_images_batch(db_path, &wallhaven_batch)?.0
    } else {
        db::insert_reddit_images_batch(db_path, &reddit_batch)?.0
    };

    log::info!("[adopt_orphan_files] done: added={}/{}", added, names.len());
    Ok(added)
}

#[tauri::command]
pub async fn clean_thumbnails(
    state: tauri::State<'_, AppState>,
) -> Result<CleanThumbnailsResult, AppError> {
    log::info!("[CMD] clean_thumbnails called");
    let config = crate::state::load_config(&state)?;
    let wh_thumb_dir = config.wallhaven_thumb_dir();
    let wh_cleaned =
        db::clean_stale_thumbnails(&wh_thumb_dir.to_string_lossy(), &config.wallhaven_save_dir);
    let rd_thumb_dir = config.reddit_thumb_dir();
    let rd_cleaned =
        db::clean_stale_thumbnails(&rd_thumb_dir.to_string_lossy(), &config.reddit_save_dir);
    Ok(CleanThumbnailsResult {
        wallhaven: wh_cleaned,
        reddit: rd_cleaned,
    })
}
