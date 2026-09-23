//! 把文件移入系统回收站。
//!
//! 图库删除是这个应用里唯一**不可逆**的操作（不喜欢 / 孤儿清理此前都是直接跑
//! `std::fs::remove_file`），而桌面惯例是先进回收站。这里把它换掉。
//!
//! 原则：**移入回收站失败就报错，绝不静默降级成永久删除**——宁可让用户看到失败，
//! 也不能在"以为进了回收站"的前提下把文件彻底删掉。
//!
//! - Linux：优先调 `gio trash`（由 glib 实现，会处理每设备回收站目录、挂载点与权限），
//!   不可用或失败时退回本模块自带的 XDG Trash 实现。
//! - Windows：`SHFileOperationW` + `FOF_ALLOWUNDO`（即资源管理器那套回收站）。
//!
//! 缩略图缓存仍走 `thumbnail::remove_thumbnails` 真删：它可再生，进回收站只会堆垃圾。
//!
//! 自带的 XDG 实现刻意写成纯 std、不依赖平台 API，这样在 macOS/Windows 上也会被编译，
//! 单测能本地跑到（否则这块只有 CI 的 Linux job 才会编到）。

use crate::state::AppError;
use std::path::{Path, PathBuf};

/// 把 `path` 移入回收站。文件本就不存在时视为成功（调用方是幂等的删除流程）。
pub fn move_to_trash(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        windows::recycle(path)
    }
    #[cfg(not(windows))]
    {
        linux::trash(path)
    }
}

// ---------------------------------------------------------------------------
// Linux：gio 优先，退回自带 XDG 实现
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
mod linux {
    use super::*;
    use crate::exec::{cmd, has_command};

    pub(super) fn trash(path: &Path) -> Result<(), AppError> {
        let absolute = path
            .canonicalize()
            .map_err(|e| AppError::Other(format!("获取绝对路径失败: {e}")))?;

        if has_command("gio") {
            // 路径先 canonicalize 过，一定是 `/` 开头的绝对路径，不会被当成选项解析，
            // 所以不需要 `--`（gio 对未知的 `--` 处理并不一致，索性不加）。
            match cmd("gio").arg("trash").arg(&absolute).output() {
                Ok(out) if out.status.success() => {
                    log::info!("[trash] gio trash 成功: {}", absolute.display());
                    return Ok(());
                }
                Ok(out) => log::warn!(
                    "[trash] gio trash 退出码 {:?}: {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
                Err(e) => log::warn!("[trash] 无法执行 gio trash: {e}"),
            }
        }

        let data_dir = xdg_data_home().ok_or_else(|| {
            AppError::Other("无法定位 XDG 数据目录（$XDG_DATA_HOME 与 $HOME 都不可用）".into())
        })?;
        super::xdg::trash_with(&data_dir.join("Trash"), &absolute)
            .map(|_| ())
            .map_err(|e| AppError::Other(format!("移入回收站失败: {e}")))
    }

    fn xdg_data_home() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
    }
}

// ---------------------------------------------------------------------------
// XDG Trash 规范的最小实现（纯 std，任何平台都能编译与测试）
// ---------------------------------------------------------------------------

#[cfg_attr(windows, allow(dead_code))] // Windows 走回收站 API，这块只在 Linux 用
mod xdg {
    use super::*;
    use std::io;

