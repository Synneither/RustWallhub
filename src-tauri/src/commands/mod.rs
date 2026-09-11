//! Tauri command modules, grouped by functional domain.

pub mod database;
pub mod download;
/// 下载流程的公共部分（事件发送、文件回滚）。不导出为命令，仅供各下载模块复用。
pub mod download_common;
pub mod gallery;
pub mod reddit;
pub mod settings;
pub mod sync;
pub mod system;
pub mod wallhaven;

// Re-export all commands for `generate_handler!`
pub use database::*;
pub use download::*;
pub use gallery::*;
pub use reddit::*;
pub use settings::*;
pub use sync::*;
pub use system::*;
pub use wallhaven::*;
