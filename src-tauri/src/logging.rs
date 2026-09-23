//! 日志初始化的统一入口：stderr + 持久文件双写。
//!
//! 从桌面启动器或双击 AppImage 启动时 stderr 无处可去（Windows 的 release 构建还带
//! `windows_subsystem = "windows"`，连控制台都没有），出问题时用户手上没有任何线索——
//! 壁纸后端探测、`[linux-env]` 环境摘要这些排查信息全都看不见。
//!
//! 这里把日志 tee 一份到持久目录，只保留最近两个文件（超过 2 MiB 滚动一次）。
//!
//! 代价：env_logger 对自定义 writer 会关掉 ANSI 颜色（它无法判断 tee 出来的流是不是
//! 终端），所以终端里的日志不再带色。换来的是终端输出与文件内容完全一致，排查时
//! 可以直接对日志文件复制粘贴。

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

/// 单个日志文件的上限；超过就在启动时滚动一次（保留 1 个 `.1` 备份）。
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

/// 初始化日志。成功时返回日志文件路径，失败则退化成只写 stderr。
pub fn init(default_filter: &str) -> Option<PathBuf> {
    let path = log_file_path();
    let target = path.as_deref().and_then(open_log_file);

    match target {
        Some(sink) => {
            env_logger::Builder::from_env(
                env_logger::Env::default().default_filter_or(default_filter),
            )
            .format_timestamp_millis()
            .target(env_logger::Target::Pipe(Box::new(sink)))
            .init();
            path
        }
        None => {
            env_logger::Builder::from_env(
                env_logger::Env::default().default_filter_or(default_filter),
            )
            .format_timestamp_millis()
            .init();
            None
        }
    }
}

/// 日志落盘目录：Linux/macOS 走 XDG 的 `~/.local/state`，Windows 落在
/// `%LOCALAPPDATA%\state`（`dirs::state_dir()` 在 Windows 上返回 None）。
fn log_file_path() -> Option<PathBuf> {
    let base = dirs::state_dir()
        .or_else(|| dirs::data_local_dir().map(|dir| dir.join("state")))
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/state")))?;
    Some(
        base.join("rustwallhub")
            .join("logs")
            .join("rustwallhub.log"),
    )
}

fn open_log_file(path: &std::path::Path) -> Option<TeeWriter> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    rotate_if_needed(path);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    Some(TeeWriter {
        file: Mutex::new(file),
    })
}

/// 超过上限就把当前文件挪成 `.1`（覆盖旧的备份），让日志不会无限增长。
/// 只保留一份备份：排查问题要的是"最近一次运行的现场"，不是历史归档。
fn rotate_if_needed(path: &std::path::Path) {
    let too_big = std::fs::metadata(path)
        .map(|meta| meta.len() > MAX_LOG_BYTES)
        .unwrap_or(false);
    if !too_big {
        return;
    }
    let backup = path.with_extension("log.1");
    let _ = std::fs::remove_file(&backup);
    if let Err(e) = std::fs::rename(path, &backup) {
        // 滚动失败不影响继续写（追加模式，最坏是文件继续变大）
        eprintln!("[logging] 日志滚动失败: {e}");
    }
}

/// 同时写 stderr 与文件的 writer。
///
/// 写文件失败时只往 stderr 提示一次并继续：日志本身出问题绝不能影响应用运行。
struct TeeWriter {
    file: Mutex<std::fs::File>,
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(buf);
        if let Ok(mut file) = self.file.lock() {
            if let Err(e) = file.write_all(buf) {
                eprintln!("[logging] 写日志文件失败: {e}");
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file_path_is_under_a_persistent_dir() {
        let path = log_file_path().expect("应该能定位到日志目录");
        assert!(
            path.ends_with("rustwallhub/logs/rustwallhub.log"),
            "{path:?}"
        );
        // 绝不能落在 /tmp 这类会被清理的位置，否则重启后现场就没了
        assert!(!path.to_string_lossy().contains("/tmp/"));
    }

    #[test]
    fn rotate_moves_oversized_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rustwallhub.log");
        std::fs::write(&path, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).unwrap();

        rotate_if_needed(&path);

        assert!(!path.exists(), "超限的日志应被挪走");
        assert!(path.with_extension("log.1").exists(), "应留下 .1 备份");
    }

    #[test]
    fn rotate_keeps_small_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rustwallhub.log");
        std::fs::write(&path, b"small").unwrap();

        rotate_if_needed(&path);

        assert!(path.exists());
        assert!(!path.with_extension("log.1").exists());
    }
}
