//! 图片记录的增删改查：存在性检查、批量入库、分页列表、缺失列表。

#[cfg(test)]
use super::open;
use super::stats::existing_file_names;
use super::{db_exists, invalidate_stats, with_cached_connection, ImageRecord};
use rusqlite::Result as SqlResult;

pub fn get_existing_wallhaven_ids(db_path: &str) -> SqlResult<Vec<String>> {
    let ids = with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare("SELECT wallhaven_id FROM images")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<SqlResult<Vec<String>>>()?;
        Ok(ids)
    })?;
    log::info!(
        "[DB] get_existing_wallhaven_ids: {} ids from {}",
        ids.len(),
        db_path
    );
    Ok(ids)
}

pub fn get_existing_reddit_urls(db_path: &str) -> SqlResult<Vec<String>> {
    let urls = with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare("SELECT url FROM images")?;
        let urls = stmt
            .query_map([], |row| row.get(0))?
            .collect::<SqlResult<Vec<String>>>()?;
        Ok(urls)
    })?;
    log::info!(
        "[DB] get_existing_reddit_urls: {} urls from {}",
        urls.len(),
        db_path
    );
    Ok(urls)
}

#[cfg(test)]
pub fn insert_wallhaven_image(
    db_path: &str,
    wallhaven_id: &str,
    name: &str,
    hash: &str,
    url: &str,
    source_url: &str,
    resolution: &str,
) -> SqlResult<bool> {
    let conn = open(db_path)?;
    let result = match conn.execute(
        "INSERT INTO images (wallhaven_id, name, hash, url, source_url, resolution) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![wallhaven_id, name, hash, url, source_url, resolution],
    ) {
        Ok(_) => true,
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            log::warn!("[DB] insert_wallhaven_image: duplicate id={} name={}", wallhaven_id, name);
            false
        }
        Err(e) => return Err(e),
    };
    if result {
        log::info!(
            "[DB] insert_wallhaven_image: id={} name={}",
            wallhaven_id,
            name
        );
        invalidate_stats(db_path);
    }
    Ok(result)
}

#[cfg(test)]
pub fn insert_reddit_image(
    db_path: &str,
    name: &str,
    hash: &str,
    url: &str,
    title: &str,
    permalink: &str,
) -> SqlResult<bool> {
    let conn = open(db_path)?;
    let result = match conn.execute(
        "INSERT INTO images (name, hash, url, title, permalink) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![name, hash, url, title, permalink],
    ) {
        Ok(_) => true,
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            log::warn!("[DB] insert_reddit_image: duplicate url={}", url);
            false
        }
        Err(e) => return Err(e),
    };
    if result {
        log::info!("[DB] insert_reddit_image: name={}", name);
        invalidate_stats(db_path);
    }
    Ok(result)
}

pub fn insert_wallhaven_images_batch(
    db_path: &str,
    images: &[(String, String, String, String, String, String)],
) -> SqlResult<(u64, u64)> {
    let (added, skipped, _) = insert_wallhaven_images_batch_detailed(db_path, images)?;
    Ok((added, skipped))
}

