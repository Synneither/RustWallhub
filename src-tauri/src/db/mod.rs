//! 数据库层：连接管理、初始化、迁移，以及按域拆分出去的子模块。
//!
//! - `records` 图片记录的增删改查
//! - `stats`   统计与"标记缺失"写操作
//! - `sync`    快照导出 / 导入合并
//! - `thumbs`  缩略图维护

pub mod records;
pub mod stats;
pub mod sync;
#[cfg(test)]
mod tests;
pub mod thumbs;

// 子模块统一重导出，外部仍按 `db::xxx` 调用，拆分对调用方透明
pub use records::*;
pub use stats::*;
pub use sync::*;
pub use thumbs::*;

use rusqlite::{Connection, OpenFlags, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

/// 每个连接都会用到的连接级 PRAGMA（busy_timeout / synchronous / cache_size
/// 都不是持久化设置，必须每次 open 后重新配置）。
fn configure_connection(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "PRAGMA busy_timeout=5000;
         PRAGMA synchronous=NORMAL;
         PRAGMA cache_size=-64000;
         PRAGMA temp_store=MEMORY;",
    )
}

/// 每个进程只需对每个 DB 执行一次旧版本冗余索引清理。
static MIGRATED_DBS: std::sync::OnceLock<std::sync::Mutex<HashSet<String>>> =
    std::sync::OnceLock::new();

type SharedConnection = std::sync::Arc<std::sync::Mutex<Connection>>;

/// 进程级 SQLite 连接缓存。数据库文件数量固定且很小，这里按路径缓存连接，
/// 避免图库/统计/下载等高频命令反复 `Connection::open`。
static CONNECTION_CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, SharedConnection>>> =
    std::sync::OnceLock::new();

type StatsCacheKey = (String, String);
type StatsCacheMap = HashMap<StatsCacheKey, (Instant, Option<std::time::SystemTime>, DbStats)>;

/// 统计结果短缓存。写入路径会主动失效；外部改动最多 2 秒后可见。
static STATS_CACHE: std::sync::OnceLock<std::sync::Mutex<StatsCacheMap>> =
    std::sync::OnceLock::new();

/// 使某个 DB 的统计缓存失效（下载完成、删除、标记、恢复等写操作后调用）。
pub fn invalidate_stats(db_path: &str) {
    if let Some(cache) = STATS_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.retain(|(db, _), _| db != db_path);
        }
    }
}

fn cached_connection(db_path: &str) -> SqlResult<SharedConnection> {
    let cache = CONNECTION_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut cache = cache.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
    if let Some(conn) = cache.get(db_path) {
        // 如果文件已被外部删除/重建，必须丢弃旧连接，否则会继续操作已删除的 inode。
        if Path::new(db_path).exists() {
            return Ok(conn.clone());
        }
        cache.remove(db_path);
    }

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure_connection(&conn)?;
    migrate_legacy_indexes(&conn, db_path)?;
    let conn = std::sync::Arc::new(std::sync::Mutex::new(conn));
    cache.insert(db_path.to_string(), conn.clone());
    Ok(conn)
}

/// 获取缓存连接并执行闭包；同时保证同一 DB 的写事务不会并发交错。
pub fn with_cached_connection<T>(
    db_path: &str,
    f: impl FnOnce(&mut Connection) -> SqlResult<T>,
) -> SqlResult<T> {
    let conn = cached_connection(db_path)?;
    let mut guard = conn.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
    f(&mut guard)
}

/// 显式关闭并移除某个 DB 路径的缓存连接（初始化/重建数据库前调用）。
pub fn invalidate_connection(db_path: &str) {
    if let Some(cache) = CONNECTION_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.remove(db_path);
        }
    }
}

fn migrate_legacy_indexes(conn: &Connection, db_path: &str) -> SqlResult<()> {
    let migrated = MIGRATED_DBS.get_or_init(|| std::sync::Mutex::new(HashSet::new()));
    let mut guard = migrated.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
    // 索引维护不放在 `if guard.insert(...)` 里：DROP 是一次性的，
    // 而 CREATE INDEX IF NOT EXISTS 需要每次开库都确认，否则旧库永远拿不到新索引。
    if guard.insert(db_path.to_string()) {
        // 旧版本为 UNIQUE 字段又创建了显式索引；唯一约束已有自动索引，显式索引是冗余的。
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_url;
             DROP INDEX IF EXISTS idx_hash;
             DROP INDEX IF EXISTS idx_wallhaven_id;",
        )?;
    }
    ensure_created_at_index(conn)?;
    Ok(())
}

