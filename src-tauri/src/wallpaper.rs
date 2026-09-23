//! Wallpaper setting for Linux and Windows desktop environments.
//! Each setter probes whether its environment is available, returns `None` if not.

use crate::exec::{cmd, has_command};
use crate::state::{escape_path_percent, AppError, AppState};
use serde::Serialize;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::Emitter;

// ---------------------------------------------------------------------------
// 告警消息与 Noctalia CLI
// ---------------------------------------------------------------------------

/// 执行 `noctalia msg <args>`，成功时返回 stdout。
///
/// v5 的 CLI 在 NixOS flake 安装下叫 `noctalia-shell`，所以两个名字都试；
/// 外壳没在跑时命令本身就是非零退出，直接当不可用处理，不额外做探测。
pub(crate) fn noctalia_msg(args: &[&str]) -> Option<String> {
    for cli in ["noctalia", "noctalia-shell"] {
        if !has_command(cli) {
            continue;
        }
        let Ok(out) = cmd(cli).arg("msg").args(args).output() else {
            continue;
        };
        if out.status.success() {
            return Some(String::from_utf8_lossy(&out.stdout).to_string());
        }
    }
    None
}

fn output_ok(command: &mut Command) -> bool {
    command.output().is_ok_and(|out| out.status.success())
}

/// 拼「壁纸已设置 (后端 · 显示器)」提示，避免各后端各写一遍。
fn done_message(backend: &str, monitor: Option<&str>) -> String {
    match monitor {
        Some(name) => format!("壁纸已设置 ({backend} · {name})"),
        None => format!("壁纸已设置 ({backend})"),
    }
}

