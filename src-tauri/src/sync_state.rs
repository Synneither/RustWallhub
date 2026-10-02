//! 同步链路的本地簿记——用来跳过"什么都没变"的同步。
//!
//! 存两类标记：
//! - `remote_etags`：上次**成功导入**的远端快照 ETag。远端 ETag 没变时，下载 + 合并必然是
//!   空操作，可以整段跳过（省掉每次启动约 600ms 的全量下载与一次数据库写入）。
//! - `uploaded_marks`：上次**成功上传**时本地数据库文件的 `(mtime, size)`。两者都没变就说明
//!   本地没有新数据，退出时无需上传（省掉每次退出约 250ms，并避免网络异常时卡住退出）。
//!
//! # 为什么放在这里而不是 `AppConfig`
//!
//! 配置文件里就有 OSS 凭据，用户很可能手工复制到另一台机器。若"上次导入的 ETag"跟着配置过去，
//! 新设备会误判"远端无变化"而跳过首次全量同步——那是**静默丢数据**。所以簿记刻意放在
//! 机器本地的状态目录（`state::app_state_dir()`），与配置解耦。
//!
//! 落盘失败只记日志：簿记丢了最坏也只是多做一次同步，不该影响同步本身。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 文件内容之外、足以判断"是否变过"的最小标记。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileMark {
    /// 修改时间（自 Unix 纪元起的毫秒）
    pub mtime_ms: u64,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SyncState {
    /// 远端快照标记：key = 快照对象 key（含 OSS prefix），value = ETag。
    /// 带 key 是为了让用户改 prefix/bucket 后自动失效，而不是错误命中旧记录。
    #[serde(default)]
    pub remote_etags: HashMap<String, String>,
    /// 上次上传时本地库的标记：key = 数据库文件路径。
    /// 用路径作 key，路径改了自然没有基线，于是照常上传（保守方向）。
    #[serde(default)]
    pub uploaded_marks: HashMap<String, FileMark>,
}

/// 读文件的 `(mtime, size)`。文件不存在或取不到元信息时返回 `None`
/// （调用方一律当作"变化了"，宁可多做一次同步）。
pub fn file_mark(path: &str) -> Option<FileMark> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let mtime_ms = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some(FileMark {
        mtime_ms,
        size: meta.len(),
    })
}

fn state_file() -> Option<PathBuf> {
    Some(crate::state::app_state_dir()?.join("sync-state.json"))
}

/// 串行化"读-改-写"：启动拉取与退出上传可能并发改这个文件。
static FILE_LOCK: Mutex<()> = Mutex::new(());

/// 读取簿记。文件不存在（首次运行）或内容损坏时返回默认值，都不算错误。
pub fn load() -> SyncState {
    let Some(path) = state_file() else {
        return SyncState::default();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            log::warn!("[sync-state] 解析失败，按空簿记处理：{e}");
            SyncState::default()
        }),
        Err(_) => SyncState::default(),
    }
}

/// 读 - 改 - 写。整体持锁，写入走临时文件 + rename（避免半截 JSON）。
pub fn update<F: FnOnce(&mut SyncState)>(f: F) {
    let Some(path) = state_file() else {
        return;
    };
    // 锁被 poison 说明上次有人 panic：簿记本身无一致性风险，取回内部值继续用。
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load();
    f(&mut state);
    if let Err(e) = write_atomic(&path, &state) {
        log::warn!("[sync-state] 写入失败 {}: {e}", path.display());
    }
}

fn write_atomic(path: &Path, state: &SyncState) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn file_mark_tracks_mtime_and_size() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("a.db");
        std::fs::write(&p, b"hello").unwrap();
        let a = file_mark(&p.to_string_lossy()).expect("应能取到标记");
        assert_eq!(a.size, 5);

        // 内容变了 → 标记必须变（哪怕大小相同）
        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(&p, b"world").unwrap();
        let b = file_mark(&p.to_string_lossy()).expect("应能取到标记");
        assert_eq!(b.size, 5);
        assert_ne!(a, b, "同样大小但内容变了，mtime 也必须能分辨出来");
    }

    #[test]
    fn file_mark_missing_file_is_none() {
        assert!(file_mark("C:/definitely/not/here/nope.db").is_none());
    }

    #[test]
    fn state_round_trips_through_json() {
        // 簿记要跨进程存活，字段改动必须保持向后/向前可读。
        let mut s = SyncState::default();
        s.remote_etags.insert("k".into(), "etag1".into());
        s.uploaded_marks.insert(
            "C:/db/a.db".into(),
            FileMark {
                mtime_ms: 123,
                size: 456,
            },
        );
        let bytes = serde_json::to_vec_pretty(&s).unwrap();
        let back: SyncState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            back.remote_etags.get("k").map(String::as_str),
            Some("etag1")
        );
        assert_eq!(
            back.uploaded_marks.get("C:/db/a.db"),
            Some(&FileMark {
                mtime_ms: 123,
                size: 456
            })
        );
    }

    #[test]
    fn missing_fields_default_to_empty() {
        // 旧版本写的文件（只有部分字段）不该让同步直接失败。
        let s: SyncState = serde_json::from_str("{\"remote_etags\":{\"a\":\"b\"}}").unwrap();
        assert_eq!(s.remote_etags.len(), 1);
        assert!(s.uploaded_marks.is_empty());
        let empty: SyncState = serde_json::from_str("{}").unwrap();
        assert!(empty.remote_etags.is_empty() && empty.uploaded_marks.is_empty());
    }
}
