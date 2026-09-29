<script setup lang="ts">
import { computed, ref, watch, onMounted, onBeforeUnmount } from "vue";
import type { WallhavenImageEntry, WallhavenSearchResult, WallhavenSelected } from "../types";
import {
  downloadWallhavenSelected,
  searchWallhaven,
  startWallhavenDownload,
} from "../utils/api";
import { clearNewImages, toast } from "../stores/app";
import { useAsyncAction } from "../composables/useAsyncAction";
import { useGridDensity, densityItems } from "../composables/useGridDensity";
import { useClickOrDoubleClick, useSelectionMap } from "../composables/useSelection";
import { useWallhavenPreview } from "../composables/useWallhavenPreview";
import { useEffectiveDpr } from "../composables/useEffectiveDpr";
import { useContainerWidth } from "../composables/useContainerWidth";
import { openUrlSafe } from "../utils/openUrl";
import { friendlyError } from "../utils/errors";
import { maxCoveredWidthForPixels } from "../utils/thumbSize";
import ProgressCard from "../components/ProgressCard.vue";
import NewImagesStrip from "../components/NewImagesStrip.vue";
import EmptyState from "../components/EmptyState.vue";
import GridSizeBar from "../components/GridSizeBar.vue";
import ImageViewer from "../components/ImageViewer.vue";
import WallhavenSearchForm from "../components/WallhavenSearchForm.vue";

/* ── 搜索条件 ──
 * 草稿、校验、保存都在 WallhavenSearchForm.vue 内部（搜索条件就是下载配置）。
 * 这里只保留一个句柄：任何下载动作前都要先落盘，否则后端用的还是旧条件。 */
type SearchFormApi = { persist: () => Promise<boolean>; validate: () => Promise<boolean> };
const searchForm = ref<SearchFormApi | null>(null);

/** 把表单里的条件落盘。没有表单或保存失败时返回 false，调用方据此中止。 */
async function persistFilter(): Promise<boolean> {
  return (await searchForm.value?.persist()) ?? false;
}

/* ── 结果网格密度 ──
 * 档位表里的 min 是参考宽度（内容宽 1168px）下的列宽，实际列宽按容器宽度等比缩放，
 * 否则 auto-fill 只会不停加列、卡片尺寸恒定不变（见 useGridDensity 的说明）。
 * 缩略图用的是 Wallhaven 远端 large 档（实测约 500px 宽），前端换不了尺寸，
 * 所以卡片宽度按它能覆盖的最大宽度封顶，到顶后转为增加列数。
 * 偏好存 localStorage（key 见下），不进 config.json。 */

/** Wallhaven 远端 large 档缩略图的实测宽度；无法调整，只能据此封顶。 */
const REMOTE_THUMB_WIDTH = 500;

/** 网格元素：量容器宽度用 */
const gridEl = ref<HTMLElement | null>(null);
/** .view 是这一页真正的滚动容器（不像图库那样由网格自己滚） */
const viewRoot = ref<HTMLElement | null>(null);
/** 结果区 wrapper（工具栏 + 网格 + 翻页条），翻页后把它带回视口顶部 */
const resultsEl = ref<HTMLElement | null>(null);
/** 容器宽度跟踪（ResizeObserver + v-if 换元素自动重挂）已收敛到 useContainerWidth */
const { containerWidth } = useContainerWidth(gridEl);
/** 屏幕像素比（响应变化：拖到别的显示器 / 改系统缩放时要重算上限） */
const deviceDpr = useEffectiveDpr();
const maxCell = computed(() => maxCoveredWidthForPixels(REMOTE_THUMB_WIDTH, deviceDpr.value));

const { density: cellSize, items: SIZE_ITEMS, gridStyle: cellStyle } = useGridDensity(
  "rustwallhub-wallhaven-cell-size",
  densityItems({ compact: "170px", normal: "240px", large: "330px" }),
  "normal",
  // minCellHeight 与 .wh-cell 的 min-height 保持一致
  { containerWidth, maxCell, minCellHeight: 90 },
);

