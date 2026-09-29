import { computed, ref, type ComputedRef, type Ref } from "vue";
import type { ImageInfo, LocalImageEntry, Source } from "../types";
import { getImageInfo } from "../utils/api";
import { toastError } from "../stores/app";
import { openUrlSafe } from "../utils/openUrl";

/** 全屏查看器需要的条目形态（与 `components/ImageViewer.vue` 的 `ViewerImage` 对应）。 */
interface ViewerEntry {
  name: string;
  path: string;
}

export interface GalleryDetailApi {
  /* ── 全屏查看器 ── */
  viewerOpen: Ref<boolean>;
  viewerIndex: Ref<number>;
  viewerImages: ComputedRef<ViewerEntry[]>;
  /** 打开查看器并定位到某张（不在当前页时落在第一张） */
  openViewerFor(img: LocalImageEntry): void;
  /* ── 详情抽屉 ── */
  detailOpen: Ref<boolean>;
  detailLoading: Ref<boolean>;
  detail: Ref<ImageInfo | null>;
  /** 触发详情时的原始条目，抽屉的预览/删除直接用它，避免用 detail 字段手工拼 entry */
  detailEntry: Ref<LocalImageEntry | null>;
  detailPreviewSrc: ComputedRef<string>;
  openDetail(img: LocalImageEntry): Promise<void>;
  /** 打开图片来源页（不是资产地址，交给系统浏览器） */
  onOpenLink(url: string | null): Promise<void>;
}

/**
 * 图库的「查看当前这张图」两个入口：全屏查看器与详情抽屉。
 *
 * 两者的共同点是都依赖「当前页条目」和「已解析的缩略图地址」，都带竞态守卫
 * （快速点开 A/B 两图时慢响应不得覆盖快响应），所以一并收在这里，视图只负责渲染。
 */
export function useGalleryDetail(options: {
  /** 当前来源（详情要按来源查库） */
  source: () => Source;
  /** 当前页条目 */
  entries: () => LocalImageEntry[];
  /** 取某条的缩略图地址（未命中缓存时回退原图），抽屉的小预览复用它 */
  thumbOf: (img: LocalImageEntry) => string;
}): GalleryDetailApi {
  /* ── 全屏查看器 ── */
  const viewerOpen = ref(false);
  const viewerIndex = ref(0);
  const viewerImages = computed<ViewerEntry[]>(() =>
    options.entries().map((i) => ({ name: i.name, path: i.path })),
  );

  function openViewerFor(img: LocalImageEntry) {
    const idx = options.entries().findIndex((i) => i.name === img.name);
    viewerIndex.value = Math.max(0, idx);
    viewerOpen.value = true;
  }

  /* ── 详情抽屉 ── */
  const detailOpen = ref(false);
  const detailLoading = ref(false);
  const detail = ref<ImageInfo | null>(null);
  const detailEntry = ref<LocalImageEntry | null>(null);
  /** 抽屉里的预览最高 240px，用页面已经解析好的缩略图而不是原图（4K 图解码约 33MB）。
   *  未命中缓存时 thumbOf 会退回原图地址，等价于旧行为。 */
  const detailPreviewSrc = computed(() =>
    detailEntry.value ? options.thumbOf(detailEntry.value) : "",
  );
  /** 详情请求竞态控制：快速点开 A/B 两图时，慢响应不得覆盖快响应、也不得误关抽屉。 */
  let detailSeq = 0;

  async function openDetail(img: LocalImageEntry) {
    const seq = ++detailSeq;
    detailEntry.value = img;
    detailOpen.value = true;
    detailLoading.value = true;
    detail.value = null;
    try {
      const info = await getImageInfo(options.source(), img.name);
      if (seq !== detailSeq) return;
      detail.value = info;
    } catch (e) {
      if (seq !== detailSeq) return;
      toastError(e);
      detailOpen.value = false;
    } finally {
      if (seq === detailSeq) detailLoading.value = false;
    }
  }

  async function onOpenLink(url: string | null) {
    await openUrlSafe(url);
  }

  return {
    viewerOpen,
    viewerIndex,
    viewerImages,
    openViewerFor,
    detailOpen,
    detailLoading,
    detail,
    detailEntry,
    detailPreviewSrc,
    openDetail,
    onOpenLink,
  };
}
