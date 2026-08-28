//! 数据库层单元测试（拆分前位于 db.rs 末尾，整体搬移至此）。

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
        insert_wallhaven_image(db.path(), "wh001", "a.jpg", "h1", "u1", "s1", "1920x1080").unwrap()
    );
    assert!(
        !insert_wallhaven_image(db.path(), "wh001", "b.jpg", "h2", "u2", "s2", "1920x1080")
            .unwrap()
    );
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
    insert_wallhaven_image(db.path(), "id1", "keep.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
    insert_wallhaven_image(db.path(), "id2", "gone.jpg", "h2", "u2", "s2", "3840x2160").unwrap();

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
    insert_wallhaven_image(db.path(), "id1", "liked.jpg", "h1", "u1", "s1", "1920x1080").unwrap();
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
    assert!(clean_stale_thumbnails(&td.to_string_lossy(), &save_dir.path().to_string_lossy()) > 0);
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
        &[(
            "id1".into(),
            "a.jpg".into(),
            "h1".into(),
            "u1".into(),
            "s1".into(),
            "1920x1080".into(),
        )],
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
    let snap = snap_dir
        .path()
        .join("snap.db")
        .to_string_lossy()
        .to_string();
    init_wallhaven_db(&snap).unwrap();
    let snap_conn = Connection::open(&snap).unwrap();
    for (wid, name, hash, love) in [
        ("id1", "a.jpg", "h1", 1),
        ("id2", "b.jpg", "h2", 1),
        ("id3", "c.jpg", "h3", 1),
    ] {
        snap_conn
            .execute(
                "INSERT INTO images (wallhaven_id, name, hash, url, source_url, resolution, love)
                     VALUES (?1, ?2, ?3, ?4, ?5, '1080p', ?6)",
                rusqlite::params![
                    wid,
                    name,
                    hash,
                    format!("u-{wid}"),
                    format!("s-{wid}"),
                    love
                ],
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
    let snap = snap_dir
        .path()
        .join("snap.db")
        .to_string_lossy()
        .to_string();
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
