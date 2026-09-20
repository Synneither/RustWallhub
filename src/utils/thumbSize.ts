/**
 * 缩略图档位的选择规则。
 *
 * 后端只生成三档缩略图（`thumbnail.rs`：基准宽 240px × dpr，即 240 / 480 / 720），
 * 文件名里带宽度（`xxx__w480.webp`），三档可以共存、互不覆盖。
 *
 * ## 为什么不能固定用 config.thumbnail_dpr
 *
 * 网格列宽是 `repeat(auto-fill, minmax(--grid-cell-min, 1fr))` —— `1fr` 会把剩余空间
 * 摊给每一列，所以**真实列宽永远大于档位的标称 min**。实测（窗口 1280–2560）：
 *
 * | 档位 | 标称 min | 实际渲染列宽 |
 * |---|---|---|
 * | 紧凑 | 120px | 122 – 137px |
 * | 标准 | 170px | 176 – 195px |
 * | 大图 | 240px | 246 – 286px |
 *
 * 于是「按 min 选档位」的假设在"大图"档上就破了：标称 240px 对应缩略图 480px（2x 档），
 * 但真实列宽 286px 在 200% 缩放的屏幕上需要 572px 物理像素 —— 只有 480px 可用，
 * 图片被放大约 1.2 倍显示，看起来就是"缩略图太小 / 发虚"。1440 宽窗口下放大到 1.19x，
 * 而如果用户在设置里选了 1x（240px），"大图"档会被放大到 **2.38x**，糊得很明显。
 *
 * 所以档位要按「实际绘制宽度 × 屏幕像素比」现算，而不是照搬配置里的固定值。
 */

/** 缩略图基准宽，必须与后端 `thumbnail.rs` 的 `THUMB_BASE_WIDTH` 一致。 */
export const THUMB_BASE_WIDTH = 240;

/** 后端支持的最高档位（与 `thumbnail_dpr` 的 1–3 约束一致）。 */
export const THUMB_MAX_DPR = 3;

/**
 * 卡片是 16/10，而壁纸大多是 16/9（比卡片更宽）。`object-fit: cover` 会以**高度**对齐，
 * 把图画得比卡片宽约 11%（多出来的部分横向裁掉）。所以实际绘制宽度约为列宽的 1.11 倍，
 * 算档位时必须带上这个系数，否则会系统性低估一档。
 */
const COVER_WIDTH_FACTOR = 16 / 9 / (16 / 10);

function clampDpr(dpr: number): number {
  if (!Number.isFinite(dpr) || dpr < 1) return 1;
  return Math.min(THUMB_MAX_DPR, Math.round(dpr));
}

/**
 * 按实际绘制尺寸挑缩略图档位。
 *
 * @param drawnWidth 图片实际绘制宽度（CSS px，通常传网格列宽）
 * @param deviceDpr  屏幕像素比（`window.devicePixelRatio`）
 * @param floorDpr   用户在设置里选的「缩略图清晰度」，作为**最低档位**保留 ——
 *                   它仍然表示"至少这么清晰"，只是不再阻止为高缩放屏幕临时提高档位。
 */
export function pickThumbDpr(drawnWidth: number, deviceDpr: number, floorDpr: number): number {
  const floor = clampDpr(floorDpr);
  if (!(drawnWidth > 0) || !(deviceDpr > 0)) return floor;
  const needed = Math.ceil((drawnWidth * COVER_WIDTH_FACTOR * deviceDpr) / THUMB_BASE_WIDTH);
  return clampDpr(Math.max(floor, needed));
}

/**
 * 一档缩略图能覆盖的最大 CSS 绘制宽度 —— 超过这个宽度图片就会被放大显示（发虚）。
 *
 * 网格用它给卡片尺寸封顶：窗口继续变宽时不再让卡片变大，而是转为增加列数，
 * 因为再大就没有足够的像素可用了。
 *
 * @param thumbDpr 可用的最高档位（本地缩略图传 `THUMB_MAX_DPR`；远端固定档位传它实际宽度对应的档位）
 */
export function maxCoveredWidth(thumbDpr: number, deviceDpr: number): number {
  const dpr = Math.max(1, Math.min(THUMB_MAX_DPR, thumbDpr));
  const dev = Math.max(1, deviceDpr);
  return (THUMB_BASE_WIDTH * dpr) / (COVER_WIDTH_FACTOR * dev);
}

/** 远端缩略图（Wallhaven 只有 small/large/original）能覆盖的最大绘制宽度。 */
export function maxCoveredWidthForPixels(nativeWidth: number, deviceDpr: number): number {
  return nativeWidth / (COVER_WIDTH_FACTOR * Math.max(1, deviceDpr));
}
