use std::path::Path;

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

/// JPEG magic bytes: FF D8 FF
const JPEG_HEADER: [u8; 3] = [0xFF, 0xD8, 0xFF];
/// PNG magic bytes
const PNG_HEADER: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
/// GIF87a magic bytes
const GIF87_HEADER: [u8; 6] = *b"GIF87a";
/// GIF89a magic bytes
const GIF89_HEADER: [u8; 6] = *b"GIF89a";
/// WebP magic: RIFF....WEBP
const WEBP_RIFF: [u8; 4] = *b"RIFF";
const WEBP_ID: [u8; 4] = *b"WEBP";

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

/// 检测数据是否为有效的图片（通过 magic bytes 验证）
fn has_valid_magic_bytes(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    data.starts_with(&JPEG_HEADER)
        || data.starts_with(&PNG_HEADER)
        || data.starts_with(&GIF87_HEADER)
        || data.starts_with(&GIF89_HEADER)
        || (data.len() > 12 && data.starts_with(&WEBP_RIFF) && data[8..12] == WEBP_ID)
}

pub fn is_valid_image(data: &[u8], content_type: &str) -> bool {
    if data.is_empty() {
        return false;
    }
    // 优先通过 content-type 匹配对应格式的 magic bytes
    if content_type.contains("image/jpeg") {
        return data.starts_with(&JPEG_HEADER);
    }
    if content_type.contains("image/png") {
        return data.starts_with(&PNG_HEADER);
    }
    if content_type.contains("image/gif") {
        return data.starts_with(&GIF87_HEADER) || data.starts_with(&GIF89_HEADER);
    }
    if content_type.contains("image/webp") {
        return data.len() > 12 && data.starts_with(&WEBP_RIFF) && data[8..12] == WEBP_ID;
    }
    // 无 content-type 时自动检测 magic bytes
    has_valid_magic_bytes(data)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_is_image_valid_extensions() {
        assert!(file_is_image(Path::new("photo.jpg")));
        assert!(file_is_image(Path::new("photo.jpeg")));
        assert!(file_is_image(Path::new("photo.png")));
        assert!(file_is_image(Path::new("photo.gif")));
        assert!(file_is_image(Path::new("photo.webp")));
        assert!(file_is_image(Path::new("photo.JPG")));
        assert!(file_is_image(Path::new("photo.PNG")));
    }

    #[test]
    fn test_file_is_image_invalid_extensions() {
        assert!(!file_is_image(Path::new("photo.txt")));
        assert!(!file_is_image(Path::new("photo.bmp")));
        assert!(!file_is_image(Path::new("photo.svg")));
        assert!(!file_is_image(Path::new("photo.mp4")));
        assert!(!file_is_image(Path::new("file")));
    }

    #[test]
    fn test_get_file_extension_from_content_type() {
        assert_eq!(get_file_extension("image/jpeg", "http://example.com/img.jpg"), "jpg");
        assert_eq!(get_file_extension("image/png", "http://example.com/img.png"), "png");
        assert_eq!(get_file_extension("image/gif", "http://example.com/img.gif"), "gif");
        assert_eq!(get_file_extension("image/webp", "http://example.com/img.webp"), "webp");
    }

    #[test]
    fn test_get_file_extension_fallback_to_url() {
        assert_eq!(get_file_extension("application/octet-stream", "http://example.com/img.png"), "png");
        assert_eq!(get_file_extension("application/octet-stream", "http://example.com/img.gif"), "gif");
        assert_eq!(get_file_extension("application/octet-stream", "http://example.com/img.webp"), "webp");
        assert_eq!(get_file_extension("application/octet-stream", "http://example.com/img.jpg"), "jpg");
    }

    #[test]
    fn test_get_file_extension_unknown_fallback_jpg() {
        assert_eq!(get_file_extension("application/octet-stream", "http://example.com/img"), "jpg");
        assert_eq!(get_file_extension("image/something-weird", "http://example.com/data"), "jpg");
    }

    #[test]
    fn test_is_valid_image_jpeg() {
        let jpeg_header = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
        assert!(is_valid_image(&jpeg_header, "image/jpeg"));
        assert!(is_valid_image(&jpeg_header, "image/jpeg; charset=utf-8"));
    }

    #[test]
    fn test_is_valid_image_png() {
        let png_header = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
        assert!(is_valid_image(&png_header, "image/png"));
    }

    #[test]
    fn test_is_valid_image_gif() {
        assert!(is_valid_image(b"GIF87a...", "image/gif"));
        assert!(is_valid_image(b"GIF89a...", "image/gif"));
    }

    #[test]
    fn test_is_valid_image_webp() {
        let webp = b"RIFF....WEBPabcd".to_vec();
        assert!(webp.len() > 12);
        assert!(is_valid_image(&webp, "image/webp"));
    }

    #[test]
    fn test_is_valid_image_empty() {
        assert!(!is_valid_image(&[], "image/jpeg"));
        assert!(!is_valid_image(&[], ""));
    }

    #[test]
    fn test_is_valid_image_invalid_data() {
        assert!(!is_valid_image(b"not an image at all", "text/plain"));
    }

    #[test]
    fn test_is_valid_image_auto_detect_without_content_type() {
        let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        assert!(is_valid_image(&jpeg, ""));
        let png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(is_valid_image(&png, ""));
        assert!(is_valid_image(b"GIF89a...", ""));
    }

    #[test]
    fn test_url_has_query_params() {
        let url = "https://example.com/image.jpg?w=1920&q=75";
        assert_eq!(get_file_extension("image/jpeg", url), "jpg");
    }

    #[test]
    fn test_content_type_case_insensitive() {
        assert_eq!(get_file_extension("IMAGE/JPEG", "http://example.com/img"), "jpg");
        assert_eq!(get_file_extension("Image/Png", "http://example.com/img"), "png");
    }

    #[test]
    fn test_compute_md5() {
        let data = b"hello world";
        let hash = compute_md5(data);
        assert_eq!(hash.len(), 32);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_compute_md5_empty() {
        let hash = compute_md5(b"");
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_compute_md5_different_inputs_different_hashes() {
        let h1 = compute_md5(b"image1");
        let h2 = compute_md5(b"image2");
        assert_ne!(h1, h2);
    }
}

pub async fn download_image_bytes(
    client: &reqwest::Client,
    url: &str,
) -> Result<(Vec<u8>, String), String> {
    log::info!("[downloader] download_image_bytes: url={}", url);
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("下载请求失败: {e}"))?;

    if !resp.status().is_success() {
        log::warn!("[downloader] bad status: {} for {}", resp.status(), url);
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
        log::warn!("[downloader] invalid image data: url={} type={}", url, content_type);
        return Err("无效的图片数据".to_string());
    }

    log::info!("[downloader] download ok: {} bytes type={}", bytes.len(), content_type);
    Ok((bytes.to_vec(), content_type))
}

pub async fn download_urls_concurrent(
    client: &reqwest::Client,
    urls: &[String],
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    max_retries: u32,
) -> Vec<Result<(Vec<u8>, String), String>> {
    let count = urls.len();
    if count == 0 {
        return Vec::new();
    }

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(6));
    let mut handles = Vec::with_capacity(count);

    for (idx, url) in urls.iter().enumerate() {
        let semaphore = semaphore.clone();
        let client = client.clone();
        let url = url.clone();
        let cancel = cancel.clone();
        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire().await;
            let mut last_err = String::new();
            for attempt in 0..=max_retries {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return (idx, Err("下载已取消".to_string()));
                }
                match download_image_bytes(&client, &url).await {
                    Ok(res) => return (idx, Ok(res)),
                    Err(e) => {
                        last_err = e;
                        if attempt < max_retries {
                            tokio::time::sleep(std::time::Duration::from_secs(2u64.pow(attempt))).await;
                        }
                    }
                }
            }
            (idx, Err(format!("下载失败（已重试 {max_retries} 次）: {last_err}")))
        });
        handles.push(handle);
    }

    let mut results = Vec::with_capacity(count);
    for handle in handles {
        let (idx, result) = match handle.await {
            Ok(r) => r,
            Err(_) => (0, Err("下载任务异常".to_string())),
        };
        results.push((idx, result));
    }
    results.sort_by_key(|(idx, _)| *idx);
    results.into_iter().map(|(_, r)| r).collect()
}
