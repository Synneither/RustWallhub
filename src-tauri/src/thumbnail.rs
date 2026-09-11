use rayon::prelude::*;
use std::path::{Path, PathBuf};

/// 基础缩略图宽度（按 1x DPR）。前端传入 devicePixelRatio 后按比例放大。
const THUMB_BASE_WIDTH: u32 = 240;

/// 允许的最大 DPR 档位（与前端 thumbnail_dpr 的 1-3 约束一致）。
const MAX_DPR: u32 = 3;

/// 单张缩略图解码时的内存预算估算值。
///
/// 基准（`bench_thumb_pipeline`）实测一张 4K JPEG 解码后驻留约 31 MiB，
/// 8K 会到 ~130 MiB。这里取 48 MiB 作为「一张图在解码峰值下大约占多少」
/// 的保守估计，用来推导线程数上限。
const APPROX_DECODE_MIB: usize = 48;

/// 缩略图解码允许占用的内存预算。超过它就不再加线程——
/// 解码是纯 CPU 密集且每张图要驻留一整张位图，线程数直接等于峰值内存倍数。
const DECODE_MEMORY_BUDGET_MIB: usize = 480;

/// 自定义线程池，限制并行缩略图生成的核数，避免 CPU 与内存双双吃满。
///
/// 线程数由两个约束取小：留 2 个核给 UI/数据库，以及内存预算
/// （`DECODE_MEMORY_BUDGET_MIB / APPROX_DECODE_MIB`）。
/// 基准显示解码占缩略图全链路约 80% 的时间，所以这一项直接决定整页缩略图的
/// 墙钟耗时：16 核机器上从固定 6 提到 10，一页 48 张大约快 40%。
fn thumbnail_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let cpus = std::thread::available_parallelism().map_or(4, |n| n.get());
        let by_cpu = cpus.saturating_sub(2).max(1);
        let by_memory = (DECODE_MEMORY_BUDGET_MIB / APPROX_DECODE_MIB).max(1);
        let threads = by_cpu.min(by_memory).clamp(2, 12);
        log::info!(
            "[thumbnail] 解码线程池: {threads} 线程 (核数 {cpus}, 内存预算 {DECODE_MEMORY_BUDGET_MIB} MiB)"
        );
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

/// JPEG 快速路径：在 DCT 域直接降采样解码到接近目标尺寸。
///
/// `image` 的 JPEG 后端是 `zune-jpeg`，**不支持降采样解码**，所以原来生成 240px 缩略图
/// 也要把整张 4K 图完整解码（约 31 MiB 驻留、占全链路 80% 的时间，见 `bench_thumb_pipeline`）。
///
/// `jpeg-decoder` 支持 1/8、1/4、1/2 的 IDCT 缩放：解码量按像素数成比例下降
/// （1/8 即 1/64），之后再交给 `image::thumbnail` 精修到精确宽度。
///
/// 失败（非 JPEG、CMYK、尺寸对不上等）一律返回 `None`，由调用方回退到常规解码。
fn decode_jpeg_scaled(src: &Path, target_width: u32) -> Option<image::DynamicImage> {
    use jpeg_decoder::{Decoder, PixelFormat};

    let ext = src
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    if ext != "jpg" && ext != "jpeg" {
        return None;
    }

    let file = std::fs::File::open(src).ok()?;
    let mut decoder = Decoder::new(std::io::BufReader::new(file));

    // `scale()` 的语义是「选最小的、能使**至少一个轴** >= 请求尺寸的缩放因子」。
    // 所以两个轴都得按目标宽高比给足，不能只约束宽度——否则高度恒为满足条件，
    // 会一路选到最小的 1/8，结果比目标小、再放大就糊了。
    decoder.read_info().ok()?;
    let (orig_w, orig_h) = {
        let info = decoder.info()?;
        (info.width as u32, info.height as u32)
    };
    if orig_w == 0 || orig_h == 0 {
        return None;
    }
    // 目标高度按原图宽高比折算，向上取整保证不低于目标宽度。
    let want_w = target_width.min(u16::MAX as u32);
    let want_h = (orig_h as u64 * want_w as u64)
        .div_ceil(orig_w as u64)
        .min(u16::MAX as u64);
    let (out_w, out_h) = decoder.scale(want_w as u16, want_h as u16).ok()?;

    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;

    let (w, h) = (out_w as u32, out_h as u32);
    let expected = match info.pixel_format {
        PixelFormat::L8 => w as usize * h as usize,
        PixelFormat::RGB24 => w as usize * h as usize * 3,
        // CMYK 需要额外色彩转换、L16（16 位灰度）极少见：都不走快路径，回退常规解码。
        PixelFormat::CMYK32 | PixelFormat::L16 => return None,
    };
    // 尺寸对不上说明对输出语义的理解有误：宁可回退也不能用错误的数据。
    if pixels.len() != expected || w == 0 || h == 0 {
        return None;
    }

    match info.pixel_format {
        PixelFormat::L8 => {
            image::GrayImage::from_raw(w, h, pixels).map(image::DynamicImage::ImageLuma8)
        }
        PixelFormat::RGB24 => {
            image::RgbImage::from_raw(w, h, pixels).map(image::DynamicImage::ImageRgb8)
        }
        PixelFormat::CMYK32 | PixelFormat::L16 => None,
    }
}

