mod config;
mod db;
mod downloader;
mod reddit;
mod thumbnail;
mod wallhaven;

use config::AppConfig;
use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use tauri::{Emitter, Manager};

#[derive(Clone, Serialize)]
struct DownloadProgress {
    source: String,
    done: u32,
    total: u32,
    message: String,
}

#[derive(Clone, Serialize)]
struct DownloadComplete {
    source: String,
    success: u32,
    total: u32,
    message: String,
}

struct AppState {
    config_path: Mutex<PathBuf>,
}

#[derive(thiserror::Error, Debug)]
enum AppError {
    #[error("{0}")]
    Db(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Other(String),
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

fn find_upward(
    base_dir: &std::path::Path,
    relative: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let mut current = base_dir.to_path_buf();
    loop {
        let candidate = current.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn database_score(path: &std::path::Path) -> Option<i64> {
    if !path.exists() {
        return None;
    }
    let conn = Connection::open(path).ok()?;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))
        .ok()?;
    Some(count)
}

fn normalize_config_path(base_dir: &std::path::Path, value: String) -> String {
    let path = std::path::PathBuf::from(&value);
    if path.is_absolute() {
        return value;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| base_dir.to_path_buf());
    let cwd_resolved = cwd.join(&path);
    let config_resolved = base_dir.join(&path);

    let mut candidates = Vec::new();
    candidates.push(cwd_resolved.clone());
    if let Some(found) = find_upward(&cwd, &path) {
        if found != cwd_resolved {
            candidates.push(found);
        }
    }
    candidates.push(config_resolved.clone());
    if let Some(found) = find_upward(base_dir, &path) {
        if found != config_resolved {
            candidates.push(found);
        }
    }

    let mut best: Option<(&std::path::PathBuf, i64)> = None;
    for candidate in &candidates {
        if let Some(score) = database_score(candidate) {
            if best.is_none() || score > best.as_ref().unwrap().1 {
                best = Some((candidate, score));
            }
        }
    }

    if let Some((best_path, _)) = best {
        return best_path.to_string_lossy().to_string();
    }

    if cwd_resolved.exists() {
        return cwd_resolved.to_string_lossy().to_string();
    }
    if config_resolved.exists() {
        return config_resolved.to_string_lossy().to_string();
    }
    candidates
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or(config_resolved)
        .to_string_lossy()
        .to_string()
}

fn load_config(state: &tauri::State<'_, AppState>) -> Result<AppConfig, AppError> {
    let path = state
        .config_path
        .lock()
        .map_err(|e| AppError::Config(format!("锁定配置失败: {e}")))?
        .clone();
    let mut config = AppConfig::load(&path).map_err(|e| AppError::Config(e))?;
    if let Some(base_dir) = path.parent() {
        config.wallhaven_db_path = normalize_config_path(base_dir, config.wallhaven_db_path);
        config.reddit_db_path = normalize_config_path(base_dir, config.reddit_db_path);
        config.wallhaven_save_dir = normalize_config_path(base_dir, config.wallhaven_save_dir);
        config.reddit_save_dir = normalize_config_path(base_dir, config.reddit_save_dir);
    }
    Ok(config)
}

fn save_config(state: &tauri::State<'_, AppState>, config: &AppConfig) -> Result<(), AppError> {
    let path = state
        .config_path
        .lock()
        .map_err(|e| AppError::Config(format!("锁定配置失败: {e}")))?
        .clone();
    config.save(&path).map_err(|e| AppError::Config(e))
}

#[tauri::command]
async fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, AppError> {
    load_config(&state)
}

#[tauri::command]
async fn save_settings(
    state: tauri::State<'_, AppState>,
    config: AppConfig,
) -> Result<(), AppError> {
    save_config(&state, &config)
}

#[derive(serde::Serialize)]
struct StatsResponse {
    wallhaven: db::DbStats,
    reddit: db::DbStats,
}

#[tauri::command]
async fn get_stats(state: tauri::State<'_, AppState>) -> Result<StatsResponse, AppError> {
    let config = load_config(&state)?;
    let wh_db_path = config.wallhaven_db_path.clone();
    let rd_db_path = config.reddit_db_path.clone();
    println!(
        "get_stats resolving db paths: wallhaven={}, reddit={}",
        wh_db_path, rd_db_path
    );
    let wh_stats = db::get_wallhaven_stats(&wh_db_path)?;
    let rd_stats = db::get_reddit_stats(&rd_db_path)?;
    println!(
        "get_stats result: wallhaven={:?}, reddit={:?}",
        wh_stats, rd_stats
    );
    Ok(StatsResponse {
        wallhaven: wh_stats,
        reddit: rd_stats,
    })
}

#[tauri::command]
async fn start_wallhaven_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    let config = load_config(&state)?;
    let app_clone = app.clone();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let rt = tokio::runtime::Handle::current();
        let client = reqwest::Client::builder()
            .user_agent("RustWallhub/1.0")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;

        let wh_client = wallhaven::WallhavenClient::new(config.wallhaven_api_key.clone());

        std::fs::create_dir_all(&config.wallhaven_save_dir)
            .map_err(|e| format!("创建目录失败: {e}"))?;

        let existing_ids =
            db::get_existing_wallhaven_ids(&config.wallhaven_db_path).map_err(|e| e.to_string())?;
        let existing_set: std::collections::HashSet<String> = existing_ids.into_iter().collect();

        let mut collected: Vec<(wallhaven::WallhavenImage,)> = Vec::new();
        let target = config.wallhaven_max_images;
        let mut page = 1u32;
        let max_pages = 100u32;

        while (collected.len() as u32) < target && page <= max_pages {
            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: "wallhaven".into(),
                    done: collected.len() as u32,
                    total: target,
                    message: format!("正在获取第 {page} 页..."),
                },
            );

