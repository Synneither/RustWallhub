//! 文件名 / 路径的安全校验。
//!
//! 所有从 IPC 收到的 `name` 参数，在拼到目录后面之前都必须过这里：拒绝路径分隔符、
//! `..`、绝对路径，并确认解析结果仍落在 base 之内。
//!
//! 注意 `safe_join` 内部走 `canonicalize`，**Windows 上返回的是 verbatim 形式**
//! （`\\?\C://...`）。要把结果交给 shell / Win32 API 时，先过
//! [`crate::winpath::plain_path`]。

use crate::error::AppError;
use std::path::PathBuf;

/// 校验一个字符串是“纯文件名”而不是路径（拒绝 `/`、绝对路径与 `..` 组件）。
/// 所有从 IPC 接收、随后要拼接到目录后面的 filename 参数都应先经过此校验。
pub fn ensure_plain_filename(name: &str) -> Result<(), AppError> {
    use std::path::Component;

    let path = std::path::Path::new(name);
    let mut components = path.components();
    let valid = match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => !has_path_separator(name),
        _ => false,
    };
    if !valid {
        return Err(AppError::Other("非法的文件路径".into()));
    }
    Ok(())
}

/// Windows 上反斜杠是路径分隔符，出现在文件名里即视为穿越尝试；
/// Linux/macOS 上反斜杠是**合法**的文件名字符，不能因此拒绝该文件。
#[cfg(target_os = "windows")]
fn has_path_separator(name: &str) -> bool {
    name.contains('\\')
}

#[cfg(not(target_os = "windows"))]
fn has_path_separator(_name: &str) -> bool {
    false
}

/// Safely join `name` onto `base`, rejecting path-traversal attempts like `../`.
/// Returns the canonicalized path if it lies within `base`, otherwise an error.
pub fn safe_join(base: &std::path::Path, name: &str) -> Result<PathBuf, AppError> {
    let base_canonical = base
        .canonicalize()
        .map_err(|e| AppError::Other(format!("无法解析基础路径: {e}")))?;
    safe_join_with(base, &base_canonical, name)
}

/// 与 [`safe_join`] 相同的校验，但复用已 canonicalize 过的 base。
/// 单个文件在循环里反复 canonicalize base 是纯浪费（每个文件 2 次 syscall）。
fn safe_join_with(
    base: &std::path::Path,
    base_canonical: &std::path::Path,
    name: &str,
) -> Result<PathBuf, AppError> {
    ensure_plain_filename(name)?;
    let candidate = base.join(name);
    // If the file doesn't exist yet, canonicalize the parent and join the filename
    let resolved = if candidate.exists() {
        candidate.canonicalize()
    } else {
        Ok(base_canonical.join(name))
    };
    let resolved = resolved.map_err(|e| AppError::Other(format!("无法解析路径: {e}")))?;
    if !resolved.starts_with(base_canonical) {
        return Err(AppError::Other("非法的文件路径".into()));
    }
    Ok(resolved)
}

/// 批量解析文件名 → 安全路径。base 只 canonicalize 一次，且**单个失败不会中断整批**：
/// 失败项只记日志并跳过，返回成功解析的 `(文件名, 路径)` 列表。
///
/// 批量操作（删除/标记/收养）里若用 `safe_join(...)?`，第一个失败就会让剩余文件全部
/// 得不到处理，而用户点了「删除 20 个」往往期望尽力完成。
pub fn safe_join_all<'a>(base: &std::path::Path, names: &'a [String]) -> Vec<(&'a str, PathBuf)> {
    let base_canonical = match base.canonicalize() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[safe_join_all] 无法解析基础路径 {}: {}", base.display(), e);
            return Vec::new();
        }
    };
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        match safe_join_with(base, &base_canonical, name) {
            Ok(path) => out.push((name.as_str(), path)),
            Err(e) => log::warn!("[safe_join_all] 跳过 {}: {}", name, e),
        }
    }
    out
}