fn resize_and_save(img: image::DynamicImage, dst: &Path, max_width: u32) -> Result<(), String> {
    let (w, _h) = (img.width(), img.height());
    let thumb = if w > max_width {
        // thumbnail 保持纵横比，避免缩略图变形
        img.thumbnail(max_width, u32::MAX)
    } else {
        img
    };

    thumb
        .save(dst)
        .map_err(|e| format!("save thumbnail ({}) failed: {e}", dst.display()))
}

/// 生成单张缩略图，**假定 `thumb_dir` 已存在**。
///
/// 批量场景下由 `ensure_batch_thumbnails` 统一建目录：原来每个文件都调一次
/// `create_dir_all`，一页 48 张就是 48 次冗余 syscall。
fn ensure_thumbnail_inner(
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

    let max_w = thumb_max_width(dpr);
    // JPEG 走 DCT 域降采样的快路径；其余格式（或快路径失败）回退到常规解码。
    let img = match decode_jpeg_scaled(&src, max_w) {
        Some(img) => img,
        None => image::ImageReader::open(&src)
            .map_err(|e| format!("open image failed {}: {e}", filename))?
            .decode()
            .map_err(|e| format!("decode image failed {}: {e}", filename))?,
    };

    resize_and_save(img, &dst, max_w)?;
    Ok(dst)
}

#[cfg(test)]
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
    std::fs::create_dir_all(thumb_dir).map_err(|e| format!("create thumb dir failed: {e}"))?;

    let max_w = thumb_max_width(dpr);
    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("decode image from memory failed {}: {e}", filename))?;

    resize_and_save(img, &dst, max_w)?;
    Ok(dst)
}

/// 删除某文件的所有缩略图（兼容新旧格式 + 多 DPR）
pub fn remove_thumbnails(thumb_dir: &Path, filename: &str) {
    let _ = std::fs::remove_file(thumb_dir.join(filename));
    // thumbnail_dpr 允许 1-3，这里覆盖全部档位；用常量替代魔法数字 [1,2,3]。
    for dpr in 1..=MAX_DPR {
        let _ = std::fs::remove_file(thumb_dir.join(thumb_filename_for_dpr(filename, dpr)));
    }
}

