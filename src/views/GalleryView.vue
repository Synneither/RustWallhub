<script setup lang="ts">
import {
  computed,
  onActivated,
  onBeforeUnmount,
  onDeactivated,
  onMounted,
  ref,
  shallowRef,
  watch,
} from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { LocalImageEntry, OrphanFile } from "../types";
import {
  adoptOrphanFiles,
  assetUrl,
  browseImageFiles,
  deleteOrphanFile,
  deleteOrphanFiles,
  dislikeFile,
  dislikeFiles,
  getActiveWallpaper,
  listOrphanFiles,
  resolveThumbnails,
} from "../utils/api";
import { appState, activeDownloadSources, askConfirm, dbReady, toast, toastError } from "../stores/app";
import EmptyState from "../components/EmptyState.vue";
import GalleryCard from "../components/GalleryCard.vue";
import GalleryBatchBar from "../components/GalleryBatchBar.vue";
import GridSizeBar from "../components/GridSizeBar.vue";
import ProgressCard from "../components/ProgressCard.vue";
import ImageViewer from "../components/ImageViewer.vue";
import ImageDetailDrawer from "../components/ImageDetailDrawer.vue";
import { useSelection } from "../composables/useSelection";
import { densityItems, useGridDensity } from "../composables/useGridDensity";
import { useEffectiveDpr } from "../composables/useEffectiveDpr";
import { useThumbCache } from "../composables/useThumbCache";
import { useGalleryDetail } from "../composables/useGalleryDetail";
import { useContainerWidth } from "../composables/useContainerWidth";
import { friendlyError } from "../utils/errors";
import { basename, pathKey } from "../utils/path";
import { pickThumbDpr, maxCoveredWidth, THUMB_MAX_DPR } from "../utils/thumbSize";

// Vuetify 组件按需局部导入（见 main.ts 的注册策略说明）：只在这个视图/组件里用到，
// 挂全局注册会让首屏无条件背上它们。
import { VBtnToggle } from "vuetify/components/VBtnToggle";
import { VChip } from "vuetify/components/VChip";
import { VPagination } from "vuetify/components/VPagination";
import { VSelect } from "vuetify/components/VSelect";
import { VTextField } from "vuetify/components/VTextField";

/* ════ 浏览状态 ════ */
type SourceTab = "wallhaven" | "reddit";
const source = ref<SourceTab>("wallhaven");
const search = ref("");
const searchDebounced = ref("");
const sortBy = ref("default");
const page = ref(1);
const pageSize = ref(48);
const orphanOnly = ref(false);

/* ════ 自定义目录模式 ════
 * 后端 browse_image_files 支持 custom_dir；此模式下缩略图/删除/详情/孤儿
 * 等依赖源目录与数据库的能力不可用，仅保留浏览。 */
const customDir = ref<string | null>(null);
const customDirName = computed(() => basename(customDir.value));

async function pickCustomDir() {
  try {
    const dir = await openDialog({ directory: true, defaultPath: customDir.value ?? undefined });
    if (typeof dir === "string") customDir.value = dir;
  } catch (e) {
    toastError(e);
  }
}

function exitCustomDir() {
  customDir.value = null;
}

const SORT_ITEMS = [
  { title: "默认（孤儿优先）", value: "default" },
  { title: "名称 ↑", value: "name_asc" },
  { title: "名称 ↓", value: "name_desc" },
  { title: "大小 ↑", value: "size_asc" },
  { title: "大小 ↓", value: "size_desc" },
  { title: "日期 ↓", value: "date_desc" },
  { title: "日期 ↑", value: "date_asc" },
];
const PAGE_SIZE_ITEMS = [24, 48, 96];

/* ── 网格密度 ──
 * 档位表里的 min 是「参考宽度（内容宽 1168px，约等于 1440 宽窗口）下的列宽」，
 * 实际列宽按容器宽度等比缩放（见 useGridDensity 的说明）。
 * 上限由缩略图能覆盖的最大绘制宽度决定：本地缩略图 240px × 最高 3 档 = 720px，
 * 再宽的卡片就只是把图放大，所以到顶后转为增加列数。 */

