use std::path::{Path, PathBuf};

const THUMB_MAX_WIDTH: u32 = 320;

/// 获取缩略图保存目录（在原图目录下创建 thumb_cache）
pub fn thumb_dir(image_dir: &Path) -> PathBuf {
    image_dir.join("thumb_cache")
}

/// 获取缩略图的完整路径
pub fn thumb_path(image_dir: &Path, filename: &str) -> PathBuf {
    thumb_dir(image_dir).join(filename)
}

/// 检查缩略图是否存在
pub fn thumb_exists(image_dir: &Path, filename: &str) -> bool {
    thumb_path(image_dir, filename).exists()
}

/// 为一张图片生成缩略图，如果已经存在则跳过
pub fn ensure_thumbnail(image_dir: &Path, filename: &str) -> Result<PathBuf, String> {
    let src = image_dir.join(filename);
    let dst_dir = thumb_dir(image_dir);
    let dst = dst_dir.join(filename);

    if dst.exists() {
        return Ok(dst);
    }

    // 创建缩略图目录
    std::fs::create_dir_all(&dst_dir).map_err(|e| format!("创建缩略图目录失败: {e}"))?;

    // 打开原图
    let img = image::ImageReader::open(&src)
        .map_err(|e| format!("打开图片失败 {}: {e}", filename))?
        .decode()
        .map_err(|e| format!("解码图片失败 {}: {e}", filename))?;

    // 计算等比缩放尺寸
    let (w, h) = (img.width(), img.height());
    let thumb = if w > THUMB_MAX_WIDTH {
        let new_w = THUMB_MAX_WIDTH;
        let new_h = (h as f64 * THUMB_MAX_WIDTH as f64 / w as f64) as u32;
        img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3)
    } else {
        // 原图已经很小，直接保存原图
        img
    };

    // 保存缩略图（根据扩展名选择格式）
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("jpg")
        .to_lowercase();

    match ext.as_str() {
        "png" => thumb
            .save(&dst)
            .map_err(|e| format!("保存 PNG 缩略图失败: {e}")),
        "webp" => thumb
            .save(&dst)
            .map_err(|e| format!("保存 WebP 缩略图失败: {e}")),
        "gif" => thumb
            .save(&dst)
            .map_err(|e| format!("保存 GIF 缩略图失败: {e}")),
        _ => {
            // JPEG 默认
            let mut file = std::fs::File::create(&dst)
                .map_err(|e| format!("创建缩略图文件失败: {e}"))?;
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 85);
            encoder
                .encode(
                    thumb.as_bytes(),
                    thumb.width(),
                    thumb.height(),
                    thumb.color().into(),
                )
                .map_err(|e| format!("编码 JPEG 缩略图失败: {e}"))?;
            Ok(())
        }
    }?;

    Ok(dst)
}

/// 批量生成缩略图（已存在的跳过）
#[allow(dead_code)]
pub fn ensure_batch_thumbnails(
    image_dir: &Path,
    filenames: &[String],
) -> Vec<(String, PathBuf)> {
    filenames
        .iter()
        .map(|name| {
            let path = match ensure_thumbnail(image_dir, name) {
                Ok(p) => p,
                Err(_) => image_dir.join(name), // 缩略图生成失败时，回退到原图
            };
            (name.clone(), path)
        })
        .collect()
}
