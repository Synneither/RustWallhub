use crate::downloader;
use rusqlite::{Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    pub total: i64,
    pub love: i64,
    pub dislike: i64,
}

fn open(db_path: &str) -> SqlResult<Connection> {
    Connection::open(db_path)
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

/// SQLite 性能调优：WAL 模式 + 缓存
fn optimize_db(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA cache_size=-64000;
         PRAGMA temp_store=MEMORY;
         PRAGMA busy_timeout=5000;",
    )
}

pub fn init_wallhaven_db(db_path: &str) -> SqlResult<()> {
    log::info!("[DB] init_wallhaven_db: path={}", db_path);
    let conn = open(db_path)?;
    optimize_db(&conn)?;
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
    ensure_love_column(&conn)
}

pub fn init_reddit_db(db_path: &str) -> SqlResult<()> {
    log::info!("[DB] init_reddit_db: path={}", db_path);
    let conn = open(db_path)?;
    optimize_db(&conn)?;
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
    conn.execute_batch("ALTER TABLE images ADD COLUMN title TEXT;").ok();
    conn.execute_batch("ALTER TABLE images ADD COLUMN permalink TEXT;").ok();
    ensure_love_column(&conn)
}

pub fn get_existing_wallhaven_ids(db_path: &str) -> SqlResult<Vec<String>> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare("SELECT wallhaven_id FROM images")?;
    let ids = stmt
        .query_map([], |row| row.get(0))?
        .collect::<SqlResult<Vec<String>>>()?;
    log::info!("[DB] get_existing_wallhaven_ids: {} ids from {}", ids.len(), db_path);
    Ok(ids)
}

pub fn get_existing_reddit_urls(db_path: &str) -> SqlResult<Vec<String>> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare("SELECT url FROM images")?;
    let urls = stmt
        .query_map([], |row| row.get(0))?
        .collect::<SqlResult<Vec<String>>>()?;
    log::info!("[DB] get_existing_reddit_urls: {} urls from {}", urls.len(), db_path);
    Ok(urls)
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
        log::info!("[DB] insert_wallhaven_image: id={} name={}", wallhaven_id, name);
    }
    Ok(result)
}

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
    }
    Ok(result)
}

pub fn insert_wallhaven_images_batch(
    db_path: &str,
    images: &[(String, String, String, String, String, String)],
) -> SqlResult<(u64, u64)> {
    let mut conn = open(db_path)?;
    let tx = conn.transaction()?;
    let mut added = 0u64;
    let mut skipped = 0u64;
    for (wallhaven_id, name, hash, url, source_url, resolution) in images {
        match tx.execute(
            "INSERT INTO images (wallhaven_id, name, hash, url, source_url, resolution) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![wallhaven_id, name, hash, url, source_url, resolution],
        ) {
            Ok(_) => added += 1,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                skipped += 1;
            }
            Err(e) => return Err(e),
        }
    }
    tx.commit()?;
    log::info!("[DB] insert_wallhaven_images_batch: added={} skipped={}", added, skipped);
    Ok((added, skipped))
}

pub fn insert_reddit_images_batch(
    db_path: &str,
    images: &[(String, String, String, String, String)],
) -> SqlResult<(u64, u64)> {
    let mut conn = open(db_path)?;
    let tx = conn.transaction()?;
    let mut added = 0u64;
    let mut skipped = 0u64;
    for (name, hash, url, title, permalink) in images {
        match tx.execute(
            "INSERT INTO images (name, hash, url, title, permalink) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![name, hash, url, title, permalink],
        ) {
            Ok(_) => added += 1,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                skipped += 1;
            }
            Err(e) => return Err(e),
        }
    }
    tx.commit()?;
    log::info!("[DB] insert_reddit_images_batch: added={} skipped={}", added, skipped);
    Ok((added, skipped))
}

pub fn delete_image_by_name(db_path: &str, name: &str) -> SqlResult<bool> {
    log::info!("[DB] delete_image_by_name: name={}", name);
    let conn = open(db_path)?;
    let count = conn.execute("DELETE FROM images WHERE name = ?1", rusqlite::params![name])?;
    log::info!("[DB] delete_image_by_name: deleted={}", count > 0);
    Ok(count > 0)
}

