import { computed, ref, watch, type ComputedRef, type Ref } from "vue";

/** 网格密度档位 */
export type GridDensity = "compact" | "normal" | "large";

export interface DensityOption {
  value: GridDensity;
  label: string;
  /** 参考宽度（见 REFERENCE_WIDTH）下的列宽 */
  min: string;
  /** 参考宽度下屏外卡片的占位高度，需随列宽同步，否则滚动条跳动 */
  ph: string;
}

/**
 * 档位标称值对应的参考容器内容宽度 —— 大约等于 1440 宽窗口（减去侧栏与内边距）的内容宽度。
 * 档位表里的 120/170/240 都是这个宽度下的列宽，实际列宽按容器宽度**等比向上缩放**。
 */
const REFERENCE_WIDTH = 1168;

export interface DensityScale {
  /** 网格容器的内容宽度；为 0（还没量到）时退回档位标称值 */
  containerWidth: Ref<number>;
  /** 列宽上限：超过这个宽度缩略图就会被放大显示，见 utils/thumbSize.ts */
  maxCell: Ref<number> | ComputedRef<number>;
}

export interface GridDensityApi {
  density: Ref<GridDensity>;
  items: DensityOption[];
  /** 绑到网格容器上：--grid-cell-min / --grid-cell-ph */
  gridStyle: ComputedRef<Record<string, string>>;
}

/**
 * 图片网格的密度档位（纯 UI 偏好，存 localStorage，不进 config.json）。
 *
 * 档位语义是「参考宽度下的列宽」，实际列宽 = 档位值 × (容器宽 / 参考宽)，**随窗口等比缩放**。
 *
 * 为什么必须缩放：网格用的是 `repeat(auto-fill, minmax(min, 1fr))`，`1fr` 会把剩余空间
 * 摊给每一列，但**列数也会随窗口一起增加**，两者互相抵消 —— min 写死时卡片尺寸几乎不变。
 * 实测内容宽从 1008 涨到 3568（窗口 1280→3840），「标准」档只是从 5 列变成 20 列，
 * 卡片始终在 171–195px 之间，窗口越大反而略小。用户的直观感受就是"图永远这么小"。
 *
 * 上限由缩略图分辨率决定：本地缩略图基准宽 240px × 最高 3 档 = 720px，
 * 覆盖不住更宽的卡片就会发虚，所以让上限接管、转为增加列数。
 */
export function useGridDensity(
  storageKey: string,
  items: DensityOption[],
  fallback: GridDensity = "normal",
  scale?: DensityScale,
): GridDensityApi {
  const valid = new Set(items.map((i) => i.value));

  function read(): GridDensity {
    try {
      const v = localStorage.getItem(storageKey);
      if (v && valid.has(v as GridDensity)) return v as GridDensity;
    } catch {
      /* localStorage 不可用（隐私模式/被禁用）时退回默认值 */
    }
    return fallback;
  }

  const density = ref<GridDensity>(read());

  watch(density, (v) => {
    try {
      localStorage.setItem(storageKey, v);
    } catch {
      /* 写入失败不影响使用 */
    }
  });

  const gridStyle = computed(() => {
    const o = items.find((i) => i.value === density.value) ?? items[0];
    const cw = scale?.containerWidth.value ?? 0;
    // 还没量到容器宽度（或调用方没开启缩放）：用档位标称值
    if (!scale || !(cw > 0)) {
      return { "--grid-cell-min": o.min, "--grid-cell-ph": o.ph };
    }

    const k = cw / REFERENCE_WIDTH;
    const ref = Number.parseFloat(o.min);
    // 只向上缩放：不低于档位标称值（窄窗口下与改动前完全一致，避免卡片反而变小），
    // 上限是缩略图能覆盖的宽度（超过就只是把图放大，不如转为增加列数）。
    const cell = Math.min(Math.max(ref * k, ref), Math.max(ref, scale.maxCell.value));
    // 占位高按「档位里 ph 与列宽的比例」跟着最终列宽走，
    // 这样即使列宽被上限截断，屏外卡片的高度估算也不会失配。
    const phRatio = Number.parseFloat(o.ph) / ref;
    return {
      "--grid-cell-min": `${Math.round(cell)}px`,
      "--grid-cell-ph": `${Math.round(cell * phRatio)}px`,
    };
  });

  return { density, items, gridStyle };
}
