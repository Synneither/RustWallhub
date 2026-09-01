//! 图片记录的增删改查：存在性检查、批量入库、分页列表、缺失列表。

#[cfg(test)]
use super::open;
use super::stats::existing_file_names;
use super::{invalidate_stats, with_cached_connection, ImageRecord};
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
