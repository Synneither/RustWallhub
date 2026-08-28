use crate::downloader;
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
    if guard.insert(db_path.to_string()) {
        // 旧版本为 UNIQUE 字段又创建了显式索引；唯一约束已有自动索引，显式索引是冗余的。
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_url;
             DROP INDEX IF EXISTS idx_hash;
             DROP INDEX IF EXISTS idx_wallhaven_id;",
        )?;
    }
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
    ensure_love_column(&conn)
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
    ensure_love_column(&conn)
}

// ---------------------------------------------------------------------------
// 快照导出 / 导入合并（多设备同步）
// ---------------------------------------------------------------------------

/// 快照导入合并结果
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ImportStats {
    /// 新插入的记录数（本地不存在）
    pub inserted: i64,
    /// 由快照恢复为 love=1 的本地记录数
    pub loved: i64,
}

/// 导出数据库快照到 `snapshot_path`（VACUUM INTO：原子写出、自带 checkpoint、碎片整理）。
/// 目标文件已存在时会被删除后重建。
pub fn export_snapshot(db_path: &str, snapshot_path: &str) -> SqlResult<()> {
    fn io_err(e: std::io::Error) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
            Some(e.to_string()),
        )
    }
    if let Some(parent) = Path::new(snapshot_path).parent() {
        std::fs::create_dir_all(parent).map_err(io_err)?;
    }
    // VACUUM INTO 要求目标文件不存在
    if Path::new(snapshot_path).exists() {
        std::fs::remove_file(snapshot_path).map_err(io_err)?;
    }
    with_cached_connection(db_path, |conn| {
        conn.execute("VACUUM INTO ?1", rusqlite::params![snapshot_path])?;
        Ok(())
    })
}

/// 从 Wallhaven 快照合并导入（ATTACH + 记录级合并）。
/// 合并语义：快照中 love=1 的记录会把本地同 id 记录恢复为 love=1；
/// 本地不存在的记录按原样插入；其余本地数据不动。
pub fn import_wallhaven_snapshot(db_path: &str, snapshot_path: &str) -> SqlResult<ImportStats> {
    with_cached_connection(db_path, |conn| {
        attach_guard(conn, snapshot_path, |conn| {
            let loved = conn.execute(
                "UPDATE main.images SET love = 1
                 WHERE love = 0
                   AND EXISTS (SELECT 1 FROM incoming.images i
                               WHERE i.wallhaven_id = main.images.wallhaven_id
                                 AND i.love = 1)",
                [],
            )? as i64;
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO main.images
                    (wallhaven_id, name, hash, url, source_url, resolution, love, created_at)
                 SELECT wallhaven_id, name, hash, url, source_url, resolution, love, created_at
                 FROM incoming.images",
                [],
            )? as i64;
            Ok(ImportStats { inserted, loved })
        })
    })
}

/// 从 Reddit 快照合并导入（合并键为 hash/url 唯一约束）。
pub fn import_reddit_snapshot(db_path: &str, snapshot_path: &str) -> SqlResult<ImportStats> {
    with_cached_connection(db_path, |conn| {
        attach_guard(conn, snapshot_path, |conn| {
            let loved = conn.execute(
                "UPDATE main.images SET love = 1
                 WHERE love = 0
                   AND EXISTS (SELECT 1 FROM incoming.images i
                               WHERE i.hash = main.images.hash
                                 AND i.love = 1)",
                [],
            )? as i64;
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO main.images
                    (name, hash, url, title, permalink, love, created_at)
                 SELECT name, hash, url, title, permalink, love, created_at
                 FROM incoming.images",
                [],
            )? as i64;
            Ok(ImportStats { inserted, loved })
        })
    })
}