/* ── 搜索 ── */
const searching = ref(false);
const result = ref<WallhavenSearchResult | null>(null);
const searchError = ref("");
let searchSeq = 0;

/* ── 大图预览的滚动缓冲 ──
 * 续接、下载后补位、竞态作废都在 composable 里；这里负责给它「当前页」和「取下一页」。
 * 预览列表 = 当前页 + 已续接的后续页，索引由查看器内部维护并回传（v-model:index），
 * 所以「下载当前图 / 选入下载」始终作用在正在看的那一张上。 */
const {
  index: previewIndex,
  open: previewOpen,
  image: previewImage,
  list: previewList,
  loadingMore,
  openAt: openPreview,
  close: closePreviewRaw,
  advance: advancePreview,
  reset: resetPreview,
} = useWallhavenPreview({
  page: () => result.value,
  // 取下一页：期间用户重新搜索就返回 null 作废本次续接
  loadPage: async (page) => {
    const seq = searchSeq;
    const next = await searchWallhaven(page);
    return seq === searchSeq ? next : null;
  },
  // 续接成功时把网格也切到该页，保持两边一致
  commitPage: (next) => {
    result.value = next;
  },
});

/** 跳页输入框的值：跟随当前页同步，失焦时回填真实页码。 */
const pageInput = ref(1);
watch(
  () => result.value?.page,
  (p) => {
    pageInput.value = p ?? 1;
  },
);

/** 搜索/翻页。
 *  `keepSelection`：翻页时保留已选（跨页挑选后一次下载），换搜索条件时才清空。 */
async function doSearch(page: number, keepSelection = false) {
  const seq = ++searchSeq;
  searching.value = true;
  searchError.value = "";
  try {
    const next = await searchWallhaven(page);
    if (seq !== searchSeq) return;
    result.value = next;
    if (!keepSelection) selected.clear();
    // 换页后旧索引可能越界，收起预览并丢弃续接缓冲
    resetPreview();
  } catch (e) {
    if (seq !== searchSeq) return;
    searchError.value = friendlyError(e);
    result.value = null;
  } finally {
    if (seq === searchSeq) searching.value = false;
  }
}

/** 保存并搜索：失败重试按钮与搜索表单的「保存并搜索」走同一条路径。
 *  先过表单校验（字段下有红字却照样保存、最后只看到后端报错，是之前最容易让人以为
 *  "存进去了"的地方），再落盘，最后才发搜索请求。 */
async function onSaveAndSearch() {
  if (!(await searchForm.value?.validate())) return;
  if (!(await persistFilter())) return;
  await doSearch(1);
}

/** 翻页后把结果区带回视口顶部。
 *  翻页条现在贴在结果区底边，点完「下一页」如果停在原地，看到的是新页的末尾。
 *  先滚再发请求：滚动立刻有反馈，新图回来时人已经停在结果区头部了。 */
function scrollResultsToTop() {
  const view = viewRoot.value;
  const target = resultsEl.value;
  // 防御：搜索失败时结果区会整块卸载，拿到的可能已是摘除的节点
  if (!view || !target || !result.value) return;
  // 用相对 .view 的位移而不是 scrollIntoView：.view 上方还有窗口标题栏，
  // scrollIntoView 会把结果区顶部对齐到**视口**顶部、顶进 .view 的可视区之外。
  const delta = target.getBoundingClientRect().top - view.getBoundingClientRect().top;
  const pad = parseFloat(getComputedStyle(view).paddingTop) || 0;
  view.scrollTo({ top: Math.max(0, view.scrollTop + delta - pad), behavior: "smooth" });
}

/** 跳页：只有左右箭头时，从第 1 页到第 20 页要点 19 次。 */
async function onJumpPage(target: number) {
  const cur = result.value;
  if (!cur) return;
  const p = Math.min(Math.max(1, Math.round(target || 1)), cur.total_pages);
  if (p === cur.page) return;
  scrollResultsToTop();
  await doSearch(p, true);
}

