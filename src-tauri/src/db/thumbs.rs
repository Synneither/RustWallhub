//! 缩略图相关：反向解析缩略图名、清理失效缩略图。

use crate::downloader;
use crate::thumbnail::{MAX_DPR, THUMB_BASE_WIDTH, THUMB_ENCODING_TAG};
use std::collections::HashSet;
use std::path::Path;

/// 缩略图名里的编码版本标记形如 `q85`（`q` + 全数字）。
fn is_encoding_tag(tag: &str) -> bool {
    tag.strip_prefix('q')
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
}

/// 解析 `{stem}__w{width}[.{tag}].webp` 形式的缩略图名，返回
/// `(原图 stem, 档位宽度, 编码标记)`。
///
/// 更早的格式（缩略图名 == 原图名，如 `photo.jpg`）不匹配这个结构，返回 `None`。
fn parse_thumb_name(name: &str) -> Option<(&str, u32, Option<&str>)> {
    let rest = name.strip_suffix(".webp")?;
    // 剥掉可选的编码标记段（`.q85`）；无标记的是换有损编码之前的缓存。
    let (core, encoding) = match rest.rsplit_once('.') {
        Some((head, tag)) if is_encoding_tag(tag) => (head, Some(tag)),
        _ => (rest, None),
    };
    let (stem, width) = core.rsplit_once("__w")?;
    if stem.is_empty() || width.is_empty() || !width.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((stem, width.parse().ok()?, encoding))
}

/// 把缩略图名还原成它对应的原图文件名候选集合（用于判断缩略图是否还有效）。
///
/// 兼容三种命名：
/// - 旧格式：缩略图名 == 原图名
/// - 无编码标记：`stem__w480.webp`
/// - 带编码标记：`stem__w480.q85.webp`
fn candidate_source_names(thumb_name: &str) -> Vec<String> {
    let mut names = vec![thumb_name.to_string()];
    if let Some((stem, _width, _tag)) = parse_thumb_name(thumb_name) {
        names.extend(
            downloader::IMAGE_EXTENSIONS
                .iter()
                .map(|ext| format!("{stem}.{ext}")),
        );
    }
    names
}

/// 该缩略图是否由**当前**编码参数生成。
///
/// 不带标记（换有损编码之前的无损缓存）或标记对不上的都算过期：换编码后文件名变了，
/// 它们不会再被 `ensure_thumbnail_inner` 命中（那里查的是新名字），留着只会白占磁盘。
fn is_current_encoding(thumb_name: &str) -> bool {
    parse_thumb_name(thumb_name)
        .and_then(|(_, _, tag)| tag)
        .is_some_and(|tag| tag == THUMB_ENCODING_TAG)
}

/// 清理失效缩略图：原图已不存在的、以及编码版本过期的。
///
/// 判定改为对**保存目录的文件名快照**做集合查找：原来是每个缩略图最多 8 次
/// `Path::exists()`（1 次原名 + 7 种扩展名），几千张缩略图就是上万次 stat。
/// 现在整个保存目录只扫一次，判定变成纯内存查找。
pub fn clean_stale_thumbnails(thumbnail_dir: &str, save_dir: &str) -> u64 {
    clean_stale_thumbnails_keeping(thumbnail_dir, save_dir, None)
}

/// 同 [`clean_stale_thumbnails`]，另可指定「要保留的最低档位」。
///
/// `keep_min_dpr = Some(n)` 时，档位宽度低于 `240 * n` 的缩略图也会被清掉
/// （例：`thumbnail_dpr = 3` 时清掉 w240 / w480，只留 w720）。
///
/// 之所以做成**可选**而不是默认行为：实际档位取决于网格密度与屏幕像素比
/// （见前端 `thumbSize.ts` 的 `pickThumbDpr`，`thumbnail_dpr` 是「最低档位」而非固定档），
/// 用户调小网格或换显示器后可能需要别的档位。启动时的自动清理只做保守的
/// 「孤儿 + 过期编码」（传 `None`）；只有用户在设置页主动点「清理缩略图」时，
/// 才按当前最低档位做积极清理。
pub fn clean_stale_thumbnails_keeping(
    thumbnail_dir: &str,
    save_dir: &str,
    keep_min_dpr: Option<u32>,
) -> u64 {
    let thumb_dir_path = Path::new(thumbnail_dir);
    if !thumb_dir_path.is_dir() {
        return 0;
    }

    // 先取保存目录快照（带短缓存，与缺失/孤儿/统计共用同一次扫描）。
    let existing: HashSet<String> = super::dir_listing(save_dir).keys().cloned().collect();
    let min_width = keep_min_dpr.map(|d| THUMB_BASE_WIDTH * d.clamp(1, MAX_DPR));

    let mut cleaned = 0u64;
    if let Ok(entries) = std::fs::read_dir(thumb_dir_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !downloader::file_is_image(&path) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();

            let source_missing = !candidate_source_names(&name)
                .iter()
                .any(|candidate| existing.contains(candidate));
            let encoding_stale = !is_current_encoding(&name);
            let below_floor = match (min_width, parse_thumb_name(&name)) {
                (Some(min), Some((_, width, _))) => width < min,
                _ => false,
            };

            if source_missing || encoding_stale || below_floor {
                std::fs::remove_file(&path).ok();
                cleaned += 1;
            }
        }
    }
    log::info!(
        "[DB] clean_stale_thumbnails: cleaned={cleaned} (dir={thumbnail_dir}, keep_min_dpr={keep_min_dpr:?})"
    );
    cleaned
}
