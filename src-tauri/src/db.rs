use crate::downloader;
use rusqlite::{Connection, Result as SqlResult};
use serde::Serialize;

#[derive(Clone, Serialize, Debug)]
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
    pub total: i64,
    pub love: i64,
    pub dislike: i64,
}

pub fn init_wallhaven_db(db_path: &str) -> SqlResult<()> {
    let conn = Connection::open(db_path)?;
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
        );
        CREATE INDEX IF NOT EXISTS idx_url ON images(url);
        CREATE INDEX IF NOT EXISTS idx_hash ON images(hash);
        CREATE INDEX IF NOT EXISTS idx_wallhaven_id ON images(wallhaven_id);",
    )?;
    ensure_love_column(&conn)?;
    Ok(())
}

pub fn init_reddit_db(db_path: &str) -> SqlResult<()> {
    let conn = Connection::open(db_path)?;
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
        );
        CREATE INDEX IF NOT EXISTS idx_url ON images(url);
        CREATE INDEX IF NOT EXISTS idx_hash ON images(hash);",
    )?;
    // 迁移：为旧数据库添加 title 和 permalink 列
    conn.execute_batch("ALTER TABLE images ADD COLUMN title TEXT;")
        .ok();
    conn.execute_batch("ALTER TABLE images ADD COLUMN permalink TEXT;")
        .ok();
    ensure_love_column(&conn)?;
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
    }

    if has_stable {
        conn.execute(
            "UPDATE images SET love = stable WHERE stable IS NOT NULL",
            [],
        )?;
    }

    Ok(())
}

pub fn insert_wallhaven_image(
    db_path: &str,
    wallhaven_id: &str,
    name: &str,
    hash: &str,
    url: &str,
    source_url: &str,
    resolution: &str,
) -> SqlResult<bool> {
    let conn = Connection::open(db_path)?;
    match conn.execute(
        "INSERT INTO images (wallhaven_id, name, hash, url, source_url, resolution) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![wallhaven_id, name, hash, url, source_url, resolution],
    ) {
        Ok(_) => Ok(true),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

pub fn insert_reddit_image(
    db_path: &str,
    name: &str,
    hash: &str,
    url: &str,
    title: &str,
    permalink: &str,
) -> SqlResult<bool> {
    let conn = Connection::open(db_path)?;
    match conn.execute(
        "INSERT INTO images (name, hash, url, title, permalink) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![name, hash, url, title, permalink],
    ) {
        Ok(_) => Ok(true),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

pub fn get_existing_wallhaven_ids(db_path: &str) -> SqlResult<Vec<String>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT wallhaven_id FROM images")?;
    let ids = stmt
        .query_map([], |row| row.get(0))?
        .collect::<SqlResult<Vec<String>>>()?;
    Ok(ids)
}

/// 获取所有壁纸总数（含 wallhaven_id 列的表
/// 删除单张壁纸（按 name）
pub fn delete_wallhaven_image(db_path: &str, name: &str) -> SqlResult<bool> {
    let conn = Connection::open(db_path)?;
    let count = conn.execute(
        "DELETE FROM images WHERE name = ?1",
        rusqlite::params![name],
    )?;
    Ok(count > 0)
}

/// 删除单张 Reddit 壁纸（按 name）
pub fn delete_reddit_image(db_path: &str, name: &str) -> SqlResult<bool> {
    let conn = Connection::open(db_path)?;
    let count = conn.execute(
        "DELETE FROM images WHERE name = ?1",
        rusqlite::params![name],
    )?;
    Ok(count > 0)
}

/// 清理缩略图缓存目录中已经不存在对应原图的失效缩略图
pub fn clean_stale_thumbnails(thumbnail_dir: &str) -> std::io::Result<u64> {
    let thumb_dir_path = std::path::Path::new(thumbnail_dir);
    if !thumb_dir_path.is_dir() {
        return Ok(0);
    }
    let mut cleaned = 0u64;
    if let Ok(entries) = std::fs::read_dir(thumb_dir_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && downloader::file_is_image(&path) {
                let name = entry.file_name().to_string_lossy().to_string();
                // 原图应在 thumb_dir 的上级目录
                if let Some(parent) = thumb_dir_path.parent() {
                    let original = parent.join(&name);
                    if !original.exists() {
                        std::fs::remove_file(&path).ok();
                        cleaned += 1;
                    }
                }
            }
        }
    }
    Ok(cleaned)
}

pub fn get_existing_reddit_urls(db_path: &str) -> SqlResult<Vec<String>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT url FROM images")?;
    let urls = stmt
        .query_map([], |row| row.get(0))?
        .collect::<SqlResult<Vec<String>>>()?;
    Ok(urls)
}

pub fn get_wallhaven_stats(db_path: &str) -> SqlResult<DbStats> {
    let conn = Connection::open(db_path)?;
    ensure_love_column(&conn)?;
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))?;
    let love: i64 = conn.query_row(
        "SELECT COUNT(*) FROM images WHERE COALESCE(love, stable) = 1",
        [],
        |row| row.get(0),
    )?;
    Ok(DbStats {
        total,
        love,
        dislike: total - love,
    })
}

