//! 数据库统计：总数、love=1 数量、缺失数量，以及"标记缺失"类写操作。
//!
//! 统计结果有短缓存（写入操作会主动失效），外部改动最多 `STATS_CACHE_TTL_SECS` 秒后可见。

use super::{
    dir_listing, invalidate_stats, with_cached_connection, DbStats, DirListing, STATS_CACHE,
    STATS_CACHE_TTL_SECS,
};
use rusqlite::Result as SqlResult;
use std::sync::Arc;
use std::time::Instant;

/// 批量 UPDATE 时每条语句绑定的最大参数个数。
///
/// SQLite 默认 `SQLITE_MAX_VARIABLE_NUMBER` 为 999（新版 32766），
/// 250 既能一次覆盖绝大多数场景，又留足余量。
const SQL_PARAM_CHUNK: usize = 250;

/// 统计结果短缓存策略：**不依赖目录 mtime**。
///
/// 曾经用 `save_dir` 的 mtime 判断缓存是否新鲜，但下载是往 `save_dir` 的子目录里写文件，
/// Windows 上子目录写入不一定更新父目录 mtime，导致缓存频繁误判失效、每次统计都全量扫盘。
/// 现在只靠两条规则：TTL 到期，或写操作主动调用 `invalidate_stats`。
pub fn get_db_stats(db_path: &str, save_dir: &str) -> SqlResult<DbStats> {
    let key = (db_path.to_string(), save_dir.to_string());
    if let Some(cache) = STATS_CACHE.get() {
        if let Ok(cache) = cache.lock() {
            if let Some((cached_at, stats)) = cache.get(&key) {
                if cached_at.elapsed().as_secs() < STATS_CACHE_TTL_SECS {
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
            cache.insert(key, (Instant::now(), stats.clone()));
        }
    }
    log::info!(
        "[DB] get_db_stats({}): {:?} (missing by file existence)",
        db_path,
        stats
    );
    Ok(stats)
}

/// 保存目录里**普通文件**的名字集合（带短缓存，见 `dir_listing`）。
///
/// 原来逐行 `Path::exists` 会产生 N 次 syscall；改成一次 `read_dir` 建表后按名查哈希。
/// 再叠一层进程内缓存，让「缺失列表 / 孤儿列表 / 统计」三处共用同一次目录扫描。
pub(crate) fn existing_file_names(save_dir: &str) -> Arc<DirListing> {
    dir_listing(save_dir)
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
        // 先挑出缺失的 id，再用一条 IN(...) 批量更新。
        // 原来逐条 UPDATE，一库几千条缺失就是几千条语句（虽然在同一事务里，
        // 但每条都要走一次语句执行 + 写 WAL）。
        let missing: Vec<i64> = rows
            .into_iter()
            .filter(|(_, name)| !existing.contains_key(name))
            .map(|(id, _)| id)
            .collect();

        let mut updated = 0u64;
        for chunk in missing.chunks(SQL_PARAM_CHUNK) {
            // 占位符个数按块长生成（块长来自常量，不涉及注入）。
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!("UPDATE images SET love = 0 WHERE id IN ({placeholders})");
            let mut stmt = tx.prepare_cached(&sql)?;
            updated += stmt.execute(rusqlite::params_from_iter(chunk.iter()))? as u64;
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
        .filter(|name| !existing.contains_key(*name))
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