// ---------------------------------------------------------------------------
// Monitor info
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
    use super::{cmd, MonitorInfo};
    use serde_json::Value;

    /// 依次尝试各合成器，取第一个能给出结果的方式。
    ///
    /// Wayland 侧优先用合成器自己的 IPC 而不是 xrandr：niri 只跑 Wayland，
    /// xrandr 要么不可用、要么只能看到 XWayland 的假输出；而且合成器给出的
    /// 输出名（`eDP-1` / `HDMI-A-1`）正好是 Noctalia `wallpaper-set` 需要的连接器名。
    pub fn list_monitors() -> Vec<MonitorInfo> {
        niri_monitors()
            .or_else(hyprctl_monitors)
            .or_else(sway_monitors)
            .or_else(xrandr_monitors)
            .unwrap_or_else(|| {
                vec![MonitorInfo {
                    id: "default".to_string(),
                    name: "Default".to_string(),
                    is_primary: true,
                    width: 0,
                    height: 0,
                }]
            })
    }

    fn json_u32(value: Option<&Value>) -> u32 {
        value.and_then(Value::as_u64).unwrap_or(0) as u32
    }

    fn json_string(value: Option<&Value>) -> Option<String> {
        value.and_then(Value::as_str).map(str::to_string)
    }

    /// 执行 `<program> <args...>` 并解析 stdout 为 JSON，非零退出或解析失败都返回 None。
    fn json_command(program: &str, args: &[&str]) -> Option<Value> {
        let out = cmd(program).args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        serde_json::from_slice(&out.stdout).ok()
    }

    /// niri：`niri msg --json outputs` 返回 `{连接器名: {...}}` 映射。
    /// `logical` 为 null 表示该输出已关闭，不进列表。
    fn niri_monitors() -> Option<Vec<MonitorInfo>> {
        let outputs = json_command("niri", &["msg", "--json", "outputs"])?;
        let map = outputs.as_object()?;
        // niri 没有 "主显示器" 概念，用当前聚焦的输出代替。
        let focused = json_command("niri", &["msg", "--json", "focused-output"])
            .and_then(|value| json_string(value.get("name")));

        let mut monitors: Vec<MonitorInfo> = map
            .iter()
            .filter_map(|(name, info)| {
                let logical = info.get("logical")?;
                Some(MonitorInfo {
                    id: name.clone(),
                    name: name.clone(),
                    is_primary: focused.as_deref() == Some(name.as_str()),
                    width: json_u32(logical.get("width")),
                    height: json_u32(logical.get("height")),
                })
            })
            .collect();

        if monitors.is_empty() {
            return None;
        }
        // 聚焦输出拿不到时（不同 niri 版本的字段差异），至少保留一块标记为主。
        if !monitors.iter().any(|m| m.is_primary) {
            monitors[0].is_primary = true;
        }
        Some(monitors)
    }

    /// Hyprland：`hyprctl monitors -j` 返回数组，`focused` 当主显示器标记。
    fn hyprctl_monitors() -> Option<Vec<MonitorInfo>> {
        let value = json_command("hyprctl", &["monitors", "-j"])?;
        let monitors: Vec<MonitorInfo> = value
            .as_array()?
            .iter()
            .filter_map(|monitor| {
                let name = json_string(monitor.get("name"))?;
                Some(MonitorInfo {
                    id: name.clone(),
                    name,
                    is_primary: monitor
                        .get("focused")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    width: json_u32(monitor.get("width")),
                    height: json_u32(monitor.get("height")),
                })
            })
            .collect();
        (!monitors.is_empty()).then_some(monitors)
    }

    /// sway / i3 系：`swaymsg -t get_outputs`，跳过未启用的输出。
    fn sway_monitors() -> Option<Vec<MonitorInfo>> {
        let value = json_command("swaymsg", &["-t", "get_outputs"])?;
        let monitors: Vec<MonitorInfo> = value
            .as_array()?
            .iter()
            .filter(|output| {
                output
                    .get("active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .filter_map(|output| {
                let name = json_string(output.get("name"))?;
                let rect = output.get("rect");
                Some(MonitorInfo {
                    id: name.clone(),
                    name,
                    is_primary: output
                        .get("primary")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    width: json_u32(rect.and_then(|r| r.get("width"))),
                    height: json_u32(rect.and_then(|r| r.get("height"))),
                })
            })
            .collect();
        (!monitors.is_empty()).then_some(monitors)
    }

    /// X11 回退：`xrandr --listmonitors`。
    fn xrandr_monitors() -> Option<Vec<MonitorInfo>> {
        let out = cmd("xrandr").arg("--listmonitors").output().ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let monitors: Vec<MonitorInfo> = stdout
            .lines()
            .skip(1) // skip header line
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 3 {
                    return None;
                }
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
            })
            .collect();
        (!monitors.is_empty()).then_some(monitors)
    }
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

    /// 从 COM 返回的宽字符串指针读出 String。
    ///
    /// `IDesktopWallpaper::GetMonitorDevicePathAt` 通过 `CoTaskMemAlloc` 返回 LPWSTR，
    /// 按 API 契约以 NUL 结尾，但**分配长度只有 `len + 1`**。所以不能先
    /// `from_raw_parts(ptr, MAX_COM_STRING_LEN)` 声称有 4096 个元素再找 NUL ——
    /// 那是在构造一个越界切片（UB），即使实际只读到第一个 NUL 也已经是未定义行为。
    ///
    /// 这里逐字符读到 NUL，再按**实际长度**构造切片；`MAX_COM_STRING_LEN` 仅作为
    /// 万一未按约定结尾时的兜底上限，避免无限越界。`std::ffi::OsString::from_wide`
    /// 同样需要一个已确定长度的切片，无法直接用在裸指针上。
    fn from_wide(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        // SAFETY: 调用方保证 ptr 指向契约上以 NUL 结尾的宽字符串，循环会在 NUL 处终止；
        // 上限只是防御性兜底。
        while len < MAX_COM_STRING_LEN && unsafe { ptr.add(len).read() } != 0 {
            len += 1;
        }
        // SAFETY: 上面已逐字符确认 [0, len) 可读，且未越过分配范围。
        let wide: &[u16] = unsafe { std::slice::from_raw_parts(ptr, len) };
        String::from_utf16_lossy(wide)
    }

    /// COM 宽字符串的扫描上限。正常显示器设备路径不到 200 字符，
    /// 留足余量的同时保证即使未正确 NUL 结尾也不会越界读到进程外。
    const MAX_COM_STRING_LEN: usize = 4096;

    /// `CoUninitialize` 的 RAII 守卫。放到结构体里，任何提前 return 或 panic
    /// 都会在栈展开时配对调用，不会漏掉。
    struct ComInitGuard {
        should_uninit: bool,
    }

    impl ComInitGuard {
        /// 成功初始化 COM 时返回 `Some(guard)`；失败返回 `None`。
        fn new() -> Option<Self> {
            let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED) };
            // S_OK = 首次初始化，S_FALSE = 之前已初始化，两者都可用
            if hr != S_OK && hr != 1 {
                return None;
            }
            Some(Self {
                should_uninit: true,
            })
        }
    }

    impl Drop for ComInitGuard {
        fn drop(&mut self) {
            if self.should_uninit {
                unsafe { CoUninitialize() };
            }
        }
    }

    /// COM 接口指针的 RAII 守卫，保证 `Release` 一定被调用。
    struct ComPtrGuard(*mut std::ffi::c_void);

    impl ComPtrGuard {
        fn vtbl(&self) -> &ComVtbl {
            unsafe { &*(self.0 as *const *const ComVtbl).read() }
        }

        fn as_ptr(&self) -> *mut std::ffi::c_void {
            self.0
        }
    }

    impl Drop for ComPtrGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let vtbl = &*(self.0 as *const *const ComVtbl).read();
                    (vtbl.release)(self.0);
                }
            }
        }
    }

    /// Set wallpaper on a specific monitor (or all monitors if monitor_id is empty).
    /// Uses the IDesktopWallpaper COM interface.
    pub fn set_wallpaper_for_monitor(path: &str, monitor_id: &str) -> Result<(), String> {
        let _com = match ComInitGuard::new() {
            Some(guard) => guard,
            None => return Err("CoInitializeEx failed".to_string()),
        };

        let p_wallpaper = unsafe {
            let mut ptr: *mut std::ffi::c_void = ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_DESKTOP_WALLPAPER,
                ptr::null(),
                CLSCTX_ALL,
                &IID_IDESKTOP_WALLPAPER,
                &mut ptr,
            );
            if hr != S_OK {
                return Err(format!("CoCreateInstance failed: 0x{:08X}", hr as u32));
            }
            ComPtrGuard(ptr)
        };

        let vtbl = p_wallpaper.vtbl();
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

        let hr =
            unsafe { (vtbl.set_wallpaper)(p_wallpaper.as_ptr(), monitor_ptr, path_wide.as_ptr()) };

        if hr != S_OK {
            return Err(format!("SetWallpaper failed: 0x{:08X}", hr as u32));
        }
        Ok(())
    }

    /// Get the number of monitor device paths and each path.
    pub fn get_monitor_device_paths() -> Vec<String> {
        let _com = match ComInitGuard::new() {
            Some(guard) => guard,
            None => return Vec::new(),
        };

        let p_wallpaper = unsafe {
            let mut ptr: *mut std::ffi::c_void = ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_DESKTOP_WALLPAPER,
                ptr::null(),
                CLSCTX_ALL,
                &IID_IDESKTOP_WALLPAPER,
                &mut ptr,
            );
            if hr != S_OK {
                return Vec::new();
            }
            ComPtrGuard(ptr)
        };

        let vtbl = p_wallpaper.vtbl();

        let mut count: u32 = 0;
        if unsafe { (vtbl.get_monitor_device_path_count)(p_wallpaper.as_ptr(), &mut count) } != S_OK
        {
            return Vec::new();
        }

        let mut paths = Vec::new();
        for i in 0..count {
            let mut ptr_path: *mut u16 = ptr::null_mut();
            let hr = unsafe {
                (vtbl.get_monitor_device_path_at)(p_wallpaper.as_ptr(), i, &mut ptr_path)
            };
            if hr == S_OK && !ptr_path.is_null() {
                paths.push(from_wide(ptr_path));
                unsafe { CoTaskMemFree(ptr_path as *const std::ffi::c_void) };
            }
        }

        paths
    }
}

