//! Gallery commands: browse_image_files, resolve_thumbnail(s), clean_thumbnails,
//! delete_image, dislike_file, delete_orphan_file, adopt_orphan_files.

use crate::config::Source;
use crate::db;
use crate::downloader;
use crate::state::{self, AppError, AppState, FileEntry, FileListCache};
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

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_filtered_image_paths(
    state: tauri::State<'_, AppState>,
    source: Source,
    search: Option<String>,
    sort_by: Option<String>,
) -> Result<Vec<String>, AppError> {
    log::info!(
        "[CMD] list_filtered_image_paths: source={:?}, search={:?}, sort_by={:?}",
        source,
        search,
        sort_by
    );
    // 复用 browse_image_files 的扫描/筛选/缓存逻辑，但只把路径返回给轮播使用，
    // 避免把整页 LocalImageEntry（含大小、时间、孤儿标记）序列化到前端。
    let list = browse_image_files(state, source, 0, usize::MAX, None, search, sort_by).await?;
    Ok(list.images.into_iter().map(|img| img.path).collect())
}

#[tauri::command]
pub async fn browse_image_files(
    state: tauri::State<'_, AppState>,
    source: Source,
    offset: usize,
    limit: usize,
    custom_dir: Option<String>,
    search: Option<String>,
    sort_by: Option<String>,
) -> Result<LocalImageList, AppError> {
    log::info!(
        "[CMD] browse_image_files: source={:?}, offset={}, limit={}, custom_dir={:?}, search={:?}, sort_by={:?}",
        source,
        offset,
        limit,
        custom_dir,
        search,
        sort_by
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

    let search_query = search.unwrap_or_default().trim().to_lowercase();
    let sort = sort_by.unwrap_or_else(|| "default".to_string());

    {
        if let Ok(mut cache) = state.file_cache.lock() {
            if let Some(ref mut cached) = *cache {
                let src_str = source.to_string();
                // 目录 mtime 未变时直接复用；同时保留 5 分钟兜底刷新，覆盖“覆盖写文件但目录 mtime 不变”的场景。
                let current_modified = path.metadata().and_then(|m| m.modified()).ok();
                let fresh = cached.dir_modified == current_modified
                    && cached.cached_at.elapsed().as_secs() < 300;
                if cached.source == src_str && cached.dir_path == dir && fresh {
                    // 快路径：无搜索且排序键与缓存一致 → 直接切片，O(limit)，
                    // 不必每翻一页就对全部条目重排一次。
                    if search_query.is_empty() && cached.sorted_by == sort {
                        return Ok(slice_page(&cached.items, offset, limit));
                    }
                    // 慢路径：过滤 + 排序。缓存里始终是全量条目，搜索词不会污染它。
                    let list = page_from_cache(&cached.items, &search_query, &sort, offset, limit);
                    // 无搜索说明只是排序键变了：把重排结果写回缓存，后续翻页就能走快路径。
                    if search_query.is_empty() {
                        let mut reordered = cached.items.to_vec();
                        apply_sort(&mut reordered, &sort);
                        cached.items = reordered.into();
                        cached.sorted_by = sort.clone();
                    }
                    return Ok(list);
                }
            }
        }
    }

    // 目录扫描 + SQLite 查询都是阻塞操作，放到 spawn_blocking，避免卡住 Tauri async runtime。
    let wh_db_path = config.wallhaven_db_path.clone();
    let rd_db_path = config.reddit_db_path.clone();
    let scan_path = path.clone();
    // 闭包是 move 的：scan_sort 进闭包，缓存字段需要另一份副本。
    let scan_sort = sort.clone();
    let scan_sort_cached = sort.clone();
    let (entries, images, dir_modified) = tokio::task::spawn_blocking(move || {
        let db_names: HashSet<String> = match source {
            Source::Wallhaven => db::get_all_filenames(&wh_db_path)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            Source::Reddit => db::get_all_filenames(&rd_db_path)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            Source::All => {
                let mut names: HashSet<String> = db::get_all_filenames(&wh_db_path)
                    .unwrap_or_default()
                    .into_iter()
                    .collect();
                names.extend(db::get_all_filenames(&rd_db_path).unwrap_or_default());
                names
            }
        };

        let mut entries: Vec<FileEntry> = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&scan_path) {
            for entry in read_dir.flatten() {
                let file_path = entry.path();
                if file_path.is_file() && downloader::file_is_image(&file_path) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // 注意：这里**不能**按搜索词过滤。缓存存的是扫描结果，一旦存了子集，
                    // 用户清空搜索框后（目录 mtime 未变、仍在 5 分钟新鲜期内）图库就会
                    // 凭空少图。搜索统一在 page_from_cache 里对全量条目应用。
                    let metadata = entry.metadata().ok();
                    let is_orphan = !db_names.contains(&name);
                    entries.push(FileEntry {
                        name,
                        path: file_path.to_string_lossy().to_string(),
                        size: metadata.as_ref().map_or(0, |m| m.len()),
                        is_orphan,
                        modified: metadata.and_then(|m| m.modified().ok()),
                    });
                }
            }
        }

        apply_sort(&mut entries, &scan_sort);
        let total = entries.len();
        let page_start = offset.min(total);
        let page_end = page_start.saturating_add(limit).min(total);
        let images = entries[page_start..page_end]
            .iter()
            .map(file_entry_to_image)
            .collect();
        let dir_modified = scan_path.metadata().and_then(|m| m.modified()).ok();
        (entries, LocalImageList { images, total }, dir_modified)
    })
    .await
    .map_err(|e| AppError::Other(format!("图库扫描任务异常: {e}")))?;

    {
        if let Ok(mut cache) = state.file_cache.lock() {
            *cache = Some(FileListCache {
                source: source.to_string(),
                dir_path: dir,
                items: entries.into(),
                cached_at: Instant::now(),
                dir_modified,
                sorted_by: scan_sort_cached,
            });
        }
    }

    Ok(images)
}