/// 只读打开已存在的数据库（不创建文件）。
/// 文件不存在时返回错误，避免任何查询路径静默创建空库。
fn open(db_path: &str) -> SqlResult<Connection> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure_connection(&conn)?;
    migrate_legacy_indexes(&conn, db_path)?;
    Ok(conn)
}

/// 显式创建/打开数据库（仅初始化命令使用）。
fn open_create(db_path: &str) -> SqlResult<Connection> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    configure_connection(&conn)?;
    Ok(conn)
}

/// 数据库文件是否存在
pub fn db_exists(db_path: &str) -> bool {
    Path::new(db_path).exists()
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ImageRecord {
    pub id: i64,
    pub name: String,
    pub hash: String,
    pub url: String,
    pub source_url: String,
    pub resolution: String,
    pub title: Option<String>,
    pub permalink: Option<String>,
    pub love: i32,
    pub created_at: String,
    pub source: String,
}

#[derive(Clone, Serialize, Debug)]
pub struct DbStats {
    /// 数据库记录总数
    pub total: i64,
    /// love=1（正常状态）的记录数
    pub love: i64,
    /// 缺失数：love=1 但保存目录中文件不存在的记录数
    pub dislike: i64,
}

fn ensure_text_column(conn: &Connection, table: &str, column: &str) -> SqlResult<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let column_names: Vec<String> = stmt
        .query_map([], |row| row.get(1))?
        .collect::<SqlResult<_>>()?;
    if !column_names.iter().any(|name| name == column) {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT"), [])?;
    }
    Ok(())
}

fn ensure_love_column(conn: &Connection) -> SqlResult<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(images)")?;
    let column_names: Vec<String> = stmt
        .query_map([], |row| row.get(1))?
        .collect::<SqlResult<_>>()?;

    let has_love = column_names.iter().any(|name| name == "love");
    let has_stable = column_names.iter().any(|name| name == "stable");

    if !has_love {
        conn.execute(
            "ALTER TABLE images ADD COLUMN love INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
        // 仅在刚添加 love 列时迁移旧 stable 数据，避免重复覆盖用户数据
        if has_stable {
            conn.execute(
                "UPDATE images SET love = stable WHERE stable IS NOT NULL",
                [],
            )?;
        }
    }

    Ok(())
}

pub fn init_wallhaven_db(db_path: &str) -> SqlResult<()> {
    log::info!("[DB] init_wallhaven_db: path={}", db_path);
    invalidate_connection(db_path);
    let conn = open_create(db_path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS images (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            wallhaven_id TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            hash TEXT NOT NULL UNIQUE,
            url TEXT NOT NULL UNIQUE,
            source_url TEXT,
            resolution TEXT,
            love INTEGER NOT NULL DEFAULT 1,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );",
    )?;
    // UNIQUE 约束本身会创建自动索引，旧版本创建的显式索引是冗余的。
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_url;
         DROP INDEX IF EXISTS idx_hash;
         DROP INDEX IF EXISTS idx_wallhaven_id;",
    )?;
    ensure_love_column(&conn)?;
    ensure_created_at_index(&conn)
}

pub fn init_reddit_db(db_path: &str) -> SqlResult<()> {
    log::info!("[DB] init_reddit_db: path={}", db_path);
    invalidate_connection(db_path);
    let conn = open_create(db_path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS images (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            hash TEXT NOT NULL UNIQUE,
            url TEXT NOT NULL UNIQUE,
            title TEXT,
            permalink TEXT,
            love INTEGER NOT NULL DEFAULT 1,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );",
    )?;
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_url;
         DROP INDEX IF EXISTS idx_hash;",
    )?;
    ensure_text_column(&conn, "images", "title")?;
    ensure_text_column(&conn, "images", "permalink")?;
    ensure_love_column(&conn)?;
    ensure_created_at_index(&conn)
}

// ---------------------------------------------------------------------------
// 索引与退出维护
// ---------------------------------------------------------------------------

/// 列表查询按 `created_at DESC, id DESC` 排序，建索引可直接省掉排序。
/// 数据量小时无感，库到几千条后能明显减少列表翻页的开销。
fn ensure_created_at_index(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_images_created_at ON images(created_at DESC, id DESC);",
    )
}

/// 退出前收尾：WAL 归零 + 让 SQLite 更新查询计划统计（官方建议的做法）。
///
/// 不做这件事的后果：
/// - 直接复制 `.db` 做备份会漏掉尚未 checkpoint 的事务（WAL 里的内容）
/// - 小写入永远触发不到 SQLite 默认的 1000 页自动 checkpoint，WAL 会一直原地增长
pub fn maintain_on_exit(db_path: &str) -> SqlResult<()> {
    if !db_exists(db_path) {
        return Ok(());
    }
    with_cached_connection(db_path, |conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA optimize;")
    })
}
