//! 外部命令查找与构造。
//!
//! 桌面集成（壁纸后端、回收站、桌面项）都要 spawn 第三方命令，而 GUI 启动的进程
//! 拿到的 `PATH` 常常是裁剪过的版本，所以这里统一做一次「PATH + 补充目录」解析。

use std::path::{Path, PathBuf};
use std::process::Command;

/// PATH 之外还要补查的 bin 目录。
///
/// AppImage 从文件管理器 / niri 的 spawn 启动时，继承到的 PATH 往往被裁剪过
/// （缺少登录 shell 才注入的 `~/.local/bin`、Nix profile 等），
/// 而 `noctalia`、`awww`、`gio` 恰好常装在这些位置。少了这一步会表现为
/// “明明装了却探测不到”。
fn extra_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".nix-profile/bin"));
        dirs.push(home.join("bin"));
    }
    // NixOS / Nix 单用户与多用户安装
    dirs.push(PathBuf::from("/run/current-system/sw/bin"));
    dirs.push(PathBuf::from("/nix/var/nix/profiles/default/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/usr/bin"));
    dirs
}

/// 在给定目录列表里查找可执行文件，返回首个命中项。
fn find_in_dirs(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
}

/// 文件存在且可执行。Unix 上要求任一执行位，否则 `~/.local/bin` 里同名的
/// 普通文件会被误判成命令。
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// 解析命令的实际路径：带路径分隔符的按原样使用，否则先查 PATH、再查补充目录。
/// 都没找到时返回 `None`，由 [`cmd`] 退回裸命令名让系统自己再找一次。
pub fn resolve_executable(name: &str) -> Option<PathBuf> {
    if name.contains('/') || name.contains('\\') {
        let path = PathBuf::from(name);
        return is_executable_file(&path).then_some(path);
    }
    let mut dirs: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    dirs.extend(extra_bin_dirs());
    find_in_dirs(name, &dirs)
}

/// 构造已解析路径的 `Command`（找不到时用裸名字，行为与直接 `Command::new` 一致）。
pub fn cmd(name: &str) -> Command {
    match resolve_executable(name) {
        Some(path) => Command::new(path),
        None => Command::new(name),
    }
}

/// 该命令是否可用（不实际执行）。
pub fn has_command(name: &str) -> bool {
    resolve_executable(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch_executable(path: &Path) {
        std::fs::write(path, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn find_in_dirs_finds_executable() {
        let dir = TempDir::new().unwrap();
        let bin = dir.path().join("noctalia");
        touch_executable(&bin);

        let dirs = vec![dir.path().to_path_buf()];
        assert_eq!(find_in_dirs("noctalia", &dirs), Some(bin));
        assert_eq!(find_in_dirs("definitely-missing", &dirs), None);
    }

    #[test]
    fn find_in_dirs_ignores_directories() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("niri")).unwrap();

        // 同名目录不能当成命令，否则会去 spawn 一个目录（永远失败）。
        assert_eq!(find_in_dirs("niri", &[dir.path().to_path_buf()]), None);
    }

    #[cfg(unix)]
    #[test]
    fn find_in_dirs_ignores_non_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("awww");
        std::fs::write(&file, b"").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(find_in_dirs("awww", &[dir.path().to_path_buf()]), None);
    }

    #[test]
    fn resolve_executable_handles_explicit_paths() {
        let dir = TempDir::new().unwrap();
        let bin = dir.path().join("swaymsg");
        touch_executable(&bin);

        let explicit = bin.to_string_lossy().to_string();
        assert_eq!(resolve_executable(&explicit), Some(bin));
        assert_eq!(resolve_executable("/nonexistent/dir/awww"), None);
    }
}