/// 并行生成批量缩略图（线程数见 `thumbnail_pool`）。
pub fn ensure_batch_thumbnails(
    thumb_dir: &Path,
    source_dir: &Path,
    filenames: &[String],
    dpr: u32,
) -> Vec<(String, PathBuf)> {
    // 整个批次只建一次目录，而不是每张图一次。
    if filenames.is_empty() {
        return Vec::new();
    }
    if let Err(e) = std::fs::create_dir_all(thumb_dir) {
        log::warn!(
            "[thumbnail] 创建缩略图目录失败 {}: {e}",
            thumb_dir.display()
        );
    }

    thumbnail_pool().install(|| {
        filenames
            .par_iter()
            .map(|name| {
                let path = match ensure_thumbnail_inner(thumb_dir, source_dir, name, dpr) {
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
        encoder
            .encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8)
            .unwrap();
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
        encoder
            .encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8)
            .unwrap();
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
        encoder
            .encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8)
            .unwrap();
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
        encoder
            .encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8)
            .unwrap();
        let bytes = buf.into_inner();
        let src_path = src_dir.path().join("source.jpg");
        std::fs::write(&src_path, &bytes).unwrap();

        // 走批量入口，也就是生产上真正被 resolve_thumbnails 调用的那条路径
        let result = ensure_batch_thumbnails(
            thumb_dir.path(),
            src_dir.path(),
            &["source.jpg".to_string()],
            1,
        );
        assert_eq!(result.len(), 1);
        assert!(result[0].1.exists());
    }

    #[test]
    fn test_resolve_thumb_path_new() {
        let thumb_dir = tempfile::tempdir().unwrap();
        let src_dir = tempfile::tempdir().unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut buf);
        let img = image::DynamicImage::new_rgb8(800, 600);
        encoder
            .encode(img.as_bytes(), 800, 600, image::ExtendedColorType::Rgb8)
            .unwrap();
        let bytes = buf.into_inner();
        let src_path = src_dir.path().join("img.jpg");
        std::fs::write(&src_path, &bytes).unwrap();

        let result = ensure_batch_thumbnails(
            thumb_dir.path(),
            src_dir.path(),
            &["img.jpg".to_string()],
            2,
        );
        assert_eq!(result.len(), 1);
        assert!(result[0].1.to_string_lossy().contains("__w480"));
    }

    /// 快路径的关键正确性：降采样后的宽度**不能小于目标宽度**。
    ///
    /// `scale()` 选的是「能使至少一个轴 >= 请求尺寸的最小缩放因子」，
    /// 如果只约束宽度、高度传 1，高度恒满足条件，就会一路选到最小的 1/8，
    /// 输出的图比目标小、再被 `thumbnail()` 放大就糊了（1080p 曾因此输出 240 而非 480）。
    #[test]
    fn test_jpeg_scaled_decode_never_undershoots_target() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.jpg");
        let mut rgb = image::RgbImage::new(1920, 1080);
        for (x, y, px) in rgb.enumerate_pixels_mut() {
            *px = image::Rgb([
                ((x * 7 + y * 13) % 256) as u8,
                ((x * 11 + y * 3) % 256) as u8,
                ((x * 5 + y * 17) % 256) as u8,
            ]);
        }
        image::DynamicImage::ImageRgb8(rgb).save(&src).unwrap();

        let img = decode_jpeg_scaled(&src, 480).expect("1920x1080 JPEG 应走通快路径");
        assert!(
            img.width() >= 480,
            "降采样结果宽度 {} 不能小于目标 480，否则会被放大导致模糊",
            img.width()
        );
        // 也不该离谱地大：1920 选 1/4 正好是 480
        assert!(img.width() <= 960, "缩放因子选得过于保守：{}", img.width());
        assert_eq!(img.width() * 9, img.height() * 16, "应保持 16:9 宽高比");
    }

    /// 非 JPEG 必须走回退路径（返回 None），不能误判格式。
    #[test]
    fn test_scaled_decode_skips_non_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("a.png");
        image::DynamicImage::ImageRgb8(image::RgbImage::new(64, 64))
            .save(&png)
            .unwrap();
        assert!(decode_jpeg_scaled(&png, 240).is_none());
        // 不存在的文件同样回退，不能 panic
        assert!(decode_jpeg_scaled(&dir.path().join("missing.jpg"), 240).is_none());
    }

    /// 一次性性能基准，量化「解码整张原图只为生成 240px 缩略图」的开销占比。
    ///
    /// 手动运行（默认 ignored，不拖慢常规测试）：
    ///   cargo test --release --lib bench_thumb_pipeline -- --ignored --nocapture
    #[test]
    #[ignore = "performance benchmark, run manually with --ignored --nocapture"]
    fn bench_thumb_pipeline() {
        use std::time::Instant;

        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let thumb_dir = dir.path().join("thumbs");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&thumb_dir).unwrap();

        for (w, h, label) in [(3840u32, 2160u32, "4K"), (1920, 1080, "1080p")] {
            let name = format!("bench-{label}.jpg");
            let path = src_dir.join(&name);
            // 渐变噪声，避免纯色被编码器压成极小文件（要的是接近真实的解码成本）
            let mut rgb = image::RgbImage::new(w, h);
            for (x, y, px) in rgb.enumerate_pixels_mut() {
                *px = image::Rgb([
                    ((x * 7 + y * 13) % 256) as u8,
                    ((x * 11 + y * 3) % 256) as u8,
                    ((x * 5 + y * 17) % 256) as u8,
                ]);
            }
            image::DynamicImage::ImageRgb8(rgb).save(&path).unwrap();
            let file_kib = std::fs::metadata(&path).unwrap().len() / 1024;

            // 1) header-only 读尺寸（不解码像素）
            let t = Instant::now();
            let dims = image::ImageReader::open(&path)
                .unwrap()
                .with_guessed_format()
                .unwrap()
                .into_dimensions()
                .unwrap();
            let dims_us = t.elapsed().as_micros();

            // 2a) 常规完整解码（image / zune-jpeg，不支持降采样）
            let t = Instant::now();
            let decoded = image::ImageReader::open(&path).unwrap().decode().unwrap();
            let decode_ms = t.elapsed().as_millis();
            let decoded_mib =
                (decoded.width() as u64 * decoded.height() as u64 * 4) / (1024 * 1024);

            // 2b) JPEG 快路径：DCT 域降采样解码
            let t = Instant::now();
            let scaled = decode_jpeg_scaled(&path, 480);
            let fast_ms = t.elapsed().as_millis();
            let scaled_desc = match &scaled {
                Some(img) => format!("{}x{}", img.width(), img.height()),
                None => "不可用（回退）".to_string(),
            };

            // 3) 缩放 + 编码
            let t = Instant::now();
            let thumb = decoded.thumbnail(480, u32::MAX);
            let resize_ms = t.elapsed().as_millis();
            let t = Instant::now();
            thumb.save(thumb_dir.join(format!("{label}.webp"))).unwrap();
            let save_ms = t.elapsed().as_millis();

            let total = (decode_ms + resize_ms + save_ms).max(1);
            println!(
                "{label} {w}x{h}: jpeg {file_kib} KiB\n  \
                 读尺寸 {dims_us}us | 常规解码 {decode_ms}ms (驻留 {decoded_mib} MiB)\n  \
                 JPEG 降采样解码 {fast_ms}ms → {scaled_desc}\n  \
                 缩放 {resize_ms}ms | 编码存盘 {save_ms}ms\n  \
                 → 解码占全链路 {}%",
                decode_ms * 100 / total
            );
            assert_eq!(dims, (w, h));
            // 快路径必须真的更快，否则就没意义了
            assert!(
                fast_ms <= decode_ms,
                "{label}: 快路径 {fast_ms}ms 竟慢于常规 {decode_ms}ms"
            );
        }
    }
}