fn page_from_cache(
    items: &[FileEntry],
    search_query: &str,
    sort_by: &str,
    offset: usize,
    limit: usize,
) -> LocalImageList {
    let mut indices: Vec<usize> = (0..items.len())
        .filter(|&i| search_query.is_empty() || items[i].name.to_lowercase().contains(search_query))
        .collect();
    indices.sort_by(|&a, &b| entry_cmp(&items[a], &items[b], sort_by));

    let total = indices.len();
    let page_start = offset.min(total);
    let page_end = page_start.saturating_add(limit).min(total);
    let images = indices[page_start..page_end]
        .iter()
        .map(|&i| file_entry_to_image(&items[i]))
        .collect();
    LocalImageList { images, total }
}

/// 条目已按请求顺序排好且无搜索过滤时，直接切片分页：O(limit)，不触碰其余条目。
fn slice_page(items: &[FileEntry], offset: usize, limit: usize) -> LocalImageList {
    let total = items.len();
    let page_start = offset.min(total);
    let page_end = page_start.saturating_add(limit).min(total);
    LocalImageList {
        images: items[page_start..page_end]
            .iter()
            .map(file_entry_to_image)
            .collect(),
        total,
    }
}

/// 把 Unix 时间戳（秒，UTC）格式化为 `YYYY-MM-DD HH:MM:SS`。
///
/// 旧实现按 `days/365`、`(days%365)/30`、`days%30` 硬算年月日，完全忽略闰年与真实
/// 月长，误差最大 ±26 天且逐年漂移（例如 2026-08-29 会显示成 2026-09-24）。
/// 这里改用精确的 civil-from-days 换算。
fn format_timestamp(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86400) as i64);
    let rem = secs % 86400;
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 「1970-01-01 起的天数」→ (年, 月, 日)。
/// Howard Hinnant 的 `civil_from_days`（`days_from_civil` 的逆运算），正确处理闰年与
/// 400 年周期。除法语义与 C++ 版一致（Rust 整数除法同样向零取整）。
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn entry_cmp(a: &FileEntry, b: &FileEntry, sort_by: &str) -> std::cmp::Ordering {
    match sort_by {
        "name_asc" => a.name.cmp(&b.name),
        "name_desc" => b.name.cmp(&a.name),
        "size_asc" => a.size.cmp(&b.size),
        "size_desc" => b.size.cmp(&a.size),
        "date_desc" => b.modified.cmp(&a.modified),
        "date_asc" => a.modified.cmp(&b.modified),
        _ => {
            // default: orphans first, then by name desc
            a.is_orphan
                .cmp(&b.is_orphan)
                .reverse()
                .then(b.name.cmp(&a.name))
        }
    }
}

fn apply_sort(entries: &mut [FileEntry], sort_by: &str) {
    entries.sort_by(|a, b| entry_cmp(a, b, sort_by));
}