// ---------------------------------------------------------------------------

/// Noctalia（Quickshell 桌面外壳，niri 上最常见的搭配）
///
/// 壁纸由外壳自己绘制（layer-shell 背景层 + GLSL 转场），主题调色板也是从壁纸派生的，
/// 所以必须走它的 IPC：另起 awww/swaybg 去抢背景层会互相覆盖，Noctalia 的主题色也不会跟着变。
///
/// - v5：`noctalia msg wallpaper-set [连接器名] <路径>`（NixOS flake 装的是 `noctalia-shell msg ...`）
/// - v4：`qs -c noctalia-shell ipc call wallpaper set <路径> [连接器名]`
///
/// 不额外做可用性探测：外壳没在跑时命令本身就是非零退出，直接当失败处理即可，
/// 顺带省掉一次进程启动。
fn set_noctalia_wallpaper(path_str: &str, monitor: Option<&str>) -> Option<String> {
    let mut args = vec!["wallpaper-set"];
    args.extend(monitor);
    args.push(path_str);
    if noctalia_msg(&args).is_some() {
        return Some(done_message("Noctalia", monitor));
    }

    if has_command("qs") {
        let mut args = vec![
            "-c",
            "noctalia-shell",
            "ipc",
            "call",
            "wallpaper",
            "set",
            path_str,
        ];
        args.extend(monitor);
        if output_ok(cmd("qs").args(&args)) {
            return Some(done_message("Noctalia", monitor));
        }
    }
    None
}

