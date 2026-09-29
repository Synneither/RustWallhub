import { computed, ref, watch, type ComputedRef, type Ref } from "vue";
import type { WallhavenImageEntry, WallhavenSearchResult } from "../types";

/** 全屏查看器需要的条目形态（见 `components/ImageViewer.vue` 的 `ViewerImage`）。 */
export interface PreviewListItem {
  name: string;
  path: string;
  rawUrl: string;
  placeholderUrl: string;
}

export interface WallhavenPreviewApi {
  /** 当前索引；-1 = 未打开 */
  index: Ref<number>;
  open: ComputedRef<boolean>;
  image: ComputedRef<WallhavenImageEntry | null>;
  list: ComputedRef<PreviewListItem[]>;
  /** 续接加载中（底部提示用） */
  loadingMore: Ref<boolean>;
  /** 在某张图上打开预览 */
  openAt(img: WallhavenImageEntry): void;
  close(): void;
  /** 下载完自动跳下一张（连续挑图时不用手动翻） */
  advance(): void;
  /** 结果页换了：收起预览并丢弃缓冲 */
  reset(): void;
}

/** 距离末尾还剩几张时就预取下一页，做到无缝续接 */
const PREFETCH_AHEAD = 2;

/**
 * 大图预览的滚动缓冲：预览列表 = 当前结果页 + 已续接的后续页（索引由查看器内部维护并回传，
 * 见 `v-model:index`），这样「下载当前图 / 选入下载」始终作用在正在看的那一张上。
 *
 * **不要直接拿结果页数组当预览列表**：续接时会往后追加，一旦它跟着结果页重置，索引就会错位。
 *
 * 翻页与搜索的竞态由调用方在 `loadPage` 里判定（重新搜索后返回 `null` 即可作废本次续接），
 * 所以这里不持有搜索序号。
 */
export function useWallhavenPreview(options: {
  /** 当前结果页；`null` = 还没有结果 */
  page: () => WallhavenSearchResult | null;
  /** 取某一页；返回 `null` 表示这次取数已作废（例如期间用户重新搜索了） */
  loadPage: (page: number) => Promise<WallhavenSearchResult | null>;
  /** 续接成功：让调用方把自己的结果页换成新页，保持网格与预览一致 */
  commitPage: (result: WallhavenSearchResult) => void;
}): WallhavenPreviewApi {
  const index = ref(-1);
  const items = ref<WallhavenImageEntry[]>([]);
  const loadingMore = ref(false);
  /** 后续页已取尽/取失败，不再尝试，避免反复请求同一页 */
  const exhausted = ref(false);
  /** 用户已要求前进、但缓冲刚好到边界时的补位标记（见 `advance`） */
  let pendingAdvance = false;

  const open = computed(() => index.value >= 0);
  const list = computed<PreviewListItem[]>(() =>
    items.value.map((i) => ({
      name: i.id,
      path: i.path,
      rawUrl: i.path, // 远程原图，不能走 asset 协议
      placeholderUrl: i.thumbnail_url, // 网格里已加载过，秒开
    })),
  );
  const image = computed<WallhavenImageEntry | null>(() => items.value[index.value] ?? null);

  function openAt(img: WallhavenImageEntry) {
    pendingAdvance = false; // 清掉上一轮遗留的补位标记
    let i = items.value.findIndex((x) => x.id === img.id);
    if (i < 0) {
      // 不在缓冲里（如刚翻过页）：以当前页重建，索引按新表算
      items.value = [...(options.page()?.images ?? [])];
      exhausted.value = false;
      i = items.value.findIndex((x) => x.id === img.id);
    }
    if (i < 0) return;
    index.value = i;
  }

  function close() {
    index.value = -1;
  }

  function reset() {
    index.value = -1;
    items.value = [];
    exhausted.value = false;
    // 重置时也要丢掉补位标记：否则它会在下一次预取成功时误触发一次自动前进。
    pendingAdvance = false;
  }

  /** 追加下一页到缓冲；调用方同时把结果页换成该页，保持两边一致 */
  async function extend() {
    const cur = options.page();
    if (!cur || loadingMore.value || exhausted.value) return;
    if (cur.page >= cur.total_pages) {
      exhausted.value = true;
      return;
    }
    loadingMore.value = true;
    try {
      const next = await options.loadPage(cur.page + 1);
      if (!next) return; // 已作废（期间重新搜索）
      if (next.images.length === 0) {
        exhausted.value = true;
        return;
      }
      items.value = [...items.value, ...next.images];
      options.commitPage(next);
      // 只有用户先前明确要求前进（下载后自动跳下一张）才补位；
      // 单纯预取完不能自动翻页，否则会把用户正在看的那张顶掉。
      if (pendingAdvance) {
        pendingAdvance = false;
        index.value = Math.min(index.value + 1, items.value.length - 1);
      }
    } catch {
      // 静默失败：网格与分页按钮仍可正常用，这里只是不再自动续接
      exhausted.value = true;
    } finally {
      loadingMore.value = false;
    }
  }

  /* 索引变化即检查是否接近末尾 */
  watch(index, (i) => {
    if (i < 0 || exhausted.value) return;
    if (i < items.value.length - PREFETCH_AHEAD) return;
    void extend();
  });

  function advance() {
    if (index.value < items.value.length - 1) {
      index.value += 1;
    } else if (!exhausted.value) {
      // 正好卡在已加载的末尾：等续接取回下一页后由 extend 补位
      pendingAdvance = true;
    }
  }

  return { index, open, image, list, loadingMore, openAt, close, advance, reset };
}
