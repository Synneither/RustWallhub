import { nextTick, onBeforeUnmount, onMounted, ref, watch, type Ref } from "vue";

export interface ContainerWidthOptions {
  /**
   * 容器宽度更新并让出一拍（nextTick）后要做的额外测量。
   * 典型用途：GalleryView 在新的 --grid-cell-min 生效、网格重排之后量实际列宽。
   */
  afterMeasure?: () => void | Promise<void>;
}

/**
 * 跟踪网格容器内容宽度（ResizeObserver）。
 *
 * GalleryView 与 WallhavenView 此前各有一份几乎相同的样板：
 * gridEl ref + containerWidth ref + observeGrid()/ResizeObserver +
 * watch(gridEl) 重新挂观察 + onBeforeUnmount 断开。收敛到这里。
 *
 * 观察对象是**网格元素自身**：网格铺满容器内容盒，所以窗口缩放会体现在它的
 * clientWidth 上；而改 --grid-cell-min 只改变列数、不改变网格自身尺寸，
 * 因此不会因自己的输出自激成 ResizeObserver 死循环。
 *
 * 模板里 v-if 切换（骨架屏 ⇄ 真实网格 ⇄ 空态）会替换网格元素，
 * 元素一换就重新挂 observer——这由内部的 watch(gridEl) 处理，调用方不用管。
 */
export function useContainerWidth(
  gridEl: Ref<HTMLElement | null>,
  options: ContainerWidthOptions = {},
) {
  /** 网格容器的内容宽度（CSS px）；0 表示还没量到 */
  const containerWidth = ref(0);
  let ro: ResizeObserver | null = null;

  /** 量容器宽度，让出一拍后执行 afterMeasure（列宽要等网格重排后才读得到） */
  async function sync() {
    const el = gridEl.value;
    if (!el) return;
    const cw = el.clientWidth;
    if (cw > 0) containerWidth.value = Math.round(cw);
    if (options.afterMeasure) {
      await nextTick();
      await options.afterMeasure();
    }
  }

  function observe() {
    ro?.disconnect();
    const el = gridEl.value;
    if (!el || typeof ResizeObserver === "undefined") return;
    ro = new ResizeObserver(() => void sync());
    ro.observe(el);
  }

  // 元素被 v-if 替换时重新挂 observer 并立即量一次
  watch(gridEl, () => {
    observe();
    void sync();
  });

  onMounted(() => {
    observe();
    void sync();
  });

  onBeforeUnmount(() => {
    ro?.disconnect();
    ro = null;
  });

  return { containerWidth, sync };
}
