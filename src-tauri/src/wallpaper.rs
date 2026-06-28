//! Wallpaper setting for Linux and Windows desktop environments.
//! Each setter probes whether its environment is available, returns `None` if not.

use crate::AppError;
use std::process::Command;

// ---------------------------------------------------------------------------

/// GNOME (gsettings)
fn set_gnome_wallpaper(path_str: &str) -> Option<String> {
    if Command::new("gsettings")
        .args(["get", "org.gnome.desktop.background", "picture-uri"])
        .output()
        .is_err()
    {
        return None;
    }
    let uri = format!("file://{path_str}");
    if let Ok(output) = Command::new("gsettings")
        .args(["set", "org.gnome.desktop.background", "picture-uri", &uri])
        .output()
    {
        if output.status.success() {
            Command::new("gsettings")
                .args([
                    "set",
                    "org.gnome.desktop.background",
                    "picture-uri-dark",
                    &uri,
                ])
                .output()
                .ok();
            return Some("\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (GNOME)".to_string());
        }
    }
    None
}

/// XFCE (xfconf-query)
fn set_xfce_wallpaper(path_str: &str) -> Option<String> {
    let output = Command::new("xfconf-query")
        .args(["-c", "xfce4-desktop", "-lv"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() == 2 && parts[0].contains("last-image") {
            if let Ok(output) = Command::new("xfconf-query")
                .args(["-c", "xfce4-desktop", "-p", parts[0].trim(), "-s", path_str])
                .output()
            {
                if output.status.success() {
                    return Some("\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (XFCE)".to_string());
                }
            }
        }
    }
    None
}

/// KDE Plasma (qdbus)
fn set_kde_wallpaper(path_str: &str) -> Option<String> {
    let has_kde = Command::new("kwriteconfig5")
        .args(["--help"])
        .output()
        .is_ok()
        || Command::new("kwriteconfig6")
            .args(["--help"])
            .output()
            .is_ok();
    if !has_kde {
        return None;
    }
    log::info!("[set_wallpaper] detected KDE Plasma");
    // 转义路径中的特殊字符，防止 qdbus JavaScript 上下文中的注入
    let escaped = path_str
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    let script = format!(
        "var allDesktops = desktops();
for (var i = 0; i < allDesktops.length; i++) {{
    var d = allDesktops[i];
    d.wallpaperPlugin = 'org.kde.image';
    d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General'];
    d.writeConfig('Image', 'file://{escaped}');
}}"
    );
    let output = Command::new("qdbus")
        .args([
            "org.kde.plasmashell",
            "/PlasmaShell",
            "org.kde.PlasmaShell.evaluateScript",
            &script,
        ])
        .output()
        .ok()?;
    if output.status.success() {
        return Some("\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (KDE)".to_string());
    }
    None
}

/// sway (swaymsg)
fn set_sway_wallpaper(path_str: &str) -> Option<String> {
    let output = Command::new("swaymsg")
        .args(["-t", "get_outputs"])
        .output()
        .ok()?;
    if output.status.success() {
        Command::new("swaymsg")
            .args(["output", "*", "bg", path_str, "fill"])
            .output()
            .ok()?;
        return Some("\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (sway)".to_string());
    }
    None
}

/// Hyprland (hyprpaper)
fn set_hyprland_wallpaper(path_str: &str) -> Option<String> {
    if Command::new("hyprctl").arg("--version").output().is_err() {
        return None;
    }
    let monitors = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .ok()
        .map(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .filter(|l| l.contains("\"name\":"))
                .filter_map(|l| {
                    let parts: Vec<&str> = l.splitn(2, ':').collect();
                    (parts.len() == 2).then(|| {
                        parts[1]
                            .trim()
                            .trim_matches('"')
                            .trim_matches(',')
                            .to_string()
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Command::new("hyprctl")
        .args(["hyprpaper", "preload", path_str])
        .output()
        .ok();

    let ok = if monitors.is_empty() {
        Command::new("hyprctl")
            .args(["hyprpaper", "wallpaper", &format!(",{path_str}")])
            .output()
            .is_ok_and(|o| o.status.success())
    } else {
        monitors.iter().all(|monitor| {
            Command::new("hyprctl")
                .args(["hyprpaper", "wallpaper", &format!("{monitor},{path_str}")])
                .output()
                .is_ok_and(|o| o.status.success())
        })
    };

    ok.then(|| "\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (Hyprland)".to_string())
}

/// swww
fn set_swww_wallpaper(path_str: &str) -> Option<String> {
    if Command::new("swww").arg("--version").output().is_err() {
        return None;
    }
    let output = Command::new("swww")
        .args([
            "img",
            "--transition-type",
            "fade",
            "--transition-step",
            "60",
            path_str,
        ])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| "\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (swww)".to_string())
}

/// feh（最后回退）
fn set_feh_wallpaper(path_str: &str) -> Option<String> {
    let output = Command::new("feh")
        .args(["--bg-fill", path_str])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| "\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (feh)".to_string())
}

/// Windows — 通过 SystemParametersInfoW 设置壁纸
#[cfg(target_os = "windows")]
fn set_windows_wallpaper(path_str: &str) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;

    let wide: Vec<u16> = std::ffi::OsStr::new(path_str)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    const SPI_SETDESKWALLPAPER: u32 = 0x0014;
    const SPIF_UPDATEINIFILE: u32 = 0x0001;
    const SPIF_SENDCHANGE: u32 = 0x0002;

    extern "system" {
        fn SystemParametersInfoW(
            uiAction: u32,
            uiParam: u32,
            pvParam: *const std::ffi::c_void,
            fWinIni: u32,
        ) -> i32;
    }

    let result = unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            wide.as_ptr() as *const std::ffi::c_void,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    };

    (result != 0).then(|| "\u{58c1}\u{7eb8}\u{5df2}\u{8bbe}\u{7f6e} (Windows)".to_string())
}

#[tauri::command]
pub(crate) async fn set_wallpaper(file_path: String) -> Result<String, AppError> {
    log::info!("[CMD] set_wallpaper: file={}", file_path);
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err(AppError::Other(format!("文件不存在: {}", file_path)));
    }
    let absolute_path = path
        .canonicalize()
        .map_err(|e| AppError::Other(format!("获取绝对路径失败: {e}")))?;
    let path_str = absolute_path.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    if let Some(result) = set_windows_wallpaper(&path_str) {
        return Ok(result);
    }

    set_gnome_wallpaper(&path_str)
        .or_else(|| set_xfce_wallpaper(&path_str))
        .or_else(|| set_kde_wallpaper(&path_str))
        .or_else(|| set_sway_wallpaper(&path_str))
        .or_else(|| set_hyprland_wallpaper(&path_str))
        .or_else(|| set_swww_wallpaper(&path_str))
        .or_else(|| set_feh_wallpaper(&path_str))
        .ok_or_else(|| AppError::Other(
            "未检测到支持的桌面环境。支持: Windows, GNOME, KDE, XFCE, sway, Hyprland, niri(swww), swww, feh".to_string(),
        ))
}