/// GNOME (gsettings)
fn set_gnome_wallpaper(path_str: &str, monitor: Option<&str>) -> Option<String> {
    if !has_command("gsettings") {
        return None;
    }
    let uri = format!("file://{}", escape_path_percent(path_str));
    // 亮/暗两套 key 都写，否则跟随系统主题切换后会回退到旧图。
    if output_ok(cmd("gsettings").args([
        "set",
        "org.gnome.desktop.background",
        "picture-uri",
        &uri,
    ])) {
        let _ = output_ok(cmd("gsettings").args([
            "set",
            "org.gnome.desktop.background",
            "picture-uri-dark",
            &uri,
        ]));
        return Some(done_message("GNOME", monitor));
    }
    None
}

/// XFCE (xfconf-query)
fn set_xfce_wallpaper(path_str: &str, monitor: Option<&str>) -> Option<String> {
    if !has_command("xfconf-query") {
        return None;
    }
    let output = cmd("xfconf-query")
        .args(["-c", "xfce4-desktop", "-lv"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut any = false;
    let mut all_ok = true;
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() == 2 && parts[0].contains("last-image") {
            // 每个显示器/工作区都有一个独立的 last-image 属性，逐个设置，
            // 不要命中第一个就 return——否则多显示器/多工作区只设了第一块。
            any = true;
            if !output_ok(cmd("xfconf-query").args([
                "-c",
                "xfce4-desktop",
                "-p",
                parts[0].trim(),
                "-s",
                path_str,
            ])) {
                all_ok = false;
            }
        }
    }
    (any && all_ok).then(|| done_message("XFCE", monitor))
}

/// KDE Plasma (kwriteconfig 探测可用性 + qdbus 调 plasmashell 脚本)
fn set_kde_wallpaper(path_str: &str, monitor: Option<&str>) -> Option<String> {
    if !["kwriteconfig6", "kwriteconfig5"]
        .into_iter()
        .any(has_command)
    {
        return None;
    }
    // Plasma 6 里 qdbus 改名为 qdbus6，老名字可能已经不存在。
    let qdbus = ["qdbus6", "qdbus", "qdbus-qt5"]
        .into_iter()
        .find(|tool| has_command(tool))?;
    log::info!("[set_wallpaper] detected KDE Plasma (qdbus: {qdbus})");
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
        escape_path_percent(&escaped)
    );
    let output = cmd(qdbus)
        .args([
            "org.kde.plasmashell",
            "/PlasmaShell",
            "org.kde.PlasmaShell.evaluateScript",
            &script,
        ])
        .output()
        .ok()?;
    if output.status.success() {
        return Some(done_message("KDE", monitor));
    }
    None
}

