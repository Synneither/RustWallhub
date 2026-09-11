use std::path::Path;

pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

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

    // Fallback: extract from URL path
    let lower_url = url.to_lowercase();
    let path = lower_url.split('?').next().unwrap_or(url);
    if let Some(ext) = path.rsplit('.').next() {
        if IMAGE_EXTENSIONS.contains(&ext) {
            return ext.to_string();
        }
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
    // 用 eq_ignore_ascii_case 逐一比对，避免每次调用都 `to_lowercase()` 分配一个 String
    // ——图库目录扫描和缩略图清理都会逐文件调它，放大后很可观。
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| IMAGE_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// 单张图片下载的硬上限，防止异常源或错误 Content-Type 把内存撑爆。
///
/// 这个值直接决定下载阶段的内存上界，改之前先算一遍账：
/// `download_urls_concurrent` 会把**整批**的图片字节全部收进内存后才返回
/// （调用方随后才逐张落盘），所以峰值驻留 ≈ `chunk_size * MAX_IMAGE_BYTES`，
/// 而 `chunk_size` 就是并发数（见 `download_chunk_size`）。默认并发 6 → 约 6 张。
///
/// 64 MiB 的依据：8K JPEG 约 10–20 MiB，8K 无损 PNG 约 40–60 MiB，
/// 已经覆盖真实壁纸场景；配合上面的账，最坏峰值约 384 MiB。
/// 调太小会把合法的大图误判为超限，不要靠它压内存。
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

/// 一批下载多少张——**必须恰好等于并发数，不要放大**。
///
/// 原因：`download_urls_concurrent` 会 `await` 到整批全部完成才返回，调用方拿到结果后
/// 才逐张落盘。也就是说**批内所有图片的字节在落盘前会同时驻留内存**，
/// 峰值 ≈ `chunk_size × 单张大小`。
///
/// 批大小取并发数时，同时在飞的最多就是这么多张，已经达到下界；
/// 原来写成 `concurrency * 2` 并不能让下载与落盘流水线化（两者是串行的），
/// 只是白白把峰值翻倍。
pub fn download_chunk_size(concurrency: u32) -> usize {
    concurrency.max(1) as usize
}

pub fn compute_md5(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

/// 流式计算文件 MD5，边读边喂给哈希器。
///
/// [`compute_md5`] 需要先把整张图读进内存，4K 壁纸单张可达 50MB，批量收养孤儿文件时
/// 内存峰值会很明显。这里按 64KB 分块读取，峰值恒定。
pub fn compute_md5_file(path: &Path) -> Result<String, std::io::Error> {
    use std::io::Read;

    let mut ctx = md5::Context::new();
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.consume(&buf[..n]);
    }
    Ok(format!("{:x}", ctx.compute()))
}
pub async fn download_image_bytes(
    client: &reqwest::Client,
    url: &str,
) -> Result<(Vec<u8>, String), String> {
    log::info!("[downloader] download_image_bytes: url={}", url);
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载请求失败: {e}"))?;

    if !resp.status().is_success() {
        log::warn!("[downloader] bad status: {} for {}", resp.status(), url);
        return Err(format!("下载返回状态码: {}", resp.status()));
    }

    if resp
        .content_length()
        .is_some_and(|len| len as usize > MAX_IMAGE_BYTES)
    {
        return Err(format!(
            "图片超过大小限制（上限 {} MiB）",
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // 流式读取并在未知 Content-Length 时也强制限制最大体积。
    let mut bytes = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("读取下载数据失败: {e}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_IMAGE_BYTES {
            return Err(format!(
                "图片超过大小限制（上限 {} MiB）",
                MAX_IMAGE_BYTES / (1024 * 1024)
            ));
        }
        bytes.extend_from_slice(&chunk);
    }

    if !is_valid_image(&bytes, &content_type) {
        log::warn!(
            "[downloader] invalid image data: url={} type={}",
            url,
            content_type
        );
        return Err("无效的图片数据".to_string());
    }

    log::info!(
        "[downloader] download ok: {} bytes type={}",
        bytes.len(),
        content_type
    );
    Ok((bytes, content_type))
}

pub async fn download_urls_concurrent(
    client: &reqwest::Client,
    urls: &[String],
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    concurrency: u32,
    max_retries: u32,
) -> Vec<Result<(Vec<u8>, String), String>> {
    let count = urls.len();
    if count == 0 {
        return Vec::new();
    }

    let limit = concurrency.max(1) as usize;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(limit));
    let mut handles = Vec::with_capacity(count);

    for (idx, url) in urls.iter().enumerate() {
        let semaphore = semaphore.clone();
        let client = client.clone();
        let url = url.clone();
        let cancel = cancel.clone();
        let handle = tokio::spawn(async move {
            let mut last_err = String::new();
            for attempt in 0..=max_retries {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return (idx, Err("下载已取消".to_string()));
                }
                // 每次尝试前才获取许可，退避等待期间把槽位让给别人。
                // 若在循环外长期持有，大量失败时会占着槽位睡 1+2+4 秒，并发 6 时吞吐直接塌方。
                let Ok(permit) = semaphore.acquire().await else {
                    return (idx, Err("下载任务已终止".to_string()));
                };
                match download_image_bytes(&client, &url).await {
                    Ok(res) => return (idx, Ok(res)),
                    Err(e) => {
                        last_err = e;
                        drop(permit); // 退避前先归还许可，别占着槽位睡觉
                        if attempt < max_retries {
                            tokio::time::sleep(std::time::Duration::from_secs(2u64.pow(attempt)))
                                .await;
                        }
                    }
                }
            }
            (
                idx,
                Err(format!("下载失败（已重试 {max_retries} 次）: {last_err}")),
            )
        });
        handles.push(handle);
    }

    let mut results = Vec::with_capacity(count);
    for (original_idx, handle) in handles.into_iter().enumerate() {
        let (idx, result) = match handle.await {
            Ok(r) => r,
            Err(e) => {
                log::error!(
                    "[downloader] task panicked at index {}: {}",
                    original_idx,
                    e
                );
                (original_idx, Err("下载任务异常".to_string()))
            }
        };
        results.push((idx, result));
    }
    // handles 是按原始顺序 await 的，且每个 task 返回自身 idx，因此 results 已经有序。
    results.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_chunk_size_equals_concurrency() {
        // 批大小必须恰好等于并发数：`download_urls_concurrent` 会等整批全部完成才返回，
        // 批内所有字节在落盘前同时驻留内存。放大批大小只会把峰值翻倍，没有流水线收益。
        assert_eq!(download_chunk_size(6), 6);
        assert_eq!(download_chunk_size(1), 1);
        assert_eq!(download_chunk_size(12), 12);
        // 并发数为 0 时必须退化成 1：`chunks(0)` 会 panic。
        assert_eq!(download_chunk_size(0), 1);
    }

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
        assert_eq!(
            get_file_extension("image/jpeg", "http://example.com/img.jpg"),
            "jpg"
        );
        assert_eq!(
            get_file_extension("image/png", "http://example.com/img.png"),
            "png"
        );
        assert_eq!(
            get_file_extension("image/gif", "http://example.com/img.gif"),
            "gif"
        );
        assert_eq!(
            get_file_extension("image/webp", "http://example.com/img.webp"),
            "webp"
        );
    }

    #[test]
    fn test_get_file_extension_fallback_to_url() {
        assert_eq!(
            get_file_extension("application/octet-stream", "http://example.com/img.png"),
            "png"
        );
        assert_eq!(
            get_file_extension("application/octet-stream", "http://example.com/img.gif"),
            "gif"
        );
        assert_eq!(
            get_file_extension("application/octet-stream", "http://example.com/img.webp"),
            "webp"
        );
        assert_eq!(
            get_file_extension("application/octet-stream", "http://example.com/img.jpg"),
            "jpg"
        );
    }

    #[test]
    fn test_get_file_extension_unknown_fallback_jpg() {
        assert_eq!(
            get_file_extension("application/octet-stream", "http://example.com/img"),
            "jpg"
        );
        assert_eq!(
            get_file_extension("image/something-weird", "http://example.com/data"),
            "jpg"
        );
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
        assert_eq!(
            get_file_extension("IMAGE/JPEG", "http://example.com/img"),
            "jpg"
        );
        assert_eq!(
            get_file_extension("Image/Png", "http://example.com/img"),
            "png"
        );
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

    #[tokio::test]
    async fn test_download_urls_concurrent_empty() {
        let client = reqwest::Client::new();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let results = download_urls_concurrent(&client, &[], cancel, 6, 3).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_download_urls_concurrent_cancel() {
        let client = reqwest::Client::new();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let urls = vec![
            "https://example.com/a.jpg".to_string(),
            "https://example.com/b.jpg".to_string(),
        ];
        let results = download_urls_concurrent(&client, &urls, cancel, 6, 0).await;
        assert_eq!(results.len(), 2);
        for result in &results {
            let err = result.as_ref().unwrap_err();
            assert!(err.contains("取消"), "expected cancel message, got: {err}");
        }
    }

    #[tokio::test]
    async fn test_download_urls_concurrent_invalid_url() {
        let client = reqwest::Client::new();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let urls = vec!["not-a-valid-url".to_string()];
        let results = download_urls_concurrent(&client, &urls, cancel, 6, 0).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[tokio::test]
    async fn test_download_urls_concurrent_success() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let jpeg_data: &[u8] = &[
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06,
            0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D,
            0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D,
            0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28,
            0x37, 0x29, 0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32,
            0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01,
            0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
            0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x10,
            0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00,
            0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06,
            0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42,
            0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16,
            0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
            0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55,
            0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73,
            0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
            0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
            0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA,
            0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6,
            0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
            0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x08,
            0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0xD2, 0xCF, 0x20, 0xFF, 0xD9,
        ];
        let body = jpeg_data.to_vec();

        let jpeg_data = body.clone();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let b = barrier.clone();
        // 用**阻塞** accept，而不是"非阻塞轮询 + sleep"：
        // 轮询要求 accept 恰好在连接到达的窗口内被调用，高负载（并行跑 96 个测试）时
        // 调度抖动会让它反复错过，客户端每次重试都连不上 → 偶发失败。
        // 阻塞 accept 由内核挂起等待，连接到达必被唤醒，没有这个不确定性。
        listener.set_nonblocking(false).unwrap();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            b.wait();
            if let Ok((mut stream, _)) = listener.accept() {
                // 先读取客户端请求，避免"服务器先关闭连接、客户端随后写入被 RST(10054)"的时序竞争
                let mut req_buf = [0u8; 4096];
                let _ = stream.read(&mut req_buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    jpeg_data.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&jpeg_data);
                // 服务完这一个请求即可；客户端成功后不会再连。
            }
        });
        barrier.wait();
        // 给子线程一点时间进入 accept 轮询，减少 CI 上首次连接失败的概率。
        std::thread::sleep(std::time::Duration::from_millis(100));

        // no_proxy：本测试访问本地 mock server，系统代理开启时必须绕过，否则请求被代理转发而失败
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let url = format!("http://127.0.0.1:{}/test.jpg", port);
        let results = download_urls_concurrent(&client, &[url], cancel, 1, 3).await;

        assert_eq!(results.len(), 1);
        let (bytes, content_type) = results[0].as_ref().expect("download should succeed");
        assert!(!bytes.is_empty(), "should get image bytes");
        assert_eq!(content_type, "image/jpeg");
    }

    /// 重试/退避路径此前完全没有覆盖——现有用例要么传 `max_retries=0`，要么首轮就成功，
    /// 所以 `2u64.pow(attempt)` 的退避分支与「退避前归还信号量」从来没被执行过。
    ///
    /// 让 mock server 第一次返回 500、第二次返回 200，验证退避后确实重试且最终成功，
    /// 并断言请求次数正好是 2（既没漏重试，也没多试）。
    #[tokio::test]
    async fn test_download_retries_after_failure_then_succeeds() {
        // JPEG magic bytes 就够：is_valid_image 对 image/jpeg 只校验文件头。
        const BODY: [u8; 6] = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits_srv = hits.clone();

        // 在主线程就切非阻塞：放进子线程的话，bind 之后、切换之前的窗口里
        // 阻塞 accept（理由同 test_download_urls_concurrent_success）：
        // 非阻塞轮询要求 accept 恰好在连接到达的窗口内被调用，高负载下会反复错过。
        listener.set_nonblocking(false).unwrap();

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let b = barrier.clone();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            b.wait();
            // 服务 2 个连接后自然退出：第 1 个返回 500 让客户端退避重试，第 2 个返回 200。
            // 用固定次数而不是 deadline 循环，线程能干净退出，不会空转抢 CPU。
            while let Ok((mut stream, _)) = listener.accept() {
                let mut req_buf = [0u8; 4096];
                let _ = stream.read(&mut req_buf);
                let n = hits_srv.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n == 0 {
                    let _ = stream.write_all(
                        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                }
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    BODY.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&BODY);
                break;
            }
        });
        barrier.wait();
        std::thread::sleep(std::time::Duration::from_millis(100));

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let url = format!("http://127.0.0.1:{port}/retry.jpg");
        // 给足重试预算（4 次尝试）而不是刚好 1 次：mock server 只在它「第一个成功 accept 的
        // 连接」上返回 500，之后返回 200，所以无论前几次连接是否因机器负载抖动而失败，
        // hits 最终都恰好是 2。预算给紧会让偶发的 TCP 抖动把唯一的重试机会吃掉 → 测试变 flaky。
        let results = download_urls_concurrent(&client, &[url], cancel, 1, 3).await;

        assert_eq!(results.len(), 1);
        assert!(
            results[0].is_ok(),
            "退避重试后应当成功: {:?}",
            results[0].as_ref().err()
        );
        assert_eq!(
            hits.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "应当正好有 2 个连接被处理（1 次 500 + 1 次 200），且成功后不再重试"
        );
    }

    /// 一直失败时要耗尽重试次数并返回带次数说明的错误，且不 panic。
    /// 用 max_retries=0 跑，避免退避 sleep 拖慢测试。
    #[tokio::test]
    async fn test_download_exhausts_retries_reports_count() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(false).unwrap();

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let b = barrier.clone();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            b.wait();
            // 阻塞 accept，服务 1 个连接后退出：max_retries=0 意味着客户端只请求一次。
            // 固定次数能让线程干净退出，不会像之前那样一直轮询空转抢 CPU。
            if let Ok((mut stream, _)) = listener.accept() {
                let mut req_buf = [0u8; 4096];
                let _ = stream.read(&mut req_buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        barrier.wait();
        std::thread::sleep(std::time::Duration::from_millis(100));

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let url = format!("http://127.0.0.1:{port}/always-503.jpg");
        let results = download_urls_concurrent(&client, &[url], cancel, 1, 0).await;

        assert_eq!(results.len(), 1);
        // 注意：即使连接层失败（而非拿到 503），错误信息同样带"已重试 0 次"，
        // 因为 max_retries=0 时第一次失败就直接出循环。所以这个断言不依赖 mock server 是否可达。
        let err = results[0].as_ref().expect_err("503 应当失败");
        assert!(
            err.contains("已重试 0 次"),
            "错误信息应包含重试次数，实际: {err}"
        );
    }
}