pub fn delete_wallhaven_image(db_path: &str, name: &str) -> SqlResult<bool> {
    delete_image_by_name(db_path, name)
}

pub fn delete_reddit_image(db_path: &str, name: &str) -> SqlResult<bool> {
    delete_image_by_name(db_path, name)
}

pub fn clean_stale_thumbnails(thumbnail_dir: &str, save_dir: &str) -> std::io::Result<u64> {
    let thumb_dir_path = Path::new(thumbnail_dir);
    if !thumb_dir_path.is_dir() {
        return Ok(0);
    }
    let mut cleaned = 0u64;
    if let Ok(entries) = std::fs::read_dir(thumb_dir_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && downloader::file_is_image(&path) {
                let name = entry.file_name().to_string_lossy().to_string();
                let original = Path::new(save_dir).join(&name);
                if !original.exists() {
                    std::fs::remove_file(&path).ok();
                    cleaned += 1;
                }
            }
        }
    }
    log::info!("[DB] clean_stale_thumbnails: cleaned={}", cleaned);
    Ok(cleaned)
}

pub fn get_db_stats(db_path: &str) -> SqlResult<DbStats> {
    let conn = open(db_path)?;
    let (total, love): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN love=1 THEN 1 ELSE 0 END), 0) FROM images",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let stats = DbStats { total, love, dislike: total - love };
    log::info!("[DB] get_db_stats({}): {:?}", db_path, stats);
    Ok(stats)
}

fn mark_missing_dislike(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    log::info!("[DB] mark_missing_dislike: dir={}", save_dir);
    let mut conn = open(db_path)?;
    let tx = conn.transaction()?;
    let rows: Vec<(i64, String)> = {
        let mut stmt = tx.prepare("SELECT id, name FROM images WHERE love = 1")?;
        let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        mapped.collect::<SqlResult<Vec<_>>>()?
    };
    let mut updated = 0u64;
    for (id, name) in rows {
        let file_path = Path::new(save_dir).join(&name);
        if !file_path.exists() {
            tx.execute(
                "UPDATE images SET love = 0 WHERE id = ?1",
                rusqlite::params![id],
            )?;
            updated += 1;
        }
    }
    tx.commit()?;
    log::info!("[DB] mark_missing_dislike: updated={}", updated);
    Ok(updated)
}

fn count_missing(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare("SELECT name FROM images WHERE love = 1")?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<SqlResult<Vec<_>>>()?;

    let mut missing = 0u64;
    for name in &names {
        let file_path = Path::new(save_dir).join(name);
        if !file_path.exists() {
            missing += 1;
        }
    }
    log::info!("[DB] count_missing: {} missing in {}", missing, save_dir);
    Ok(missing)
}

pub fn count_missing_wallhaven(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    count_missing(db_path, save_dir)
}

pub fn count_missing_reddit(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    count_missing(db_path, save_dir)
}

pub fn mark_missing_dislike_wallhaven(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    mark_missing_dislike(db_path, save_dir)
}

pub fn mark_missing_dislike_reddit(db_path: &str, save_dir: &str) -> SqlResult<u64> {
    mark_missing_dislike(db_path, save_dir)
}

fn restore_love(db_path: &str) -> SqlResult<u64> {
    let conn = open(db_path)?;
    let count = conn.execute(
        "UPDATE images SET love = 1 WHERE love = 0",
        [],
    )?;
    log::info!("[DB] restore_love: restored={}", count);
    Ok(count as u64)
}

pub fn restore_love_db(db_path: &str) -> SqlResult<u64> {
    restore_love(db_path)
}

pub fn mark_dislike_by_name(db_path: &str, name: &str) -> SqlResult<bool> {
    let conn = open(db_path)?;
    let count = conn.execute(
        "UPDATE images SET love = 0 WHERE name = ?1",
        rusqlite::params![name],
    )?;
    if count > 0 {
        log::info!("[DB] mark_dislike_by_name: name={}", name);
    }
    Ok(count > 0)
}

