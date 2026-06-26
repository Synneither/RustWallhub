use std::path::{Path, PathBuf};
use rayon::prelude::*;

/// 基础缩略图宽度（按 1x DPR）。前端传入 devicePixelRatio 后按比例放大。
const THUMB_BASE_WIDTH: u32 = 240;

/// 自定义线程池，限制并行缩略图生成的核数，避免 CPU 满载
fn thumbnail_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let threads = (cpus / 2).clamp(2, 6);
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("thumb-{i}"))
            .build()
            .expect("创建缩略图线程池失败")
    })
}

/// 生成带 DPR 信息的缩略图文件名，例如 `photo__w480.webp`
pub fn thumb_filename_for_dpr(filename: &str, dpr: u32) -> String {
    thumb_filename(filename, dpr)
}

fn thumb_filename(filename: &str, dpr: u32) -> String {
    let dpr = dpr.max(1);
    let width = THUMB_BASE_WIDTH * dpr;
    if let Some(dot) = filename.rfind('.') {
        format!("{}__w{}.webp", &filename[..dot], width)
    } else {
        format!("{filename}__w{width}.webp")
    }
}

fn thumb_max_width(dpr: u32) -> u32 {
    THUMB_BASE_WIDTH * dpr.max(1)
}

pub fn thumb_path(thumb_dir: &Path, filename: &str, dpr: u32) -> PathBuf {
    thumb_dir.join(thumb_filename(filename, dpr))
}

/// 获取缩略图路径：新路径 → 旧 240px 兼容 → 生成
pub fn resolve_thumb_path(
    thumb_dir: &Path,
    source_dir: &Path,
    filename: &str,
    dpr: u32,
) -> Result<PathBuf, String> {
    let new_path = thumb_path(thumb_dir, filename, dpr);
    if new_path.exists() {
        return Ok(new_path);
    }
    let old_path = thumb_dir.join(filename);
    if old_path.exists() {
        return Ok(old_path);
    }
    ensure_thumbnail(thumb_dir, source_dir, filename, dpr)
}

fn resize_and_save(
    img: image::DynamicImage,
    dst: &Path,
    _filename: &str,
    max_width: u32,
) -> Result<(), String> {
    let (w, h) = (img.width(), img.height());
    let thumb = if w > max_width {
        let new_w = max_width;
        let new_h = (h as f64 * max_width as f64 / w as f64) as u32;
        // thumbnail_exact 是 image 库针对缩略图优化的缩放路径
        img.thumbnail_exact(new_w, new_h)
    } else {
        img
    };

    thumb
        .save(dst)
        .map_err(|e| format!("save thumbnail ({}) failed: {e}", dst.display()))
}

pub fn ensure_thumbnail(
    thumb_dir: &Path,
    source_dir: &Path,
    filename: &str,
    dpr: u32,
) -> Result<PathBuf, String> {
    let dst = thumb_path(thumb_dir, filename, dpr);
    if dst.exists() {
        return Ok(dst);
    }

    log::info!(
        "[thumbnail] {} -> {}",
        filename,
        dst.file_name().unwrap_or_default().to_string_lossy()
    );
    let src = source_dir.join(filename);
    std::fs::create_dir_all(thumb_dir)
        .map_err(|e| format!("create thumb dir failed: {e}"))?;

    let max_w = thumb_max_width(dpr);
    let img = image::ImageReader::open(&src)
        .map_err(|e| format!("open image failed {}: {e}", filename))?
        .decode()
        .map_err(|e| format!("decode image failed {}: {e}", filename))?;

    resize_and_save(img, &dst, filename, max_w)?;
    Ok(dst)
}

pub fn save_thumbnail_from_bytes(
    thumb_dir: &Path,
    filename: &str,
    bytes: &[u8],
    dpr: u32,
) -> Result<PathBuf, String> {
    let dst = thumb_path(thumb_dir, filename, dpr);
    if dst.exists() {
        return Ok(dst);
    }

    log::info!(
        "[thumbnail] save_thumbnail: {} -> {}",
        filename,
        dst.file_name().unwrap_or_default().to_string_lossy()
    );
    std::fs::create_dir_all(thumb_dir)
        .map_err(|e| format!("create thumb dir failed: {e}"))?;

    let max_w = thumb_max_width(dpr);
    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("decode image from memory failed {}: {e}", filename))?;

    resize_and_save(img, &dst, filename, max_w)?;
    Ok(dst)
}