/** 网格元素：既是真正的滚动容器，也是量列宽的对象。 */
const gridEl = ref<HTMLElement | null>(null);
/** 容器宽度跟踪（ResizeObserver + v-if 换元素自动重挂）已收敛到 useContainerWidth。
 *  afterMeasure：量完容器宽度、让出一拍后量实际列宽（此时新的 --grid-cell-min
 *  已生效、网格已重排）。返回值 sync 即原来的 syncLayout。 */
const { containerWidth, sync: syncLayout } = useContainerWidth(gridEl, {
  afterMeasure: measureCellWidth,
});
/** 屏幕像素比（响应变化：拖到别的显示器 / 改系统缩放时要重算档位） */
const deviceDpr = useEffectiveDpr();
/** 卡片宽度上限：超过它，缩略图就会被放大显示 */
const maxCell = computed(() => maxCoveredWidth(THUMB_MAX_DPR, deviceDpr.value));

const { density: cellSize, items: SIZE_ITEMS, gridStyle } = useGridDensity(
  "rustwallhub-gallery-cell-size",
  densityItems({ compact: "120px", normal: "170px", large: "240px" }),
  "normal",
  // minCellHeight 与 .gallery-cell 的 min-height 保持一致（style.css），用于算屏外卡片占位高度
  { containerWidth, maxCell, minCellHeight: 96 },
);

const loading = ref(false);
const loadError = ref("");
const images = ref<LocalImageEntry[]>([]);
const total = ref(0);
/** 孤儿模式：全量孤儿列表，前端分页。
 * 用 shallowRef：这个数组可能有数千条 entry，整批替换即可，不需要逐项深度代理。 */
const orphanAll = shallowRef<LocalImageEntry[]>([]);
/** 缩略图 URL 缓存（LRU，实现见 useThumbCache） */
const thumbCache = useThumbCache(600);

let searchTimer: ReturnType<typeof setTimeout> | null = null;
watch(search, (v) => {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    searchDebounced.value = v.trim();
  }, 300);
});

/* ════ 加载竞态控制 ════
 * 多个 watcher 可能在同一个 tick 同时触发 load()；用 loadSeq 保证只有最后一次
 * 请求的结果会被采用，再用 0ms timer 合并同一轮的重复触发。 */
let loadSeq = 0;
let reloadTimer: ReturnType<typeof setTimeout> | null = null;
let viewActive = false;

/** 翻页后把滚动位置带回顶部。
 *  注意**真正的滚动容器是网格本身**（`.gallery-view > .gallery-grid` 上有 `flex:1;
 *  overflow-y:auto`），不是根节点 `.view` —— 只重置 .view 是不生效的。
 *  两个都重置：外层万一在某些布局下也溢出了，也同样该回顶部。 */
const viewRoot = ref<HTMLElement | null>(null);
// gridEl 声明在上方「网格密度」处（useGridDensity 需要它）

function scrollToTop() {
  viewRoot.value?.scrollTo({ top: 0 });
  gridEl.value?.scrollTo({ top: 0 });
}

/* ════ 布局测量 ════
 * 两件事都靠实测值：
 *  1. 容器宽度 → 决定卡片尺寸（gridStyle 按它缩放；测量逻辑在 useContainerWidth）
 *  2. 实际列宽 → 决定缩略图档位（见 utils/thumbSize.ts） */
/** 实测的网格列宽（CSS px）；0 表示还没量到。 */
const cellCssWidth = ref(0);

/** 量解析后的列宽：`gridTemplateColumns` 形如 "286.4px 286.4px 286.4px"。 */
function measureCellWidth() {
  const el = gridEl.value;
  if (!el) return;
  const first = getComputedStyle(el).gridTemplateColumns.split(" ")[0];
  const w = Number.parseFloat(first);
  // 取整后再比较：亚像素抖动不该触发重新解析缩略图
  if (Number.isFinite(w) && w > 0) cellCssWidth.value = Math.round(w);
}