    /// 回收站里的落点：`files/` 存文件本体，`info/*.trashinfo` 存原始路径与删除时间。
    pub(super) fn trash_with(base: &Path, source: &Path) -> io::Result<PathBuf> {
        let files_dir = base.join("files");
        let info_dir = base.join("info");
        std::fs::create_dir_all(&files_dir)?;
        std::fs::create_dir_all(&info_dir)?;

        let name = source
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "路径没有文件名"))?
            .to_string_lossy()
            .to_string();
        let (stem, ext) = split_name(&name);

        // 重名时按 `name.2.ext`、`name.3.ext` 递增，直到 files/ 与 info/ 都不冲突。
        let mut candidate = name;
        let mut counter = 1u32;
        while files_dir.join(&candidate).exists()
            || info_dir.join(format!("{candidate}.trashinfo")).exists()
        {
            counter += 1;
            if counter > 10_000 {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "回收站里同名文件过多",
                ));
            }
            candidate = match ext {
                Some(ref ext) => format!("{stem}.{counter}.{ext}"),
                None => format!("{stem}.{counter}"),
            };
        }

        let target = files_dir.join(&candidate);
        move_path(source, &target)?;

        // 先移文件再写 info：写 info 失败就把文件挪回去，宁可整体失败也不留半个垃圾条目。
        let info_path = info_dir.join(format!("{candidate}.trashinfo"));
        if let Err(e) = write_trashinfo(&info_path, source) {
            let _ = std::fs::rename(&target, source);
            return Err(e);
        }
        log::info!(
            "[trash] {} → {} (XDG 内置实现)",
            source.display(),
            target.display()
        );
        Ok(target)
    }

    /// 同设备直接 rename；跨设备（EXDEV，比如从外挂盘删）退化成复制 + 删除。
    ///
    /// 规范建议跨设备时用该设备顶层的 `.Trash-$uid`，但那条路径要额外处理挂载点权限，
    /// 而回收站内容对用户是等价的，所以这里只做复制。目录不处理——本应用的删除对象
    /// 都是图片文件。
    fn move_path(source: &Path, target: &Path) -> io::Result<()> {
        match std::fs::rename(source, target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
                if source.is_dir() {
                    return Err(e);
                }
                std::fs::copy(source, target)?;
                std::fs::remove_file(source)
            }
            Err(e) => Err(e),
        }
    }

    fn write_trashinfo(info_path: &Path, source: &Path) -> io::Result<()> {
        let content = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            crate::state::escape_path_percent(&source.to_string_lossy()),
            deletion_date(),
        );
        std::fs::write(info_path, content)
    }

    /// `DeletionDate` 要求**本地时间**的 `YYYY-MM-DDThh:mm:ss`。为一行时间戳引一个时区库
    /// 不划算，交给 `date` 命令；取不到就留空（回收站 UI 显示"未知时间"，条目仍可用）。
    /// 这条只在 `gio` 缺席的回退路径上执行，不在热路径上。
    fn deletion_date() -> String {
        crate::exec::cmd("date")
            .arg("+%Y-%m-%dT%H:%M:%S")
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .unwrap_or_default()
    }

    /// 拆出扩展名用于重名编号；`.hidden`（点开头）按无扩展名处理。
    pub(super) fn split_name(name: &str) -> (String, Option<String>) {
        match name.rfind('.') {
            Some(idx) if idx > 0 => (name[..idx].to_string(), Some(name[idx + 1..].to_string())),
            _ => (name.to_string(), None),
        }
    }
}

