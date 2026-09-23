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

use std::path::Path;

/// 在 GTK/WebKit 初始化**之前**调用一次（`run()` 开头，此时进程仍是单线程，
/// 设置环境变量不会有并发读写的风险）。
pub fn init() {
    if !cfg!(target_os = "linux") {
        return;
    }
    log_environment();
    apply_webkit_workarounds();
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