pub fn get_reddit_stats(db_path: &str) -> SqlResult<DbStats> {
    let conn = Connection::open(db_path)?;
    ensure_love_column(&conn)?;
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))?;
    let love: i64 = conn.query_row(
        "SELECT COUNT(*) FROM images WHERE COALESCE(love, stable) = 1",
        [],
        |row| row.get(0),
    )?;
    Ok(DbStats {
        total,
        love,
        dislike: total - love,
    })
}

pub fn mark_missing_dislike_wallhaven(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT id, name FROM images WHERE COALESCE(love, stable) = 1")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<SqlResult<Vec<_>>>()?;

    let mut updated = 0u64;
    for (id, name) in rows {
        let file_path = std::path::Path::new(save_dir).join(&name);
        if !file_path.exists() {
            conn.execute(
                "UPDATE images SET love = 0 WHERE id = ?1",
                rusqlite::params![id],
            )?;
            updated += 1;
        }
    }
    Ok(updated)
}

pub fn mark_missing_dislike_reddit(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare("SELECT id, name FROM images WHERE COALESCE(love, stable) = 1")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<SqlResult<Vec<_>>>()?;

    let mut updated = 0u64;
    for (id, name) in rows {
        let file_path = std::path::Path::new(save_dir).join(&name);
        if !file_path.exists() {
            conn.execute(
                "UPDATE images SET love = 0 WHERE id = ?1",
                rusqlite::params![id],
            )?;
            updated += 1;
        }
    }
    Ok(updated)
}

pub fn restore_love_wallhaven(db_path: &str) -> SqlResult<u64> {
    let conn = Connection::open(db_path)?;
    let count = conn.execute(
        "UPDATE images SET love = 1 WHERE COALESCE(love, stable) = 0",
        [],
    )?;
    Ok(count as u64)
}

pub fn restore_love_reddit(db_path: &str) -> SqlResult<u64> {
    let conn = Connection::open(db_path)?;
    let count = conn.execute(
        "UPDATE images SET love = 1 WHERE COALESCE(love, stable) = 0",
        [],
    )?;
    Ok(count as u64)
}

pub fn get_wallhaven_images(db_path: &str, limit: i64, offset: i64) -> SqlResult<Vec<ImageRecord>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, hash, url, COALESCE(source_url, ''), COALESCE(resolution, 'unknown'), COALESCE(love, stable, 1), COALESCE(created_at, '') FROM images ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
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
}

pub fn get_reddit_images(db_path: &str, limit: i64, offset: i64) -> SqlResult<Vec<ImageRecord>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, hash, url, COALESCE(title, ''), COALESCE(permalink, ''), COALESCE(love, stable, 1), COALESCE(created_at, '') FROM images ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
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
                source: "reddit".to_string(),
            })
        })?
        .collect::<SqlResult<Vec<_>>>()?;
    Ok(images)
}

pub fn get_wallhaven_missing_love(db_path: &str) -> SqlResult<Vec<ImageRecord>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, hash, url, COALESCE(source_url, ''), COALESCE(resolution, 'unknown'), COALESCE(love, stable, 1), COALESCE(created_at, '') FROM images WHERE COALESCE(love, stable) = 1",
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
}

pub fn get_reddit_missing_love(db_path: &str) -> SqlResult<Vec<ImageRecord>> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, hash, url, COALESCE(title, ''), COALESCE(permalink, ''), COALESCE(love, stable, 1), COALESCE(created_at, '') FROM images WHERE COALESCE(love, stable) = 1",
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
                source: "reddit".to_string(),
            })
        })?
        .collect::<SqlResult<Vec<_>>>()?;
    Ok(images)
}