// ---------------------------------------------------------------------------
// Windows：SHFileOperationW
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::ffi::OsStrExt;

    const FO_DELETE: u32 = 0x0003;
    const FOF_SILENT: u16 = 0x0004;
    const FOF_NOCONFIRMATION: u16 = 0x0010;
    const FOF_ALLOWUNDO: u16 = 0x0040;
    const FOF_NOERRORUI: u16 = 0x0400;

    #[repr(C)]
    struct ShFileOpStructW {
        hwnd: *mut std::ffi::c_void,
        w_func: u32,
        p_from: *const u16,
        p_to: *const u16,
        f_flags: u16,
        f_any_operations_aborted: i32,
        h_name_mappings: *mut std::ffi::c_void,
        lpsz_progress_title: *const u16,
    }

    // 显式链接 shell32：不指望工具链默认库列表里恰好带上它。
    #[link(name = "shell32")]
    extern "system" {
        // Rust 的 non_snake_case 会管到 extern 声明，所以本地名用蛇形 + link_name 指向真实符号。
        #[link_name = "SHFileOperationW"]
        fn sh_file_operation_w(file_op: *mut ShFileOpStructW) -> i32;
    }

    /// 回收站（FOF_ALLOWUNDO）。`pFrom` 是**双 NUL 结尾**的宽字符路径列表。
    pub(super) fn recycle(path: &Path) -> Result<(), AppError> {
        let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
        from.push(0);
        from.push(0);

        let mut op = ShFileOpStructW {
            hwnd: std::ptr::null_mut(),
            w_func: FO_DELETE,
            p_from: from.as_ptr(),
            p_to: std::ptr::null(),
            // 不弹确认框与错误弹窗：UI 侧已经确认过了，失败靠返回值汇报给前端。
            f_flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
            f_any_operations_aborted: 0,
            h_name_mappings: std::ptr::null_mut(),
            lpsz_progress_title: std::ptr::null(),
        };

        // SAFETY: `from` 在调用期间存活且以双 NUL 结尾；op 中所有指针都指向有效数据，
        // 其余字段按 API 契约置空/置位。
        let code = unsafe { sh_file_operation_w(&mut op) };
        if code != 0 {
            return Err(AppError::Other(format!(
                "移入回收站失败（SHFileOperationW 返回 {code}）"
            )));
        }
        if op.f_any_operations_aborted != 0 {
            return Err(AppError::Other("移入回收站被中断".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::xdg::{split_name, trash_with};
    use super::*;

    #[test]
    fn trash_moves_file_and_writes_info() {
        let base = tempfile::TempDir::new().unwrap();
        let src_dir = tempfile::TempDir::new().unwrap();
        let file = src_dir.path().join("壁纸 图.jpg");
        std::fs::write(&file, b"data").unwrap();

        let moved = trash_with(base.path(), &file).unwrap();

        assert!(!file.exists(), "原文件应已移走");
        assert!(moved.exists(), "回收站里应能看到文件");
        assert_eq!(std::fs::read(&moved).unwrap(), b"data");

        let info = std::fs::read_to_string(base.path().join("info").join(format!(
            "{}.trashinfo",
            moved.file_name().unwrap().to_string_lossy()
        )))
        .unwrap();
        assert!(info.starts_with("[Trash Info]\n"), "格式不对: {info}");
        // Path 必须 percent-encode，否则含空格/中文的路径在还原时会被解析错
        let expected = crate::state::escape_path_percent(&file.to_string_lossy());
        assert!(
            info.contains(&format!("Path={expected}")),
            "info 里的原始路径应被编码: {info}"
        );
        assert!(!info.contains("Path= "), "空格必须编码: {info}");
    }

    #[test]
    fn trash_renames_on_name_conflict() {
        let base = tempfile::TempDir::new().unwrap();
        let src_dir = tempfile::TempDir::new().unwrap();
        let file = src_dir.path().join("a.jpg");
        std::fs::write(&file, b"1").unwrap();
        let first = trash_with(base.path(), &file).unwrap();

        std::fs::write(&file, b"2").unwrap();
        let second = trash_with(base.path(), &file).unwrap();

        assert_ne!(first, second, "重名时应换落点，不能覆盖回收站里的旧文件");
        assert!(first.exists() && second.exists());
        assert_eq!(std::fs::read(&second).unwrap(), b"2");
    }

    #[test]
    fn trash_missing_file_is_ok() {
        let base = tempfile::TempDir::new().unwrap();
        // 与调用方的幂等语义一致：文件已不在 = 已经不在回收站外，直接成功
        assert!(move_to_trash(&base.path().join("nope.jpg")).is_ok());
    }

    #[test]
    fn split_name_keeps_extension() {
        assert_eq!(
            split_name("a.jpg"),
            ("a".to_string(), Some("jpg".to_string()))
        );
        assert_eq!(split_name("no-ext"), ("no-ext".to_string(), None));
        // 点开头的隐藏文件不该被当成"扩展名"
        assert_eq!(split_name(".hidden"), (".hidden".to_string(), None));
    }
}
