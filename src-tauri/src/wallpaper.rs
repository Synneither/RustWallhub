//! Wallpaper setting for Linux and Windows desktop environments.
//! Each setter probes whether its environment is available, returns `None` if not.

use crate::state::{AppError, AppState};
use serde::Serialize;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::Emitter;

// ---------------------------------------------------------------------------
// Monitor info
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub is_primary: bool,
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// Windows monitor enumeration (raw FFI)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
#[allow(clippy::upper_case_acronyms)] // Win32/COM 类型别名保持官方命名（HDC/HRESULT/PCWSTR）
mod win_monitors {
    use super::MonitorInfo;

    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct MonitorInfoExW {
        size: u32,
        monitor: Rect,
        work_area: Rect,
        flags: u32,
        device_name: [u16; 32],
    }

    type Hmonitor = *mut std::ffi::c_void;
    type HDC = *mut std::ffi::c_void;
    type EnumMonitorsProc = unsafe extern "system" fn(
        hmon: Hmonitor,
        hdc: HDC,
        rect: *mut Rect,
        data: *mut std::ffi::c_void,
    ) -> i32;

    extern "system" {
        fn EnumDisplayMonitors(
            hdc: HDC,
            rect: *const Rect,
            proc: EnumMonitorsProc,
            data: *mut std::ffi::c_void,
        ) -> i32;
        fn GetMonitorInfoW(hmon: Hmonitor, info: *mut MonitorInfoExW) -> i32;
    }

    const MONITORINFOF_PRIMARY: u32 = 0x00000001;

    struct EnumContext {
        monitors: Vec<MonitorInfo>,
    }

    unsafe extern "system" fn enum_callback(
        hmon: Hmonitor,
        _hdc: HDC,
        _rect: *mut Rect,
        data: *mut std::ffi::c_void,
    ) -> i32 {
        let ctx = &mut *(data as *mut EnumContext);
        let mut info: MonitorInfoExW = std::mem::zeroed();
        info.size = std::mem::size_of::<MonitorInfoExW>() as u32;
        if GetMonitorInfoW(hmon, &mut info) != 0 {
            let name_len = info
                .device_name
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(info.device_name.len());
            let name = String::from_utf16_lossy(&info.device_name[..name_len]);
            ctx.monitors.push(MonitorInfo {
                id: name.clone(),
                name,
                is_primary: (info.flags & MONITORINFOF_PRIMARY) != 0,
                width: (info.monitor.right - info.monitor.left) as u32,
                height: (info.monitor.bottom - info.monitor.top) as u32,
            });
        }
        1 // continue enumeration
    }

    pub fn list_monitors() -> Vec<MonitorInfo> {
        let mut ctx = EnumContext {
            monitors: Vec::new(),
        };
        unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                enum_callback,
                &mut ctx as *mut _ as *mut std::ffi::c_void,
            );
        }
        ctx.monitors
    }
}

#[cfg(not(target_os = "windows"))]
mod win_monitors {
    use super::MonitorInfo;
    use std::process::Command;

    pub fn list_monitors() -> Vec<MonitorInfo> {
        // Linux: try xrandr or hyprctl
        if let Ok(output) = Command::new("xrandr").args(["--listmonitors"]).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let monitors: Vec<MonitorInfo> = stdout
                    .lines()
                    .skip(1) // skip header line
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 3 {
                            let name = parts[parts.len() - 1].to_string();
                            let dims = parts.get(1).and_then(|s| {
                                let d: Vec<&str> = s.split('x').collect();
                                if d.len() == 2 {
                                    Some((d[0].parse().unwrap_or(0), d[1].parse().unwrap_or(0)))
                                } else {
                                    None
                                }
                            });
                            Some(MonitorInfo {
                                id: name.clone(),
                                name,
                                is_primary: parts.first().is_some_and(|s| s.contains('*')),
                                width: dims.map(|(w, _)| w).unwrap_or(0),
                                height: dims.map(|(_, h)| h).unwrap_or(0),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                if !monitors.is_empty() {
                    return monitors;
                }
            }
        }
        // Fallback: single virtual monitor
        vec![MonitorInfo {
            id: "default".to_string(),
            name: "Default".to_string(),
            is_primary: true,
            width: 0,
            height: 0,
        }]
    }
}

/// Percent-encode a file path for use in a `file://` URI.
/// Handles spaces, non-ASCII characters, and other special characters.
fn url_escape_path(path: &str) -> String {
    path.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '~') {
                vec![c]
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf)
                    .as_bytes()
                    .iter()
                    .flat_map(|b| format!("%{:02X}", b).chars().collect::<Vec<_>>())
                    .collect()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Windows IDesktopWallpaper COM interface (raw FFI for per-monitor wallpaper)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
#[allow(clippy::upper_case_acronyms)] // COM 类型别名保持官方命名（HRESULT/PCWSTR 等）
mod com_wallpaper {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    // CLSID_CDesktopWallpaper = {C2CF3110-460E-4FC1-B9D0-8A8C443A7140}
    const CLSID_DESKTOP_WALLPAPER: Guid = Guid {
        data1: 0xC2CF3110,
        data2: 0x460E,
        data3: 0x4FC1,
        data4: [0xB9, 0xD0, 0x8A, 0x8C, 0x44, 0x3A, 0x71, 0x40],
    };

