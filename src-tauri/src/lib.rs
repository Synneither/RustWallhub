use rusqlite::{Connection, Result};
use std::fs;
use std::path::Path; // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}
#[derive(Debug)]
struct Wallpaper {
    id: i32,
    name: String,
}
fn find_pictures() -> Result<Vec<Wallpaper>> {
    let conn = Connection::open("wallpaler_images.db")?;
    let mut stmt = conn.prepare("SELECT id, name FROM images")?;
    let wallpaper_iter = stmt.query_map((), |row| {
        Ok(Wallpaper {
            id: row.get(0)?,
            name: row.get(1)?,
        })
    })?;

    let mut wallpapers = Vec::new();
    for wallpaper in wallpaper_iter {
        wallpapers.push(wallpaper?);
    }
    Ok(wallpapers)
}

#[tauri::command]
fn read_images_from_dir(dir_path: &str) -> Result<Vec<Vec<u8>>, String> {
    let dir = Path::new(dir_path);

    // 确保是目录
    if !dir.is_dir() {
        return Err("提供的路径不是目录".to_string());
    }

    let mut image_data_vec = Vec::new();

    // 遍历目录
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        let path = entry.path();

                        // 只处理图片文件
                        if let Some(ext) = path.extension() {
                            let ext_str = ext.to_string_lossy().to_lowercase();
                            if ["png", "jpg", "jpeg", "gif", "bmp", "webp"]
                                .contains(&ext_str.as_str())
                            {
                                match fs::read(&path) {
                                    Ok(data) => {
                                        println!(
                                            "已读取: {:?} ({} 字节)",
                                            path.file_name().unwrap_or_default(),
                                            data.len()
                                        );
                                        image_data_vec.push(data);
                                    }
                                    Err(e) => {
                                        eprintln!("无法读取文件 {:?}: {}", path, e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("无法读取目录条目: {}", e);
                    }
                }
            }

            if image_data_vec.is_empty() {
                Err("目录中没有找到图片文件".to_string())
            } else {
                Ok(image_data_vec)
            }
        }
        Err(e) => Err(format!("无法读取目录: {}", e)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_readimage() {
        let path = "/home/synneither/Pictures/背景/wallhaven/".to_string();
        println!("Testing read_images_from_dir with path: {}", path);
        match read_images_from_dir(&path) {
            Ok(data) => println!("Image data length: {}", data.len()),
            Err(e) => eprintln!("Error reading image: {}", e),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet, read_images_from_dir])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