/// 批量插入并返回真正新增的 name 列表，供下载任务精确发 `image-downloaded` 事件。
pub fn insert_wallhaven_images_batch_detailed(
    db_path: &str,
    images: &[(String, String, String, String, String, String)],
) -> SqlResult<(u64, u64, Vec<String>)> {
    let result = with_cached_connection(db_path, |conn| {
        let tx = conn.transaction()?;
        let mut added = 0u64;
        let mut skipped = 0u64;
        let mut added_names = Vec::new();
        {
            // prepare_cached：循环内复用已编译语句，避免每条记录重新 prepare 一遍。
            let mut stmt = tx.prepare_cached(
                "INSERT INTO images (wallhaven_id, name, hash, url, source_url, resolution) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (wallhaven_id, name, hash, url, source_url, resolution) in images {
                match stmt.execute(rusqlite::params![
                    wallhaven_id,
                    name,
                    hash,
                    url,
                    source_url,
                    resolution
                ]) {
                    Ok(_) => {
                        added += 1;
                        added_names.push(name.clone());
                    }
                    Err(rusqlite::Error::SqliteFailure(err, _))
                        if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        skipped += 1;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        tx.commit()?;
        Ok((added, skipped, added_names))
    })?;
    if result.0 > 0 {
        invalidate_stats(db_path);
    }
    log::info!(
        "[DB] insert_wallhaven_images_batch: added={} skipped={}",
        result.0,
        result.1
    );
    Ok(result)
}

pub fn insert_reddit_images_batch(
    db_path: &str,
    images: &[(String, String, String, String, String)],
) -> SqlResult<(u64, u64)> {
    let (added, skipped, _) = insert_reddit_images_batch_detailed(db_path, images)?;
    Ok((added, skipped))
}

/// 批量插入并返回真正新增的 name 列表，供下载任务精确发 `image-downloaded` 事件。
pub fn insert_reddit_images_batch_detailed(
    db_path: &str,
    images: &[(String, String, String, String, String)],
) -> SqlResult<(u64, u64, Vec<String>)> {
    let result = with_cached_connection(db_path, |conn| {
        let tx = conn.transaction()?;
        let mut added = 0u64;
        let mut skipped = 0u64;
        let mut added_names = Vec::new();
        {
            // prepare_cached：循环内复用已编译语句，避免每条记录重新 prepare 一遍。
            let mut stmt = tx.prepare_cached(
                "INSERT INTO images (name, hash, url, title, permalink) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (name, hash, url, title, permalink) in images {
                match stmt.execute(rusqlite::params![name, hash, url, title, permalink]) {
                    Ok(_) => {
                        added += 1;
                        added_names.push(name.clone());
                    }
                    Err(rusqlite::Error::SqliteFailure(err, _))
                        if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        skipped += 1;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        tx.commit()?;
        Ok((added, skipped, added_names))
    })?;
    if result.0 > 0 {
        invalidate_stats(db_path);
    }
    log::info!(
        "[DB] insert_reddit_images_batch: added={} skipped={}",
        result.0,
        result.1
    );
    Ok(result)
}

pub fn mark_dislike_by_name(db_path: &str, name: &str) -> SqlResult<bool> {
    let count = with_cached_connection(db_path, |conn| {
        conn.execute(
            "UPDATE images SET love = 0 WHERE name = ?1",
            rusqlite::params![name],
        )
    })?;
    if count > 0 {
        log::info!("[DB] mark_dislike_by_name: name={}", name);
        invalidate_stats(db_path);
    }
    Ok(count > 0)
}

/// 批量将图片标记为不喜欢（love=0）。用于图库批量删除，避免逐张打开连接。
pub fn mark_dislike_by_names(db_path: &str, names: &[String]) -> SqlResult<u64> {
    let count = with_cached_connection(db_path, |conn| {
        let tx = conn.transaction()?;
        let mut count = 0u64;
        {
            let mut stmt = tx.prepare("UPDATE images SET love = 0 WHERE name = ?1")?;
            for name in names {
                count += stmt.execute(rusqlite::params![name])? as u64;
            }
        }
        tx.commit()?;
        Ok(count)
    })?;
    if count > 0 {
        invalidate_stats(db_path);
    }
    log::info!(
        "[DB] mark_dislike_by_names: marked={}/{}",
        count,
        names.len()
    );
    Ok(count)
}

/// 按文件名批量删除记录，返回真正删掉的行数。
///
/// 给「缺失文件」用的硬删除：原文件已不在磁盘，记录除了占着缺失列表之外没有别的价值。
/// 与 `mark_dislike_by_names` 的区别是**不可撤销**——标记为不喜欢还能用
/// 「恢复所有已标记」撤销，删除要恢复只能重新入库，所以调用方必须先二次确认。
///
/// 不存在于库中的名字会被静默跳过（不报错），这样按来源分组批量调用时
/// 不需要先过滤（例如合并来源里混着另一个库的名字）。
pub fn delete_records_by_names(db_path: &str, names: &[String]) -> SqlResult<u64> {
    let count = with_cached_connection(db_path, |conn| {
        let tx = conn.transaction()?;
        let mut count = 0u64;
        {
            // 复用同一条 prepared statement：批量删除的瓶颈在 WAL 写入而不是解析。
            let mut stmt = tx.prepare("DELETE FROM images WHERE name = ?1")?;
            for name in names {
                count += stmt.execute(rusqlite::params![name])? as u64;
            }
        }
        tx.commit()?;
        Ok(count)
    })?;
    if count > 0 {
        invalidate_stats(db_path);
    }
    log::info!(
        "[DB] delete_records_by_names: deleted={}/{}",
        count,
        names.len()
    );
    Ok(count)
}

pub fn get_wallhaven_images(db_path: &str, limit: i64, offset: i64) -> SqlResult<Vec<ImageRecord>> {
    with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, hash, url, COALESCE(source_url, ''), COALESCE(resolution, 'unknown'), COALESCE(love, 1), COALESCE(created_at, '') FROM images ORDER BY created_at DESC, id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let images = stmt
            .query_map(rusqlite::params![limit, offset], |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hash: row.get(2)?,
                    url: row.get(3)?,
                    source_url: row.get(4)?,
                    resolution: row.get(5)?,
                    title: None,
                    permalink: None,
                    love: row.get(6)?,
                    created_at: row.get(7)?,
                    source: "wallhaven".to_string(),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(images)
    })
}

pub fn get_reddit_images(db_path: &str, limit: i64, offset: i64) -> SqlResult<Vec<ImageRecord>> {
    with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, hash, url, COALESCE(title, ''), COALESCE(permalink, ''), COALESCE(love, 1), COALESCE(created_at, '') FROM images ORDER BY created_at DESC, id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let images = stmt
            .query_map(rusqlite::params![limit, offset], |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hash: row.get(2)?,
                    url: row.get(3)?,
                    source_url: String::new(),
                    resolution: String::new(),
                    title: row.get(4).ok(),
                    permalink: row.get(5).ok(),
                    love: row.get(6)?,
                    created_at: row.get(7)?,
                    source: "reddit".to_string(),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(images)
    })
}

pub fn get_wallhaven_missing_love(db_path: &str) -> SqlResult<Vec<ImageRecord>> {
    with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, hash, url, COALESCE(source_url, ''), COALESCE(resolution, 'unknown'), COALESCE(love, 1), COALESCE(created_at, '') FROM images WHERE love = 1",
        )?;
        let images = stmt
            .query_map([], |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hash: row.get(2)?,
                    url: row.get(3)?,
                    source_url: row.get(4)?,
                    resolution: row.get(5)?,
                    title: None,
                    permalink: None,
                    love: row.get(6)?,
                    created_at: row.get(7)?,
                    source: "wallhaven".to_string(),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(images)
    })
}

/// 单库分页取数的函数指针类型：两个库的列集不同，但排序键一致，
/// 所以归并层可以只依赖「返回的 Vec 已按 (created_at DESC, id DESC) 有序」这一个约定。
type PagedFetch = fn(&str, i64, i64) -> SqlResult<Vec<ImageRecord>>;

/// 归并用的全序比较：`created_at` 降序 → `id` 降序 → `source` 降序。
///
/// 第三项不是装饰品：两个库的 `id` 都从 1 自增、`created_at` 只精确到秒，所以
/// `(created_at, id)` 在两库之间完全可能并列（同一秒里两个库各插了一条 id=5）。
/// 并列行的先后如果不定，翻页边界上同一行会出现在相邻两页、或者被整段跳过。
/// 加一个在两个库里取值必定不同的排序列，顺序才是确定的。
fn merge_order(a: &ImageRecord, b: &ImageRecord) -> std::cmp::Ordering {
    b.created_at
        .cmp(&a.created_at)
        .then_with(|| b.id.cmp(&a.id))
        .then_with(|| b.source.cmp(&a.source))
}

/// 单库的有序游标：按需分块取数，只在缓冲用尽时再查下一块。
///
/// 这样深翻页时不需要一次性把 `offset + limit` 条读进内存（那样等于把旧实现的
/// 内存问题换个地方重现），内存只与块大小和本页条数相关。
struct SideCursor {
    db_path: String,
    fetch: PagedFetch,
    buf: Vec<ImageRecord>,
    pos: usize,
    /// 已消费条数，作为下一块的 OFFSET
    consumed: i64,
    exhausted: bool,
}

impl SideCursor {
    fn new(db_path: &str, fetch: PagedFetch) -> Self {
        Self {
            db_path: db_path.to_string(),
            fetch,
            buf: Vec::new(),
            pos: 0,
            consumed: 0,
            exhausted: false,
        }
    }

    fn refill(&mut self, chunk: i64) -> SqlResult<()> {
        // 先取到局部变量再赋值，避免在同一个语句里既借用 self.db_path 又写 self.buf。
        let next = (self.fetch)(&self.db_path, chunk, self.consumed)?;
        self.buf = next;
        self.pos = 0;
        if self.buf.is_empty() {
            self.exhausted = true;
        }
        Ok(())
    }

    /// 当前指向的条目；缓冲空了就先补一块。
    fn peek(&mut self, chunk: i64) -> SqlResult<Option<&ImageRecord>> {
        if self.pos >= self.buf.len() {
            if self.exhausted {
                return Ok(None);
            }
            self.refill(chunk)?;
        }
        Ok(self.buf.get(self.pos))
    }

    /// 只推进游标，不做深拷贝。
    fn advance(&mut self) {
        self.pos += 1;
        self.consumed += 1;
    }
}

/// 跨两个库合并分页（`Source::All`）。
///
/// **不要改回 `ATTACH` + `UNION ALL` + `ORDER BY` + `LIMIT`。** 那样写虽然只有一条 SQL，
/// 但两个库是独立文件，SQLite 无法把索引用于跨库的 `ORDER BY`，只能对**两侧各建一棵
/// 临时 B-Tree 做全量排序**（`EXPLAIN QUERY PLAN` 实测：两侧都是 `SCAN` +
/// `USE TEMP B-TREE FOR ORDER BY`，`idx_images_created_at` 完全没被用上）。
/// 结果是每次翻页的代价与 `offset` 无关地都是 O(n log n)，并且随库增长超线性：
/// 实测 5 万 + 5 万行时首页 38ms、`offset` 25000 要 **1362ms**。
///
/// 现在改成：两个库各自 `ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?`（走索引，
/// `EXPLAIN` 为 `SCAN ... USING INDEX idx_images_created_at`，无临时 B-Tree），
/// 然后流式归并两条已有序的流，再切出 `[offset, offset + limit)`。
/// 同样的数据量下深翻页只要几毫秒（实测 0.4–6.5ms），内存只与块大小和本页条数相关。
///
/// `reddit_db_path` 为空或文件不存在时只返回 wallhaven 的结果。
pub fn get_all_images_paged(
    wallhaven_db_path: &str,
    reddit_db_path: &str,
    limit: i64,
    offset: i64,
) -> SqlResult<Vec<ImageRecord>> {
    // reddit 库不可用 → 退化为单库查询，语义与"只有 wallhaven 有数据"一致。
    if reddit_db_path.is_empty() || !db_exists(reddit_db_path) {
        return get_wallhaven_images(wallhaven_db_path, limit, offset);
    }
    if limit <= 0 || offset < 0 {
        return Ok(Vec::new());
    }

    let chunk = limit.max(64);
    let mut left = SideCursor::new(wallhaven_db_path, get_wallhaven_images);
    let mut right = SideCursor::new(reddit_db_path, get_reddit_images);
    let total = offset.saturating_add(limit);
    let mut out = Vec::with_capacity(limit as usize);

    // 归并两个有序流，取前 total 条里的第 [offset, offset+limit) 段。
    // offset 之前的条目只推进游标、不深拷贝——旧实现的"深翻页要把两万条读进内存"
    // 正是要避免的那件事。
    for index in 0..total {
        let take_left = {
            let l = left.peek(chunk)?;
            let r = right.peek(chunk)?;
            match (l, r) {
                // 相等时取左侧，归并结果才是确定的
                (Some(a), Some(b)) => merge_order(a, b) != std::cmp::Ordering::Greater,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            }
        };

        if index >= offset {
            let item = if take_left {
                left.peek(chunk)?.cloned()
            } else {
                right.peek(chunk)?.cloned()
            };
            match item {
                Some(record) => out.push(record),
                None => break,
            }
        }

        if take_left {
            left.advance();
        } else {
            right.advance();
        }
    }

    Ok(out)
}

pub fn get_wallhaven_missing_files(db_path: &str, save_dir: &str) -> SqlResult<Vec<ImageRecord>> {
    let existing = existing_file_names(save_dir);
    with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, hash, url, COALESCE(source_url, ''), COALESCE(resolution, 'unknown'), COALESCE(love, 1), COALESCE(created_at, '') FROM images WHERE love = 1 ORDER BY created_at DESC, id DESC",
        )?;
        let images = stmt
            .query_map([], |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hash: row.get(2)?,
                    url: row.get(3)?,
                    source_url: row.get(4)?,
                    resolution: row.get(5)?,
                    title: None,
                    permalink: None,
                    love: row.get(6)?,
                    created_at: row.get(7)?,
                    source: "wallhaven".to_string(),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?
            .into_iter()
            .filter(|img| !existing.contains_key(&img.name))
            .collect::<Vec<_>>();
        Ok(images)
    })
}

