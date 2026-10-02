//! 快照导出 / 导入合并（多设备同步）。
//!
//! 同步的是 `VACUUM INTO` 产出的单文件快照；合并按记录进行，绝不整库替换。

use super::with_cached_connection;
use rusqlite::{Connection, Result as SqlResult};
use serde::Serialize;
use std::path::Path;

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
            // 两条语句必须原子完成：中途失败若留下"只恢复了 love、没插入新记录"的半成品，
            // 用户看到的统计与实际数据就对不上了。ATTACH 不能在事务里做，所以事务开在它之后。
            let tx = conn.transaction()?;
            let loved = tx.execute(
                "UPDATE main.images SET love = 1
                 WHERE love = 0
                   AND EXISTS (SELECT 1 FROM incoming.images i
                               WHERE i.wallhaven_id = main.images.wallhaven_id
                                 AND i.love = 1)",
                [],
            )? as i64;
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO main.images
                    (wallhaven_id, name, hash, url, source_url, resolution, love, created_at)
                 SELECT wallhaven_id, name, hash, url, source_url, resolution, love, created_at
                 FROM incoming.images",
                [],
            )? as i64;
            commit_or_rollback(tx, inserted, loved)?;
            Ok(ImportStats { inserted, loved })
        })
    })
}

/// 从 Reddit 快照合并导入（合并键为 hash/url 唯一约束）。
pub fn import_reddit_snapshot(db_path: &str, snapshot_path: &str) -> SqlResult<ImportStats> {
    with_cached_connection(db_path, |conn| {
        attach_guard(conn, snapshot_path, |conn| {
            // 同 import_wallhaven_snapshot：两条语句包在一个事务里。
            let tx = conn.transaction()?;
            let loved = tx.execute(
                "UPDATE main.images SET love = 1
                 WHERE love = 0
                   AND EXISTS (SELECT 1 FROM incoming.images i
                               WHERE i.hash = main.images.hash
                                 AND i.love = 1)",
                [],
            )? as i64;
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO main.images
                    (name, hash, url, title, permalink, love, created_at)
                 SELECT name, hash, url, title, permalink, love, created_at
                 FROM incoming.images",
                [],
            )? as i64;
            commit_or_rollback(tx, inserted, loved)?;
            Ok(ImportStats { inserted, loved })
        })
    })
}

/// 无实际变化时**回滚**而不是提交。
///
/// 两条合并语句本身是幂等的，但"跑过语句 + COMMIT"即使一条行都没改，SQLite 也会改写主库文件
/// （实测：`COMMIT` 会让 `.db` 字节变化，`ROLLBACK` 不会；`ATTACH`/`DETACH`、空事务都不写）。
/// 那个写入是纯粹的浪费，更麻烦的是它会让上游基于文件 `mtime` 的"无需上传"判断永远失效——
/// 每次拉取都把本地库标记成"已改动"，于是下次退出又传一遍，多设备之间来回互传。
///
/// 所以只在真的改到了行时才提交。
fn commit_or_rollback(tx: rusqlite::Transaction<'_>, inserted: i64, loved: i64) -> SqlResult<()> {
    if inserted == 0 && loved == 0 {
        tx.rollback()
    } else {
        tx.commit()
    }
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
