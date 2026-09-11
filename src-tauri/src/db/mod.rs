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
use std::sync::Arc;
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

/// 文件身份标识，用于检测「路径相同但文件已被外部替换」。
/// Unix 用 (inode, device)，精确；Windows 没有稳定的 inode 等价物（`file_index` 是 nightly），
/// 退化为 (文件大小, 最后写入时间) 弱身份——文件被替换时这两者几乎总会变化。
type FileIdentity = (u64, u64);

#[cfg(unix)]
fn file_identity(meta: &std::fs::Metadata) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.ino(), meta.dev()))
}

#[cfg(windows)]
fn file_identity(meta: &std::fs::Metadata) -> Option<FileIdentity> {
    use std::os::windows::fs::MetadataExt;
    Some((meta.file_size(), meta.last_write_time()))
}

/// 进程级 SQLite 连接缓存。数据库文件数量固定且很小，这里按路径缓存连接，
/// 避免图库/统计/下载等高频命令反复 `Connection::open`。
/// 值附带打开时的文件身份，命中时比对身份：路径仍在但 inode 变了（文件被替换）
/// 就丢弃旧连接重连，否则会继续往已 unlink 的旧 inode 写、数据静默丢失。
type ConnectionCacheEntry = (SharedConnection, Option<FileIdentity>);
type ConnectionCacheMap = std::sync::Mutex<HashMap<String, ConnectionCacheEntry>>;

static CONNECTION_CACHE: std::sync::OnceLock<ConnectionCacheMap> = std::sync::OnceLock::new();

type StatsCacheKey = (String, String);
type StatsCacheMap = HashMap<StatsCacheKey, (Instant, DbStats)>;

/// 统计缓存的存活秒数。刷新依赖写操作主动失效，超过这个时长也会强制重算兜底。
pub(crate) const STATS_CACHE_TTL_SECS: u64 = 10;

/// 统计结果短缓存。写入路径会主动失效；外部改动最多 `STATS_CACHE_TTL_SECS` 秒后可见。
static STATS_CACHE: std::sync::OnceLock<std::sync::Mutex<StatsCacheMap>> =
    std::sync::OnceLock::new();

/// 目录内容快照：文件名 → 文件大小（字节）。
///
/// 只收**普通文件**，不含子目录——缺失判定按文件名匹配，若把同名子目录算作"存在"
/// 会让缺失数漏报。
pub(crate) type DirListing = HashMap<String, u64>;

type DirListingCacheMap = HashMap<String, (Instant, Arc<DirListing>)>;

/// 目录列表缓存的兜底存活秒数。
///
/// 之所以要缓存：一次「数据库」页刷新会对每个源目录做 3 次全量 `read_dir`
/// ——缺失列表、孤儿列表、统计各一次；两个源就是 6 次。缓存让它们共用一次扫描。
pub(crate) const DIR_LISTING_TTL_SECS: u64 = 5;

static DIR_LISTING_CACHE: std::sync::OnceLock<std::sync::Mutex<DirListingCacheMap>> =
    std::sync::OnceLock::new();

/// 真正扫一次目录，得到「普通文件名 → 大小」快照。目录不存在时返回空表。
///
/// `file_type` 与 `metadata` 都取自目录项本身，不额外触发 stat。
fn scan_dir(dir: &str) -> DirListing {
    let mut files = DirListing::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_file() {
            continue;
        }
        let size = entry.metadata().map_or(0, |m| m.len());
        files.insert(entry.file_name().to_string_lossy().to_string(), size);
    }
    files
}