    // IID_IDesktopWallpaper = {B92B56A9-8B55-4E14-9A89-0199BBB6F93B}
    const IID_IDESKTOP_WALLPAPER: Guid = Guid {
        data1: 0xB92B56A9,
        data2: 0x8B55,
        data3: 0x4E14,
        data4: [0x9A, 0x89, 0x01, 0x99, 0xBB, 0xB6, 0xF9, 0x3B],
    };

    const CLSCTX_ALL: u32 = 0x0017;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const S_OK: i32 = 0;

    type HRESULT = i32;
    type PCWSTR = *const u16;

    #[repr(C)]
    struct ComVtbl {
        query_interface: unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *const Guid,
            *mut *mut std::ffi::c_void,
        ) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        // IDesktopWallpaper methods
        set_wallpaper: unsafe extern "system" fn(*mut std::ffi::c_void, PCWSTR, PCWSTR) -> HRESULT,
        get_wallpaper:
            unsafe extern "system" fn(*mut std::ffi::c_void, PCWSTR, *mut *mut u16) -> HRESULT,
        get_monitor_device_path_count:
            unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32) -> HRESULT,
        get_monitor_device_path_at:
            unsafe extern "system" fn(*mut std::ffi::c_void, u32, *mut *mut u16) -> HRESULT,
        // ... remaining methods not needed
    }

    extern "system" {
        fn CoInitializeEx(reserved: *const std::ffi::c_void, coInit: u32) -> HRESULT;
        fn CoUninitialize();
        fn CoCreateInstance(
            rclsid: *const Guid,
            punk_outer: *const std::ffi::c_void,
            clsctx: u32,
            riid: *const Guid,
            ppv: *mut *mut std::ffi::c_void,
        ) -> HRESULT;
        fn CoTaskMemFree(ptr: *const std::ffi::c_void);
    }

    fn to_wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn from_wide(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0;
        unsafe {
            while *ptr.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
        }
    }

    /// Set wallpaper on a specific monitor (or all monitors if monitor_id is empty).
    /// Uses the IDesktopWallpaper COM interface.
    pub fn set_wallpaper_for_monitor(path: &str, monitor_id: &str) -> Result<(), String> {
        unsafe {
            let hr = CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED);
            // S_OK = first init, S_FALSE = already initialized — both are fine
            if hr != S_OK && hr != 1 {
                return Err(format!("CoInitializeEx failed: 0x{:08X}", hr as u32));
            }

            let mut p_wallpaper: *mut std::ffi::c_void = ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_DESKTOP_WALLPAPER,
                ptr::null(),
                CLSCTX_ALL,
                &IID_IDESKTOP_WALLPAPER,
                &mut p_wallpaper,
            );
            if hr != S_OK {
                CoUninitialize();
                return Err(format!("CoCreateInstance failed: 0x{:08X}", hr as u32));
            }

            let vtbl = &*(p_wallpaper as *const *const ComVtbl).read();
            let path_wide = to_wide(path);
            let monitor_wide_vec = if monitor_id.is_empty() {
                None
            } else {
                Some(to_wide(monitor_id))
            };
            let monitor_ptr = monitor_wide_vec
                .as_ref()
                .map(|v| v.as_ptr())
                .unwrap_or(ptr::null());

            let hr = (vtbl.set_wallpaper)(p_wallpaper, monitor_ptr, path_wide.as_ptr());

            (vtbl.release)(p_wallpaper);
            CoUninitialize();

            if hr != S_OK {
                return Err(format!("SetWallpaper failed: 0x{:08X}", hr as u32));
            }
        }
        Ok(())
    }

    /// Get the number of monitor device paths and each path.
    pub fn get_monitor_device_paths() -> Vec<String> {
        unsafe {
            let hr = CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED);
            if hr != S_OK && hr != 1 {
                return Vec::new();
            }

            let mut p_wallpaper: *mut std::ffi::c_void = ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_DESKTOP_WALLPAPER,
                ptr::null(),
                CLSCTX_ALL,
                &IID_IDESKTOP_WALLPAPER,
                &mut p_wallpaper,
            );
            if hr != S_OK {
                CoUninitialize();
                return Vec::new();
            }

            let vtbl = &*(p_wallpaper as *const *const ComVtbl).read();

            let mut count: u32 = 0;
            let hr = (vtbl.get_monitor_device_path_count)(p_wallpaper, &mut count);
            if hr != S_OK {
                (vtbl.release)(p_wallpaper);
                CoUninitialize();
                return Vec::new();
            }

            let mut paths = Vec::new();
            for i in 0..count {
                let mut ptr_path: *mut u16 = ptr::null_mut();
                let hr = (vtbl.get_monitor_device_path_at)(p_wallpaper, i, &mut ptr_path);
                if hr == S_OK && !ptr_path.is_null() {
                    paths.push(from_wide(ptr_path));
                    CoTaskMemFree(ptr_path as *const std::ffi::c_void);
                }
            }

            (vtbl.release)(p_wallpaper);
            CoUninitialize();
            paths
        }
    }
}

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
    let uri = format!("file://{}", url_escape_path(path_str));
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
    d.writeConfig('Image', 'file://{}');
}}",
        url_escape_path(&escaped)
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
pub(crate) async fn set_wallpaper(
    file_path: String,
    monitor: Option<String>,
) -> Result<String, AppError> {
    log::info!(
        "[CMD] set_wallpaper: file={}, monitor={:?}",
        file_path,
        monitor
    );
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err(AppError::Other(format!("文件不存在: {}", file_path)));
    }
    let absolute_path = path
        .canonicalize()
        .map_err(|e| AppError::Other(format!("获取绝对路径失败: {e}")))?;
    let path_str = absolute_path.to_string_lossy().to_string();

    // If a specific monitor is requested, use IDesktopWallpaper on Windows
    #[cfg(target_os = "windows")]
    if let Some(ref mon) = monitor {
        if !mon.is_empty() && mon != "all" {
            match com_wallpaper::set_wallpaper_for_monitor(&path_str, mon) {
                Ok(_) => return Ok("壁纸已设置 (指定显示器)".to_string()),
                Err(e) => {
                    log::warn!(
                        "[set_wallpaper] IDesktopWallpaper 失败，回退到 SystemParametersInfoW: {}",
                        e
                    );
                    // Fall through to default method
                }
            }
        }
    }

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

