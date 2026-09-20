import { onBeforeUnmount, ref, type Ref } from "vue";

/**
 * 屏幕像素比，并且**响应它的变化**。
 *
 * 直接读 `window.devicePixelRatio` 拿到的是「读的那一刻」的值，没有变化通知，所以
 * 把它用在 computed 里时，computed 只会在别的依赖（比如容器宽度）变化时才重新求值 ——
 * 把窗口拖到另一块缩放比例不同的显示器、或者改 Windows 的缩放设置时，缩略图档位与
 * 卡片宽度上限就不会重算，界面还是按旧比例显示。
 *
 * 做法是绑到当前 dppx 的媒体查询上：只有值真的变了那条查询才会触发，触发后重新绑定。
 */
export function useDeviceDpr(): Ref<number> {
  const dpr = ref(window.devicePixelRatio || 1);
  let media: MediaQueryList | null = null;

  function onChange() {
    const next = window.devicePixelRatio || 1;
    if (next === dpr.value) return;
    dpr.value = next;
    bind();
  }

  function bind() {
    media?.removeEventListener("change", onChange);
    media = window.matchMedia(`(resolution: ${dpr.value}dppx)`);
    media.addEventListener("change", onChange);
  }

  bind();
  onBeforeUnmount(() => {
    media?.removeEventListener("change", onChange);
    media = null;
  });

  return dpr;
}
