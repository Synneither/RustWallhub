//! 数据库统计：总数、love=1 数量、缺失数量，以及"标记缺失"类写操作。
//!
//! 统计结果有短缓存（写入操作会主动失效），外部改动最多 10 秒后可见。

use super::{invalidate_stats, with_cached_connection, DbStats, STATS_CACHE};
use rusqlite::Result as SqlResult;
use std::collections::HashSet;
use std::time::Instant;

pub fn get_db_stats(db_path: &str, save_dir: &str) -> SqlResult<DbStats> {
    let key = (db_path.to_string(), save_dir.to_string());
    let current_modified = std::fs::metadata(save_dir).and_then(|m| m.modified()).ok();
    if let Some(cache) = STATS_CACHE.get() {
        if let Ok(cache) = cache.lock() {
            if let Some((cached_at, cached_modified, stats)) = cache.get(&key) {
                if *cached_modified == current_modified && cached_at.elapsed().as_secs() < 10 {
                    return Ok(stats.clone());
                }
            }
        }
    }

    let (total, love) = with_cached_connection(db_path, |conn| {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN love=1 THEN 1 ELSE 0 END), 0) FROM images",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    })?;
    let missing = count_missing(db_path, save_dir)? as i64;
    let stats = DbStats {
        total,
        love,
        dislike: missing,
    };
    if let Some(cache) = STATS_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.insert(key, (Instant::now(), current_modified, stats.clone()));
        }
    }
    log::info!(
        "[DB] get_db_stats({}): {:?} (missing by file existence)",
        db_path,
        stats
    );
    Ok(stats)
}

/// 一次性扫描保存目录，把目录里的所有文件名放入 HashSet。
/// 原来逐行 `Path::exists` 在大图库下会产生 N 次 syscall，这里改成 O(目录项) 的哈希查找。
pub(crate) fn existing_file_names(save_dir: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    if let Ok(entries) = std::fs::read_dir(save_dir) {
        for entry in entries.flatten() {
            names.insert(entry.file_name().to_string_lossy().to_string());
        }
    }
    names
}

fn mark_missing_dislike(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    log::info!("[DB] mark_missing_dislike: dir={}", save_dir);
    let existing = existing_file_names(save_dir);
    let updated = with_cached_connection(db_path, |conn| {
        let tx = conn.transaction()?;
        let rows: Vec<(i64, String)> = {
            let mut stmt = tx.prepare("SELECT id, name FROM images WHERE love = 1")?;
            let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            mapped.collect::<SqlResult<Vec<_>>>()?
        };
        let mut updated = 0u64;
        for (id, name) in rows {
            if !existing.contains(&name) {
                tx.execute(
                    "UPDATE images SET love = 0 WHERE id = ?1",
                    rusqlite::params![id],
                )?;
                updated += 1;
            }
        }
        tx.commit()?;
        Ok(updated)
    })?;
    if updated > 0 {
        invalidate_stats(db_path);
    }
    log::info!("[DB] mark_missing_dislike: updated={}", updated);
    Ok(updated)
}

pub(crate) fn count_missing(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    // 先在短锁内取出 DB 文件名，再在锁外扫描目录，避免文件扫描阻塞其他 DB 操作。
    let names = with_cached_connection(db_path, |conn| {
        let mut stmt = conn.prepare("SELECT name FROM images WHERE love = 1")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<SqlResult<Vec<_>>>()
    })?;
    let existing = existing_file_names(save_dir);
    let missing = names
        .iter()
        .filter(|name| !existing.contains(*name))
        .count() as u64;
    log::info!("[DB] count_missing: {} missing in {}", missing, save_dir);
    Ok(missing)
}

pub fn mark_missing_dislike_wallhaven(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    mark_missing_dislike(db_path, save_dir)
}

pub fn mark_missing_dislike_reddit(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    mark_missing_dislike(db_path, save_dir)
}

fn restore_love(db_path: &str) -> SqlResult<u64> {
    let count = with_cached_connection(db_path, |conn| {
        conn.execute("UPDATE images SET love = 1 WHERE love = 0", [])
    })?;
    if count > 0 {
        invalidate_stats(db_path);
    }
    log::info!("[DB] restore_love: restored={}", count);
    Ok(count as u64)
}

pub fn restore_love_db(db_path: &str) -> SqlResult<u64> {
    restore_love(db_path)
}