            let resp = rt.block_on(wh_client.search(
                page,
                &config.wallhaven_categories,
                &config.wallhaven_purity,
                &config.wallhaven_sorting,
                &config.wallhaven_top_range,
                &config.wallhaven_atleast,
                &config.wallhaven_ratios,
            ));

            match resp {
                Ok(data) => {
                    if data.data.is_empty() {
                        break;
                    }
                    for img in data.data {
                        if (collected.len() as u32) >= target {
                            break;
                        }
                        if !existing_set.contains(&img.id) {
                            collected.push((img,));
                        }
                    }
                }
                Err(e) => {
                    let _ = app_clone.emit(
                        "download-progress",
                        DownloadProgress {
                            source: "wallhaven".into(),
                            done: collected.len() as u32,
                            total: target,
                            message: format!("获取第 {page} 页失败: {e}"),
                        },
                    );
                    break;
                }
            }
            page += 1;
            std::thread::sleep(std::time::Duration::from_secs(2));
        }

        let total = collected.len() as u32;
        let mut success = 0u32;

        for (i, (img,)) in collected.iter().enumerate() {
            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: "wallhaven".into(),
                    done: i as u32,
                    total,
                    message: format!("正在下载 {} ({}/{})", img.id, i + 1, total),
                },
            );

            let url = &img.path;
            let result = rt.block_on(downloader::download_image_bytes(&client, url));

            match result {
                Ok((bytes, content_type)) => {
                    let ext = downloader::get_file_extension(&content_type, url);
                    let safe_id = img
                        .id
                        .chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>();
                    let filename = format!("wallhaven_{safe_id}.{ext}");
                    let save_path =
                        std::path::Path::new(&config.wallhaven_save_dir).join(&filename);
                    let hash = downloader::compute_md5(&bytes);

                    if std::fs::write(&save_path, &bytes).is_ok() {
                        if db::insert_wallhaven_image(
                            &config.wallhaven_db_path,
                            &img.id,
                            &filename,
                            &hash,
                            url,
                            &img.short_url,
                            &img.resolution,
                        )
                        .unwrap_or(false)
                        {
                            success += 1;
                        }
                    }
                }
                Err(_) => {}
            }
        }

        let _ = app_clone.emit(
            "download-complete",
            DownloadComplete {
                source: "wallhaven".into(),
                success,
                total,
                message: format!("Wallhaven 下载完成: 成功 {success}/{total}"),
            },
        );

        Ok(())
    });

    Ok("Wallhaven 下载已启动".to_string())
}

