//! Linux 桌面项安装：`--install-desktop` / `--uninstall-desktop`。
//!
//! AppImage 不会被桌面环境的启动器自动收录（除非装了 AppImageLauncher 之类的工具）。
//! 在 niri + Noctalia 这类组合下，表现就是"启动器里搜不到 RustWallhub，只能去文件
//! 管理器点文件"。这里把 .desktop 与图标写进 XDG 用户目录，装完即可被启动器/dock 收录。
//!
//! 不启动 UI：这是纯命令行行为，跑完打印结果就退出（见 `lib.rs::run` 开头）。

use std::path::{Path, PathBuf};

pub const FLAG_INSTALL: &str = "--install-desktop";
pub const FLAG_UNINSTALL: &str = "--uninstall-desktop";

/// 桌面项文件名必须与 Wayland 的 `app-id` 一致，dock 才能把窗口和图标对上。
/// 本应用没有开启 `enable_gtk_app_id`，GTK 会退回用程序名（二进制名 `rustwallhub`）。
const DESKTOP_FILE: &str = "rustwallhub.desktop";
const ICON_NAME: &str = "rustwallhub";

/// 命中命令行开关时返回要打印的结果；没有则返回 `None`（正常启动 UI）。
pub fn handle_cli_args() -> Option<Result<String, String>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == FLAG_INSTALL) {
        return Some(install());
    }
    if args.iter().any(|arg| arg == FLAG_UNINSTALL) {
        return Some(uninstall());
    }
    None
}

fn install() -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err(format!("{FLAG_INSTALL} 仅用于 Linux 的 AppImage"));
    }
    // 只有从 AppImage 运行时才有"往启动器里加一条"的意义；
    // 用包管理器装的实例，包本身已经带了 desktop 文件。
    let appimage = std::env::var("APPIMAGE").unwrap_or_default();
    if appimage.is_empty() {
        return Err("当前不是从 AppImage 运行的实例，无需安装桌面项".to_string());
    }
    let data_dir = data_dir().ok_or_else(|| "无法定位 XDG 数据目录".to_string())?;

    let icon_line = install_icon(&data_dir);
    if icon_line.is_none() {
        // 找不到图标就不写 Icon=：宁可让启动器显示通用图标，也不要指向一个不存在的文件。
        log::warn!("[desktop-entry] AppDir 里没找到 png 图标，桌面项将不指定 Icon");
    }

    let desktop_dir = data_dir.join("applications");
    std::fs::create_dir_all(&desktop_dir)
        .map_err(|e| format!("创建 {} 失败: {e}", desktop_dir.display()))?;
    let desktop_path = desktop_dir.join(DESKTOP_FILE);
    let existed = desktop_path.exists();
    std::fs::write(
        &desktop_path,
        desktop_entry_content(&appimage, icon_line.as_deref()),
    )
    .map_err(|e| format!("写入 {} 失败: {e}", desktop_path.display()))?;

    refresh_caches(&desktop_dir, &data_dir);

    Ok(format!(
        "{}桌面项：{}\n启动器里现在应该能搜到 RustWallhub 了。\n（卸载：{}）",
        if existed { "已更新" } else { "已安装" },
        desktop_path.display(),
        FLAG_UNINSTALL
    ))
}

fn uninstall() -> Result<String, String> {
    let data_dir = data_dir().ok_or_else(|| "无法定位 XDG 数据目录".to_string())?;
    let desktop_path = data_dir.join("applications").join(DESKTOP_FILE);
    let icon_path = icon_path(&data_dir);

    let mut removed = Vec::new();
    for path in [desktop_path, icon_path] {
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("删除 {} 失败: {e}", path.display()))?;
            removed.push(path);
        }
    }
    refresh_caches(&data_dir.join("applications"), &data_dir);

    if removed.is_empty() {
        Ok("没有找到已安装的桌面项，无需卸载".to_string())
    } else {
        Ok(format!(
            "已移除桌面项：{}",
            removed
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("、")
        ))
    }
}