pub fn get_reddit_missing_files(db_path: &str, save_dir: &str) -> SqlResult<Vec<ImageRecord>> {
    let existing = existing_file_names(save_dir);
    with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, hash, url, COALESCE(title, ''), COALESCE(permalink, ''), COALESCE(love, 1), COALESCE(created_at, '') FROM images WHERE love = 1 ORDER BY created_at DESC, id DESC",
        )?;
        let images = stmt
            .query_map([], |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hash: row.get(2)?,
                    url: row.get(3)?,
                    source_url: String::new(),
                    resolution: String::new(),
                    title: row.get(4).ok(),
                    permalink: row.get(5).ok(),
                    love: row.get(6)?,
                    created_at: row.get(7)?,
                    source: "reddit".to_string(),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?
            .into_iter()
            .filter(|img| !existing.contains_key(&img.name))
            .collect::<Vec<_>>();
        Ok(images)
    })
}

pub fn get_all_filenames(db_path: &str) -> SqlResult<Vec<String>> {
    with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare("SELECT name FROM images")?;
        let names = stmt
            .query_map([], |row| row.get(0))?
            .collect::<SqlResult<Vec<String>>>()?;
        Ok(names)
    })
}

/// Query a single image record by filename from the Wallhaven DB.
pub fn get_wallhaven_image_by_name(db_path: &str, name: &str) -> SqlResult<Option<ImageRecord>> {
    with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, hash, url, COALESCE(source_url, ''), COALESCE(resolution, 'unknown'), COALESCE(love, 1), COALESCE(created_at, '') FROM images WHERE name = ?1 LIMIT 1",
        )?;
        let mut images = stmt
            .query_map(rusqlite::params![name], |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hash: row.get(2)?,
                    url: row.get(3)?,
                    source_url: row.get(4)?,
                    resolution: row.get(5)?,
                    title: None,
                    permalink: None,
                    love: row.get(6)?,
                    created_at: row.get(7)?,
                    source: "wallhaven".to_string(),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(images.pop())
    })
}

