//! System commands: get_active_wallpaper, scan_directory.
//!
//! 「当前壁纸」按来源优先级读取，任何一个环节失败都只是跳过，不影响后续来源：
//! 1. Noctalia v5 CLI（`noctalia msg wallpaper-get`）——GUI 改过的值写在 settings.toml 里，
//!    v4 时代的 `~/.cache/noctalia/wallpapers.json` 在 v5 上不再更新
//! 2. Noctalia v4 缓存 JSON（老版本）
//! 3. awww / swww 的守护进程缓存（`~/.cache/<工具>/<输出名>` 里存的就是当前图路径）
//! 4. Noctalia v5 的 `settings.toml` 兜底（CLI 不在 PATH 上时，例如从某些启动器拉起）

use crate::exec::{cmd, has_command};
use crate::state::AppError;
use crate::wallpaper::noctalia_msg;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 「当前壁纸」的短缓存。
///
/// 读取要 spawn 外部命令（`noctalia msg wallpaper-get` + `theme-mode-get`，非 Noctalia
/// 会话下还有 gsettings），而仪表盘每次进入/切回视图都会调一次。壁纸切换是低频操作，
/// 5 秒的陈旧窗口换掉每次进页面的一串进程启动是划算的。
///
/// 缓存里同时存「没有壁纸」（`None`）：探测不到也是结果，没必要反复重试。
static ACTIVE_WALLPAPER_CACHE: Mutex<Option<(Instant, Option<String>)>> = Mutex::new(None);
const ACTIVE_WALLPAPER_TTL: Duration = Duration::from_secs(5);

/// 设过壁纸后调用，让仪表盘下次进入能立刻反映新壁纸，而不是等 TTL 过期。
pub(crate) fn invalidate_active_wallpaper() {
    if let Ok(mut guard) = ACTIVE_WALLPAPER_CACHE.lock() {
        *guard = None;
    }
}

#[derive(Serialize)]
pub struct ActiveWallpaper {
    pub path: Option<String>,
}

/// 获取当前桌面壁纸路径。
///
/// 该路径通常不在本应用的保存目录内，所以要单独授权给 asset 协议，
/// 否则仪表盘的「当前壁纸」缩略图在 asset scope 收紧后会加载失败。
#[tauri::command]
pub async fn get_active_wallpaper(app: tauri::AppHandle) -> Result<ActiveWallpaper, AppError> {
    // 读取文件 + 外部命令都是阻塞操作。
    let result = tokio::task::spawn_blocking(get_active_wallpaper_sync)
        .await
        .map_err(|e| AppError::Other(format!("获取当前壁纸失败: {e}")))??;

    if let Some(ref path) = result.path {
        crate::state::allow_asset_file(&app, path);
    }
    Ok(result)
}

fn get_active_wallpaper_sync() -> Result<ActiveWallpaper, AppError> {
    if let Some(cached) = cached_active_wallpaper() {
        return Ok(ActiveWallpaper { path: cached });
    }
    let theme = theme_mode();
    let path = noctalia_cli_wallpaper()
        .or_else(|| noctalia_v4_cache(&theme))
        .or_else(daemon_cache_wallpaper)
        .or_else(noctalia_settings_toml);
    if let Ok(mut guard) = ACTIVE_WALLPAPER_CACHE.lock() {
        *guard = Some((Instant::now(), path.clone()));
    }
    Ok(ActiveWallpaper { path })
}

/// 命中未过期的缓存时返回 `Some`（内部区分「缓存了 None」与「没缓存」）。
fn cached_active_wallpaper() -> Option<Option<String>> {
    let guard = ACTIVE_WALLPAPER_CACHE.lock().ok()?;
    guard
        .as_ref()
        .filter(|(at, _)| at.elapsed() < ACTIVE_WALLPAPER_TTL)
        .map(|(_, path)| path.clone())
}

/// Noctalia v5：CLI 直接给出持久化的默认壁纸路径。
fn noctalia_cli_wallpaper() -> Option<String> {
    noctalia_msg(&["wallpaper-get"]).and_then(|out| valid_wallpaper_path(&out))
}

/// Noctalia v4：按主题模式从缓存 JSON 里挑（结构 `{"wallpapers": {显示器: {light, dark}}}`）。
fn noctalia_v4_cache(theme: &str) -> Option<String> {
    let cache = dirs::cache_dir()?.join("noctalia").join("wallpapers.json");
    let content = std::fs::read_to_string(cache).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let key = if theme == "dark" { "dark" } else { "light" };

    // 优先 eDP-1（笔记本内屏），找不到时用缓存里的第一块显示器。
    let raw = json
        .pointer(&format!("/wallpapers/eDP-1/{key}"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            json.pointer("/wallpapers")
                .and_then(|v| v.as_object())
                .and_then(|monitors| {
                    monitors.keys().find_map(|monitor| {
                        json.pointer(&format!("/wallpapers/{monitor}/{key}"))?
                            .as_str()
                    })
                })
        })?;
    valid_wallpaper_path(raw)
}

