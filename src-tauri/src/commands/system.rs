//! System commands: get_active_wallpaper, scan_directory.

use crate::state::AppError;
use serde::Serialize;
use std::process::Command;

#[derive(Serialize)]
pub struct ActiveWallpaper {
    pub path: Option<String>,
}

/// 获取当前桌面壁纸路径（从 noctalia 缓存）
#[tauri::command]
pub async fn get_active_wallpaper() -> Result<ActiveWallpaper, AppError> {
    // 读取文件 + gsettings 都是阻塞操作。
    tokio::task::spawn_blocking(get_active_wallpaper_sync)
        .await
        .map_err(|e| AppError::Other(format!("获取当前壁纸失败: {e}")))?
}

fn get_active_wallpaper_sync() -> Result<ActiveWallpaper, AppError> {
    let noc_path = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("noctalia")
        .join("wallpapers.json");

    if !noc_path.exists() {
        return Ok(ActiveWallpaper { path: None });
    }

    let content = std::fs::read_to_string(&noc_path)
        .map_err(|e| AppError::Other(format!("读取 noctalia 配置失败: {e}")))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Other(format!("解析 noctalia 配置失败: {e}")))?;

    // gsettings 探测失败（非 GNOME / 命令不可用）时回退 light 主题，
    // 避免 `is_none_or` 把「探测失败」误判成「dark 主题」而读错 key。
    let is_dark = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains("dark")
        })
        .unwrap_or(false);

    let key = if is_dark { "dark" } else { "light" };

    // 优先 eDP-1，找不到时回退到 noctalia 缓存中的第一个显示器。
    let path = json
        .pointer(&format!("/wallpapers/eDP-1/{key}"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            json.pointer("/wallpapers")
                .and_then(|v| v.as_object())
                .and_then(|monitors| {
                    monitors.iter().find_map(|(monitor, _)| {
                        json.pointer(&format!("/wallpapers/{monitor}/{key}"))
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string)
                    })
                })
        })
        .filter(|path| !path.is_empty());

    Ok(ActiveWallpaper { path })
}