#[tauri::command]
pub(crate) async fn list_monitors() -> Result<Vec<MonitorInfo>, AppError> {
    log::info!("[CMD] list_monitors called");

    #[cfg(target_os = "windows")]
    {
        let mut monitors = win_monitors::list_monitors();
        // Try to get device paths from IDesktopWallpaper and merge
        let device_paths = com_wallpaper::get_monitor_device_paths();
        if !device_paths.is_empty() && device_paths.len() == monitors.len() {
            // Merge device paths into monitor entries
            for (i, path) in device_paths.iter().enumerate() {
                if i < monitors.len() {
                    monitors[i].id = path.clone();
                }
            }
        } else if !device_paths.is_empty() {
            // If counts don't match, use device paths directly with display names
            let display_monitors = win_monitors::list_monitors();
            monitors = device_paths
                .iter()
                .enumerate()
                .map(|(i, path)| MonitorInfo {
                    id: path.clone(),
                    name: display_monitors
                        .get(i)
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| format!("Monitor {}", i + 1)),
                    is_primary: i == 0,
                    width: display_monitors.get(i).map(|m| m.width).unwrap_or(0),
                    height: display_monitors.get(i).map(|m| m.height).unwrap_or(0),
                })
                .collect();
        }
        log::info!("[list_monitors] found {} monitors", monitors.len());
        Ok(monitors)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let monitors = win_monitors::list_monitors();
        log::info!("[list_monitors] found {} monitors", monitors.len());
        Ok(monitors)
    }
}