pub fn get_wallhaven_images(db_path: &str, limit: i64, offset: i64) -> SqlResult<Vec<ImageRecord>> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, hash, url, COALESCE(source_url, ''), COALESCE(resolution, 'unknown'), COALESCE(love, 1), COALESCE(created_at, '') FROM images ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
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
    let conn = open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, hash, url, COALESCE(title, ''), COALESCE(permalink, ''), COALESCE(love, 1), COALESCE(created_at, '') FROM images ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
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
}

pub fn get_wallhaven_missing_love(db_path: &str) -> SqlResult<Vec<ImageRecord>> {
    let conn = open(db_path)?;
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
}

pub fn get_wallhaven_missing_files(db_path: &str, save_dir: &str) -> SqlResult<Vec<ImageRecord>> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, hash, url, COALESCE(source_url, ''), COALESCE(resolution, 'unknown'), COALESCE(love, 1), COALESCE(created_at, '') FROM images WHERE love = 1 ORDER BY created_at DESC",
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
        .filter(|img| !Path::new(save_dir).join(&img.name).exists())
        .collect();
    Ok(images)
}

pub fn get_reddit_missing_files(db_path: &str, save_dir: &str) -> SqlResult<Vec<ImageRecord>> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, hash, url, COALESCE(title, ''), COALESCE(permalink, ''), COALESCE(love, 1), COALESCE(created_at, '') FROM images WHERE love = 1 ORDER BY created_at DESC",
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
        .filter(|img| !Path::new(save_dir).join(&img.name).exists())
        .collect();
    Ok(images)
}

pub fn get_all_filenames(db_path: &str) -> SqlResult<Vec<String>> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare("SELECT name FROM images")?;
    let names = stmt
        .query_map([], |row| row.get(0))?
        .collect::<SqlResult<Vec<String>>>()?;
    Ok(names)
}