/** 该用哪一档缩略图。cellCssWidth 还没量到时退回 170（标准档的参考列宽），
 *  在 floorDpr=2 的默认设置下与改动前一致。 */
const thumbDpr = computed(() =>
  pickThumbDpr(cellCssWidth.value || 170, deviceDpr.value, appState.config?.thumbnail_dpr ?? 2),
);

/** 改密度后列宽会变，但网格自身尺寸没变、ResizeObserver 不会触发，要显式再量一次。 */
watch(cellSize, () => void syncLayout());

/** 档位变化（窗口缩放 / 改密度 / 拖到另一块屏）：缓存里的 URL 指向的是另一个分辨率的
 *  缩略图，必须整批丢弃重新解析，否则会继续用旧档位显示。 */
watch(thumbDpr, () => {
  if (thumbCache.size.value === 0) return;
  thumbCache.clear();
  void loadThumbs();
});

/** 上次成功加载完成时的 galleryEpoch。
 *  用它判断"切回本页时数据到底变没变"，避免每次切页都无条件重载 ——
 *  重载会让整页网格走一遍 .gallery-grid--loading 的置灰，而数据通常没变。 */
let loadedEpoch = -1;

function scheduleLoad() {
  if (reloadTimer) clearTimeout(reloadTimer);
  reloadTimer = setTimeout(() => {
    reloadTimer = null;
    if (viewActive) void load();
  }, 0);
}

watch(source, () => {
  // 切换来源后旧缩略图 URL 不能跨源复用；整批丢弃（一次替换引用，只触发一次更新）
  thumbCache.clear();
});

const totalPages = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)));
const orphanCountOnPage = computed(() => images.value.filter((i) => i.is_orphan).length);

function toEntry(o: OrphanFile): LocalImageEntry {
  return { name: o.name, path: o.path, thumb_path: null, size: o.size, is_orphan: true, modified_date: null };
}

async function load() {
  if (!dbReady.value && !customDir.value) return;
  const seq = ++loadSeq;
  loading.value = true;
  loadError.value = "";
  try {
    if (orphanOnly.value && !customDir.value) {
      const all = (await listOrphanFiles(source.value)).map(toEntry);
      if (seq !== loadSeq) return;
      orphanAll.value = all;
      total.value = all.length;
      const start = (page.value - 1) * pageSize.value;
      images.value = all.slice(start, start + pageSize.value);
      // 等网格真正渲染出来再量尺寸：档位与缩略图都要按实际显示尺寸选
      await syncLayout();
      await loadThumbs(seq);
    } else {
      const res = await browseImageFiles(source.value, {
        offset: (page.value - 1) * pageSize.value,
        limit: pageSize.value,
        customDir: customDir.value ?? undefined,
        search: searchDebounced.value || undefined,
        sortBy: sortBy.value,
      });
      if (seq !== loadSeq) return;
      total.value = res.total;
      images.value = res.images;
      if (customDir.value) {
        // 自定义目录无缩略图管线，直接用原图；同时避免同名文件命中旧缩略图缓存
        thumbCache.cache(res.images.map((img) => [img.name, assetUrl(img.path)] as const));
      } else {
        // 等网格真正渲染出来再量尺寸：档位与缩略图都要按实际显示尺寸选
        await syncLayout();
        await loadThumbs(seq);
      }
    }
    // 记下本次加载对应的数据版本：切回本页时用它判断"数据到底变没变"，
    // 没变就不重载（重载会让整页网格置灰闪一下）。
    if (seq === loadSeq) loadedEpoch = appState.galleryEpoch;
  } catch (e) {
    if (seq === loadSeq) {
      loadError.value = friendlyError(e);
      images.value = [];
      total.value = 0;
    }
  } finally {
    if (seq === loadSeq) loading.value = false;
  }
}