#[tauri::command]
async fn start_reddit_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    let config = load_config(&state)?;
    let app_clone = app.clone();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let rt = tokio::runtime::Handle::current();
        let client = reqwest::Client::builder()
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;

        let reddit_client = reddit::RedditClient::new();

        std::fs::create_dir_all(&config.reddit_save_dir)
            .map_err(|e| format!("创建目录失败: {e}"))?;

        let existing_urls =
            db::get_existing_reddit_urls(&config.reddit_db_path).map_err(|e| e.to_string())?;
        let existing_set: std::collections::HashSet<String> = existing_urls.into_iter().collect();

        let target = config.reddit_max_images;
        let mut collected: Vec<reddit::RedditImage> = Vec::new();
        let mut after: Option<String> = None;
        let mut empty_batches = 0u32;

        while (collected.len() as u32) < target {
            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: "reddit".into(),
                    done: collected.len() as u32,
                    total: target,
                    message: format!("正在获取帖子... (已找到 {} 张)", collected.len()),
                },
            );

            let result =
                rt.block_on(reddit_client.fetch_posts(after.as_deref(), config.reddit_max_posts));

            match result {
                Ok((images, next_after)) => {
                    let prev_len = collected.len();
                    for img in images {
                        if (collected.len() as u32) >= target {
                            break;
                        }
                        if !existing_set.contains(&img.image_url) {
                            collected.push(img);
                        }
                    }
                    if collected.len() == prev_len {
                        empty_batches += 1;
                        if empty_batches >= 3 {
                            break;
                        }
                    } else {
                        empty_batches = 0;
                    }
                    after = next_after;
                    if after.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = app_clone.emit(
                        "download-progress",
                        DownloadProgress {
                            source: "reddit".into(),
                            done: collected.len() as u32,
                            total: target,
                            message: format!("获取帖子失败: {e}"),
                        },
                    );
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }

        let total = collected.len() as u32;
        let mut success = 0u32;

        for (i, img) in collected.iter().enumerate() {
            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: "reddit".into(),
                    done: i as u32,
                    total,
                    message: format!("正在下载 ({}/{})", i + 1, total),
                },
            );

            let result = rt.block_on(downloader::download_image_bytes(&client, &img.image_url));

            match result {
                Ok((bytes, content_type)) => {
                    let ext = downloader::get_file_extension(&content_type, &img.image_url);
                    let hash = downloader::compute_md5(&bytes);
                    let filename = format!("{hash}.{ext}");
                    let save_path = std::path::Path::new(&config.reddit_save_dir).join(&filename);

                    if std::fs::write(&save_path, &bytes).is_ok() {
                        if db::insert_reddit_image(
                            &config.reddit_db_path,
                            &filename,
                            &hash,
                            &img.image_url,
                            &img.title,
                            &img.permalink,
                        )
                        .unwrap_or(false)
                        {
                            success += 1;
                        }
                    }
                }
                Err(_) => {}
            }
        }

        let _ = app_clone.emit(
            "download-complete",
            DownloadComplete {
                source: "reddit".into(),
                success,
                total,
                message: format!("Reddit 下载完成: 成功 {success}/{total}"),
            },
        );

        Ok(())
    });

    Ok("Reddit 下载已启动".to_string())
}

#[tauri::command]
async fn start_db_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    source: String,
) -> Result<String, AppError> {
    let config = load_config(&state)?;
    let app_clone = app.clone();
    let source_clone = source.clone();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let rt = tokio::runtime::Handle::current();
        let client = reqwest::Client::builder()
            .user_agent("RustWallhub/1.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;

        let (save_dir, db_path) = if source_clone == "wallhaven" {
            (
                config.wallhaven_save_dir.clone(),
                config.wallhaven_db_path.clone(),
            )
        } else if source_clone == "reddit" {
            (
                config.reddit_save_dir.clone(),
                config.reddit_db_path.clone(),
            )
        } else {
            return Err("未知源".to_string());
        };

        std::fs::create_dir_all(&save_dir).map_err(|e| format!("创建目录失败: {e}"))?;

        let images = if source_clone == "wallhaven" {
            db::get_wallhaven_missing_love(&db_path).map_err(|e| e.to_string())?
        } else {
            db::get_reddit_missing_love(&db_path).map_err(|e| e.to_string())?
        };

        let total = images.len() as u32;
        let mut success = 0u32;

        for (i, img) in images.iter().enumerate() {
            let file_path = std::path::Path::new(&save_dir).join(&img.name);
            if file_path.exists() {
                continue;
            }

            let _ = app_clone.emit(
                "download-progress",
                DownloadProgress {
                    source: source_clone.clone(),
                    done: i as u32,
                    total,
                    message: format!("正在下载 {} ({}/{})", img.name, i + 1, total),
                },
            );

            let result = rt.block_on(downloader::download_image_bytes(&client, &img.url));

            if let Ok((bytes, _content_type)) = result {
                if std::fs::write(&file_path, &bytes).is_ok() {
                    success += 1;
                }
            }
        }

        let _ = app_clone.emit(
            "download-complete",
            DownloadComplete {
                source: source_clone,
                success,
                total,
                message: format!("数据库下载完成: 成功 {success}/{total}"),
            },
        );

        Ok(())
    });

    Ok(format!("{source} 数据库下载已启动"))
}

