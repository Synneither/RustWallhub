use std::path::Path;

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

pub fn get_file_extension(content_type: &str, url: &str) -> String {
    let ct = content_type.to_lowercase();
    if ct.contains("image/jpeg") {
        return "jpg".to_string();
    }
    if ct.contains("image/png") {
        return "png".to_string();
    }
    if ct.contains("image/gif") {
        return "gif".to_string();
    }
    if ct.contains("image/webp") {
        return "webp".to_string();
    }

    let url_lower = url.to_lowercase();
    let url_path = url_lower.split('?').next().unwrap_or(&url_lower);
    if url_path.ends_with(".png") {
        return "png".to_string();
    }
    if url_path.ends_with(".gif") {
        return "gif".to_string();
    }
    if url_path.ends_with(".webp") {
        return "webp".to_string();
    }
    "jpg".to_string()
}

pub fn is_valid_image(data: &[u8], content_type: &str) -> bool {
    if data.is_empty() {
        return false;
    }
    if content_type.contains("image/jpeg") {
        return data.starts_with(&[0xFF, 0xD8, 0xFF]);
    }
    if content_type.contains("image/png") {
        return data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }
    if content_type.contains("image/gif") {
        return data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a");
    }
    if content_type.contains("image/webp") {
        return data.len() > 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP";
    }

    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return true;
    }
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return true;
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return true;
    }
    if data.len() > 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return true;
    }

    false
}

pub fn file_is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn compute_md5(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

pub async fn download_image_bytes(
    client: &reqwest::Client,
    url: &str,
) -> Result<(Vec<u8>, String), String> {
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("下载请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("下载返回状态码: {}", resp.status()));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取下载数据失败: {e}"))?;

    if !is_valid_image(&bytes, &content_type) {
        return Err("无效的图片数据".to_string());
    }

    Ok((bytes.to_vec(), content_type))
}
