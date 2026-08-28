mod commands;
mod config;
mod db;
mod downloader;
mod oss;
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
use wallpaper::{
    is_slideshow_running, list_monitors, set_wallpaper, start_slideshow, stop_slideshow,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    log::info!("RustWallhub 启动");

    let app = tauri::Builder::default()
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
            if let Err(e) = std::fs::create_dir_all(&config.wallhaven_save_dir) {
                log::error!("[startup] 创建 wallhaven 目录失败: {e}");
            }
            if let Err(e) = std::fs::create_dir_all(&config.reddit_save_dir) {
                log::error!("[startup] 创建 reddit 目录失败: {e}");
            }
            if let Some(wh_parent) = std::path::Path::new(&wh_db).parent() {
                if !wh_parent.as_os_str().is_empty() {
                    if let Err(e) = std::fs::create_dir_all(wh_parent) {
                        log::error!("[startup] 创建 wallhaven DB 目录失败: {e}");
                    }
                }
            }
            if let Some(rd_parent) = std::path::Path::new(&rd_db).parent() {
                if !rd_parent.as_os_str().is_empty() {
                    if let Err(e) = std::fs::create_dir_all(rd_parent) {
                        log::error!("[startup] 创建 reddit DB 目录失败: {e}");
                    }
                }
            }

            // 只创建目录，不初始化数据库。
            // 数据库文件由前端启动时通过 check_databases / init_databases
            // 询问用户确认后显式创建，避免静默新建。

            let mut client_builder = reqwest::Client::builder()
                .user_agent("RustWallhub/1.0")
                .timeout(Duration::from_secs(config.request_timeout));
            if !config.proxy_url.is_empty() {
                if let Ok(proxy) = reqwest::Proxy::all(&config.proxy_url) {
                    client_builder = client_builder.proxy(proxy);
                    log::info!("[startup] 使用代理: {}", config.proxy_url);
                } else {
                    log::warn!("[startup] 代理设置失败: {}", config.proxy_url);
                }
            }
            let client = client_builder.build().expect("创建 HTTP client 失败");

            let auto_update = config.auto_update;
            let auto_sync_start = config.oss_auto_download_on_start;

            app.manage(AppState {
                config_path: Mutex::new(config_path),
                file_cache: Mutex::new(None),
                cancel_flag: Mutex::new(None),
                http_client: Mutex::new(client),
                config_cache: Mutex::new(Some(config)),
                slideshow_cancel: Mutex::new(None),
            });

            if auto_update {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    commands::settings::startup_check_update(&app_handle).await;
                });
            }

            // 启动自动拉取：延后几秒，等前端完成数据库初始化引导
            if auto_sync_start {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(6)).await;
                    commands::sync::auto_sync_on_startup(&app_handle).await;
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // settings
            get_config,
            save_settings,
            get_stats,
            check_databases,
            init_databases,
            check_update,
            install_update,
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
            list_filtered_image_paths,
            resolve_thumbnails,
            dislike_file,
            dislike_files,
            delete_orphan_file,
            delete_orphan_files,
            adopt_orphan_files,
            clean_thumbnails,
            get_image_info,
            // database
            list_database_images,
            list_orphan_files,
            mark_disliked_files,
            restore_all_files,
            list_missing_images,
            // sync
            export_snapshots,
            import_snapshots,
            oss_sync_upload,
            oss_sync_download,
            test_oss_config,
            // system
            get_active_wallpaper,
            // wallpaper (from wallpaper module)
            set_wallpaper,
            start_slideshow,
            stop_slideshow,
            is_slideshow_running,
            list_monitors,
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用时出错");

    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            // 收尾任务（清临时文件、WAL 归零、按需上传快照）每次进程只跑一次，
            // 否则 exit() 触发的第二次 ExitRequested 会再次拦截，形成死循环。
            if commands::sync::begin_exit_tasks() {
                api.prevent_exit();
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    commands::sync::run_exit_tasks(handle.clone()).await;
                    handle.exit(0);
                });
            }
        }
    });
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
    async fn test_save_image_writes_file_only() {
        let dir = TempDir::new().unwrap();
        let save_path = dir.path().join("test.jpg");
        let data = tiny_jpeg();

        let result = state::save_image(&save_path, &data).await;
        assert!(result.is_ok(), "save_image should succeed");
        assert!(save_path.exists(), "file should be written");
        // 缩略图已改为惰性生成，下载阶段不应创建缩略图文件。
        assert!(!thumbnail::thumb_path(dir.path(), "test.jpg", 2).exists());
    }

    #[tokio::test]
    async fn test_save_image_fails_on_invalid_path() {
        let save_path = std::path::Path::new("/nonexistent/dir/test.jpg");
        let data = b"data";

        let result = state::save_image(save_path, data).await;
        assert!(result.is_err(), "save_image should fail on unwritable path");
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