// ---------------------------------------------------------------------------
// Wallpaper slideshow
// ---------------------------------------------------------------------------

/// Set the wallpaper to the given path (internal, non-command version).
/// Returns the result message on success.
fn do_set_wallpaper(path_str: &str) -> Result<String, AppError> {
    let path = std::path::Path::new(path_str);
    if !path.exists() {
        return Err(AppError::Other(format!("文件不存在: {}", path_str)));
    }
    let absolute_path = path
        .canonicalize()
        .map_err(|e| AppError::Other(format!("获取绝对路径失败: {e}")))?;
    let abs_str = absolute_path.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    if let Some(result) = set_windows_wallpaper(&abs_str) {
        return Ok(result);
    }

    set_gnome_wallpaper(&abs_str)
        .or_else(|| set_xfce_wallpaper(&abs_str))
        .or_else(|| set_kde_wallpaper(&abs_str))
        .or_else(|| set_sway_wallpaper(&abs_str))
        .or_else(|| set_hyprland_wallpaper(&abs_str))
        .or_else(|| set_swww_wallpaper(&abs_str))
        .or_else(|| set_feh_wallpaper(&abs_str))
        .ok_or_else(|| AppError::Other("未检测到支持的桌面环境".to_string()))
}

#[derive(Clone, serde::Serialize)]
struct SlideshowTick {
    index: usize,
    total: usize,
    name: String,
    path: String,
}

#[tauri::command]
pub(crate) async fn start_slideshow(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    file_paths: Vec<String>,
    interval_secs: u64,
) -> Result<(), AppError> {
    if file_paths.is_empty() {
        return Err(AppError::Other("图片列表为空".into()));
    }
    if interval_secs < 5 {
        return Err(AppError::Other("轮播间隔不能小于 5 秒".into()));
    }

    // Cancel any existing slideshow
    if let Ok(cancel) = state.slideshow_cancel.lock() {
        if let Some(ref flag) = *cancel {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut slot) = state.slideshow_cancel.lock() {
        *slot = Some(cancel_flag.clone());
    }

    let total = file_paths.len();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let mut index = 0usize;
        log::info!(
            "[slideshow] 启动轮播: {} 张图片, 间隔 {}s",
            total,
            interval_secs
        );
        loop {
            if cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                log::info!("[slideshow] 已停止");
                break;
            }

            let path = &file_paths[index];
            match do_set_wallpaper(path) {
                Ok(_) => {
                    let name = std::path::Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let _ = app_handle.emit(
                        "slideshow-tick",
                        SlideshowTick {
                            index,
                            total,
                            name,
                            path: path.clone(),
                        },
                    );
                    log::info!("[slideshow] 切换壁纸 {}/{}: {}", index + 1, total, path);
                }
                Err(e) => {
                    log::warn!("[slideshow] 设置壁纸失败: {}", e);
                }
            }

            index = (index + 1) % total;

            // Sleep in 1s increments so we can check cancel flag more frequently
            let mut elapsed = 0u64;
            while elapsed < interval_secs {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                elapsed += 1;
                if cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub(crate) async fn stop_slideshow(state: tauri::State<'_, AppState>) -> Result<bool, AppError> {
    let stopped = if let Ok(cancel) = state.slideshow_cancel.lock() {
        if let Some(ref flag) = *cancel {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            true
        } else {
            false
        }
    } else {
        false
    };
    if let Ok(mut slot) = state.slideshow_cancel.lock() {
        *slot = None;
    }
    log::info!("[slideshow] stop_slideshow: stopped={}", stopped);
    Ok(stopped)
}

/// Check if a slideshow is currently running.
#[tauri::command]
pub(crate) async fn is_slideshow_running(
    state: tauri::State<'_, AppState>,
) -> Result<bool, AppError> {
    let running = if let Ok(cancel) = state.slideshow_cancel.lock() {
        cancel.is_some()
    } else {
        false
    };
    Ok(running)
}