/// awww / swww：守护进程把每个输出的当前壁纸路径写成一个小文本文件。
fn daemon_cache_wallpaper() -> Option<String> {
    let cache = dirs::cache_dir()?;
    for tool in ["awww", "swww"] {
        let Ok(entries) = std::fs::read_dir(cache.join(tool)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            if let Some(path) = valid_wallpaper_path(&content) {
                return Some(path);
            }
        }
    }
    None
}

/// Noctalia v5 的 `settings.toml` 兜底：只做最小限定的行扫描（`[wallpaper.default]` 下的
/// `path = "..."`），不值得为一行配置引入 TOML 依赖。
fn noctalia_settings_toml() -> Option<String> {
    let base = std::env::var_os("NOCTALIA_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            dirs::state_dir().or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
        })?;
    let content = std::fs::read_to_string(base.join("noctalia").join("settings.toml")).ok()?;
    parse_default_wallpaper_path(&content).and_then(|raw| valid_wallpaper_path(&raw))
}

/// 取 `[wallpaper.default]` 段里的 `path` 值（原样返回，含引号与空白，由调用方校验）。
fn parse_default_wallpaper_path(content: &str) -> Option<String> {
    let mut in_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[wallpaper.default]";
            continue;
        }
        if !in_section {
            continue;
        }
        let Some(rest) = line.strip_prefix("path") else {
            continue;
        };
        if !rest.starts_with('=') && !rest.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some(value) = rest.trim_start().strip_prefix('=') {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// 主题模式（`dark` / `light`）。
///
/// Noctalia 自己的模式优先——niri 这类会话里没有 gsettings 可读，直接问外壳最准。
fn theme_mode() -> String {
    if let Some(out) = noctalia_msg(&["theme-mode-get"]) {
        let value = out.trim().to_lowercase();
        if value.starts_with("dark") {
            return "dark".to_string();
        }
        if value.starts_with("light") {
            return "light".to_string();
        }
    }
    // 回退：GNOME 的 color-scheme。探测失败（非 GNOME / 命令不可用）时当亮色，
    // 避免把「探测失败」误判成 dark 而读错 key。
    let is_dark = if has_command("gsettings") {
        cmd("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .to_lowercase()
                    .contains("dark")
            })
            .unwrap_or(false)
    } else {
        false
    };
    if is_dark { "dark" } else { "light" }.to_string()
}

/// 校验读到的值能否当壁纸路径用：Noctalia 允许 `color:#RRGGBB` 这类纯色「壁纸」，
/// 它不是文件；不存在的路径返回给前端也只会是一张裂图，这里一并滤掉。
fn valid_wallpaper_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() || trimmed.starts_with("color:") {
        return None;
    }
    let expanded = match trimmed.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()?.join(rest).to_string_lossy().into_owned(),
        None => trimmed.to_string(),
    };
    Path::new(&expanded).is_file().then_some(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn valid_wallpaper_path_rejects_non_files() {
        assert_eq!(valid_wallpaper_path(""), None);
        assert_eq!(valid_wallpaper_path("   "), None);
        // Noctalia 允许纯色「壁纸」，它不是文件
        assert_eq!(valid_wallpaper_path("color:#FF00FF"), None);
        assert_eq!(valid_wallpaper_path("/nonexistent/wall.jpg"), None);
    }

    #[test]
    fn valid_wallpaper_path_accepts_existing_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("wall.jpg");
        std::fs::write(&file, b"x").unwrap();
        let raw = format!("\"{}\"", file.to_string_lossy());
        let expected = file.to_string_lossy().into_owned();

        // CLI 输出 / TOML 值可能带引号、首尾空白与换行
        assert_eq!(valid_wallpaper_path(&raw), Some(expected.clone()));
        assert_eq!(
            valid_wallpaper_path(&format!("  {expected}\n")),
            Some(expected)
        );
    }

    #[test]
    fn parse_default_wallpaper_path_only_reads_its_own_section() {
        let content = "\
[wallpaper]
enabled = true

[wallpaper.default]
path = \"/home/u/w.jpg\"

[theme]
path = \"/should/not/win\"
";
        assert_eq!(
            parse_default_wallpaper_path(content),
            Some("\"/home/u/w.jpg\"".to_string())
        );
        // 段内没有 path 时不能串到别的段
        assert_eq!(
            parse_default_wallpaper_path("[wallpaper]\npath = \"/x\"\n"),
            None
        );
        // 前缀相同但键名不同（paths / pathological）不能误命中
        assert_eq!(
            parse_default_wallpaper_path("[wallpaper.default]\npaths = \"/x\"\n"),
            None
        );
    }
}