/// 并行生成批量缩略图（限制核数避免 CPU 满载）
pub fn ensure_batch_thumbnails(
    thumb_dir: &Path,
    source_dir: &Path,
    filenames: &[String],
    dpr: u32,
) -> Vec<(String, PathBuf)> {
    thumbnail_pool().install(|| {
        filenames
            .par_iter()
            .map(|name| {
                let path = match ensure_thumbnail(thumb_dir, source_dir, name, dpr) {
                    Ok(p) => p,
                    Err(_) => source_dir.join(name),
                };
                (name.clone(), path)
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thumb_filename() {
        let name = thumb_filename("photo.jpg", 1);
        assert_eq!(name, "photo__w240.webp");
        let name = thumb_filename("photo.jpg", 2);
        assert_eq!(name, "photo__w480.webp");
    }

    #[test]
    fn test_thumb_path() {
        let dir = Path::new("/tmp/images");
        let tp = thumb_path(dir, "photo.jpg", 1);
        assert_eq!(tp, dir.join("photo__w240.webp"));
    }

    #[test]
    fn test_thumb_path_dpr2() {
        let dir = Path::new("/tmp/images");
        let tp = thumb_path(dir, "photo.jpg", 2);
        assert_eq!(tp, dir.join("photo__w480.webp"));
    }

    #[test]
    fn test_save_thumbnail_from_bytes_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut buf);
        let img = image::DynamicImage::new_rgb8(800, 600);
        encoder.encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8).unwrap();
        let bytes = buf.into_inner();

        let result = save_thumbnail_from_bytes(dir.path(), "test.jpg", &bytes, 1);
        assert!(result.is_ok());
        let thumb = result.unwrap();
        assert!(thumb.exists());
        let loaded = image::ImageReader::open(&thumb).unwrap().decode().unwrap();
        assert!(loaded.width() <= 240);
    }

    #[test]
    fn test_save_thumbnail_dpr2() {
        let dir = tempfile::tempdir().unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut buf);
        let img = image::DynamicImage::new_rgb8(800, 600);
        encoder.encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8).unwrap();
        let bytes = buf.into_inner();

        let result = save_thumbnail_from_bytes(dir.path(), "test.jpg", &bytes, 2);
        assert!(result.is_ok());
        let thumb = result.unwrap();
        let loaded = image::ImageReader::open(&thumb).unwrap().decode().unwrap();
        assert!(loaded.width() <= 480);
    }

    #[test]
    fn test_save_thumbnail_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut buf);
        let img = image::DynamicImage::new_rgb8(800, 600);
        encoder.encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8).unwrap();
        let bytes = buf.into_inner();

        let r1 = save_thumbnail_from_bytes(dir.path(), "same.jpg", &bytes, 2);
        let r2 = save_thumbnail_from_bytes(dir.path(), "same.jpg", &bytes, 2);
        assert!(r1.is_ok());
        assert!(r2.is_ok());
        assert_eq!(r1.unwrap(), r2.unwrap());
    }

    #[test]
    fn test_ensure_thumbnail_generates_on_disk() {
        let thumb_dir = tempfile::tempdir().unwrap();
        let src_dir = tempfile::tempdir().unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut buf);
        let img = image::DynamicImage::new_rgb8(800, 600);
        encoder.encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8).unwrap();
        let bytes = buf.into_inner();
        let src_path = src_dir.path().join("source.jpg");
        std::fs::write(&src_path, &bytes).unwrap();

        let result = ensure_thumbnail(thumb_dir.path(), src_dir.path(), "source.jpg", 1);
        assert!(result.is_ok());
        assert!(result.unwrap().exists());
    }

    #[test]
    fn test_resolve_thumb_path_new() {
        let thumb_dir = tempfile::tempdir().unwrap();
        let src_dir = tempfile::tempdir().unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut buf);
        let img = image::DynamicImage::new_rgb8(800, 600);
        encoder.encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8).unwrap();
        let bytes = buf.into_inner();
        let src_path = src_dir.path().join("img.jpg");
        std::fs::write(&src_path, &bytes).unwrap();

        let result = resolve_thumb_path(thumb_dir.path(), src_dir.path(), "img.jpg", 2);
        assert!(result.is_ok());
        assert!(result.unwrap().to_string_lossy().contains("__w480"));
    }
}
