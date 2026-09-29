//! 图库浏览：扫描保存目录、排序、分页。

use super::{LocalImageEntry, LocalImageList};
use crate::config::Source;
use crate::db;
use crate::downloader;
use crate::error::AppError;
use crate::state::{AppState, FileEntry, FileListCache};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

// 参数个数由前端 IPC 契约决定（source/offset/limit/custom_dir/search/sort_by + app/state），
// 拆结构体会同时改动前端调用点，收益不抵成本。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn browse_image_files(
    app: tauri::AppHandle,
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
        // 自定义浏览目录由用户当场选择，这时才授权给 asset 协议。
        // 静态 scope 不含任意路径，否则等于把整个用户目录暴露给前端图片加载。
        crate::state::allow_asset_dir(&app, custom);
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
        let mut skipped_non_utf8 = 0usize;
        if let Ok(read_dir) = std::fs::read_dir(&scan_path) {
            for entry in read_dir.flatten() {
                let file_path = entry.path();
                if !file_path.is_file() || !downloader::file_is_image(&file_path) {
                    continue;
                }
                // 非 UTF-8 文件名在 Linux 上是合法的，但本应用全链路用 String 传路径
                // （IPC、asset 授权、缩略图键），lossy 转换出来的路径根本打不开，
                // 列到图库里只会是一张点不动的裂图。这里直接跳过并记数，
                // 至少让日志能解释"目录里明明有图却少了几张"。
                let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                    skipped_non_utf8 += 1;
                    continue;
                };
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
        if skipped_non_utf8 > 0 {
            log::warn!(
                "[browse] 目录里有 {skipped_non_utf8} 个文件名不是合法 UTF-8，已跳过（当前版本不支持）"
            );
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
