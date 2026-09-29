import { describe, expect, it } from "vitest";
import {
  THUMB_BASE_WIDTH,
  THUMB_MAX_DPR,
  maxCoveredWidth,
  maxCoveredWidthForPixels,
  pickThumbDpr,
} from "../../src/utils/thumbSize";

/**
 * 档位选择的规则容易"看起来对"但系统性偏低一档 —— 卡片是 16/10 而壁纸多是 16/9，
 * `object-fit: cover` 会以高度对齐、把图画得比卡片宽约 11%（cover 补偿系数）。
 * 少算这个系数不会报错，只会让缩略图发虚，所以这里按实测列宽钉住结果。
 */
describe("pickThumbDpr", () => {
  it("参考列宽下取最低档", () => {
    expect(pickThumbDpr(176, 1, 1)).toBe(1);
  });

  it("大图档 + 2x 屏幕需要最高档（这正是不能固定用配置值的原因）", () => {
    // 286 × (10/9) × 2 / 240 = 2.65 → 3
    expect(pickThumbDpr(286, 2, 1)).toBe(3);
  });

  it("用户的清晰度设置是下限，不被低缩放屏幕拉低", () => {
    expect(pickThumbDpr(120, 1, 2)).toBe(2);
  });

  it("高缩放屏幕可以把档位临时提到用户设置之上", () => {
    expect(pickThumbDpr(240, 2, 1)).toBe(3);
  });

  it("超过最高档时封顶", () => {
    expect(pickThumbDpr(4000, 4, 1)).toBe(THUMB_MAX_DPR);
  });

  it("还没量到宽度（0）或 dpr 非法时退回用户设置", () => {
    expect(pickThumbDpr(0, 2, 2)).toBe(2);
    expect(pickThumbDpr(170, 0, 2)).toBe(2);
    expect(pickThumbDpr(Number.NaN, 2, 3)).toBe(3);
  });

  it("dpr 先被规整到 1..3", () => {
    expect(pickThumbDpr(170, 1, 0.4)).toBe(1);
    expect(pickThumbDpr(170, 1, 99)).toBe(THUMB_MAX_DPR);
  });
});

describe("maxCoveredWidth（卡片尺寸封顶）", () => {
  it("最高档在 1x 屏上可覆盖 240 × 3 ÷ cover 系数", () => {
    expect(maxCoveredWidth(THUMB_MAX_DPR, 1)).toBeCloseTo(648, 6);
  });

  it("2x 屏只剩一半宽度", () => {
    expect(maxCoveredWidth(THUMB_MAX_DPR, 2)).toBeCloseTo(324, 6);
  });

  it("dpr 与 deviceDpr 都被夹在合法区间内", () => {
    expect(maxCoveredWidth(99, 1)).toBeCloseTo(648, 6);
    expect(maxCoveredWidth(0, 1)).toBeCloseTo(THUMB_BASE_WIDTH / (16 / 9 / (16 / 10)), 6);
    expect(maxCoveredWidth(2, 0)).toBeCloseTo(maxCoveredWidth(2, 1), 6);
  });
});

describe("maxCoveredWidthForPixels（远端固定档位）", () => {
  it("Wallhaven large 档约 500px，1x 屏可覆盖约 450 CSS px", () => {
    expect(maxCoveredWidthForPixels(500, 1)).toBeCloseTo(450, 6);
  });

  it("2x 屏减半", () => {
    expect(maxCoveredWidthForPixels(500, 2)).toBeCloseTo(225, 6);
  });

  it("deviceDpr 非法时按 1 处理", () => {
    expect(maxCoveredWidthForPixels(500, 0)).toBeCloseTo(450, 6);
  });
});