async function loadThumbs(seq = loadSeq) {
  const names = images.value.map((i) => i.name);
  if (names.length === 0) return;
  // 档位按实测列宽现算（见 utils/thumbSize.ts）：固定用配置值会在"大图"档被放大显示
  const dpr = thumbDpr.value;
  try {
    const batch = await resolveThumbnails(source.value, names, dpr);
    // 请求期间窗口缩放/改密度会让档位变化，这一批 URL 已指向别的分辨率，丢弃
    if (seq !== loadSeq || dpr !== thumbDpr.value) return;
    thumbCache.cache(batch.items.map((it) => [it.name, assetUrl(it.thumb_path)] as const));
  } catch (e) {
    if (seq !== loadSeq) return;
    // 缩略图失败不致命，回退原图
    thumbCache.cache(images.value.map((img) => [img.name, assetUrl(img.path)] as const));
  }
}

function thumbOf(img: LocalImageEntry): string {
  // useThumbCache.get 是只读的：thumbOf 在模板里被调用，渲染期间写缓存会触发
  // 渲染中的响应式更新（Vue 会告警并可能死循环）。LRU 位置在 cache() 写入时更新。
  return thumbCache.get(img.name) ?? assetUrl(img.path);
}

/* 触发重载 */
watch([source, searchDebounced, sortBy, orphanOnly], () => {
  // 换来源/搜索条件后旧选中项已不在列表里：不清掉的话计数会虚报，
  // 批量删除还会拿当前 source 去发另一个来源的文件名。
  clearSelection();
  page.value = 1;
  scrollToTop();
  scheduleLoad();
});
watch(customDir, () => {
  // 进入/退出自定义目录时重置易冲突的状态
  orphanOnly.value = false;
  clearSelection();
  page.value = 1;
  scrollToTop();
  scheduleLoad();
});
watch([page, pageSize], () => {
  // 翻页/改每页条数后把滚动位置带回顶部：滚动容器是根节点 .view，
  // 否则从列表中段翻页会直接落在新页的中段，看起来像页码没变。
  scrollToTop();
  scheduleLoad();
});
watch(
  () => appState.galleryEpoch,
  () => {
    if (viewActive) scheduleLoad();
  },
);

onMounted(() => {
  viewActive = true;
  loadActiveWallpaper();
  // 容器宽度的首次测量与 ResizeObserver 挂载由 useContainerWidth 的 onMounted 完成
});
onActivated(() => {
  viewActive = true;
  // 当前壁纸可能在应用外被改过（系统设置、别的工具），切回图库时重新读一次，
  // 保证高亮的始终是眼下真正在用的那张。
  loadActiveWallpaper();
  // 只在数据真的变过之后才重载：以前无条件 load()，每次切回图库都会整页重取一遍
  // （还把网格置灰闪一下），而绝大多数情况下数据并没有变。
  if (appState.galleryEpoch !== loadedEpoch) scheduleLoad();
});
onDeactivated(() => {
  viewActive = false;
  if (reloadTimer) {
    clearTimeout(reloadTimer);
    reloadTimer = null;
  }
});
onBeforeUnmount(() => {
  if (searchTimer) clearTimeout(searchTimer);
  if (reloadTimer) clearTimeout(reloadTimer);
  // ResizeObserver 的断开由 useContainerWidth 的 onBeforeUnmount 完成
});

/* ════ 多选与批量 ════ */
const selectionMode = ref(false);
const { selected, toggle: toggleSelect, clear: clearSelected } = useSelection();

function clearSelection() {
  clearSelected();
  selectionMode.value = false;
}
function onCardClick(img: LocalImageEntry) {
  if (selectionMode.value) {
    toggleSelect(img.name);
  } else {
    openViewerFor(img);
  }
}

const batchRunning = ref(false);

async function onBatchDelete() {
  const names = [...selected];
  if (names.length === 0) return;
  const isOrphanMode = orphanOnly.value;
  const ok = await askConfirm(
    isOrphanMode ? "删除孤儿文件" : "批量删除",
    isOrphanMode
      ? `将把 ${names.length} 个孤儿文件移入回收站（缩略图缓存直接删除）。`
      : `将把 ${names.length} 张图片标记为不喜欢，并移入回收站（缩略图缓存直接删除）。`,
    { danger: true, confirmText: "删除" },
  );
  if (!ok) return;
  batchRunning.value = true;
  try {
    const done = isOrphanMode
      ? await deleteOrphanFiles(source.value, names)
      : await dislikeFiles(source.value, names);
    toast(`已处理 ${done} / ${names.length} 张`, done === names.length ? "success" : "info");
    clearSelection();
    await load();
  } catch (e) {
    toastError(e);
  } finally {
    batchRunning.value = false;
  }
}

