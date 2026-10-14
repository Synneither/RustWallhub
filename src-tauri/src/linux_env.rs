//! Linux / Wayland 启动期环境适配。
//!
//! WebKitGTK 的 DMA-BUF 渲染路径在**专有 NVIDIA 驱动**上有已知问题：白/黑窗口、
//! resize 时静默退出、以及 `Gdk-Message: Error 71 (Protocol error) dispatching to
//! Wayland display`。官方给的两条路按会话类型二选一：
//!
//! - X11：关掉 DMA-BUF 渲染器（`WEBKIT_DISABLE_DMABUF_RENDERER=1`）
//! - Wayland：关掉 NVIDIA 显式同步（`__NV_DISABLE_EXPLICIT_SYNC=1`）
//!
//! Wayland 这条更划算：DMA-BUF 渲染器本身是更快的路径，只有 Hyprland 这种严格
//! 校验 acquire point 的合成器会被它杀掉连接；niri 等宽容的合成器不需要牺牲它，
//! 只在 `egl-wayland2` 不可用时退掉显式同步即可（驱动 ≥ 560 且有对应 EGL
//! external platform 清单时 egl-wayland2 可用，此时什么都不用改）。
//!
//! 判定用运行时 `cfg!` 而不是 `#[cfg]`，好处是这些逻辑在 Windows 上也会被编译，
//! 本地 clippy 能直接查出问题（Linux 相关代码平时只有 CI 才会编到）。
//!
//! 另外这里还要撤掉 AppImage 打包脚本强加的 `GDK_BACKEND=x11`，见
//! [`undo_packaging_forced_x11`]。

use std::path::Path;

/// 在 GTK/WebKit 初始化**之前**调用一次（`run()` 开头，此时进程仍是单线程，
/// 设置环境变量不会有并发读写的风险）。
pub fn init() {
    if !cfg!(target_os = "linux") {
        return;
    }
    undo_packaging_forced_x11();
    log_environment();
    apply_webkit_workarounds();
}

/// AppImage 的 `AppRun`（linuxdeploy 的 `linuxdeploy-plugin-gtk.sh`）会**无条件**
/// `export GDK_BACKEND=x11`，把 GTK 从 Wayland 拽回 X11/XWayland。在 Wayland 会话下这不是
/// 用户的选择，而且会带来两个实际问题：
///
/// 1. 键盘输入进不去：GTK 走 X11 后端后，输入框能聚焦却打不进字（中文输入法同样失效）。
///    `.deb` / `cargo tauri dev` 都不带这个环境变量，所以只有 AppImage 中招。
/// 2. 窗口 `app-id` 变成 X11 的 WM_CLASS（`Rustwallhub`），与 README 里按 `rustwallhub`
///    写的 niri 窗口规则对不上，dock 图标也会错位。
///
/// 这里在 GTK 初始化之前把这条被强加的变量撤掉，让 GDK 回到会话真实的后端。只在
/// 「确实从 AppImage 启动（`APPIMAGE` 存在）且值恰为 `x11` 且会话确实是 Wayland
/// （`WAYLAND_DISPLAY` 存在）」时动手；纯 X11 会话不受影响。打包脚本会覆盖用户手写的
/// `GDK_BACKEND`，所以无法区分「用户自己设的 x11」——真要在 Wayland 上强制 X11，
/// 用 `.deb` 或 `cargo tauri dev` 运行时设置即可。
fn undo_packaging_forced_x11() {
    if std::env::var_os("APPIMAGE").is_none() || std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    let forced_x11 = matches!(
        std::env::var("GDK_BACKEND").as_deref(),
        Ok(value) if value.trim().eq_ignore_ascii_case("x11")
    );
    if !forced_x11 {
        return;
    }
    log::info!("[linux-env] 撤销 AppImage 强加的 GDK_BACKEND=x11，改用会话自带的 Wayland 后端");
    std::env::remove_var("GDK_BACKEND");
}