/// desktop 文件内容。`Icon=` 为空串时不写该行。
fn desktop_entry_content(appimage: &str, icon_line: Option<&str>) -> String {
    let mut content = String::from("[Desktop Entry]\nType=Application\nVersion=1.0\n");
    content.push_str("Name=RustWallhub\nComment=桌面壁纸管理器\n");
    content.push_str(&format!("Exec={} %U\n", quote_exec(appimage)));
    if let Some(icon) = icon_line {
        content.push_str(icon);
        content.push('\n');
    }
    content.push_str("Terminal=false\nCategories=Graphics;Utility;\n");
    // dock/任务栏靠这个把窗口（app-id = 程序名）与桌面项对应起来
    content.push_str("StartupWMClass=rustwallhub\n");
    content.push_str("Keywords=wallpaper;壁纸;RustWallhub;\n");
    content
}

/// desktop 规范的 Exec 转义：含空格等字符时用双引号包起来，反斜杠与双引号需要转义。
fn quote_exec(path: &str) -> String {
    let needs_quote = path
        .chars()
        .any(|c| c.is_whitespace() || "\"'\\>$`".contains(c));
    if !needs_quote {
        return path.to_string();
    }
    let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// 把 AppDir 里的图标复制到 hicolor 主题目录，返回 `Icon=` 行。
///
/// AppDir 根目录下的 png 名随打包器版本变化，所以直接扫一遍取第一个 png
/// （`.DirIcon` 才是权威但那是个符号链接，指向的就是同一个文件）。
fn install_icon(data_dir: &Path) -> Option<String> {
    let appdir = std::env::var("APPDIR").ok()?;
    let source = std::fs::read_dir(&appdir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        })?;

    let target = icon_path(data_dir);
    std::fs::create_dir_all(target.parent()?).ok()?;
    std::fs::copy(&source, &target).ok()?;
    Some(format!("Icon={ICON_NAME}"))
}

/// 图标落点：hicolor 主题的 256x256 目录（AppImage 自带的多为 256 或 512，交给启动器缩放）。
fn icon_path(data_dir: &Path) -> PathBuf {
    data_dir
        .join("icons/hicolor/256x256/apps")
        .join(format!("{ICON_NAME}.png"))
}

fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
}

/// 刷新启动器缓存。两个命令都不是必需的（多数启动器会自己重扫），失败就忽略。
fn refresh_caches(desktop_dir: &Path, data_dir: &Path) {
    let _ = crate::exec::cmd("update-desktop-database")
        .arg(desktop_dir)
        .output();
    let _ = crate::exec::cmd("gtk-update-icon-cache")
        .args(["-q", "-t", "-f"])
        .arg(data_dir.join("icons/hicolor"))
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_is_quoted_when_path_has_spaces() {
        assert_eq!(
            quote_exec("/home/u/rustwallhub_2.9.0_amd64.AppImage"),
            "/home/u/rustwallhub_2.9.0_amd64.AppImage"
        );
        // 空格必须加引号，否则启动器会把路径截断成两个参数
        assert_eq!(
            quote_exec("/home/u/My Apps/RustWallhub.AppImage"),
            "\"/home/u/My Apps/RustWallhub.AppImage\""
        );
        assert_eq!(quote_exec("/a\"b.AppImage"), "\"/a\\\"b.AppImage\"");
    }

    #[test]
    fn desktop_entry_has_startup_wm_class_and_optional_icon() {
        let content = desktop_entry_content("/opt/RustWallhub.AppImage", Some("Icon=rustwallhub"));
        assert!(content.starts_with("[Desktop Entry]\n"));
        assert!(content.contains("Exec=/opt/RustWallhub.AppImage %U\n"));
        assert!(content.contains("Icon=rustwallhub\n"));
        assert!(content.contains("StartupWMClass=rustwallhub\n"));
        assert!(content.contains("Categories=Graphics;Utility;\n"));

        // 没有图标时不留空 Icon= 行
        let no_icon = desktop_entry_content("/opt/x.AppImage", None);
        assert!(!no_icon.contains("Icon="));
    }

    #[test]
    fn icon_path_uses_hicolor_theme() {
        let path = icon_path(Path::new("/home/u/.local/share"));
        assert!(path.ends_with("icons/hicolor/256x256/apps/rustwallhub.png"));
    }

    #[test]
    fn cli_flag_only_triggers_on_exact_match() {
        // 只是防止 flag 常量被改成不含 `--` 的值而误判普通参数
        assert!(FLAG_INSTALL.starts_with("--"));
        assert!(FLAG_UNINSTALL.starts_with("--"));
        assert_ne!(FLAG_INSTALL, FLAG_UNINSTALL);
    }
}
