//! 读取桌面当前正在使用的壁纸（只读子系统）。
//!
//! 本应用**不设置壁纸**，读它只有两个用途：图库里高亮出当前那张、仪表盘展示。
//! 文件名刻意叫 `current_wallpaper` 而不是 `wallpaper`，避免让人以为这里能改壁纸。
//!
//! 来源按平台/桌面环境依次尝试，任何一个环节失败都只跳过、不影响后续来源：
//! 1. Windows：`IDesktopWallpaper::GetWallpaper`（只读查询，见文件末尾的 `com_wallpaper`）
//! 2. Noctalia v5 CLI（`noctalia msg wallpaper-get`）——GUI 改过的值写在 settings.toml 里，
//!    v4 时代的 `~/.cache/noctalia/wallpapers.json` 在 v5 上不再更新
//! 3. Noctalia v4 缓存 JSON（老版本）
//! 4. awww / swww 的守护进程缓存（`~/.cache/<工具>/<输出名>` 里存的就是当前图路径）
//! 5. Noctalia v5 的 `settings.toml` 兜底（CLI 不在 PATH 上时，例如从某些启动器拉起）
//!
//! 多显示器可能各有一张，所以对外给的是一个列表。

use crate::exec::{cmd, has_command};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 「当前壁纸」的短缓存。
///
/// 读取要 spawn 外部命令（`noctalia msg wallpaper-get` + `theme-mode-get`，非 Noctalia
/// 会话下还有 gsettings），而图库进入/切回视图时都会调一次。壁纸切换是低频操作，
/// 5 秒的陈旧窗口换掉每次进页面的一串进程启动是划算的。
///
/// 缓存里同时存「没有壁纸」（空列表）：探测不到也是结果，没必要反复重试。
static ACTIVE_WALLPAPER_CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
const ACTIVE_WALLPAPER_TTL: Duration = Duration::from_secs(5);

#[derive(Serialize)]
pub struct ActiveWallpaper {
    /// 正在使用的壁纸（多显示器各一张时会有多条）。空列表 = 读不到。
    pub paths: Vec<String>,
}

/// 读一遍当前壁纸（多显示器各一张；空列表 = 读不到）。
///
/// 带 5 秒短缓存：图库进入/切回视图都会调一次，而壁纸切换是低频操作。
/// 命中缓存的「空列表」也直接返回——探测不到同样是结果，没必要反复重试。
pub fn paths() -> Vec<String> {
    if let Some(cached) = cached_active_wallpaper() {
        return cached;
    }

    let mut paths = platform_wallpaper_paths();
    if paths.is_empty() {
        // `theme_mode()` 要 spawn 外部命令，只在真的要读 Noctalia v4 缓存时才求值
        // （Windows 上走不到这一步）。
        if let Some(path) = noctalia_cli_wallpaper()
            .or_else(|| noctalia_v4_cache(&theme_mode()))
            .or_else(daemon_cache_wallpaper)
            .or_else(noctalia_settings_toml)
        {
            paths.push(path);
        }
    }

    if let Ok(mut guard) = ACTIVE_WALLPAPER_CACHE.lock() {
        *guard = Some((Instant::now(), paths.clone()));
    }
    paths
}

/// 平台自带的「当前壁纸」来源。Windows 有系统 COM 查询；其它平台只能靠各桌面环境
/// 自己的 CLI/缓存（就是下面那几个 `noctalia_*` / `daemon_cache_*`）。
#[cfg(target_os = "windows")]
fn platform_wallpaper_paths() -> Vec<String> {
    com_wallpaper::wallpaper_paths()
}

#[cfg(not(target_os = "windows"))]
fn platform_wallpaper_paths() -> Vec<String> {
    Vec::new()
}

/// Windows：查「当前壁纸」的**只读** COM 封装。
///
/// 为什么不用 `SystemParametersInfoW(SPI_GETDESKWALLPAPER)`：它返回的是系统转码出来的
/// 副本（`…\AppData\Roaming\Microsoft\Windows\Themes\TranscodedWallpaper`），不是原图
/// 路径 —— 跟图库里的文件永远对不上号（本机实测确认）。`IDesktopWallpaper::GetWallpaper`
/// 给的才是原图路径，而且一个显示器给一张。
///
/// **本模块只查询**：vtable 里声明 `set_wallpaper` 只是为了占住槽位偏移（顺序必须与 SDK
/// 一致，写错了会静默调到别的方法上），代码里从不调用它 —— 本应用不设置壁纸。
#[cfg(target_os = "windows")]
#[allow(clippy::upper_case_acronyms)] // COM 类型别名保持官方命名（HRESULT/PCWSTR 等）
mod com_wallpaper {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    // CLSID_DesktopWallpaper / IID_IDesktopWallpaper：逐字节照抄 Windows SDK 的
    // `ShObjIdl_core.idl` 与 `shobjidl_core.h`。抄错一位就是 CLASS_E_CLASSNOTREG
    // （错误信息只会说"没有注册类"，不看 GUID 根本猜不到是自己写错了）。
    const CLSID_DESKTOP_WALLPAPER: Guid = Guid {
        data1: 0xC2CF3110,
        data2: 0x460E,
        data3: 0x4FC1,
        data4: [0xB9, 0xD0, 0x8A, 0x1C, 0x0C, 0x9C, 0xC4, 0xBD],
    };
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