async function onBatchAdopt() {
  const names = [...selected];
  if (names.length === 0) return;
  batchRunning.value = true;
  try {
    const n = await adoptOrphanFiles(source.value, names);
    toast(`已收养 ${n} 个文件入库`, "success");
    clearSelection();
    await load();
  } catch (e) {
    toastError(e);
  } finally {
    batchRunning.value = false;
  }
}

/* ════ 查看器与详情 ════
 * 两个入口（全屏查看器 / 详情抽屉）都依赖「当前页条目」与「已解析的缩略图地址」，
 * 也都要防竞态（快速点开 A/B 两图时慢响应不得覆盖快响应），所以一并收进 composable，
 * 视图这边只负责渲染与事件接线。 */
const {
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
} = useGalleryDetail({
  source: () => source.value,
  entries: () => images.value,
  thumbOf,
});

/* ════ 当前壁纸（只读，用于高亮） ════ */
/** 本应用只管理壁纸，不设置壁纸；读系统当前壁纸只是为了在图库里标出来。
 *  多显示器可能各有一张，所以存一组归一化后的 key。 */
const activeWallpaperKeys = shallowRef<Set<string>>(new Set());

async function loadActiveWallpaper() {
  try {
    const res = await getActiveWallpaper();
    activeWallpaperKeys.value = new Set(res.paths.map(pathKey).filter((k) => k !== ""));
  } catch {
    // 读不到就当没有当前壁纸（幻灯片/纯色壁纸本来就给不出路径），不打扰用户。
    activeWallpaperKeys.value = new Set();
  }
}

/** 这张图是不是当前桌面壁纸（任一显示器在用就算）。
 *
 *  预计算成「本页文件名集合」而不是在模板里逐卡片判断：卡片会把结果用在 class 与
 *  角标两处，一页 96 张就是每帧 ~192 次 `pathKey`（正则 + 小写化）。集合只在
 *  本页条目或当前壁纸变化时重算。 */
const currentWallpaperNames = computed(() => {
  const keys = activeWallpaperKeys.value;
  const names = new Set<string>();
  if (keys.size === 0) return names;
  for (const img of images.value) {
    if (keys.has(pathKey(img.path))) names.add(img.name);
  }
  return names;
});

/* ════ 删除（单张） ════ */
async function onDeleteSingle(img: LocalImageEntry) {
  const isOrphan = img.is_orphan;
  const ok = await askConfirm(
    isOrphan ? "删除孤儿文件" : "删除图片",
    isOrphan
      ? `将把「${img.name}」移入回收站（缩略图缓存直接删除）。`
      : `将把「${img.name}」标记为不喜欢并移入回收站（缩略图缓存直接删除）。`,
    { danger: true, confirmText: "删除" },
  );
  if (!ok) return;
  try {
    if (isOrphan) await deleteOrphanFile(source.value, img.name);
    else await dislikeFile(source.value, img.name);
    toast("已移入回收站", "success");
    detailOpen.value = false;
    await load();
  } catch (e) {
    toastError(e);
  }
}

</script>