/// sway (swaymsg)
fn set_sway_wallpaper(path_str: &str, monitor: Option<&str>) -> Option<String> {
    // 探测 sway 是否在跑：能列出输出说明 IPC 通。
    if !output_ok(cmd("swaymsg").args(["-t", "get_outputs"])) {
        return None;
    }
    // 指定显示器时只设那一块；`*` 在 swaymsg 里代表全部输出。
    let target = monitor.unwrap_or("*");
    if !output_ok(cmd("swaymsg").args(["output", target, "bg", path_str, "fill"])) {
        return None;
    }
    Some(done_message("sway", monitor))
}

/// Hyprland (hyprpaper)
fn set_hyprland_wallpaper(path_str: &str, monitor: Option<&str>) -> Option<String> {
    if !output_ok(cmd("hyprctl").arg("--version")) {
        return None;
    }
    // 先 preload 才能给显示器设置，重复 preload 同一张图是幂等的。
    let _ = output_ok(cmd("hyprctl").args(["hyprpaper", "preload", path_str]));

    let targets: Vec<String> = match monitor {
        Some(name) => vec![name.to_string()],
        None => cmd("hyprctl")
            .args(["monitors", "-j"])
            .output()
            .ok()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|line| line.contains("\"name\":"))
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.splitn(2, ':').collect();
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
            .unwrap_or_default(),
    };

    // 逐块设置，不因某一块失败就短路后续显示器（iter().all 会在第一次失败时停止）。
    let mut all_ok = !targets.is_empty();
    for target in &targets {
        if !output_ok(cmd("hyprctl").args([
            "hyprpaper",
            "wallpaper",
            &format!("{target},{path_str}"),
        ])) {
            all_ok = false;
        }
    }
    // 单显示器且拿不到名字时，用空显示器名让 hyprpaper 自己挑一个。
    if all_ok {
        return Some(done_message("Hyprland", monitor));
    }
    output_ok(cmd("hyprctl").args(["hyprpaper", "wallpaper", &format!(",{path_str}")]))
        .then(|| done_message("Hyprland", monitor))
}

/// awww / swww —— niri、sway 上常用的独立壁纸守护进程
///
/// swww 已更名为 awww（CLI 兼容），这里按 awww → swww 的顺序尝试。
/// 守护进程没在跑时先拉起来再重试一次：壁纸守护进程本来就该常驻，
/// 少了这一步会表现为「命令存在却设不上，最后落到 feh 那条死路」。
fn set_swww_wallpaper(path_str: &str, monitor: Option<&str>) -> Option<String> {
    for tool in ["awww", "swww"] {
        if !has_command(tool) {
            continue;
        }
        if apply_daemon_wallpaper(tool, path_str, monitor) {
            return Some(done_message(tool, monitor));
        }
        if start_wallpaper_daemon(tool) && apply_daemon_wallpaper(tool, path_str, monitor) {
            return Some(done_message(tool, monitor));
        }
    }
    None
}

