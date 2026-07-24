//! System commands: get_active_wallpaper, scan_directory.

use crate::state::AppError;
use serde::Serialize;
use std::process::Command;

#[derive(Serialize)]
pub struct ActiveWallpaper {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// 获取当前桌面壁纸路径（从 noctalia 缓存）
#[tauri::command]
pub async fn get_active_wallpaper() -> Result<ActiveWallpaper, AppError> {
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

    let is_dark = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()
        .is_none_or(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains("dark")
        });

    let key = if is_dark { "dark" } else { "light" };
    let path = json
        .pointer(&format!("/wallpapers/eDP-1/{key}"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(ActiveWallpaper {
        path: if path.is_empty() { None } else { Some(path) },
    })
}

#[tauri::command]
pub async fn scan_directory(dir: String) -> Result<Vec<FileInfo>, AppError> {
    log::info!("[CMD] scan_directory: dir={}", dir);
    let path = std::path::Path::new(&dir);
    if !path.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let file_path = entry.path();
            if file_path.is_file() {
                files.push(FileInfo {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: file_path.to_string_lossy().to_string(),
                    size: entry.metadata().map_or(0, |m| m.len()),
                });
            }
        }
    }
    log::info!("[CMD] scan_directory: found {} files", files.len());
    Ok(files)
}