pub fn get_reddit_missing_love(db_path: &str) -> SqlResult<Vec<ImageRecord>> {
    let conn = open(db_path)?;
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
        let count: i64 = db.conn().query_row("SELECT COUNT(*) FROM images", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_init_reddit_db() {
        let db = TestDb::reddit();
        let count: i64 = db.conn().query_row("SELECT COUNT(*) FROM images", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_insert_wallhaven_image() {
        let db = TestDb::wallhaven();
        assert!(insert_wallhaven_image(db.path(), "wh001", "wh001.jpg", "abc123", "https://wh.cc/i/wh001", "https://wh.cc/s/wh001", "1920x1080").unwrap());
    }

    #[test]
    fn test_insert_wallhaven_duplicate() {
        let db = TestDb::wallhaven();
        assert!(insert_wallhaven_image(db.path(), "wh001", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap());
        assert!(!insert_wallhaven_image(db.path(), "wh001", "b.jpg", "h2", "u2", "s2", "1920x1080").unwrap());
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
    fn test_count_missing_wallhaven() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"fake").unwrap();
        assert_eq!(count_missing_wallhaven(db.path(), &dir.path().to_string_lossy()).unwrap(), 0);

        std::fs::remove_file(dir.path().join("a.jpg")).unwrap();
        assert_eq!(count_missing_wallhaven(db.path(), &dir.path().to_string_lossy()).unwrap(), 1);
    }

    #[test]
    fn test_insert_reddit_image() {
        let db = TestDb::reddit();
        assert!(insert_reddit_image(db.path(), "img.jpg", "def456", "https://reddit.com/img.jpg", "title", "/r/123").unwrap());
    }

    #[test]
    fn test_insert_reddit_duplicate_url() {
        let db = TestDb::reddit();
        assert!(insert_reddit_image(db.path(), "a.jpg", "h1", "https://reddit.com/1", "t", "/r/1").unwrap());
        assert!(!insert_reddit_image(db.path(), "b.jpg", "h2", "https://reddit.com/1", "t", "/r/2").unwrap());
    }

    #[test]
    fn test_get_existing_reddit_urls() {
        let db = TestDb::reddit();
        insert_reddit_image(db.path(), "a.jpg", "h1", "https://reddit.com/1", "t1", "/r/1").unwrap();
        insert_reddit_image(db.path(), "b.jpg", "h2", "https://reddit.com/2", "t2", "/r/2").unwrap();
        assert_eq!(get_existing_reddit_urls(db.path()).unwrap().len(), 2);
    }

    #[test]
    fn test_get_db_stats() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        insert_wallhaven_image(db.path(), "id2", "b.jpg", "h2", "u2", "s2", "3840x2160").unwrap();
        let stats = get_db_stats(db.path()).unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.love, 2);
    }

    #[test]
    fn test_mark_dislike_and_restore_wallhaven() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "keep.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        insert_wallhaven_image(db.path(), "id2", "gone.jpg", "h2", "u2", "s2", "3840x2160").unwrap();

        let img_dir = TempDir::new().unwrap();
        std::fs::write(img_dir.path().join("keep.jpg"), b"fake").unwrap();
        assert_eq!(mark_missing_dislike_wallhaven(db.path(), &img_dir.path().to_string_lossy()).unwrap(), 1);

        assert_eq!(get_db_stats(db.path()).unwrap().love, 1);
        assert_eq!(restore_love_db(db.path()).unwrap(), 1);
        assert_eq!(get_db_stats(db.path()).unwrap().love, 2);
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
        insert_reddit_image(db.path(), "a.jpg", "h1", "https://reddit.com/1", "t1", "/r/1").unwrap();

        let img_dir = TempDir::new().unwrap();
        std::fs::write(img_dir.path().join("a.jpg"), b"data").unwrap();
        assert_eq!(get_reddit_missing_love(db.path()).unwrap().len(), 1);
    }

    #[test]
    fn test_delete_wallhaven() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "del.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        assert!(delete_wallhaven_image(db.path(), "del.jpg").unwrap());
        assert_eq!(get_db_stats(db.path()).unwrap().total, 0);
    }

    #[test]
    fn test_delete_nonexistent_wallhaven() {
        let db = TestDb::wallhaven();
        assert!(!delete_wallhaven_image(db.path(), "noexist.jpg").unwrap());
    }

    #[test]
    fn test_delete_reddit() {
        let db = TestDb::reddit();
        insert_reddit_image(db.path(), "del.jpg", "h1", "https://reddit.com/1", "t1", "/r/1").unwrap();
        assert!(delete_reddit_image(db.path(), "del.jpg").unwrap());
        assert_eq!(get_db_stats(db.path()).unwrap().total, 0);
    }

    #[test]
    fn test_get_wallhaven_images_pagination() {
        let db = TestDb::wallhaven();
        for i in 0..5 {
            insert_wallhaven_image(db.path(), &format!("id{i}"), &format!("{i}.jpg"), &format!("h{i}"), &format!("u{i}"), &format!("s{i}"), "1920x1080").unwrap();
        }
        assert_eq!(get_wallhaven_images(db.path(), 2, 0).unwrap().len(), 2);
        assert_eq!(get_wallhaven_images(db.path(), 10, 0).unwrap().len(), 5);
    }

    #[test]
    fn test_get_reddit_images() {
        let db = TestDb::reddit();
        insert_reddit_image(db.path(), "a.jpg", "h1", "https://reddit.com/1", "title1", "/r/1").unwrap();
        let images = get_reddit_images(db.path(), 10, 0).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].title, Some("title1".to_string()));
    }

    #[test]
    fn test_mark_dislike_by_name_wallhaven() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        assert!(mark_dislike_by_name(db.path(), "a.jpg").unwrap());
        let stats = get_db_stats(db.path()).unwrap();
        assert_eq!(stats.love, 0);
        assert_eq!(stats.dislike, 1);
    }

    #[test]
    fn test_mark_dislike_by_name_reddit() {
        let db = TestDb::reddit();
        insert_reddit_image(db.path(), "a.jpg", "h1", "https://reddit.com/1", "t1", "/r/1").unwrap();
        assert!(mark_dislike_by_name(db.path(), "a.jpg").unwrap());
        let stats = get_db_stats(db.path()).unwrap();
        assert_eq!(stats.love, 0);
        assert_eq!(stats.dislike, 1);
    }

    #[test]
    fn test_mark_dislike_by_name_nonexistent() {
        let db = TestDb::wallhaven();
        assert!(!mark_dislike_by_name(db.path(), "noexist.jpg").unwrap());
    }

    #[test]
    fn test_get_wallhaven_missing_files_with_dir() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "exists.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        insert_wallhaven_image(db.path(), "id2", "missing.jpg", "h2", "u2", "s2", "3840x2160").unwrap();

        let img_dir = TempDir::new().unwrap();
        std::fs::write(img_dir.path().join("exists.jpg"), b"real file").unwrap();

        let missing = get_wallhaven_missing_files(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "missing.jpg");
    }

    #[test]
    fn test_get_reddit_missing_files_with_dir() {
        let db = TestDb::reddit();
        insert_reddit_image(db.path(), "exists.jpg", "h1", "https://reddit.com/1", "t1", "/r/1").unwrap();
        insert_reddit_image(db.path(), "missing.png", "h2", "https://reddit.com/2", "t2", "/r/2").unwrap();

        let img_dir = TempDir::new().unwrap();
        std::fs::write(img_dir.path().join("exists.jpg"), b"real file").unwrap();

        let missing = get_reddit_missing_files(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "missing.png");
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
    fn test_get_all_filenames_reddit() {
        let db = TestDb::reddit();
        insert_reddit_image(db.path(), "x.jpg", "h1", "https://reddit.com/1", "t1", "/r/1").unwrap();
        insert_reddit_image(db.path(), "y.jpg", "h2", "https://reddit.com/2", "t2", "/r/2").unwrap();
        let names = get_all_filenames(db.path()).unwrap();
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn test_get_missing_files_excludes_disliked() {
        let db = TestDb::wallhaven();
        insert_wallhaven_image(db.path(), "id1", "liked.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
        insert_wallhaven_image(db.path(), "id2", "disliked.jpg", "h2", "u2", "s2", "3840x2160").unwrap();

        let img_dir = TempDir::new().unwrap();
        // 标记 dislike 为不喜欢
        mark_dislike_by_name(db.path(), "disliked.jpg").unwrap();

        let missing = get_wallhaven_missing_files(db.path(), &img_dir.path().to_string_lossy()).unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "liked.jpg");
    }

    #[test]
    fn test_love_column_migration() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("migrate.db").to_string_lossy().to_string();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE images (id INTEGER PRIMARY KEY, name TEXT, hash TEXT, url TEXT, stable INTEGER DEFAULT 1);").unwrap();
        ensure_love_column(&conn).unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(images)").unwrap();
        let cols: Vec<String> = stmt.query_map([], |r| r.get(1)).unwrap().filter_map(|c| c.ok()).collect();
        assert!(cols.contains(&"love".to_string()));
    }

    #[test]
    fn test_clean_stale_thumbnails() {
        let save_dir = TempDir::new().unwrap();
        let thumb_dir = TempDir::new().unwrap();
        let td = thumb_dir.path().to_path_buf();
        std::fs::write(td.join("orphan.jpg"), b"fake").unwrap();
        assert!(clean_stale_thumbnails(&td.to_string_lossy(), &save_dir.path().to_string_lossy()).unwrap() > 0);
    }

    #[test]
    fn test_clean_stale_thumbnails_keeps_valid() {
        let save_dir = TempDir::new().unwrap();
        let thumb_dir = TempDir::new().unwrap();
        let td = thumb_dir.path().to_path_buf();
        std::fs::write(save_dir.path().join("valid.jpg"), b"data").unwrap();
        std::fs::write(td.join("valid.jpg"), b"thumb").unwrap();
        assert_eq!(clean_stale_thumbnails(&td.to_string_lossy(), &save_dir.path().to_string_lossy()).unwrap(), 0);
    }
}