fn apply_daemon_wallpaper(tool: &str, path_str: &str, monitor: Option<&str>) -> bool {
    let mut args = vec![
        "img",
        "--transition-type",
        "fade",
        "--transition-step",
        "60",
    ];
    if let Some(name) = monitor {
        args.push("-o");
        args.push(name);
    }
    args.push(path_str);
    output_ok(cmd(tool).args(&args))
}

/// 拉起 `<tool>-daemon` 并等它把 IPC socket 建好。
/// 只用 spawn 不 wait：守护进程要常驻，父进程退出后它继续提供背景层。
fn start_wallpaper_daemon(tool: &str) -> bool {
    let daemon = format!("{tool}-daemon");
    if !has_command(&daemon) {
        return false;
    }
    match cmd(&daemon).spawn() {
        Ok(_) => {
            std::thread::sleep(std::time::Duration::from_millis(400));
            true
        }
        Err(_) => false,
    }
}

/// feh（最后回退，仅 X11 有意义：它是往 X 根窗口贴图）
fn set_feh_wallpaper(path_str: &str, monitor: Option<&str>) -> Option<String> {
    output_ok(cmd("feh").args(["--bg-fill", path_str])).then(|| done_message("feh", monitor))
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

/// 缓存上次探测成功的 Linux 桌面环境，轮播/连续设壁纸时避免逐项执行外部命令探测。
/// 用 `Mutex<Option<_>>` 而非 `OnceLock`：探测结果可能失效（DE 会话切换、工具被卸载），
/// 失败时清空缓存并回退到全量探测，而不是永久卡在一个已失效的 backend 上。
static LINUX_BACKEND: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

type LinuxSetter = fn(&str, Option<&str>) -> Option<String>;

fn set_with_backend(path: &str, monitor: Option<&str>, backend: &str) -> Option<String> {
    match backend {
        "noctalia" => set_noctalia_wallpaper(path, monitor),
        "hyprland" => set_hyprland_wallpaper(path, monitor),
        "sway" => set_sway_wallpaper(path, monitor),
        "awww" => set_swww_wallpaper(path, monitor),
        "kde" => set_kde_wallpaper(path, monitor),
        "gnome" => set_gnome_wallpaper(path, monitor),
        "xfce" => set_xfce_wallpaper(path, monitor),
        "feh" => set_feh_wallpaper(path, monitor),
        _ => None,
    }
}

/// 后端探测顺序 = 命中优先级。
///
/// Noctalia 排第一：它自己画背景层，走它的 IPC 才能让壁纸和调色板主题一致；
/// 其后是各合成器原生方案，最后才是 GNOME/KDE/XFCE 的桌面设置和 X11 的 feh。
fn linux_backends() -> [(&'static str, LinuxSetter); 8] {
    [
        ("noctalia", set_noctalia_wallpaper),
        ("hyprland", set_hyprland_wallpaper),
        ("sway", set_sway_wallpaper),
        ("awww", set_swww_wallpaper),
        ("kde", set_kde_wallpaper),
        ("gnome", set_gnome_wallpaper),
        ("xfce", set_xfce_wallpaper),
        ("feh", set_feh_wallpaper),
    ]
}

fn set_linux_wallpaper(
    path: &str,
    monitor: Option<&str>,
) -> Result<(String, &'static str), AppError> {
    // 快路径：用缓存的 backend 直接设置。持锁期间只读取缓存值，不调用外部命令。
    let cached = LINUX_BACKEND.lock().ok().and_then(|guard| *guard);
    if let Some(backend) = cached {
        if let Some(message) = set_with_backend(path, monitor, backend) {
            return Ok((message, backend));
        }
        // 缓存的 backend 失效：清空，回退到下方全量探测。
        if let Ok(mut guard) = LINUX_BACKEND.lock() {
            *guard = None;
        }
    }

    for (name, setter) in linux_backends() {
        if let Some(message) = setter(path, monitor) {
            if let Ok(mut guard) = LINUX_BACKEND.lock() {
                *guard = Some(name);
            }
            return Ok((message, name));
        }
    }

    Err(AppError::Other(
        "未检测到可用的壁纸后端。支持 Windows / Noctalia / GNOME / KDE / XFCE / sway / Hyprland / awww(swww) / feh；\
         niri 本身不画壁纸，请先启动 Noctalia，或安装并运行 awww-daemon"
            .to_string(),
    ))
}

/// 设壁纸的对外入口：设置成功后顺手失效「当前壁纸」的短缓存，
/// 否则刚设完壁纸回到仪表盘看到的还是旧图（缓存 TTL 5 秒）。
fn set_wallpaper_sync(path_str: &str, monitor: Option<&str>) -> Result<String, AppError> {
    let result = set_wallpaper_impl(path_str, monitor);
    if result.is_ok() {
        crate::commands::system::invalidate_active_wallpaper();
    }
    result
}

/// 实际的壁纸设置逻辑（同步，会探测/调用桌面环境命令，调用方应放入 spawn_blocking）。
fn set_wallpaper_impl(path_str: &str, monitor: Option<&str>) -> Result<String, AppError> {
    // If a specific monitor is requested, use IDesktopWallpaper on Windows
    #[cfg(target_os = "windows")]
    if let Some(mon) = monitor {
        if !mon.is_empty() && mon != "all" {
            match com_wallpaper::set_wallpaper_for_monitor(path_str, mon) {
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
    if let Some(result) = set_windows_wallpaper(path_str) {
        return Ok(result);
    }

    // Windows 的显示器参数已在上面消费掉；Linux 各后端自行决定用不用（Noctalia/Hyprland/sway 支持）。
    #[cfg(target_os = "windows")]
    let monitor = None::<&str>;

    set_linux_wallpaper(path_str, monitor).map(|(message, _)| message)
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

    // 桌面环境探测会启动多个外部命令，放入阻塞线程池执行。
    let monitor = monitor.clone();
    tokio::task::spawn_blocking(move || set_wallpaper_sync(&path_str, monitor.as_deref()))
        .await
        .map_err(|e| AppError::Other(format!("设置壁纸任务异常: {e}")))?
}

#[tauri::command]
pub(crate) async fn list_monitors() -> Result<Vec<MonitorInfo>, AppError> {
    log::info!("[CMD] list_monitors called");

    #[cfg(target_os = "windows")]
    {
        let mut monitors = tokio::task::spawn_blocking(win_monitors::list_monitors)
            .await
            .map_err(|e| AppError::Other(format!("获取显示器列表失败: {e}")))?;
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
        let monitors = tokio::task::spawn_blocking(win_monitors::list_monitors)
            .await
            .map_err(|e| AppError::Other(format!("获取显示器列表失败: {e}")))?;
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
    set_wallpaper_sync(&abs_str, None)
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
            let path_owned = path.clone();
            match tokio::task::spawn_blocking(move || do_set_wallpaper(&path_owned)).await {
                Ok(Ok(_)) => {
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
                Ok(Err(e)) => {
                    log::warn!("[slideshow] 设置壁纸失败: {}", e);
                }
                Err(e) => {
                    log::warn!("[slideshow] 设置壁纸任务异常: {}", e);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_message_includes_monitor_when_given() {
        assert_eq!(done_message("Noctalia", None), "壁纸已设置 (Noctalia)");
        assert_eq!(
            done_message("Noctalia", Some("DP-1")),
            "壁纸已设置 (Noctalia · DP-1)"
        );
    }

    #[test]
    fn backend_table_is_disjoint() {
        // 缓存里存的 backend 名靠 set_with_backend 分发，重名会让快路径指向错的实现。
        let backends = linux_backends();
        let mut names: Vec<&str> = backends.iter().map(|(name, _)| *name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "backend 名字不能重复");

        // 未登记的名字必须落到兜底分支，不能误命中某个后端。
        assert!(set_with_backend("/nonexistent/wallpaper.png", None, "不存在").is_none());
    }
}
