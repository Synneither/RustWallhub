import { computed, onBeforeUnmount, ref, type Ref } from "vue";
import { appState } from "../stores/app";

/**
 * 有效像素比 = 屏幕像素比 × 界面缩放。
 *
 * 为什么不能直接读 `window.devicePixelRatio`：
 *
 * 1. **没有变化通知**。它拿到的是「读的那一刻」的值，用在 computed 里时，computed 只在
 *    别的依赖（比如容器宽度）变化时才重新求值 —— 把窗口拖到另一块缩放比例不同的显示器、
 *    或者改系统缩放设置时，缩略图档位与卡片宽度上限就不会重算，界面还是按旧比例显示。
 *    做法是绑到当前 dppx 的媒体查询上：只有值真的变了那条查询才会触发，触发后重新绑定。
 * 2. **界面缩放会改变这个比值**。设置 → 外观里的界面缩放（webview zoom）会整体放大
 *    CSS 像素，于是「一个 CSS 像素落到多少物理像素」变成 `dpr × zoom`。缩略图档位、
 *    卡片尺寸上限都是按这个物理像素量算的，少算这一项会把图放大显示（发虚）——
 *    而这恰好是本功能的目标场景（分数缩放的 Wayland 桌面），不带上就等于新功能自带瑕疵。
 */
export function useEffectiveDpr(): Ref<number> {
  const screenDpr = ref(window.devicePixelRatio || 1);
  let media: MediaQueryList | null = null;

  function onChange() {
    const next = window.devicePixelRatio || 1;
    if (next === screenDpr.value) return;
    screenDpr.value = next;
    bind();
  }

  function bind() {
    media?.removeEventListener("change", onChange);
    media = window.matchMedia(`(resolution: ${screenDpr.value}dppx)`);
    media.addEventListener("change", onChange);
  }

  bind();
  onBeforeUnmount(() => {
    media?.removeEventListener("change", onChange);
    media = null;
  });

  /** 界面缩放（配置可能还没加载完成，或被人手改坏，这里兜一下）。 */
  const zoom = computed(() => {
    const value = appState.config?.ui_zoom ?? 1;
    return Number.isFinite(value) && value > 0 ? value : 1;
  });

  return computed(() => screenDpr.value * zoom.value);
}