async function onPage(delta: number) {
  if (!result.value) return;
  const next = result.value.page + delta;
  if (next < 1 || next > result.value.total_pages) return;
  scrollResultsToTop();
  await doSearch(next, true);
}

/* ── 勾选与下载 ──
 * 选中项存 `Map<id, entry>` 而不是 `Set<id>`：翻页后要保留已选，而下载 payload 需要
 * path / resolution / short_url —— 只有 id 是拼不出来的（用 Set 就只能清空重来）。
 * Map 与 Set 共享 has/size/delete/clear，所以模板里的用法不用改。 */
const selection = useSelectionMap<string, WallhavenImageEntry>();
const selected = selection.selected;

/** 模板与快捷键都按「一张图」调用，键从图上取。 */
function toggleSelect(img: WallhavenImageEntry) {
  selection.toggle(img.id, img);
}

/** 从 "2560x1440" 解析宽高比，供网格单元格按需定高（竖屏图不再被 16:10 裁切） */
function ratioOf(resolution: string): string {
  const m = /^(\d+)x(\d+)$/.exec(resolution);
  if (!m) return "16 / 10";
  const w = Number(m[1]);
  const h = Number(m[2]);
  return w > 0 && h > 0 ? `${w} / ${h}` : "16 / 10";
}

/** 单击选择 / 双击预览：消歧逻辑（延迟判定、记住上一张卡片、卸载时清计时器）在 composable 里。
 *  `openPreview` 是函数声明，已提升，所以这里可以先引用后定义。 */
const { handle: onCellClick } = useClickOrDoubleClick<WallhavenImageEntry>({
  onSingle: toggleSelect,
  onDouble: openPreview,
});

const allPageSelected = computed(() => {
  const imgs = result.value?.images ?? [];
  return imgs.length > 0 && imgs.every((i) => selected.has(i.id));
});

function toggleSelectAll() {
  const imgs = result.value?.images ?? [];
  if (allPageSelected.value) {
    imgs.forEach((i) => selected.delete(i.id));
  } else {
    // 存 entry 而不是 id：翻页后仍要能拿到 path/resolution 拼下载 payload
    selection.setAll(imgs.map((i) => [i.id, i] as const));
  }
}

const { run: onDownloadSelected, loading: startingSelected } = useAsyncAction(async () => {
  if (selected.size === 0) return;
  if (!(await persistFilter())) return;
  // 直接取选中集里的 entry：这样才能下载跨页勾选的图（以前只过滤当前页，
  // 翻页后勾选被清空，跨页挑选根本无法完成）。
  const payload: WallhavenSelected[] = [...selected.values()].map((i) => ({
    id: i.id,
    path: i.path,
    resolution: i.resolution,
    short_url: i.short_url,
  }));
  const msg = await downloadWallhavenSelected(payload);
  clearNewImages("wallhaven");
  toast(msg, "info");
  selected.clear();
});

const { run: onBatchDownload, loading: startingBatch } = useAsyncAction(async () => {
  if (!(await persistFilter())) return;
  const msg = await startWallhavenDownload();
  clearNewImages("wallhaven");
  toast(msg, "info");
});

/** 关闭预览。下载中不允许关：那会让下载完成的回调找不到要前进的目标。 */
function closePreview() {
  if (previewDownloading.value) return;
  closePreviewRaw();
}

const { run: onDownloadPreview, loading: previewDownloading } = useAsyncAction(async () => {
  const img = previewImage.value;
  if (!img) return;
  if (!(await persistFilter())) return;
  const payload: WallhavenSelected[] = [
    {
      id: img.id,
      path: img.path,
      resolution: img.resolution,
      short_url: img.short_url,
    },
  ];
  const msg = await downloadWallhavenSelected(payload);
  clearNewImages("wallhaven");
  toast(msg, "info");
  advancePreview();
});

async function onOpenSource() {
  if (!previewImage.value) return;
  await openUrlSafe(previewImage.value.short_url);
}