    /// `IDesktopWallpaper` 的 vtable。**槽位顺序必须与 SDK 的 `shobjidl_core.h` 一致**：
    ///
    /// ```text
    /// 3 SetWallpaper  4 GetWallpaper  5 GetMonitorDevicePathAt  6 GetMonitorDevicePathCount
    /// ```
    ///
    /// 顺序错了不会报编译错，只会静默调到另一个方法上（历史教训：5/6 写反过，把 Count
    /// 当 PathAt 调，实测返回 0x800706F4）。这里只用 4/5/6 三个查询方法。
    #[repr(C)]
    struct ComVtbl {
        query_interface: unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *const Guid,
            *mut *mut std::ffi::c_void,
        ) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        // IDesktopWallpaper methods（后面的方法不声明也不影响前面这些的偏移）
        set_wallpaper: unsafe extern "system" fn(*mut std::ffi::c_void, PCWSTR, PCWSTR) -> HRESULT,
        get_wallpaper:
            unsafe extern "system" fn(*mut std::ffi::c_void, PCWSTR, *mut *mut u16) -> HRESULT,
        get_monitor_device_path_at:
            unsafe extern "system" fn(*mut std::ffi::c_void, u32, *mut *mut u16) -> HRESULT,
        get_monitor_device_path_count:
            unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32) -> HRESULT,
    }

    extern "system" {
        fn CoInitializeEx(reserved: *const std::ffi::c_void, co_init: u32) -> HRESULT;
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

    /// COM 宽字符串的扫描上限。正常显示器 id / 壁纸路径远不到这个长度，
    /// 这里只作为"万一没有 NUL 结尾"的兜底，避免无限越界。
    const MAX_COM_STRING_LEN: usize = 4096;

    /// `CoUninitialize` 的 RAII 守卫：任何提前 return 或 panic 都会在栈展开时配对调用。
    struct ComInitGuard;

    impl ComInitGuard {
        fn new() -> Option<Self> {
            let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED) };
            // S_OK = 首次初始化，S_FALSE(=1) = 之前已初始化，两者都可用
            (hr == S_OK || hr == 1).then_some(Self)
        }
    }

    impl Drop for ComInitGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    /// 接口指针的 RAII 守卫，保证 `Release` 一定被调用。
    struct ComPtrGuard(*mut std::ffi::c_void);

    impl ComPtrGuard {
        fn vtbl(&self) -> &ComVtbl {
            // SAFETY: 指针来自成功的 CoCreateInstance，指向以 vtable 指针开头的 COM 对象。
            unsafe { &*(self.0 as *const *const ComVtbl).read() }
        }

        fn as_ptr(&self) -> *mut std::ffi::c_void {
            self.0
        }
    }

    impl Drop for ComPtrGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: 同 vtbl()；Release 是本对象最后一次使用。
                unsafe {
                    let vtbl = &*(self.0 as *const *const ComVtbl).read();
                    (vtbl.release)(self.0);
                }
            }
        }
    }

    /// 建 `IDesktopWallpaper` 实例。
    ///
    /// 返回元组的**顺序不能换**：Rust 按声明顺序析构，先 `Release` 再 `CoUninitialize`
    /// 才对（反了等于在 COM 已经卸载之后才去调 `Release`）。
    fn create_instance() -> Option<(ComPtrGuard, ComInitGuard)> {
        let init = ComInitGuard::new()?;

        let mut ptr: *mut std::ffi::c_void = ptr::null_mut();
        // SAFETY: 两个 GUID 都是合法的静态值，ptr 是可写的 out 参数；成功时由 COM 分配。
        let hr = unsafe {
            CoCreateInstance(
                &CLSID_DESKTOP_WALLPAPER,
                ptr::null(),
                CLSCTX_ALL,
                &IID_IDESKTOP_WALLPAPER,
                &mut ptr,
            )
        };
        if hr != S_OK {
            log::warn!(
                "[com_wallpaper] CoCreateInstance(IDesktopWallpaper) 失败: 0x{:08X}",
                hr as u32
            );
            return None;
        }
        Some((ComPtrGuard(ptr), init))
    }

    fn to_wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// 从 COM 返回的宽字符串指针读出 `String`。
    ///
    /// 不能先 `from_raw_parts(ptr, MAX_COM_STRING_LEN)` 再找 NUL —— 那是在声称一段并不
    /// 存在的可读范围（UB）。这里逐字符读到 NUL，再按**实际长度**构造切片。
    fn from_wide(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        // SAFETY: 契约上字符串以 NUL 结尾，循环会在 NUL 处停下；上限只是兜底。
        while len < MAX_COM_STRING_LEN && unsafe { ptr.add(len).read() } != 0 {
            len += 1;
        }
        // SAFETY: 上面已逐字符确认 [0, len) 可读且未越过分配范围。
        let wide: &[u16] = unsafe { std::slice::from_raw_parts(ptr, len) };
        String::from_utf16_lossy(wide)
    }

    /// 每个显示器当前用的壁纸路径（同一个文件只返回一次）。
    ///
    /// 读不到（纯色壁纸、幻灯片、没有桌面会话…）就返回空表，调用方当"没有当前壁纸"处理。
    pub fn wallpaper_paths() -> Vec<String> {
        let Some((p_wallpaper, _init)) = create_instance() else {
            return Vec::new();
        };
        let vtbl = p_wallpaper.vtbl();

        let mut count: u32 = 0;
        let hr = unsafe { (vtbl.get_monitor_device_path_count)(p_wallpaper.as_ptr(), &mut count) };
        if hr != S_OK {
            log::warn!(
                "[com_wallpaper] GetMonitorDevicePathCount 失败: 0x{:08X}",
                hr as u32
            );
            return Vec::new();
        }

        let mut paths: Vec<String> = Vec::new();
        for index in 0..count {
            let mut id_ptr: *mut u16 = ptr::null_mut();
            let hr = unsafe {
                (vtbl.get_monitor_device_path_at)(p_wallpaper.as_ptr(), index, &mut id_ptr)
            };
            if hr != S_OK || id_ptr.is_null() {
                log::warn!(
                    "[com_wallpaper] GetMonitorDevicePathAt({index}) 失败: 0x{:08X}",
                    hr as u32
                );
                continue;
            }
            // 契约上由 CoTaskMemAlloc 分配、NUL 结尾，读完立刻归还。
            let monitor_id = from_wide(id_ptr);
            unsafe { CoTaskMemFree(id_ptr as *const std::ffi::c_void) };
            if monitor_id.is_empty() {
                continue;
            }

            let id_wide = to_wide(&monitor_id);
            let mut path_ptr: *mut u16 = ptr::null_mut();
            let hr = unsafe {
                (vtbl.get_wallpaper)(p_wallpaper.as_ptr(), id_wide.as_ptr(), &mut path_ptr)
            };
            if hr != S_OK || path_ptr.is_null() {
                // 纯色壁纸 / 幻灯片时这里会失败，属正常情况，不值得刷 warn。
                log::debug!(
                    "[com_wallpaper] GetWallpaper({index}) 失败: 0x{:08X}",
                    hr as u32
                );
                continue;
            }
            let path = from_wide(path_ptr);
            unsafe { CoTaskMemFree(path_ptr as *const std::ffi::c_void) };

            // 注册表里可能残留 verbatim 形式，统一降级成普通形式，免得跟图库路径对不上。
            let path = crate::winpath::plain_str(&path);
            if Path::new(&path).is_file() && !paths.contains(&path) {
                paths.push(path);
            }
        }

        paths
    }
}