/// Query a single image record by filename from the Reddit DB.
pub fn get_reddit_image_by_name(db_path: &str, name: &str) -> SqlResult<Option<ImageRecord>> {
    with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, hash, url, COALESCE(title, ''), COALESCE(permalink, ''), COALESCE(love, 1), COALESCE(created_at, '') FROM images WHERE name = ?1 LIMIT 1",
        )?;
        let mut images = stmt
            .query_map(rusqlite::params![name], |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hash: row.get(2)?,
                    url: row.get(3)?,
                    source_url: String::new(),
                    resolution: String::new(),
                    title: row.get(4).ok(),
                    permalink: row.get(5).ok(),
                    love: row.get(6)?,
                    created_at: row.get(7)?,
                    source: "reddit".to_string(),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(images.pop())
    })
}

pub fn get_reddit_missing_love(db_path: &str) -> SqlResult<Vec<ImageRecord>> {
    with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, hash, url, COALESCE(title, ''), COALESCE(permalink, ''), COALESCE(love, 1), COALESCE(created_at, '') FROM images WHERE love = 1",
        )?;
        let images = stmt
            .query_map([], |row| {
                Ok(ImageRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hash: row.get(2)?,
                    url: row.get(3)?,
                    source_url: String::new(),
                    resolution: String::new(),
                    title: row.get(4).ok(),
                    permalink: row.get(5).ok(),
                    love: row.get(6)?,
                    created_at: row.get(7)?,
                    source: "reddit".to_string(),
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(images)
    })
}