/** 预览中按空格把当前图加入/移出下载选择 */
function onPreviewKey(e: KeyboardEvent) {
  if (!previewOpen.value) return;
  if (e.key !== " " && e.code !== "Space") return;
  const img = previewImage.value;
  if (!img) return;
  e.preventDefault();
  toggleSelect(img);
}
onMounted(() => {
  window.addEventListener("keydown", onPreviewKey);
  // 容器宽度的首次测量与 ResizeObserver 挂载由 useContainerWidth 的 onMounted 完成
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onPreviewKey);
  // ResizeObserver 的断开由 useContainerWidth 的 onBeforeUnmount 完成；
  // 单击/双击判定的 250ms 计时器由 useClickOrDoubleClick 的 onBeforeUnmount 清掉。
});
</script>

<template>
  <div ref="viewRoot" class="view wh-view">
    <div class="view-header">
      <span class="view-header__title">Wallhaven</span>
      <span class="view-header__sub">搜索条件即下载配置，保存后生效</span>
    </div>

    <!-- 搜索条件 -->
    <!-- 搜索条件：草稿 / 校验 / 保存都在组件内部，父视图通过 ref 调 validate/persist -->
    <WallhavenSearchForm
      ref="searchForm"
      :searching="searching"
      @save-and-search="onSaveAndSearch"
    />

    <!-- 下载进度 -->
    <ProgressCard source="wallhaven" title="Wallhaven 下载" />

    <!-- 搜索结果 -->
    <div v-if="searching && !result" ref="gridEl" class="wh-grid">
      <div v-for="i in 12" :key="i" class="wh-cell shimmer" />
    </div>

    <EmptyState
      v-else-if="searchError"
      error
      icon="mdi-cloud-alert-outline"
      title="搜索失败"
      :desc="searchError"
    >
      <v-btn variant="tonal" @click="onSaveAndSearch">重试</v-btn>
    </EmptyState>

    <template v-else-if="result">
      <!-- 结果区整体包一层：底部翻页条用 sticky 贴住它的底边，sticky 的包含块就是这一层，
           所以滚出结果区后它会跟着一起离开视口，不会浮在搜索条件卡上面。 -->
      <div ref="resultsEl" class="wh-results">
        <div class="wh-toolbar">
          <span class="text-caption">
            共 {{ result.total }} 张
            <template v-if="selected.size > 0"> · 已选 {{ selected.size }}</template>
          </span>
          <v-spacer />
          <GridSizeBar v-model="cellSize" :items="SIZE_ITEMS" />
          <v-btn size="small" variant="text" @click="toggleSelectAll">
            {{ allPageSelected ? "取消全选" : "全选本页" }}
          </v-btn>
          <v-btn
            size="small"
            variant="tonal"
            :disabled="selected.size === 0"
            :loading="startingSelected"
            @click="onDownloadSelected"
          >
            下载选中（{{ selected.size }}）
          </v-btn>
          <v-btn
            size="small"
            color="primary"
            variant="flat"
            :loading="startingBatch"
            @click="onBatchDownload"
          >
            按条件批量下载
          </v-btn>
        </div>

        <div
          ref="gridEl"
          class="wh-grid"
          :class="{ 'wh-grid--loading': searching && !!result }"
          :style="cellStyle"
        >
          <div
            v-for="img in result.images"
            :key="img.id"
            class="wh-cell wh-cell--clickable"
            :class="{ 'wh-cell--selected': selected.has(img.id) }"
            :style="{ aspectRatio: ratioOf(img.resolution) }"
            title="单击选择，双击预览大图"
            role="button"
            tabindex="0"
            :aria-label="img.id"
            :aria-pressed="selected.has(img.id)"
            @click="onCellClick(img)"
            @keydown.enter.prevent="toggleSelect(img)"
            @keydown.space.prevent="toggleSelect(img)"
          >
            <img :src="img.thumbnail_url" :alt="img.id" loading="lazy" decoding="async" />
            <button class="wh-cell__preview" title="预览大图" @click.stop="openPreview(img)">
              <v-icon icon="mdi-eye-outline" size="18" />
            </button>
            <span class="wh-cell__res">{{ img.resolution }}</span>
            <span class="wh-cell__check">
              <v-icon
                :icon="selected.has(img.id) ? 'mdi-checkbox-marked-circle' : 'mdi-checkbox-blank-circle-outline'"
                size="20"
                :color="selected.has(img.id) ? 'primary' : 'white'"
              />
            </span>
          </div>
        </div>

        <EmptyState
          v-if="result.images.length === 0"
          icon="mdi-image-search-outline"
          title="没有符合条件的图片"
          desc="试试放宽分辨率或调整关键词"
        />

        <!-- 翻页条：贴在结果区底边，往下滚网格时也一直点得到。
             原来它挂在工具栏里跟着页面一起滚走，翻页得先滚回顶部。 -->
        <div class="wh-pager">
          <v-btn
            size="small"
            variant="text"
            icon="mdi-chevron-left"
            :disabled="result.page <= 1 || searching"
            @click="onPage(-1)"
          />
          <!-- 跳页：只有左右箭头时，从第 1 页到第 20 页要点 19 次 -->
          <span class="wh-page">
            <input
              v-model.number="pageInput"
              class="wh-page__input"
              type="number"
              min="1"
              :max="result.total_pages"
              :aria-label="'页码，共 ' + result.total_pages + ' 页'"
              @keydown.enter.prevent="onJumpPage(pageInput)"
              @blur="pageInput = result.page"
            />
            <span class="text-caption">/ {{ result.total_pages }}</span>
          </span>
          <v-btn
            size="small"
            variant="text"
            icon="mdi-chevron-right"
            :disabled="result.page >= result.total_pages || searching"
            @click="onPage(1)"
          />
        </div>
      </div>
    </template>

    <EmptyState
      v-else
      icon="mdi-image-search-outline"
      title="设置条件后开始搜索"
      desc="搜索结果可勾选下载，也可按条件批量下载到本地"
    />

    <NewImagesStrip source="wallhaven" />

    <!-- 大图预览：全屏，← → 翻页，空格选入下载 -->
    <ImageViewer
      v-if="previewOpen"
      :images="previewList"
      :start-index="previewIndex"
      @update:index="previewIndex = $event"
      @close="closePreview"
    >
      <template #topbar>
        <span style="color: rgba(255, 255, 255, 0.65)" class="text-caption">
          {{ previewImage?.resolution }}
        </span>
      </template>

      <template #actions>
        <span v-if="loadingMore" class="wh-loading-more">
          <v-progress-circular indeterminate size="14" width="2" color="white" />
          正在加载下一页…
        </span>
        <span v-else style="color: rgba(255, 255, 255, 0.65)" class="text-caption">
          ← → 切换 · 空格选入下载 · Esc 关闭
        </span>
        <v-spacer />
        <v-btn
          v-if="previewImage"
          size="small"
          variant="text"
          color="white"
          :prepend-icon="selected.has(previewImage.id) ? 'mdi-check' : 'mdi-plus'"
          @click="toggleSelect(previewImage)"
        >
          {{ selected.has(previewImage.id) ? "已选入" : "选入下载" }}
        </v-btn>
        <v-btn
          size="small"
          variant="text"
          color="white"
          prepend-icon="mdi-open-in-new"
          @click="onOpenSource"
        >
          来源
        </v-btn>
        <v-btn
          size="small"
          color="primary"
          variant="flat"
          prepend-icon="mdi-download-outline"
          :loading="previewDownloading"
          @click="onDownloadPreview"
        >
          下载大图
        </v-btn>
      </template>
    </ImageViewer>
  </div>