#[tauri::command]
async fn mark_dislike(state: tauri::State<'_, AppState>, source: String) -> Result<u64, AppError> {
    let config = load_config(&state)?;
    if source == "wallhaven" {
        Ok(db::mark_missing_dislike_wallhaven(
            &config.wallhaven_db_path,
            &config.wallhaven_save_dir,
        )?)
    } else if source == "reddit" {
        Ok(db::mark_missing_dislike_reddit(
            &config.reddit_db_path,
            &config.reddit_save_dir,
        )?)
    } else {
        let w = db::mark_missing_dislike_wallhaven(
            &config.wallhaven_db_path,
            &config.wallhaven_save_dir,
        )?;
        let r = db::mark_missing_dislike_reddit(&config.reddit_db_path, &config.reddit_save_dir)?;
        Ok(w + r)
    }
}

#[tauri::command]
async fn restore_love(state: tauri::State<'_, AppState>, source: String) -> Result<u64, AppError> {
    let config = load_config(&state)?;
    if source == "wallhaven" {
        Ok(db::restore_love_wallhaven(&config.wallhaven_db_path)?)
    } else if source == "reddit" {
        Ok(db::restore_love_reddit(&config.reddit_db_path)?)
    } else {
        let w = db::restore_love_wallhaven(&config.wallhaven_db_path)?;
        let r = db::restore_love_reddit(&config.reddit_db_path)?;
        Ok(w + r)
    }
}

#[tauri::command]
async fn get_images(
    state: tauri::State<'_, AppState>,
    source: String,
    limit: i64,
    offset: i64,
) -> Result<Vec<db::ImageRecord>, AppError> {
    let config = load_config(&state)?;
    if source == "wallhaven" {
        Ok(db::get_wallhaven_images(
            &config.wallhaven_db_path,
            limit,
            offset,
        )?)
    } else {
        Ok(db::get_reddit_images(
            &config.reddit_db_path,
            limit,
            offset,
        )?)
    }
}

#[tauri::command]
async fn list_local_images(
    state: tauri::State<'_, AppState>,
    source: String,
    offset: usize,
    limit: usize,
) -> Result<serde_json::Value, AppError> {
    let config = load_config(&state)?;
    let dir = if source == "wallhaven" {
        &config.wallhaven_save_dir
    } else {
        &config.reddit_save_dir
    };

    let path = PathBuf::from(dir);
    if !path.is_dir() {
        return Ok(serde_json::json!({ "images": [], "total": 0 }));
    }

    let mut images: Vec<serde_json::Value> = Vec::new();
    let mut filenames: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&path) {
        for entry in entries.flatten() {
            let file_path = entry.path();
            if file_path.is_file() && downloader::file_is_image(&file_path) {
                let name = entry.file_name().to_string_lossy().to_string();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                // 检查缩略图是否已存在
                let thumb_path = if thumbnail::thumb_exists(&path, &name) {
                    Some(
                        thumbnail::thumb_path(&path, &name)
                            .to_string_lossy()
                            .to_string(),
                    )
                } else {
                    None
                };
                images.push(serde_json::json!({
                    "name": name,
                    "path": file_path.to_string_lossy().to_string(),
                    "thumb_path": thumb_path,
                    "size": size,
                }));
                filenames.push(name);
            }
        }
    }

    images.sort_by(|a, b| {
        b["name"]
            .as_str()
            .unwrap_or("")
            .cmp(a["name"].as_str().unwrap_or(""))
    });

    let total = images.len();
    let page = images
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();

    Ok(serde_json::json!({ "images": page, "total": total }))
}