/// 命中未过期的缓存时返回 `Some`（内部区分「缓存了空列表」与「没缓存」）。
fn cached_active_wallpaper() -> Option<Vec<String>> {
    let guard = ACTIVE_WALLPAPER_CACHE.lock().ok()?;
    guard
        .as_ref()
        .filter(|(at, _)| at.elapsed() < ACTIVE_WALLPAPER_TTL)
        .map(|(_, paths)| paths.clone())
}

/// 执行 `noctalia <子命令>`（v5 的 CLI 在 NixOS flake 安装下叫 `noctalia-shell`，
/// 所以两个名字都试），成功时返回 stdout。
///
/// 外壳没在跑时命令本身就是非零退出，直接当不可用处理，不额外做探测。
fn noctalia_msg(args: &[&str]) -> Option<String> {
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

    /// Windows 真机：COM 只读查询读回来的必须是真实文件。
    ///
    /// vtable 槽位写反时拿到的会是垃圾字符串（或空表），这条能当场发现；不做任何写入。
    /// CI 只跑 Linux，所以这条只在本地 Windows 上执行；读不到（幻灯片/纯色）时它平凡通过。
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_wallpaper_paths_point_at_real_files() {
        for path in super::platform_wallpaper_paths() {
            assert!(
                std::path::Path::new(&path).is_file(),
                "读到的当前壁纸应当是真实文件: {path}"
            );
        }
    }

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