</template>

<style scoped>
/* 翻页条贴在视口底边时，.view 的下内边距会在它下方再露出一条内容（实测卡片会从条
   下面探出来 40px）。设置页对同一个问题也是把根元素的 padding-bottom 归零，
   这里照做，被去掉的留白补给页面最后一个元素（本次新图条）。 */
.wh-view {
  padding-bottom: 0;
}
.wh-view > .new-strip {
  margin-bottom: var(--space-10);
}

/* 结果区：工具栏 + 网格 + 翻页条。三者原本是 .view 的直接子项、靠 .view 的 gap 隔开，
   包进 wrapper 后要自己带上同样的间距。flex:none 是必需的：.view 是确定高度的纵向
   flex 容器，内容高于视口时子项默认会被压缩（见 style.css 里 .view 的注释）。 */
.wh-results {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
  flex: none;
}
.wh-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}
/* 翻页条：贴住结果区底边，和 .settings-save-bar 用同一套贴底做法
   （sticky + --surface-deep + 上边框），往下滚网格时始终点得到。
   负外边距把底色铺满 .view 的左右留白，底下的卡片才不会从两侧露出来；
   sticky 的包含块是 .wh-results，滚出结果区后它就跟着一起走，
   不会一直浮在搜索条件卡上面。 */