#[tauri::command]
async fn get_thumbnail_path(
    state: tauri::State<'_, AppState>,
    source: String,
    filename: String,
) -> Result<String, AppError> {
    let config = load_config(&state)?;
    let dir = if source == "wallhaven" {
        &config.wallhaven_save_dir
    } else {
        &config.reddit_save_dir
    };
    let image_dir = PathBuf::from(dir);

    // 确保缩略图存在
    let result = thumbnail::ensure_thumbnail(&image_dir, &filename);
    match result {
        Ok(thumb_path) => Ok(thumb_path.to_string_lossy().to_string()),
        Err(_) => {
            // 缩略图生成失败，回退到原图
            Ok(image_dir.join(&filename).to_string_lossy().to_string())
        }
    }
}

#[tauri::command]
async fn get_thumbnail_paths(
    state: tauri::State<'_, AppState>,
    source: String,
    filenames: Vec<String>,
) -> Result<serde_json::Value, AppError> {
    let config = load_config(&state)?;
    let dir = if source == "wallhaven" {
        &config.wallhaven_save_dir
    } else {
        &config.reddit_save_dir
    };
    let image_dir = PathBuf::from(dir);

    let result: Vec<serde_json::Value> = filenames
        .into_iter()
        .map(|filename| {
            let thumb_path = thumbnail::ensure_thumbnail(&image_dir, &filename)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| image_dir.join(&filename).to_string_lossy().to_string());
            serde_json::json!({
                "name": filename,
                "thumb_path": thumb_path,
            })
        })
        .collect();

    Ok(serde_json::json!({ "items": result }))
}

#[tauri::command]
async fn set_wallpaper(file_path: String) -> Result<String, AppError> {
    // 检测桌面环境并设置壁纸
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err(AppError::Other(format!("文件不存在: {}", file_path)));
    }

    let absolute_path = path
        .canonicalize()
        .map_err(|e| AppError::Other(format!("获取绝对路径失败: {e}")))?;
    let path_str = absolute_path.to_string_lossy().to_string();

    // 尝试多种桌面环境
    // GNOME
    if Command::new("gsettings")
        .args(["get", "org.gnome.desktop.background", "picture-uri"])
        .output()
        .is_ok()
    {
        let uri = format!("file://{}", path_str);
        Command::new("gsettings")
            .args(["set", "org.gnome.desktop.background", "picture-uri", &uri])
            .output()
            .map_err(|e| AppError::Other(format!("GNOME 设置壁纸失败: {e}")))?;
        Command::new("gsettings")
            .args([
                "set",
                "org.gnome.desktop.background",
                "picture-uri-dark",
                &uri,
            ])
            .output()
            .ok(); // dark mode 可选
        return Ok("壁纸已设置 (GNOME)".to_string());
    }

    // XFCE
    if let Ok(output) = Command::new("xfconf-query")
        .args(["-c", "xfce4-desktop", "-lv"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // 找到所有图片属性
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(2, ' ').collect();
            if parts.len() == 2 && parts[0].contains("last-image") {
                Command::new("xfconf-query")
                    .args([
                        "-c",
                        "xfce4-desktop",
                        "-p",
                        parts[0].trim(),
                        "-s",
                        &path_str,
                    ])
                    .output()
                    .map_err(|e| AppError::Other(format!("XFCE 设置壁纸失败: {e}")))?;
                return Ok("壁纸已设置 (XFCE)".to_string());
            }
        }
    }

    // KDE Plasma
    if Command::new("kwriteconfig5")
        .args(["--help"])
        .output()
        .is_ok()
    {
        let script = format!(
            "var allDesktops = desktops();
for (var i = 0; i < allDesktops.length; i++) {{
    var d = allDesktops[i];
    d.wallpaperPlugin = 'org.kde.image';
    d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General'];
    d.writeConfig('Image', 'file://{path_str}');
}}"
        );
        Command::new("qdbus")
            .args([
                "org.kde.plasmashell",
                "/PlasmaShell",
                "org.kde.PlasmaShell.evaluateScript",
                &script,
            ])
            .output()
            .map_err(|e| AppError::Other(format!("KDE 设置壁纸失败: {e}")))?;
        return Ok("壁纸已设置 (KDE)".to_string());
    }

    // sway / Hyprland (wlr)
    if let Ok(output) = Command::new("swaymsg").args(["-t", "get_outputs"]).output() {
        if output.status.success() {
            Command::new("swaymsg")
                .args(["output", "*", "bg", &path_str, "fill"])
                .output()
                .map_err(|e| AppError::Other(format!("sway 设置壁纸失败: {e}")))?;
            return Ok("壁纸已设置 (sway)".to_string());
        }
    }

    // Hyprland
    if let Ok(_output) = Command::new("hyprctl")
        .args(["hyprpaper", "preload", &path_str])
        .output()
    {
        Command::new("hyprctl")
            .args(["hyprpaper", "wallpaper", ",", &path_str])
            .output()
            .map_err(|e| AppError::Other(format!("Hyprland 设置壁纸失败: {e}")))?;
        return Ok("壁纸已设置 (Hyprland)".to_string());
    }

    // 使用 feh 作为后备
    if let Ok(_output) = Command::new("feh").args(["--bg-fill", &path_str]).output() {
        return Ok("壁纸已设置 (feh)".to_string());
    }

    Err(AppError::Other(
        "未检测到支持的桌面环境。支持: GNOME, KDE, XFCE, sway, Hyprland, feh".to_string(),
    ))
}

