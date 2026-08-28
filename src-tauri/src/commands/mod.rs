//! Tauri command modules, grouped by functional domain.

pub mod database;
pub mod download;
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
