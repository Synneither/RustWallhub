import { computed, ref, watch, type ComputedRef, type Ref } from "vue";

/** 网格密度档位 */
export type GridDensity = "compact" | "normal" | "large";

export interface DensityOption {
  value: GridDensity;
  label: string;
  /** 网格最小列宽（minmax 的下限） */
  min: string;
  /** 屏外卡片占位高度，需随列宽同步，否则滚动条跳动 */
  ph: string;
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
 * 各页面自带档位表：`min` 的上限取决于缩略图实际分辨率——
 * 图库缩略图是后端生成的（基准宽 240px × DPR），超过原生宽度就会糊，
 * 而 Wallhaven 用的是远端 large 档（约 500px 宽），可以放到 330px。
 */
export function useGridDensity(
  storageKey: string,
  items: DensityOption[],
  fallback: GridDensity = "normal",
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
    return {
      "--grid-cell-min": o.min,
      "--grid-cell-ph": o.ph,
    };
  });

  return { density, items, gridStyle };
}