.wh-pager {
  position: sticky;
  bottom: 0;
  z-index: 5;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  margin: 0 calc(-1 * var(--space-8));
  padding: var(--space-2) var(--space-8);
  background: var(--surface-deep);
  border-top: 1px solid var(--border-subtle);
}
/* 跳页输入框 */
.wh-page {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  color: var(--text-secondary);
}
.wh-page__input {
  width: 56px;
  padding: 2px var(--space-2);
  border: var(--border-card);
  border-radius: var(--radius-sm);
  background: var(--surface-elevated);
  color: var(--text-primary);
  font: inherit;
  font-size: 0.8125rem;
  text-align: center;
}
.wh-page__input:focus-visible {
  outline: 2px solid var(--accent-primary);
  outline-offset: 1px;
}
.wh-loading-more {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: 0.75rem;
  color: rgba(255, 255, 255, 0.85);
}
.wh-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(var(--grid-cell-min, 240px), 1fr));
  /* 同 .gallery-grid：不给 grid-auto-rows: min-content 的话，auto 行高会按卡片的
     最小贡献（min-height: 90px）算，而不是 aspect-ratio 推出的真实高度，
     卡片就会被下一行盖住。 */
  grid-auto-rows: min-content;
  gap: var(--space-3);
  align-items: start;
}
/* 翻页加载中：旧网格降透明 + 禁点，给出明确反馈。
 * 之前只在"还没有任何结果"时显示骨架，翻页时界面纹丝不动，只能靠箭头变灰去猜。 */
.wh-grid--loading {
  opacity: 0.45;
  pointer-events: none;
  transition: opacity 0.15s;
}
.wh-cell {
  position: relative;
  aspect-ratio: 16 / 10; /* 默认值，实际由内联 style 按真实分辨率覆盖 */
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--surface-elevated);
  border: 2px solid transparent;
  min-height: 90px;
  /* 一页可达 48 张卡片，屏外卡片的布局/绘制是纯浪费。
     content-visibility: auto 让浏览器跳过它们；contain-intrinsic-size 提供占位尺寸防止滚动条跳动。 */
  content-visibility: auto;
  contain-intrinsic-size: auto var(--grid-cell-ph, 155px);
}
.wh-cell--clickable {
  cursor: pointer;
  transition: border-color 0.15s, transform 0.15s;
}
.wh-cell--clickable:hover {
  transform: translateY(-1px);
}
.wh-cell--selected {
  border-color: var(--accent-primary);
}
.wh-cell img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.wh-cell__res {
  position: absolute;
  left: 6px;
  bottom: 6px;
  padding: 1px 7px;
  border-radius: var(--radius-full);
  font-size: 0.625rem;
  background: rgba(0, 0, 0, 0.6);
  color: #fff;
}
.wh-cell__check {
  position: absolute;
  right: 6px;
  top: 6px;
  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.6));
}
.wh-cell__preview {
  position: absolute;
  right: 6px;
  bottom: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: var(--radius-full);
  background: rgba(0, 0, 0, 0.58);
  color: #fff;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s, background 0.15s;
}
.wh-cell:hover .wh-cell__preview,
.wh-cell__preview:focus-visible {
  opacity: 1;
}
.wh-cell__preview:hover {
  background: var(--accent-primary);
}
</style>
