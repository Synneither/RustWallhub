//! System commands：只读地报告桌面当前壁纸。
//!
//! 真正的读取逻辑（含 Windows 的 COM 查询）都在 [`crate::current_wallpaper`] 里，
//! 这里只负责把它暴露成命令、并按需给 asset 协议授权——当前壁纸通常不在
//! 本应用的保存目录内，不授权的话缩略图加载不出来。

use crate::current_wallpaper::{self, ActiveWallpaper};
use crate::error::AppError;

/// 获取当前桌面壁纸路径（多显示器各一张，空列表 = 读不到）。
#[tauri::command]
pub async fn get_active_wallpaper(app: tauri::AppHandle) -> Result<ActiveWallpaper, AppError> {
    // 读文件 + 外部命令都是阻塞操作。
    let paths = tokio::task::spawn_blocking(current_wallpaper::paths)
        .await
        .map_err(|e| AppError::Other(format!("获取当前壁纸失败: {e}")))?;

    for path in &paths {
        crate::state::allow_asset_file(&app, path);
    }
    Ok(ActiveWallpaper { paths })
}