/// Percent-encode 一个绝对路径。
///
/// `file://` URI（GNOME/KDE 设壁纸）与 XDG 回收站 `.trashinfo` 里的 `Path=` 值
/// 用的是同一套规则：保留 RFC 3986 的 unreserved 字符与 `/`，其余按 UTF-8 逐字节编码。
pub fn escape_path_percent(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    // 预分配（最坏情况每个字符都编码成 %XX，3 倍原长），避免逐字符堆分配。
    let mut out = String::with_capacity(path.len() * 3);
    for c in path.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0xf) as usize] as char);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_escape_path_percent() {
        assert_eq!(escape_path_percent("/a b/c.png"), "/a%20b/c.png");
        assert_eq!(
            escape_path_percent("/图片/壁纸.png"),
            "/%E5%9B%BE%E7%89%87/%E5%A3%81%E7%BA%B8.png"
        );
        // 已允许的字符不该被编码（回收站 trashinfo 里也别把 -_.~ 转义）
        assert_eq!(escape_path_percent("/home/u-x_1.2~/a"), "/home/u-x_1.2~/a");
        // 井号/问号在 URI 里有语义，必须编码，否则 file:// 链接会被截断
        assert_eq!(escape_path_percent("/a#b?c.jpg"), "/a%23b%3Fc.jpg");
    }

    #[test]
    fn test_ensure_plain_filename() {
        assert!(ensure_plain_filename("a.jpg").is_ok());
        assert!(ensure_plain_filename("a..b.jpg").is_ok());
        assert!(ensure_plain_filename("..").is_err());
        assert!(ensure_plain_filename("../a.jpg").is_err());
        assert!(ensure_plain_filename("sub/a.jpg").is_err());
        // 反斜杠在 Windows 是路径分隔符（应拒绝），在 Linux/macOS 是合法文件名字符（应接受）。
        #[cfg(windows)]
        assert!(ensure_plain_filename("sub\\a.jpg").is_err());
        #[cfg(not(windows))]
        assert!(ensure_plain_filename("sub\\a.jpg").is_ok());
        assert!(ensure_plain_filename("").is_err());
    }

    /// safe_join 是唯一的安全边界：IPC 传来的文件名都要过这里才能拼成磁盘路径。
    /// 它一旦回归，任何前端输入都能变成任意文件读写/删除。
    #[test]
    fn test_safe_join_rejects_traversal() {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        // 合法文件名：解析结果必须落在 base 内
        let joined = safe_join(base, "photo.jpg").expect("普通文件名应被接受");
        assert!(joined.starts_with(base.canonicalize().unwrap()));

        // 各种穿越/非法形态都必须被拒（这些在所有平台都非法）
        for evil in [
            "../escape.jpg",
            "..",
            ".",
            "sub/escape.jpg",
            "",
            "/etc/passwd",
        ] {
            assert!(safe_join(base, evil).is_err(), "应拒绝非法文件名: {evil:?}");
        }

        // 反斜杠只在 Windows 是分隔符；在 Linux/macOS 它是合法文件名字符，
        // 拼出来仍落在 base 内，所以那两种平台下应当接受（与 ensure_plain_filename 的约定一致）。
        #[cfg(windows)]
        for evil in [
            "sub\\escape.jpg",
            "C:\\Windows\\System32\\drivers\\etc\\hosts",
        ] {
            assert!(
                safe_join(base, evil).is_err(),
                "Windows 上应拒绝含反斜杠的文件名: {evil:?}"
            );
        }
        #[cfg(not(windows))]
        for legal in ["sub\\name.jpg"] {
            assert!(
                safe_join(base, legal).is_ok(),
                "非 Windows 平台反斜杠是合法文件名字符: {legal:?}"
            );
        }
    }

    /// 已存在的文件走 canonicalize 分支，未存在的走 base_canonical.join 分支，两条都要正确。
    #[test]
    fn test_safe_join_handles_existing_and_missing() {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        std::fs::write(base.join("real.jpg"), b"x").unwrap();
        let existing = safe_join(base, "real.jpg").expect("已存在文件应可解析");
        assert!(existing.is_file());
        assert!(existing.starts_with(base.canonicalize().unwrap()));

        let missing = safe_join(base, "not-yet.jpg").expect("未存在文件也应可解析");
        assert!(missing.starts_with(base.canonicalize().unwrap()));
    }

    /// base 不存在时 safe_join 应报错而不是 panic。
    #[test]
    fn test_safe_join_missing_base_errors() {
        let dir = TempDir::new().unwrap();
        let ghost = dir.path().join("nope");
        assert!(safe_join(&ghost, "a.jpg").is_err());
    }

    /// 批量解析的关键语义：单个非法名不能中断整批，否则「删除选中的 20 个」里
    /// 只要混进一个坏名字，剩下 19 个就都删不掉。
    #[test]
    fn test_safe_join_all_skips_invalid_without_aborting() {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        let names = vec![
            "ok1.jpg".to_string(),
            "../evil.jpg".to_string(),
            "ok2.jpg".to_string(),
            "sub/evil.jpg".to_string(),
            "ok3.jpg".to_string(),
        ];
        let resolved = safe_join_all(base, &names);

        assert_eq!(resolved.len(), 3, "应保留 3 个合法项");
        let kept: Vec<&str> = resolved.iter().map(|(n, _)| *n).collect();
        assert_eq!(kept, vec!["ok1.jpg", "ok2.jpg", "ok3.jpg"]);
        // 顺序应与输入一致，且路径都落在 base 内
        let base_canonical = base.canonicalize().unwrap();
        for (_, path) in &resolved {
            assert!(path.starts_with(&base_canonical));
        }
    }

    /// base 无法解析时返回空列表（调用方据此走"没有任何文件可处理"的分支）。
    #[test]
    fn test_safe_join_all_missing_base_returns_empty() {
        let dir = TempDir::new().unwrap();
        let ghost = dir.path().join("nope");
        let names = vec!["a.jpg".to_string()];
        assert!(safe_join_all(&ghost, &names).is_empty());
    }
}
