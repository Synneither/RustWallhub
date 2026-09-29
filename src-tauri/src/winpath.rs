//! Rust 路径与 Windows shell/Win32 API 之间那道缝。
//!
//! `std::fs::canonicalize` 在 Windows 上返回的是 **verbatim 路径**（`\\?\C:\...`，
//! UNC 是 `\\?\UNC\server\share`）。这个前缀绕开了 MAX_PATH 与路径规范化，代价是
//! 一批 shell/老 Win32 API **完全不认识它**（实测结论）：
//!
//! - `SHFileOperationW` → 一律 `DE_INVALIDFILES`（0x7C = 124），文件纹丝不动
//! - `SHCreateItemFromParsingName`（`IDesktopWallpaper::SetWallpaper` 内部就走它）
//!   → `E_INVALIDARG`
//!
//! 本应用到处用 `safe_path::safe_join` / `safe_join_all` 做路径安全校验，而它们内部就是
//! `canonicalize`，所以**凡是要把路径交给这类 API 的调用点，都得先过一遍 [`plain_path`]**。
//!
//! 反例（**不要**降级）：`IDesktopWallpaper::GetMonitorDevicePathAt` 返回的显示器 id
//! 本身就长成 `\\?\DISPLAY#...` —— 那是 shell 自己的 id 空间，不是文件系统路径。

use std::path::{Path, PathBuf};

/// Windows verbatim 前缀。
const VERBATIM_PREFIX: &str = r"\\?\";

/// 把 verbatim 路径降级成 shell/Win32 API 能接受的形式。
///
/// 返回 `None` 表示**不需要降级**（本来就是普通路径）或**不能安全降级**——两种情况下
/// 调用方都应保持原样把它交给系统，宁可看着它报错，也不能操作到另一个文件上。
///
/// 之所以要"回验"：verbatim 前缀是表达尾随空格/点（`a. `）、保留名这类名字的**唯一**
/// 方式。直接剥前缀会让 `\\?\C:\d\a. ` 被规范化成 `C:\d\a`，那就要操作错文件了。
/// 所以降级前后各 canonicalize 一次，必须指向同一个文件才放行。
///
/// 路径若表示不成 UTF-8 字符串（名字里含未配对代理项之类）也直接放弃降级——那种名字
/// 本来就只能用 verbatim 形式创建，交给调用方按原样报错即可。
pub fn plain_path(path: &Path) -> Option<PathBuf> {
    let candidate = PathBuf::from(strip_verbatim(path.to_str()?)?);

    let same = match (candidate.canonicalize(), path.canonicalize()) {
        (Ok(plain), Ok(verbatim)) => plain == verbatim,
        // 拿不到就不敢降级。调用方通常已经确认过文件存在，走到这里说明是罕见情况，
        // 保持原样让它照常报错即可。
        _ => false,
    };
    if !same {
        log::warn!(
            "[winpath] 不能安全地把 verbatim 路径降级，保持原样: {}",
            path.display()
        );
        return None;
    }
    Some(candidate)
}

/// [`plain_path`] 的字符串版本：降不了就原样返回，方便直接喂给收字符串的 API。
pub fn plain_str(path: &str) -> String {
    match plain_path(Path::new(path)) {
        Some(plain) => plain.to_string_lossy().into_owned(),
        None => path.to_string(),
    }
}

/// 纯粹的字符串降级，**不带任何校验**——只做形状判断，返回 `None` 表示"不是可降级的
/// verbatim 路径"。安全校验在 [`plain_path`] 里，别单独用它去构造要操作的路径。
///
/// 认这三种形状：
/// - `\\?\C:\...` → `C:\...`
/// - `\\?\UNC\server\share\...` → `\\server\share\...`
/// - 其它（`\\?\Volume{GUID}\...`、`\\?\GLOBALROOT\...`）→ `None`，它们没有等价的普通形式
fn strip_verbatim(text: &str) -> Option<String> {
    let rest = text.strip_prefix(VERBATIM_PREFIX)?;

    if let Some(unc) = rest.strip_prefix("UNC\\") {
        // `\\?\UNC\server\share` 里 server 段不能为空，否则拼出来是 `\\\share`
        if unc.is_empty() || unc.starts_with('\\') {
            return None;
        }
        return Some(format!(r"\\{unc}"));
    }

    // 只认盘符路径；`\\?\` 之后必须紧跟「字母 + `:` + `\`」
    let mut chars = rest.chars();
    if !chars.next()?.is_ascii_alphabetic() {
        return None;
    }
    if !chars.as_str().starts_with(":\\") {
        return None;
    }
    Some(rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_drive_letter_verbatim() {
        assert_eq!(
            strip_verbatim(r"\\?\C:\Users\me\Pictures\壁纸 图.png").as_deref(),
            Some(r"C:\Users\me\Pictures\壁纸 图.png")
        );
        assert_eq!(
            strip_verbatim(r"\\?\Z:\a.png").as_deref(),
            Some(r"Z:\a.png")
        );
    }

    #[test]
    fn strips_unc_verbatim() {
        assert_eq!(
            strip_verbatim(r"\\?\UNC\server\share\dir\a.png").as_deref(),
            Some(r"\\server\share\dir\a.png")
        );
        // `\\?\UNC\<server>\<share>` 两段也成立（server=share、share=a.png）
        assert_eq!(
            strip_verbatim(r"\\?\UNC\share\a.png").as_deref(),
            Some(r"\\share\a.png")
        );
    }

    #[test]
    fn refuses_shapes_without_plain_equivalent() {
        // 卷 GUID 路径没有等价的普通形式
        assert!(
            strip_verbatim(r"\\?\Volume{8f0b1a2c-0000-0000-0000-100000000000}\a.png").is_none()
        );
        assert!(strip_verbatim(r"\\?\GLOBALROOT\Device\HarddiskVolume1\a.png").is_none());
        // UNC 段落残缺：`\\?\UNC\` 后面什么都没有
        assert!(strip_verbatim(r"\\?\UNC\").is_none());
        assert!(strip_verbatim(r"\\?\UNC\\share").is_none());
        // 普通路径 / 相对路径
        assert!(strip_verbatim(r"C:\Users\me\a.png").is_none());
        assert!(strip_verbatim(r"\\server\share\a.png").is_none());
        assert!(strip_verbatim("a.png").is_none());
        assert!(strip_verbatim("").is_none());
        // 只是看着像，其实不是盘符
        assert!(strip_verbatim(r"\\?\dir\a.png").is_none());
        assert!(strip_verbatim(r"\\?\C:a.png").is_none());
    }

    #[test]
    fn plain_path_is_noop_for_ordinary_paths() {
        // 不需要降级的路径一律返回 None（这条与平台无关：没见过 verbatim 前缀就早退）
        assert!(plain_path(Path::new(r"C:\Users\me\a.png")).is_none());
        assert!(plain_path(Path::new("/home/me/a.png")).is_none());
        assert!(plain_path(Path::new("a.png")).is_none());
    }

    #[test]
    fn plain_str_falls_back_to_input() {
        assert_eq!(plain_str(r"C:\Users\me\a.png"), r"C:\Users\me\a.png");
        // 不存在的 verbatim 路径拿不到 canonicalize，同样原样返回
        let missing = r"\\?\C:\definitely\missing\a.png";
        assert_eq!(plain_str(missing), missing);
    }
}