fn file_entry_to_image(e: &FileEntry) -> LocalImageEntry {
    let modified_date = e
        .modified
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format_timestamp(d.as_secs()));
    LocalImageEntry {
        name: e.name.clone(),
        path: e.path.clone(),
        thumb_path: None,
        size: e.size,
        is_orphan: e.is_orphan,
        modified_date,
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
    // 文件名只能是一个普通文件名，拒绝任何 IPC 传入的路径穿越。
    let mut seen = HashSet::new();
    let mut safe_filenames = Vec::with_capacity(filenames.len());
    for name in filenames {
        state::ensure_plain_filename(&name)?;
        if seen.insert(name.clone()) {
            safe_filenames.push(name);
        }
    }

    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);
    let image_dir = PathBuf::from(&save_dir);

    // 批量缩略图包含图片解码/缩放等 CPU 密集操作，放到阻塞线程池。
    let batch_result = tokio::task::spawn_blocking(move || {
        thumbnail::ensure_batch_thumbnails(&thumb_dir, &image_dir, &safe_filenames, dpr)
    })
    .await
    .map_err(|e| AppError::Other(format!("缩略图任务异常: {e}")))?;

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
pub async fn dislike_file(
    state: tauri::State<'_, AppState>,
    source: Source,
    name: String,
) -> Result<bool, AppError> {
    log::info!("[CMD] dislike_file: source={:?}, name={}", source, name);
    state::ensure_plain_filename(&name)?;
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let db_path = config.db_path_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);

    // 数据库写入与文件删除都是阻塞操作，与批量版 dislike_files 保持一致放进阻塞线程池，
    // 避免卡住 Tauri 的 async runtime。
    tokio::task::spawn_blocking(move || {
        let db_ok = db::mark_dislike_by_name(&db_path, &name)?;

        let file_path = state::safe_join(std::path::Path::new(&save_dir), &name)?;
        if file_path.exists() {
            std::fs::remove_file(&file_path).map_err(|e| {
                log::error!("[dislike_file] 删除文件失败 {}: {}", file_path.display(), e);
                AppError::Io(e)
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
        state::ensure_plain_filename(name)?;
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
        for (name, file_path) in state::safe_join_all(std::path::Path::new(&save_dir), &names) {
            if file_path.exists() {
                if let Err(e) = std::fs::remove_file(&file_path) {
                    log::error!(
                        "[dislike_files] 删除文件失败 {}: {}",
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
    state::ensure_plain_filename(&name)?;
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);

    // 与批量版 delete_orphan_files 保持一致，文件 IO 放进阻塞线程池。
    tokio::task::spawn_blocking(move || {
        let file_path = state::safe_join(std::path::Path::new(&save_dir), &name)?;
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
        state::ensure_plain_filename(name)?;
    }
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let thumb_dir = config.thumb_dir_for(source);

    tokio::task::spawn_blocking(move || {
        let mut removed = 0u64;
        // 同 dislike_files：base 只解析一次，单个失败只跳过，不中断整批。
        for (name, file_path) in state::safe_join_all(std::path::Path::new(&save_dir), &names) {
            if file_path.exists() {
                if let Err(e) = std::fs::remove_file(&file_path) {
                    log::error!(
                        "[delete_orphan_files] 删除文件失败 {}: {}",
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
        state::ensure_plain_filename(name)?;
    }
    let config = crate::state::load_config(&state)?;
    let save_dir = config.save_dir_for(source).to_string();
    let db_path = config.db_path_for(source).to_string();

    // 收养孤儿需要读取整张图片并计算 MD5，属于阻塞 IO/CPU 操作。
    tokio::task::spawn_blocking(move || {
        let mut wallhaven_batch: Vec<(String, String, String, String, String, String)> = Vec::new();
        let mut reddit_batch: Vec<(String, String, String, String, String)> = Vec::new();

        for (name, file_path) in state::safe_join_all(std::path::Path::new(&save_dir), &names) {
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
                    String::new(),
                    String::new(),
                    "unknown".to_string(),
                ));
            } else {
                reddit_batch.push((
                    name.to_string(),
                    hash,
                    String::new(),
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

#[tauri::command]
pub async fn clean_thumbnails(
    state: tauri::State<'_, AppState>,
) -> Result<CleanThumbnailsResult, AppError> {
    log::info!("[CMD] clean_thumbnails called");
    let config = crate::state::load_config(&state)?;
    let wh_thumb_dir = config.wallhaven_thumb_dir().to_string_lossy().to_string();
    let rd_thumb_dir = config.reddit_thumb_dir().to_string_lossy().to_string();
    let wh_save_dir = config.wallhaven_save_dir.clone();
    let rd_save_dir = config.reddit_save_dir.clone();

    tokio::task::spawn_blocking(move || {
        let wallhaven = db::clean_stale_thumbnails(&wh_thumb_dir, &wh_save_dir);
        let reddit = db::clean_stale_thumbnails(&rd_thumb_dir, &rd_save_dir);
        CleanThumbnailsResult { wallhaven, reddit }
    })
    .await
    .map_err(|e| AppError::Other(format!("清理缩略图任务异常: {e}")))
}

#[tauri::command]
pub async fn get_image_info(
    state: tauri::State<'_, AppState>,
    source: Source,
    name: String,
) -> Result<ImageInfo, AppError> {
    log::info!("[CMD] get_image_info: source={:?}, name={}", source, name);
    state::ensure_plain_filename(&name)?;
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
                match db::get_wallhaven_image_by_name(&wh, &lookup_name) {
                    Ok(Some(rec)) => Ok(Some(rec)),
                    _ => db::get_reddit_image_by_name(&rd, &lookup_name),
                }
            })
            .await
            .map_err(|e| AppError::Other(format!("数据库查询任务异常: {e}")))?
            .map_err(AppError::Db)?
        }
    };

    let save_dir = if db_record.as_ref().is_some_and(|r| r.source == "wallhaven") {
        config.wallhaven_save_dir.clone()
    } else {
        config.save_dir_for(source).to_string()
    };
    let file_path = state::safe_join(std::path::Path::new(&save_dir), &name)?;
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