/// ATTACH 快照库执行闭包，退出前保证 DETACH（即使闭包出错）。
fn attach_guard<T>(
    conn: &mut Connection,
    snapshot_path: &str,
    f: impl FnOnce(&mut Connection) -> SqlResult<T>,
) -> SqlResult<T> {
    conn.execute(
        "ATTACH DATABASE ?1 AS incoming",
        rusqlite::params![snapshot_path],
    )?;
    let result = f(conn);
    let _ = conn.execute("DETACH DATABASE incoming", []);
    result
}

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

#[cfg_attr(not(test), allow(dead_code))]
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

#[cfg_attr(not(test), allow(dead_code))]
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
        for (wallhaven_id, name, hash, url, source_url, resolution) in images {
            match tx.execute(
                "INSERT INTO images (wallhaven_id, name, hash, url, source_url, resolution) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![wallhaven_id, name, hash, url, source_url, resolution],
            ) {
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
        for (name, hash, url, title, permalink) in images {
            match tx.execute(
                "INSERT INTO images (name, hash, url, title, permalink) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![name, hash, url, title, permalink],
            ) {
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
fn existing_file_names(save_dir: &str) -> HashSet<String> {
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

fn count_missing(db_path: &str, save_dir: &str) -> SqlResult<u64> {
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
            .filter(|img| !existing.contains(&img.name))
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
            .filter(|img| !existing.contains(&img.name))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct TestDb {
        _dir: TempDir,
        path: String,
    }

    impl TestDb {
        fn wallhaven() -> Self {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("test.db").to_string_lossy().to_string();
            init_wallhaven_db(&path).unwrap();
            Self { _dir: dir, path }
        }

        fn reddit() -> Self {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("test.db").to_string_lossy().to_string();
            init_reddit_db(&path).unwrap();
            Self { _dir: dir, path }
        }

        fn path(&self) -> &str {
            &self.path
        }

        fn conn(&self) -> Connection {
            Connection::open(&self.path).unwrap()
        }
    }

    #[test]
    fn test_init_wallhaven_db() {
        let db = TestDb::wallhaven();
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM images", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_init_reddit_db() {
        let db = TestDb::reddit();
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM images", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_insert_wallhaven_image() {
        let db = TestDb::wallhaven();
        assert!(insert_wallhaven_image(
            db.path(),
            "wh001",
            "wh001.jpg",
            "abc123",
            "https://wh.cc/i/wh001",
            "https://wh.cc/s/wh001",
            "1920x1080"
        )
        .unwrap());
    }

    #[test]
    fn test_insert_wallhaven_duplicate() {
        let db = TestDb::wallhaven();
        assert!(
            insert_wallhaven_image(db.path(), "wh001", "a.jpg", "h1", "u1", "s1", "1920x1080")
                .unwrap()
        );
        assert!(!insert_wallhaven_image(
            db.path(),
            "wh001",
            "b.jpg",
            "h2",
            "u2",
            "s2",
            "1920x1080"
        )
        .unwrap());
    }

    #[test]
    fn test_get_existing_wallhaven_ids() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        insert_wallhaven_image(db.path(), "id2", "b.jpg", "h2", "u2", "s2", "3840x2160").unwrap();
        let ids = get_existing_wallhaven_ids(db.path()).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"id1".to_string()));
    }

    #[test]
    fn test_count_missing() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"fake").unwrap();
        assert_eq!(
            count_missing(db.path(), &dir.path().to_string_lossy()).unwrap(),
            0
        );

        std::fs::remove_file(dir.path().join("a.jpg")).unwrap();
        assert_eq!(
            count_missing(db.path(), &dir.path().to_string_lossy()).unwrap(),
            1
        );
    }

    #[test]
    fn test_insert_reddit_image() {
        let db = TestDb::reddit();
        assert!(insert_reddit_image(
            db.path(),
            "img.jpg",
            "def456",
            "https://reddit.com/img.jpg",
            "title",
            "/r/123"
        )
        .unwrap());
    }

    #[test]
    fn test_insert_reddit_duplicate_url() {
        let db = TestDb::reddit();
        assert!(insert_reddit_image(
            db.path(),
            "a.jpg",
            "h1",
            "https://reddit.com/1",
            "t",
            "/r/1"
        )
        .unwrap());
        assert!(!insert_reddit_image(
            db.path(),
            "b.jpg",
            "h2",
            "https://reddit.com/1",
            "t",
            "/r/2"
        )
        .unwrap());
    }

    #[test]
    fn test_get_existing_reddit_urls() {
        let db = TestDb::reddit();
        insert_reddit_image(
            db.path(),
            "a.jpg",
            "h1",
            "https://reddit.com/1",
            "t1",
            "/r/1",
        )
        .unwrap();
        insert_reddit_image(
            db.path(),
            "b.jpg",
            "h2",
            "https://reddit.com/2",
            "t2",
            "/r/2",
        )
        .unwrap();
        assert_eq!(get_existing_reddit_urls(db.path()).unwrap().len(), 2);
    }

    #[test]
    fn test_get_db_stats() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        insert_wallhaven_image(db.path(), "id2", "b.jpg", "h2", "u2", "s2", "3840x2160").unwrap();
        // 文件不存在 → love=1 但缺失 = 2
        let img_dir = TempDir::new().unwrap();
        let stats = get_db_stats(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.love, 2);
        assert_eq!(stats.dislike, 2);
        // 创建其中一个文件 → 缺失 = 1
        std::fs::write(img_dir.path().join("a.jpg"), b"fake").unwrap();
        let stats = get_db_stats(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(stats.dislike, 1);
    }

    #[test]
    fn test_stats_cache_invalidated_by_insert() {
        let db = TestDb::wallhaven();
        let img_dir = TempDir::new().unwrap();
        let dir = img_dir.path().to_string_lossy().to_string();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();

        let first = get_db_stats(db.path(), &dir).unwrap();
        assert_eq!(first.total, 1);
        // 写入新记录后，即使统计缓存刚生成，也应立即看到 total=2。
        insert_wallhaven_image(db.path(), "id2", "b.jpg", "h2", "u2", "s2", "3840x2160").unwrap();
        let second = get_db_stats(db.path(), &dir).unwrap();
        assert_eq!(second.total, 2);
    }

    #[test]
    fn test_query_does_not_create_db() {
        // 核心语义：数据库不存在时，任何查询都必须报错且不得创建文件
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("missing.db");
        let dir_str = dir.path().to_string_lossy().to_string();
        let db_str = db_path.to_string_lossy().to_string();
        assert!(get_db_stats(&db_str, &dir_str).is_err());
        assert!(!db_path.exists(), "查询不应创建数据库文件");
        assert!(!db_exists(&db_str));
    }

    #[test]
    fn test_mark_dislike_and_restore_wallhaven() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "keep.jpg", "h1", "u1", "s1", "1920x1080")
            .unwrap();
        insert_wallhaven_image(db.path(), "id2", "gone.jpg", "h2", "u2", "s2", "3840x2160")
            .unwrap();

        let img_dir = TempDir::new().unwrap();
        std::fs::write(img_dir.path().join("keep.jpg"), b"fake").unwrap();
        assert_eq!(
            mark_missing_dislike_wallhaven(db.path(), &img_dir.path().to_string_lossy()).unwrap(),
            1
        );

        assert_eq!(
            get_db_stats(db.path(), &img_dir.path().to_string_lossy())
                .unwrap()
                .love,
            1
        );
        assert_eq!(restore_love_db(db.path()).unwrap(), 1);
        assert_eq!(
            get_db_stats(db.path(), &img_dir.path().to_string_lossy())
                .unwrap()
                .love,
            2
        );
    }

    #[test]
    fn test_wallhaven_missing_love_toggle() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();

        let img_dir = TempDir::new().unwrap();
        std::fs::write(img_dir.path().join("a.jpg"), b"data").unwrap();
        assert_eq!(get_wallhaven_missing_love(db.path()).unwrap().len(), 1);

        std::fs::remove_file(img_dir.path().join("a.jpg")).unwrap();
        mark_missing_dislike_wallhaven(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(get_wallhaven_missing_love(db.path()).unwrap().len(), 0);
    }

    #[test]
    fn test_reddit_missing_love() {
        let db = TestDb::reddit();
        insert_reddit_image(
            db.path(),
            "a.jpg",
            "h1",
            "https://reddit.com/1",
            "t1",
            "/r/1",
        )
        .unwrap();

        let img_dir = TempDir::new().unwrap();
        std::fs::write(img_dir.path().join("a.jpg"), b"data").unwrap();
        assert_eq!(get_reddit_missing_love(db.path()).unwrap().len(), 1);
    }
    #[test]
    fn test_get_wallhaven_images_pagination() {
        let db = TestDb::wallhaven();
        for i in 0..5 {
            insert_wallhaven_image(
                db.path(),
                &format!("id{i}"),
                &format!("{i}.jpg"),
                &format!("h{i}"),
                &format!("u{i}"),
                &format!("s{i}"),
                "1920x1080",
            )
            .unwrap();
        }
        assert_eq!(get_wallhaven_images(db.path(), 2, 0).unwrap().len(), 2);
        assert_eq!(get_wallhaven_images(db.path(), 10, 0).unwrap().len(), 5);
    }

    #[test]
    fn test_mark_dislike_by_name_wallhaven() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        assert!(mark_dislike_by_name(db.path(), "a.jpg").unwrap());
        let img_dir = TempDir::new().unwrap();
        let stats = get_db_stats(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(stats.love, 0);
        // 手动不喜欢 → love=0，不计入缺失（缺失只看 love=1 且文件不存在）
        assert_eq!(stats.dislike, 0);
    }

    #[test]
    fn test_mark_dislike_by_name_nonexistent() {
        let db = TestDb::wallhaven();
        assert!(!mark_dislike_by_name(db.path(), "noexist.jpg").unwrap());
    }

    #[test]
    fn test_mark_dislike_by_names_batch() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        insert_wallhaven_image(db.path(), "id2", "b.jpg", "h2", "u2", "s2", "3840x2160").unwrap();
        let names = vec![
            "a.jpg".to_string(),
            "b.jpg".to_string(),
            "c.jpg".to_string(),
        ];
        assert_eq!(mark_dislike_by_names(db.path(), &names).unwrap(), 2);
        let stats = get_db_stats(db.path(), "/nonexistent-dir").unwrap();
        assert_eq!(stats.love, 0);
    }

    #[test]
    fn test_get_wallhaven_missing_files_with_dir() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(
            db.path(),
            "id1",
            "exists.jpg",
            "h1",
            "u1",
            "s1",
            "1920x1080",
        )
        .unwrap();
        insert_wallhaven_image(
            db.path(),
            "id2",
            "missing.jpg",
            "h2",
            "u2",
            "s2",
            "3840x2160",
        )
        .unwrap();

        let img_dir = TempDir::new().unwrap();
        std::fs::write(img_dir.path().join("exists.jpg"), b"real file").unwrap();

        let missing =
            get_wallhaven_missing_files(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "missing.jpg");
    }

    #[test]
    fn test_get_all_filenames() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        insert_wallhaven_image(db.path(), "id2", "b.png", "h2", "u2", "s2", "3840x2160").unwrap();
        let names = get_all_filenames(db.path()).unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"a.jpg".to_string()));
        assert!(names.contains(&"b.png".to_string()));
    }

    #[test]
    fn test_get_missing_files_excludes_disliked() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "liked.jpg", "h1", "u1", "s1", "1920x1080")
            .unwrap();
        insert_wallhaven_image(
            db.path(),
            "id2",
            "disliked.jpg",
            "h2",
            "u2",
            "s2",
            "3840x2160",
        )
        .unwrap();

        let img_dir = TempDir::new().unwrap();
        // 标记 dislike 为不喜欢
        mark_dislike_by_name(db.path(), "disliked.jpg").unwrap();

        let missing =
            get_wallhaven_missing_files(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "liked.jpg");
    }

    #[test]
    fn test_open_migrates_legacy_duplicate_indexes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("legacy.db").to_string_lossy().to_string();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE images (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallhaven_id TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                hash TEXT NOT NULL UNIQUE,
                url TEXT NOT NULL UNIQUE,
                source_url TEXT,
                resolution TEXT,
                love INTEGER NOT NULL DEFAULT 1,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX idx_url ON images(url);
            CREATE INDEX idx_hash ON images(hash);
            CREATE INDEX idx_wallhaven_id ON images(wallhaven_id);",
        )
        .unwrap();
        drop(conn);

        // 任意查询路径触发 open()，应一次性清理旧版冗余索引。
        let _ = get_db_stats(&path, "/nonexistent-dir").unwrap();
        let conn = Connection::open(&path).unwrap();
        let mut stmt = conn.prepare("PRAGMA index_list(images)").unwrap();
        let indexes: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|name| name.ok())
            .collect();
        assert!(!indexes.iter().any(|name| name == "idx_url"));
        assert!(!indexes.iter().any(|name| name == "idx_hash"));
        assert!(!indexes.iter().any(|name| name == "idx_wallhaven_id"));
    }

    #[test]
    fn test_love_column_migration() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("migrate.db").to_string_lossy().to_string();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE images (id INTEGER PRIMARY KEY, name TEXT, hash TEXT, url TEXT, stable INTEGER DEFAULT 1);").unwrap();
        ensure_love_column(&conn).unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(images)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get(1))
            .unwrap()
            .filter_map(|c| c.ok())
            .collect();
        assert!(cols.contains(&"love".to_string()));
    }

    #[test]
    fn test_clean_stale_thumbnails() {
        let save_dir = TempDir::new().unwrap();
        let thumb_dir = TempDir::new().unwrap();
        let td = thumb_dir.path().to_path_buf();
        std::fs::write(td.join("orphan.jpg"), b"fake").unwrap();
        assert!(
            clean_stale_thumbnails(&td.to_string_lossy(), &save_dir.path().to_string_lossy()) > 0
        );
    }

    #[test]
    fn test_clean_stale_thumbnails_keeps_valid() {
        let save_dir = TempDir::new().unwrap();
        let thumb_dir = TempDir::new().unwrap();
        let td = thumb_dir.path().to_path_buf();
        std::fs::write(save_dir.path().join("valid.jpg"), b"data").unwrap();
        std::fs::write(td.join("valid.jpg"), b"thumb").unwrap();
        assert_eq!(
            clean_stale_thumbnails(&td.to_string_lossy(), &save_dir.path().to_string_lossy()),
            0
        );
    }

    #[test]
    fn test_clean_stale_thumbnails_keeps_dpr_format() {
        let save_dir = TempDir::new().unwrap();
        let thumb_dir = TempDir::new().unwrap();
        let td = thumb_dir.path().to_path_buf();
        std::fs::write(save_dir.path().join("valid.jpg"), b"data").unwrap();
        std::fs::write(td.join("valid__w480.webp"), b"thumb").unwrap();
        std::fs::write(td.join("orphan__w480.webp"), b"thumb").unwrap();
        assert_eq!(
            clean_stale_thumbnails(&td.to_string_lossy(), &save_dir.path().to_string_lossy()),
            1,
            "DPR 格式缩略图对应的原图存在时不应被清理，孤儿缩略图应被清理"
        );
        assert!(td.join("valid__w480.webp").exists());
        assert!(!td.join("orphan__w480.webp").exists());
    }

    #[test]
    fn test_insert_wallhaven_batch() {
        let db = TestDb::wallhaven();
        let images = vec![
            (
                "id1".into(),
                "a.jpg".into(),
                "h1".into(),
                "u1".into(),
                "s1".into(),
                "1920x1080".into(),
            ),
            (
                "id2".into(),
                "b.jpg".into(),
                "h2".into(),
                "u2".into(),
                "s2".into(),
                "3840x2160".into(),
            ),
            (
                "id3".into(),
                "c.png".into(),
                "h3".into(),
                "u3".into(),
                "s3".into(),
                "2560x1440".into(),
            ),
        ];
        let (added, skipped) = insert_wallhaven_images_batch(db.path(), &images).unwrap();
        assert_eq!(added, 3);
        assert_eq!(skipped, 0);

        // 文件均不存在 → 缺失 = 3
        let img_dir = TempDir::new().unwrap();
        let stats = get_db_stats(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.love, 3);
        assert_eq!(stats.dislike, 3);
    }

    #[test]
    fn test_insert_wallhaven_batch_with_duplicates() {
        let db = TestDb::wallhaven();
        let images = vec![
            (
                "dup".into(),
                "a.jpg".into(),
                "h1".into(),
                "u1".into(),
                "s1".into(),
                "1080p".into(),
            ),
            (
                "dup".into(),
                "b.jpg".into(),
                "h2".into(),
                "u2".into(),
                "s2".into(),
                "4k".into(),
            ),
            (
                "id2".into(),
                "c.jpg".into(),
                "h3".into(),
                "u3".into(),
                "s3".into(),
                "2k".into(),
            ),
        ];
        let (added, skipped) = insert_wallhaven_images_batch(db.path(), &images).unwrap();
        assert_eq!(added, 2);
        assert_eq!(skipped, 1);

        // Verify the duplicate was the second one (same wallhaven_id)
        let ids = get_existing_wallhaven_ids(db.path()).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"dup".to_string()));
        assert!(ids.contains(&"id2".to_string()));
    }

    #[test]
    fn test_insert_wallhaven_batch_skips_duplicate_hash() {
        let db = TestDb::wallhaven();
        // Two entries with the same hash — second should be skipped
        let images = vec![
            (
                "id1".into(),
                "a.jpg".into(),
                "same_hash".into(),
                "u1".into(),
                "s1".into(),
                "1080p".into(),
            ),
            (
                "id2".into(),
                "b.jpg".into(),
                "same_hash".into(),
                "u2".into(),
                "s2".into(),
                "4k".into(),
            ),
        ];
        let (added, skipped) = insert_wallhaven_images_batch(db.path(), &images).unwrap();
        assert_eq!(added, 1);
        assert_eq!(skipped, 1);

        let img_dir = TempDir::new().unwrap();
        let stats = get_db_stats(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(stats.total, 1);
    }

    #[test]
    fn test_insert_wallhaven_batch_empty() {
        let db = TestDb::wallhaven();
        let images: Vec<(String, String, String, String, String, String)> = vec![];
        let (added, skipped) = insert_wallhaven_images_batch(db.path(), &images).unwrap();
        assert_eq!(added, 0);
        assert_eq!(skipped, 0);

        let img_dir = TempDir::new().unwrap();
        let stats = get_db_stats(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(stats.total, 0);
    }

    /* ── 快照导出 / 导入合并 ── */

    #[test]
    fn test_export_snapshot_creates_valid_db() {
        let db = TestDb::wallhaven();
        insert_wallhaven_images_batch(
            db.path(),
            &[
                (
                    "id1".into(),
                    "a.jpg".into(),
                    "h1".into(),
                    "u1".into(),
                    "s1".into(),
                    "1920x1080".into(),
                ),
            ],
        )
        .unwrap();

        let dir = TempDir::new().unwrap();
        let snap = dir.path().join("snap.db").to_string_lossy().to_string();
        export_snapshot(db.path(), &snap).unwrap();
        assert!(dir.path().join("snap.db").exists());

        // 快照本身可读且包含数据
        let count: i64 = Connection::open(&snap)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM images", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // 覆盖已存在的目标文件也能成功
        export_snapshot(db.path(), &snap).unwrap();
    }

    #[test]
    fn test_import_wallhaven_snapshot_merge() {
        // 本地库：id1 正常、id2 被标记 dislike
        let db = TestDb::wallhaven();
        insert_wallhaven_images_batch(
            db.path(),
            &[
                (
                    "id1".into(),
                    "a.jpg".into(),
                    "h1".into(),
                    "u1".into(),
                    "s1".into(),
                    "1920x1080".into(),
                ),
                (
                    "id2".into(),
                    "b.jpg".into(),
                    "h2".into(),
                    "u2".into(),
                    "s2".into(),
                    "1920x1080".into(),
                ),
            ],
        )
        .unwrap();
        db.conn()
            .execute("UPDATE images SET love = 0 WHERE wallhaven_id = 'id2'", [])
            .unwrap();

        // 快照库：id2 love=1（另一台机器上还喜欢）、id3 新记录、id1 也存在
        let snap_dir = TempDir::new().unwrap();
        let snap = snap_dir.path().join("snap.db").to_string_lossy().to_string();
        init_wallhaven_db(&snap).unwrap();
        let snap_conn = Connection::open(&snap).unwrap();
        for (wid, name, hash, love) in [("id1", "a.jpg", "h1", 1), ("id2", "b.jpg", "h2", 1), ("id3", "c.jpg", "h3", 1)] {
            snap_conn
                .execute(
                    "INSERT INTO images (wallhaven_id, name, hash, url, source_url, resolution, love)
                     VALUES (?1, ?2, ?3, ?4, ?5, '1080p', ?6)",
                    rusqlite::params![wid, name, hash, format!("u-{wid}"), format!("s-{wid}"), love],
                )
                .unwrap();
        }

        let stats = import_wallhaven_snapshot(db.path(), &snap).unwrap();
        assert_eq!(stats.inserted, 1, "只应插入 id3");
        assert_eq!(stats.loved, 1, "id2 应被恢复为 love=1");

        let conn = db.conn();
        let (love2, total): (i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT love FROM images WHERE wallhaven_id = 'id2'),
                    (SELECT COUNT(*) FROM images)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(love2, 1);
        assert_eq!(total, 3);
    }

    #[test]
    fn test_import_reddit_snapshot_merge() {
        let db = TestDb::reddit();
        db.conn()
            .execute(
                "INSERT INTO images (name, hash, url) VALUES ('a.jpg', 'h1', 'u1')",
                [],
            )
            .unwrap();
        db.conn()
            .execute("UPDATE images SET love = 0 WHERE hash = 'h1'", [])
            .unwrap();

        let snap_dir = TempDir::new().unwrap();
        let snap = snap_dir.path().join("snap.db").to_string_lossy().to_string();
        init_reddit_db(&snap).unwrap();
        Connection::open(&snap)
            .unwrap()
            .execute(
                "INSERT INTO images (name, hash, url, love) VALUES ('a.jpg', 'h1', 'u1', 1), ('b.jpg', 'h2', 'u2', 1)",
                [],
            )
            .unwrap();

        let stats = import_reddit_snapshot(db.path(), &snap).unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.loved, 1);

        let (love1, total): (i64, i64) = db
            .conn()
            .query_row(
                "SELECT (SELECT love FROM images WHERE hash = 'h1'), (SELECT COUNT(*) FROM images)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(love1, 1);
        assert_eq!(total, 2);
    }

    #[test]
    fn test_import_snapshot_rejects_non_database_file() {
        let db = TestDb::wallhaven();
        let dir = TempDir::new().unwrap();
        let bogus = dir.path().join("bogus.db");
        std::fs::write(&bogus, "not a database").unwrap();

        let result = import_wallhaven_snapshot(db.path(), &bogus.to_string_lossy());
        assert!(result.is_err(), "导入非数据库文件应报错");
    }
}