#[tauri::command]
async fn delete_image(
    state: tauri::State<'_, AppState>,
    source: String,
    name: String,
) -> Result<bool, AppError> {
    let config = load_config(&state)?;
    let (save_dir, db_path) = if source == "wallhaven" {
        (&config.wallhaven_save_dir, &config.wallhaven_db_path)
    } else {
        (&config.reddit_save_dir, &config.reddit_db_path)
    };

    // 删除文件
    let file_path = std::path::Path::new(save_dir).join(&name);
    if file_path.exists() {
        std::fs::remove_file(&file_path)
            .map_err(|e| AppError::Other(format!("删除文件失败: {e}")))?;
    }

    // 删除缩略图
    let thumb_path = std::path::Path::new(save_dir)
        .join("thumb_cache")
        .join(&name);
    if thumb_path.exists() {
        std::fs::remove_file(&thumb_path).ok();
    }

    // 删除数据库记录
    if source == "wallhaven" {
        db::delete_wallhaven_image(db_path, &name).map_err(AppError::Db)?;
    } else {
        db::delete_reddit_image(db_path, &name).map_err(AppError::Db)?;
    }

    Ok(true)
}

#[tauri::command]
async fn clean_thumbnails(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, AppError> {
    let config = load_config(&state)?;
    let wh_cleaned = db::clean_stale_thumbnails(
        &std::path::Path::new(&config.wallhaven_save_dir)
            .join("thumb_cache")
            .to_string_lossy(),
    )
    .unwrap_or(0);
    let rd_cleaned = db::clean_stale_thumbnails(
        &std::path::Path::new(&config.reddit_save_dir)
            .join("thumb_cache")
            .to_string_lossy(),
    )
    .unwrap_or(0);
    Ok(serde_json::json!({
        "wallhaven": wh_cleaned,
        "reddit": rd_cleaned,
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let config_dir = app
                .path()
                .config_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let config_path = config_dir.join("rustwallhub").join("config.json");

            let config = AppConfig::load(&config_path).unwrap_or_default();

            let wh_db = config.wallhaven_db_path.clone();
            let rd_db = config.reddit_db_path.clone();
            std::fs::create_dir_all(&config.wallhaven_save_dir).ok();
            std::fs::create_dir_all(&config.reddit_save_dir).ok();
            // 确保数据库目录存在
            if let Some(wh_parent) = std::path::Path::new(&wh_db).parent() {
                std::fs::create_dir_all(wh_parent).ok();
            }
            if let Some(rd_parent) = std::path::Path::new(&rd_db).parent() {
                std::fs::create_dir_all(rd_parent).ok();
            }

            db::init_wallhaven_db(&wh_db).ok();
            db::init_reddit_db(&rd_db).ok();

            app.manage(AppState {
                config_path: Mutex::new(config_path),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_settings,
            get_stats,
            start_wallhaven_download,
            start_reddit_download,
            start_db_download,
            mark_dislike,
            restore_love,
            get_images,
            list_local_images,
            get_thumbnail_path,
            get_thumbnail_paths,
            set_wallpaper,
            delete_image,
            clean_thumbnails,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时出错");
}