<template>
  <div ref="viewRoot" class="view gallery-view">
    <div class="view-header">
      <span class="view-header__title">图库</span>
      <v-chip
        v-if="customDir"
        size="small"
        color="primary"
        variant="tonal"
        closable
        class="ml-2"
        :title="customDir"
        @click:close="exitCustomDir"
      >
        <v-icon icon="mdi-folder-open-outline" size="14" start />
        {{ customDirName }}
      </v-chip>
      <v-btn-toggle v-else v-model="source" mandatory density="compact" color="primary" class="ml-2">
        <v-btn value="wallhaven" size="small">Wallhaven</v-btn>
        <v-btn value="reddit" size="small">Reddit</v-btn>
      </v-btn-toggle>
      <v-btn
        icon="mdi-folder-open-outline"
        variant="text"
        size="small"
        title="浏览自定义目录"
        @click="pickCustomDir"
      />
      <v-spacer />
      <v-text-field
        v-model="search"
        placeholder="搜索文件名…"
        prepend-inner-icon="mdi-magnify"
        density="compact"
        hide-details
        clearable
        class="gallery-search settings-field"
      />
      <v-select
        v-model="sortBy"
        :items="SORT_ITEMS"
        density="compact"
        hide-details
        :disabled="orphanOnly"
        class="settings-field"
        style="max-width: 170px"
      />
      <v-btn icon="mdi-refresh" variant="text" size="small" :loading="loading" @click="load" />
    </div>

    <!-- 下载进度：此前后台下载在图库页完全不可见（也不能取消），而这正是最常一边等下载
         一边挑图的地方。按来源各挂一张卡，取消只作用于自己的来源。 -->
    <ProgressCard
      v-for="s in activeDownloadSources"
      :key="s"
      :source="s"
      class="animate-in"
    />

    <!-- 统计条 -->
    <div class="gallery-meta">
      <span class="text-caption">
        共 {{ total }} 张 · 第 {{ page }} / {{ totalPages }} 页
        <template v-if="!customDir && !orphanOnly && orphanCountOnPage > 0"> · 本页含 {{ orphanCountOnPage }} 个孤儿文件</template>
      </span>
      <template v-if="!customDir">
        <v-chip
          v-if="!orphanOnly"
          size="x-small"
          variant="outlined"
          class="gallery-meta__orphan-chip"
          @click="orphanOnly = true"
        >
          仅看孤儿文件
        </v-chip>
        <v-chip v-else size="x-small" color="warning" variant="tonal" closable @click:close="orphanOnly = false">
          孤儿文件模式
        </v-chip>
      </template>
      <v-spacer />
      <GridSizeBar v-model="cellSize" :items="SIZE_ITEMS" />
      <v-btn
        v-if="!customDir"
        size="x-small"
        :variant="selectionMode ? 'flat' : 'text'"
        :color="selectionMode ? 'primary' : undefined"
        @click="selectionMode ? clearSelection() : (selectionMode = true)"
      >
        {{ selectionMode ? `完成选择（${selected.size}）` : "选择" }}
      </v-btn>
      <v-select
        v-model="pageSize"
        :items="PAGE_SIZE_ITEMS"
        density="compact"
        hide-details
        class="settings-field"
        style="max-width: 90px"
      />
    </div>

    <!-- 批量操作条 -->
    <GalleryBatchBar
      v-if="selectionMode && selected.size > 0"
      :count="selected.size"
      :orphan-mode="orphanOnly"
      :busy="batchRunning"
      @adopt="onBatchAdopt"
      @remove="onBatchDelete"
    />

    <!-- 内容区 -->
    <EmptyState
      v-if="!dbReady && !customDir"
      icon="mdi-database-alert-outline"
      title="数据库未初始化"
      desc="请先在启动弹窗或「数据库」页面创建数据库；或点击右上角文件夹图标浏览任意目录"
    />
    <EmptyState
      v-else-if="loadError"
      error
      icon="mdi-alert-circle-outline"
      title="图库加载失败"
      :desc="loadError"
    >
      <v-btn variant="tonal" @click="load">重试</v-btn>
    </EmptyState>
    <div v-else-if="loading && images.length === 0" ref="gridEl" class="gallery-grid" :style="gridStyle">
      <div v-for="i in pageSize" :key="i" class="gallery-cell shimmer" />
    </div>
    <EmptyState
      v-else-if="images.length === 0 && searchDebounced"
      icon="mdi-magnify-close"
      title="没有匹配的图片"
      :desc="`文件名包含「${searchDebounced}」的图片不存在`"
    />
    <EmptyState
      v-else-if="images.length === 0"
      :icon="customDir ? 'mdi-folder-open-outline' : orphanOnly ? 'mdi-folder-check-outline' : 'mdi-image-off-outline'"
      :title="customDir ? '目录中没有图片' : orphanOnly ? '没有孤儿文件' : '图库为空'"
      :desc="customDir ? '该目录下没有可识别的图片文件' : orphanOnly ? '保存目录中的文件都已在数据库中登记' : '前往 Wallhaven 或 Reddit 页面下载图片'"
    />

    <div
      v-else
      ref="gridEl"
      class="gallery-grid"
      :class="{ 'gallery-grid--loading': loading }"
      :style="gridStyle"
      :aria-busy="loading"
    >
      <GalleryCard
        v-for="img in images"
        :key="img.name"
        :img="img"
        :thumb-src="thumbOf(img)"
        :selection-mode="selectionMode"
        :selected="selected.has(img.name)"
        :is-current="currentWallpaperNames.has(img.name)"
        :db-mode="!customDir"
        @activate="onCardClick"
        @detail="openDetail"
        @remove="onDeleteSingle"
      />
    </div>

    <!-- 分页（支持跳页） -->
    <div v-if="totalPages > 1" class="gallery-pager">
      <v-pagination
        v-model="page"
        :length="totalPages"
        :total-visible="7"
        density="compact"
        :disabled="loading"
      />
    </div>

    <!-- 详情抽屉 -->
    <ImageDetailDrawer
      v-model:detail-open="detailOpen"
      :detail="detail"
      :entry="detailEntry"
      :loading="detailLoading"
      :preview-src="detailPreviewSrc"
      @open-viewer="openViewerFor"
      @open-link="onOpenLink"
      @delete="onDeleteSingle"
    />

    <!-- 全屏查看器 -->
    <ImageViewer
      v-if="viewerOpen"
      :images="viewerImages"
      :start-index="viewerIndex"
      @update:index="viewerIndex = $event"
      @close="viewerOpen = false"
    />
  </div>
