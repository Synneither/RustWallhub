mod commands;
mod config;
mod db;
mod downloader;
mod reddit;
mod state;
mod thumbnail;
mod wallhaven;
mod wallpaper;

use commands::*;
use config::AppConfig;
use state::AppState;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::Manager;
use wallpaper::set_wallpaper;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    log::info!("RustWallhub 启动");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let config_dir = app
                .path()
                .config_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let config_path = config_dir.join("rustwallhub").join("config.json");

            let mut config = AppConfig::load(&config_path).unwrap_or_default();
            config.sync_db_dir();
            if let Some(base_dir) = config_path.parent() {
                config.db_dir = state::normalize_config_path(base_dir, config.db_dir);
                config.sync_db_dir();
                config.wallhaven_save_dir =
                    state::normalize_config_path(base_dir, config.wallhaven_save_dir);
                config.reddit_save_dir =
                    state::normalize_config_path(base_dir, config.reddit_save_dir);
            }

            let wh_db = config.wallhaven_db_path.clone();
            let rd_db = config.reddit_db_path.clone();
            std::fs::create_dir_all(&config.wallhaven_save_dir).ok();
            std::fs::create_dir_all(&config.reddit_save_dir).ok();
            if let Some(wh_parent) = std::path::Path::new(&wh_db).parent() {
                if !wh_parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(wh_parent).ok();
                }
            }
            if let Some(rd_parent) = std::path::Path::new(&rd_db).parent() {
                if !rd_parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(rd_parent).ok();
                }
            }

            db::init_wallhaven_db(&wh_db).ok();
            db::init_reddit_db(&rd_db).ok();

            let client = reqwest::Client::builder()
                .user_agent("RustWallhub/1.0")
                .timeout(Duration::from_secs(config.request_timeout))
                .build()
                .expect("创建 HTTP client 失败");

            let auto_update = config.auto_update;

            app.manage(AppState {
                config_path: Mutex::new(config_path),
                file_cache: Mutex::new(None),
                cancel_flag: Mutex::new(None),
                http_client: Mutex::new(client),
                config_cache: Mutex::new(Some(config)),
            });

            if auto_update {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    commands::settings::startup_check_update(&app_handle).await;
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // settings
            get_config,
            save_settings,
            get_stats,
            check_update,
            // wallhaven
            search_wallhaven,
            start_wallhaven_download,
            download_wallhaven_selected,
            // reddit
            start_reddit_download,
            // download
            recover_database_files,
            download_missing_images,
            cancel_downloads,
            // gallery
            browse_image_files,
            resolve_thumbnail,
            resolve_thumbnails,
            delete_image,
            dislike_file,
            delete_orphan_file,
            adopt_orphan_files,
            clean_thumbnails,
            // database
            list_database_images,
            list_orphan_files,
            mark_disliked_files,
            count_missing_images,
            restore_all_files,
            list_missing_images,
            // system
            get_active_wallpaper,
            scan_directory,
            // wallpaper (from wallpaper module)
            set_wallpaper,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时出错");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A tiny valid JPEG, 1x1 pixel, created programmatically.
    fn tiny_jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(1, 1, image::Rgb([128u8, 64, 32]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        buf.into_inner()
    }

    #[tokio::test]
    async fn test_save_image_writes_file_and_thumbnail() {
        let dir = TempDir::new().unwrap();
        let thumb_dir = TempDir::new().unwrap();
        let save_path = dir.path().join("test.jpg");
        let data = tiny_jpeg();

        let handle = state::save_image(&save_path, &data, thumb_dir.path(), "test.jpg", 2).await;
        assert!(handle.is_some(), "save_image should succeed");
        assert!(save_path.exists(), "file should be written");

        let _ = handle.unwrap().await;
        let thumb = thumbnail::thumb_path(thumb_dir.path(), "test.jpg", 2);
        assert!(
            thumb.exists(),
            "thumbnail should be generated at {}",
            thumb.display()
        );
    }

    #[tokio::test]
    async fn test_save_image_fails_on_invalid_path() {
        let thumb_dir = TempDir::new().unwrap();
        let save_path = std::path::Path::new("/nonexistent/dir/test.jpg");
        let data = b"data";

        let handle = state::save_image(save_path, data, thumb_dir.path(), "test.jpg", 2).await;
        assert!(
            handle.is_none(),
            "save_image should fail on unwritable path"
        );
    }

    #[test]
    fn test_normalize_config_path_absolute() {
        let abs = "/home/user/wallhaven.db".to_string();
        let base = std::path::Path::new("/tmp");
        let result = state::normalize_config_path(base, abs.clone());
        assert_eq!(result, abs);
    }

    #[test]
    fn test_normalize_config_path_relative_resolved_to_base() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("images.db");
        db::init_wallhaven_db(&db_path.to_string_lossy()).unwrap();

        let result = state::normalize_config_path(dir.path(), "images.db".to_string());
        assert!(result.contains("images.db"));
        assert!(
            result.contains(&dir.path().to_string_lossy().to_string()),
            "result should be under base_dir, got: {result}"
        );
    }
}