/// 取目录内容快照（带短缓存）。目录不存在时返回空表。
///
/// 返回 `Arc` 以便多处共享同一份列表，避免每个调用方各拷贝一次上千条记录。
pub(crate) fn dir_listing(dir: &str) -> Arc<DirListing> {
    // 单测里直接绕过缓存：测试会在毫秒级内改同一目录再读（比如删掉文件后立刻
    // 重新统计缺失数），5 秒 TTL 会稳定地返回旧结果。
    //
    // 这里**不**改用目录 mtime 做校验——统计缓存早就踩过这个坑：往子目录写文件时
    // 父目录 mtime 不一定更新，靠 mtime 判断新鲜度并不可靠。生产环境的新鲜度由
    // `invalidate_stats`（写路径）和 `refresh_file_caches`（用户点刷新）保证。
    if cfg!(test) {
        return Arc::new(scan_dir(dir));
    }

    let cache = DIR_LISTING_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some((at, listing)) = guard.get(dir) {
            if at.elapsed().as_secs() < DIR_LISTING_TTL_SECS {
                return Arc::clone(listing);
            }
        }
    }

    let listing = Arc::new(scan_dir(dir));
    if let Ok(mut guard) = cache.lock() {
        guard.insert(dir.to_string(), (Instant::now(), Arc::clone(&listing)));
    }
    listing
}

/// 清空**全部**目录列表缓存。
///
/// 故意做成全清而不是按目录清：写操作（下载落盘、删除、收养）往往只拿得到
/// db_path 而不知道对应的 save_dir。目录扫描本身不贵且读多写少，
/// 全清换来的是"任何写路径都自动生效"，不会漏。
pub fn clear_dir_listings() {
    if let Some(cache) = DIR_LISTING_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.clear();
        }
    }
}

/// 清空全部统计缓存。用于「用户显式要求刷新」的场景。
pub fn clear_stats_caches() {
    if let Some(cache) = STATS_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.clear();
        }
    }
}

/// 使某个 DB 的统计缓存失效（下载完成、删除、标记、恢复等写操作后调用）。
///
/// 同时清掉目录列表缓存：能走到这里的都是"磁盘内容可能变了"的写路径，
/// 让两处缓存保持同一步调，避免统计说"文件在"而孤儿列表说"不在"。
pub fn invalidate_stats(db_path: &str) {
    if let Some(cache) = STATS_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.retain(|(db, _), _| db != db_path);
        }
    }
    clear_dir_listings();
}

fn cached_connection(db_path: &str) -> SqlResult<SharedConnection> {
    let cache = CONNECTION_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut cache = cache.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
    if let Some((conn, cached_id)) = cache.get(db_path) {
        // 命中缓存时校验文件身份：路径仍在、且 inode/文件索引未变（未被删除重建/替换）
        // 才复用；否则丢弃旧连接，否则会继续操作已 unlink 的 inode、写入静默丢失。
        let current_id = std::fs::metadata(db_path)
            .ok()
            .and_then(|m| file_identity(&m));
        if current_id.is_some() && cached_id == &current_id {
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
    let id = std::fs::metadata(db_path)
        .ok()
        .and_then(|m| file_identity(&m));
    cache.insert(db_path.to_string(), (conn.clone(), id));
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
/// 仅被 `#[cfg(test)]` 的单条插入函数使用（批量路径走 `with_cached_connection`）。
#[cfg(test)]
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

/// 表名与列名可容纳的标识符白名单。
///
/// 这两者要拼进 SQL（SQLite 不支持把表名/列名做成参数），所以必须限制取值范围。
/// 当前调用方传的都是编译期字面量，但裸 `&str` 参数一旦被误用就是注入口子，
/// 这里用断言把风险挡在函数入口。
fn assert_sql_identifier(ident: &str) -> SqlResult<()> {
    let valid = !ident.is_empty() && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid {
        Ok(())
    } else {
        log::error!("[DB] 非法 SQL 标识符被拒绝: {:?}", ident);
        Err(rusqlite::Error::InvalidParameterName(ident.to_string()))
    }
}

fn ensure_text_column(conn: &Connection, table: &str, column: &str) -> SqlResult<()> {
    assert_sql_identifier(table)?;
    assert_sql_identifier(column)?;
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
///
/// `name` 索引服务的是按文件名查/改记录的操作：
/// `mark_dislike_by_name`（标记缺失）、`get_wallhaven_image_by_name`（图片详情）。
/// 没有它这两个操作在大库上是全表扫描。
fn ensure_created_at_index(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_images_created_at ON images(created_at DESC, id DESC);
         CREATE INDEX IF NOT EXISTS idx_images_name ON images(name);",
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