</template>

<style scoped>
.gallery-view {
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.gallery-view > .gallery-grid,
.gallery-view > .gallery-empty {
  flex: 1;
  overflow-y: auto;
}
.gallery-search {
  max-width: 240px;
}
.gallery-meta {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.gallery-meta__orphan-chip {
  cursor: pointer;
}
.gallery-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(var(--grid-cell-min, 170px), 1fr));
  /* 关键：行高必须由卡片自身撑开。
     卡片带 aspect-ratio，但 Chromium 在给 grid 的 auto 行定高时**不会**采用由
     aspect-ratio 推出的高度，而是用卡片的最小贡献（min-height: 96px）——
     于是行高只有 96px，而卡片实际有 160px，卡片下半部分会被下一行盖住。
     列宽 176px 时只被盖住 14px 不明显，卡片放大到 267px（列宽随窗口缩放后）就盖掉 56px，
     表现为「每一行只露出上半截、只有最后一行完整」。min-content 让行高跟随卡片真实高度。 */
  grid-auto-rows: min-content;
  gap: var(--space-2);
  align-content: start;
  padding-bottom: var(--space-4);
}
/* 翻页加载期间：旧网格降透明 + 禁点，给出明确的加载反馈（此前翻页时网格静止无反馈） */
.gallery-grid--loading {
  opacity: 0.45;
  pointer-events: none;
  transition: opacity 0.15s;
}
/* 当前桌面壁纸：常驻边框 + 一圈克制的辉光，扫一眼就能在图里认出来。
   放在 --selected 之前：多选是眼下更即时的操作态，两者同时命中时让选择态胜出。 */
.gallery-pager {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-2) 0 var(--space-4);
}
/* 注意：`.detail-*` 的样式全部放在 ImageDetailDrawer.vue 里。
 * 它们的作用元素在子组件内部，而这里的 <style scoped> 只会把作用域标记加到本组件模板的
 * 元素和子组件**根节点**上，写在这儿的 .detail-body / .detail-preview 之类一条也不会命中。 */
</style>