/// 记一条环境摘要，排查「在谁的会话里、跑的是哪个包」时直接看日志即可。
fn log_environment() {
    let session = if is_wayland_session() {
        "wayland"
    } else {
        "x11"
    };
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "-".to_string());
    log::info!(
        "[linux-env] session={session} desktop={desktop} appimage={} nvidia={}",
        std::env::var_os("APPIMAGE").is_some(),
        nvidia_proprietary_present(),
    );
}

fn apply_webkit_workarounds() {
    if !nvidia_proprietary_present() {
        return;
    }
    if is_wayland_session() {
        if egl_wayland2_active() {
            log::info!("[linux-env] NVIDIA + Wayland：egl-wayland2 可用，保持 WebKit 默认渲染路径");
            return;
        }
        set_env_if_absent("__NV_DISABLE_EXPLICIT_SYNC", "1");
    } else {
        set_env_if_absent("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

/// 只在用户没显式设置过时写入：用户自己在 shell 里的选择优先级更高。
fn set_env_if_absent(key: &str, value: &str) {
    if std::env::var_os(key).is_some() {
        log::info!("[linux-env] {key} 已由用户设置，跳过兼容项");
        return;
    }
    log::info!("[linux-env] 应用 WebKit 兼容项 {key}={value}");
    std::env::set_var(key, value);
}

/// GDK 实际选择的图形后端优先（逗号分隔的候选列表，第一个识别出的生效），
/// 其次是会话类型，最后才看 `WAYLAND_DISPLAY`。
fn is_wayland_session() -> bool {
    if let Ok(backend) = std::env::var("GDK_BACKEND") {
        for entry in backend.split(',') {
            match entry.trim() {
                "wayland" => return true,
                "x11" => return false,
                _ => {}
            }
        }
    }
    if let Ok(session) = std::env::var("XDG_SESSION_TYPE") {
        if !session.is_empty() {
            return session.eq_ignore_ascii_case("wayland");
        }
    }
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// 专有 NVIDIA 驱动是否在工作（nouveau 不算，它不走这条渲染路径）。
fn nvidia_proprietary_present() -> bool {
    if Path::new("/sys/module/nvidia").exists() {
        return true;
    }
    // 兜底：看 DRM 设备的 driver 软链指向谁。AppImage 里 /sys/class/drm 可读，
    // 且这条路径在 Flatpak 沙箱内同样可用。
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return false;
    };
    entries.flatten().any(|entry| {
        std::fs::read_link(entry.path().join("device/driver"))
            .ok()
            .and_then(|target| {
                target
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .is_some_and(|name| name == "nvidia")
    })
}

/// egl-wayland2 是否生效：需要驱动 ≥ 560，且装了对应的 EGL external platform 清单。
fn egl_wayland2_active() -> bool {
    if nvidia_driver_major() < Some(560) {
        return false;
    }
    const DIRS: [&str; 2] = [
        "/etc/egl/egl_external_platform.d",
        "/usr/share/egl/egl_external_platform.d",
    ];
    DIRS.iter().any(|dir| {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries.flatten().any(|entry| {
                    std::fs::read_to_string(entry.path())
                        .map(|content| {
                            content.contains("egl-wayland") || content.contains("egl_wayland")
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    })
}

/// 从 `/proc/driver/nvidia/version` 读驱动主版本号：
/// `NVRM version: NVIDIA UNIX x86_64 Kernel Module  560.35.03  ...` → `560`
fn nvidia_driver_major() -> Option<u32> {
    let content = std::fs::read_to_string("/proc/driver/nvidia/version").ok()?;
    let line = content
        .lines()
        .find(|line| line.starts_with("NVRM version"))?;
    line.split_whitespace().find_map(|token| {
        let head = token.split(['.', '-']).next()?;
        head.parse::<u32>().ok()
    })
}
