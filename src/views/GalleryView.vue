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
import { openUrl } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { ImageInfo, LocalImageEntry, MonitorInfo, OrphanFile } from "../types";
import {
  adoptOrphanFiles,
  assetUrl,
  browseImageFiles,
  deleteOrphanFile,
  deleteOrphanFiles,
  dislikeFile,
  dislikeFiles,
  getImageInfo,
  listFilteredImagePaths,
  listMonitors,
  listOrphanFiles,
  resolveThumbnails,
  setWallpaper,
  startSlideshow,
  stopSlideshow,
} from "../utils/api";
import { appState, askConfirm, dbReady, toast, toastError } from "../stores/app";
import EmptyState from "../components/EmptyState.vue";
import ImageViewer from "../components/ImageViewer.vue";
import ImageDetailDrawer from "../components/ImageDetailDrawer.vue";
import { useSelection } from "../composables/useSelection";

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
 * 等依赖源目录与数据库的能力不可用，仅保留浏览、设为壁纸与轮播。 */
const customDir = ref<string | null>(null);
const customDirName = computed(() => customDir.value?.split(/[\\/]/).pop() ?? "");

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

const loading = ref(false);
const loadError = ref("");
const images = ref<LocalImageEntry[]>([]);
const total = ref(0);
/** 孤儿模式：全量孤儿列表，前端分页 */
const orphanAll = ref<LocalImageEntry[]>([]);
/** 缩略图 URL 缓存（文件名 → asset URL），上限见 THUMB_CACHE_MAX。
 * 用 shallowRef + Map：整批写入只替换一次引用、触发一次响应式更新。
 * 此前是 reactive<Record<string,string>>（600 个键会被深度代理），
 * 切换来源时逐个 delete 会触发 600 次独立更新。 */
const thumbUrls = shallowRef<Map<string, string>>(new Map());
const THUMB_CACHE_MAX = 600;

/** 批量写入缩略图 URL：克隆一次、整体替换引用，只触发一次响应式更新。 */
function cacheThumbs(entries: Iterable<readonly [string, string]>) {
  const next = new Map(thumbUrls.value);
  for (const [name, url] of entries) next.set(name, url);
  // Map 保持插入顺序，超限时从最早插入的条目开始丢弃
  while (next.size > THUMB_CACHE_MAX) {
    const oldest = next.keys().next().value;
    if (oldest === undefined) break;
    next.delete(oldest);
  }
  thumbUrls.value = next;
}

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

function scheduleLoad() {
  if (reloadTimer) clearTimeout(reloadTimer);
  reloadTimer = setTimeout(() => {
    reloadTimer = null;
    if (viewActive) void load();
  }, 0);
}

watch(source, () => {
  // 切换来源后旧缩略图 URL 不能跨源复用；整体替换引用，只触发一次更新
  thumbUrls.value = new Map();
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
        cacheThumbs(res.images.map((img) => [img.name, assetUrl(img.path)] as const));
      } else {
        await loadThumbs(seq);
      }
    }
  } catch (e) {
    if (seq === loadSeq) {
      loadError.value = String(e);
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
  try {
    const dpr = appState.config?.thumbnail_dpr ?? 2;
    const batch = await resolveThumbnails(source.value, names, dpr);
    if (seq !== loadSeq) return;
    cacheThumbs(batch.items.map((it) => [it.name, assetUrl(it.thumb_path)] as const));
  } catch (e) {
    if (seq !== loadSeq) return;
    // 缩略图失败不致命，回退原图
    cacheThumbs(images.value.map((img) => [img.name, assetUrl(img.path)] as const));
  }
}

function thumbOf(img: LocalImageEntry): string {
  return thumbUrls.value.get(img.name) ?? assetUrl(img.path);
}

/* 触发重载 */
watch([source, searchDebounced, sortBy, orphanOnly], () => {
  page.value = 1;
  scheduleLoad();
});
watch(customDir, () => {
  // 进入/退出自定义目录时重置易冲突的状态
  orphanOnly.value = false;
  clearSelection();
  page.value = 1;
  scheduleLoad();
});
watch([page, pageSize], () => scheduleLoad());
watch(
  () => appState.galleryEpoch,
  () => {
    if (viewActive) scheduleLoad();
  },
);

onMounted(() => {
  viewActive = true;
  loadMonitors();
});
onActivated(() => {
  viewActive = true;
  scheduleLoad();
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
      ? `将永久删除 ${names.length} 个孤儿文件及其缩略图，无法恢复。`
      : `将把 ${names.length} 张图片标记为不喜欢并删除本地文件及缩略图。`,
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

/* ════ 查看器 ════ */
const viewerOpen = ref(false);
const viewerIndex = ref(0);

const viewerImages = computed(() => images.value.map((i) => ({ name: i.name, path: i.path })));

function openViewerFor(img: LocalImageEntry) {
  const idx = images.value.findIndex((i) => i.name === img.name);
  viewerIndex.value = Math.max(0, idx);
  viewerOpen.value = true;
}

/* ════ 详情 ════ */
const detailOpen = ref(false);
const detailLoading = ref(false);
const detail = ref<ImageInfo | null>(null);
/** 触发详情时的原始条目，详情抽屉的预览/删除直接用它，避免用 detail 字段手工拼 entry */
const detailEntry = ref<LocalImageEntry | null>(null);

async function openDetail(img: LocalImageEntry) {
  detailEntry.value = img;
  detailOpen.value = true;
  detailLoading.value = true;
  detail.value = null;
  try {
    detail.value = await getImageInfo(source.value, img.name);
  } catch (e) {
    toastError(e);
    detailOpen.value = false;
  } finally {
    detailLoading.value = false;
  }
}

async function onOpenLink(url: string | null) {
  if (!url) return;
  try {
    await openUrl(url);
  } catch (e) {
    toastError(e);
  }
}

/* ════ 壁纸 ════ */
const monitors = ref<MonitorInfo[]>([]);
const monitorChoice = ref<string>("all");
const settingWallpaper = ref(false);

async function loadMonitors() {
  try {
    monitors.value = await listMonitors();
  } catch {
    monitors.value = [];
  }
}

const monitorItems = computed(() => [
  { title: "全部显示器", value: "all" },
  ...monitors.value.map((m) => ({
    title: `${m.name}${m.is_primary ? "（主）" : ""} · ${m.width}×${m.height}`,
    value: m.id,
  })),
]);

async function onSetWallpaper(path: string, monitor?: string) {
  if (settingWallpaper.value) return;
  settingWallpaper.value = true;
  try {
    const msg = await setWallpaper(path, monitor && monitor !== "all" ? monitor : undefined);
    toast(msg, "success");
  } catch (e) {
    toastError(e);
  } finally {
    settingWallpaper.value = false;
  }
}

/* ════ 删除（单张） ════ */
async function onDeleteSingle(img: LocalImageEntry) {
  const isOrphan = img.is_orphan;
  const ok = await askConfirm(
    isOrphan ? "删除孤儿文件" : "删除图片",
    isOrphan
      ? `将永久删除「${img.name}」及其缩略图，无法恢复。`
      : `将把「${img.name}」标记为不喜欢并删除本地文件及缩略图。`,
    { danger: true, confirmText: "删除" },
  );
  if (!ok) return;
  try {
    if (isOrphan) await deleteOrphanFile(source.value, img.name);
    else await dislikeFile(source.value, img.name);
    toast("已删除", "success");
    detailOpen.value = false;
    await load();
  } catch (e) {
    toastError(e);
  }
}

/* ════ 轮播 ════ */
const slideshowInterval = ref(60);
const startingSlideshow = ref(false);

const slideshow = computed(() => appState.slideshow);

async function onStartSlideshow() {
  if (startingSlideshow.value) return;
  if (slideshowInterval.value < 5) {
    toast("轮播间隔不能小于 5 秒", "error");
    return;
  }
  startingSlideshow.value = true;
  try {
    // 取当前筛选（含搜索词）的全量图片
    let paths: string[];
    if (orphanOnly.value && !customDir.value) {
      paths = orphanAll.value.map((i) => i.path);
    } else if (customDir.value) {
      // 自定义目录没有后端数据库管线，仍走 browse 全量扫描。
      const res = await browseImageFiles(source.value, {
        offset: 0,
        limit: Math.max(total.value, 1),
        customDir: customDir.value,
        search: searchDebounced.value || undefined,
        sortBy: sortBy.value,
      });
      paths = res.images.map((i) => i.path);
    } else {
      // 正常目录使用轻量路径列表命令，避免序列化整页元数据。
      paths = await listFilteredImagePaths(
        source.value,
        searchDebounced.value || undefined,
        sortBy.value,
      );
    }
    if (paths.length === 0) {
      toast("当前筛选没有图片", "info");
      return;
    }
    await startSlideshow(paths, slideshowInterval.value);
    appState.slideshow.running = true;
    toast(`轮播已启动：${paths.length} 张，每 ${slideshowInterval.value} 秒切换`, "success");
  } catch (e) {
    toastError(e);
  } finally {
    startingSlideshow.value = false;
  }
}

async function onStopSlideshow() {
  try {
    await stopSlideshow();
    appState.slideshow.running = false;
    appState.slideshow.current = null;
    toast("轮播已停止", "info");
  } catch (e) {
    toastError(e);
  }
}
</script>

<template>
  <div class="view gallery-view">
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

    <!-- 轮播控制条 -->
    <div class="panel-card slideshow-bar animate-in">
      <v-icon icon="mdi-play-circle-outline" size="18" color="primary" />
      <template v-if="!slideshow.running">
        <span class="text-body">壁纸轮播</span>
        <v-text-field
          v-model.number="slideshowInterval"
          type="number"
          suffix="秒"
          density="compact"
          hide-details
          class="settings-field slideshow-bar__interval"
        />
        <v-btn size="small" color="primary" variant="flat" :loading="startingSlideshow" @click="onStartSlideshow">
          用当前筛选启动（{{ total }} 张）
        </v-btn>
      </template>
      <template v-else>
        <span class="text-body">轮播中</span>
        <span class="text-caption slideshow-bar__tick">
          <template v-if="slideshow.current">
            {{ slideshow.current.index + 1 }} / {{ slideshow.current.total }} · {{ slideshow.current.name }}
          </template>
          <template v-else>等待切换…</template>
        </span>
        <v-btn size="small" variant="tonal" color="error" @click="onStopSlideshow">停止轮播</v-btn>
      </template>
    </div>

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
    <div v-if="selectionMode && selected.size > 0" class="gallery-batch animate-in">
      <span class="text-body">已选 {{ selected.size }} 项</span>
      <v-spacer />
      <v-btn
        v-if="orphanOnly"
        size="small"
        variant="tonal"
        :loading="batchRunning"
        @click="onBatchAdopt"
      >
        收养入库
      </v-btn>
      <v-btn size="small" variant="tonal" color="error" :loading="batchRunning" @click="onBatchDelete">
        删除
      </v-btn>
    </div>

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
    <div v-else-if="loading && images.length === 0" class="gallery-grid">
      <div v-for="i in pageSize" :key="i" class="gallery-card shimmer" />
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

    <div v-else class="gallery-grid" :aria-busy="loading" :class="{ 'gallery-grid--loading': loading }">
      <div
        v-for="img in images"
        :key="img.name"
        class="gallery-card"
        :class="{ 'gallery-card--selected': selectionMode && selected.has(img.name) }"
        role="button"
        tabindex="0"
        :aria-label="img.name"
        :aria-pressed="selectionMode ? selected.has(img.name) : null"
        @click="onCardClick(img)"
        @keydown.enter.prevent="onCardClick(img)"
        @keydown.space.prevent="onCardClick(img)"
      >
        <img :src="thumbOf(img)" :alt="img.name" loading="lazy" />
        <span v-if="img.is_orphan && !customDir" class="gallery-card__orphan">孤儿</span>

        <!-- 选择态角标 -->
        <span v-if="selectionMode && !customDir" class="gallery-card__check">
          <v-icon
            :icon="selected.has(img.name) ? 'mdi-checkbox-marked-circle' : 'mdi-checkbox-blank-circle-outline'"
            size="20"
            :color="selected.has(img.name) ? 'primary' : 'white'"
          />
        </span>

        <!-- hover 操作 -->
        <div v-if="!selectionMode" class="gallery-card__overlay" @click.stop>
          <v-btn icon="mdi-monitor" size="x-small" variant="flat" class="overlay-btn" title="设为壁纸" @click="onSetWallpaper(img.path)" />
          <template v-if="!customDir">
            <v-btn icon="mdi-information-outline" size="x-small" variant="flat" class="overlay-btn" title="详情" @click="openDetail(img)" />
            <v-btn icon="mdi-delete-outline" size="x-small" variant="flat" class="overlay-btn overlay-btn--danger" title="删除" @click="onDeleteSingle(img)" />
          </template>
        </div>
      </div>
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
      :monitor-items="monitorItems"
      v-model:monitor="monitorChoice"
      :setting-wallpaper="settingWallpaper"
      @open-viewer="openViewerFor"
      @open-link="onOpenLink"
      @set-wallpaper="(path, monitor) => onSetWallpaper(path, monitor)"
      @delete="onDeleteSingle"
    />

    <!-- 全屏查看器 -->
    <ImageViewer
      v-if="viewerOpen"
      :images="viewerImages"
      :start-index="viewerIndex"
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
.gallery-batch {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-4);
  border-radius: var(--radius-md);
  background: var(--accent-primary-dim);
  border: var(--border-active);
}
.gallery-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
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
.gallery-card {
  position: relative;
  aspect-ratio: 16 / 10;
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--surface-elevated);
  border: 2px solid transparent;
  cursor: pointer;
  min-height: 96px;
}
.gallery-card img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  transition: transform 0.2s var(--ease-out);
}
.gallery-card:hover img {
  transform: scale(1.03);
}
.gallery-card--selected {
  border-color: var(--accent-primary);
}
.gallery-card__orphan {
  position: absolute;
  left: 6px;
  top: 6px;
  padding: 1px 7px;
  border-radius: var(--radius-full);
  font-size: 0.625rem;
  background: color-mix(in srgb, var(--accent-reddit) 85%, black);
  color: #fff;
}
.gallery-card__check {
  position: absolute;
  right: 6px;
  top: 6px;
  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.6));
}
.gallery-card__overlay {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  justify-content: center;
  gap: 6px;
  padding: 18px 6px 8px;
  background: linear-gradient(to top, rgba(0, 0, 0, 0.72), transparent);
  opacity: 0;
  transition: opacity 0.15s;
}
.gallery-card:hover .gallery-card__overlay {
  opacity: 1;
}
.overlay-btn {
  background: rgba(30, 30, 34, 0.9) !important;
  color: #fff !important;
}
.overlay-btn--danger {
  color: var(--accent-error) !important;
}
.gallery-pager {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-2) 0 var(--space-4);
}
.slideshow-bar {
  flex-direction: row;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
}
.slideshow-bar__interval {
  max-width: 110px;
}
.slideshow-bar__tick {
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}
.detail-drawer {
  background: var(--surface-card) !important;
  border-left: 1px solid var(--border-subtle);
}
.detail-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-4);
  height: 100%;
  overflow-y: auto;
}
.detail-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.detail-head__name {
  font-size: 0.9375rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.detail-preview {
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--preview-bg);
  cursor: zoom-in;
}
.detail-preview img {
  width: 100%;
  display: block;
  object-fit: contain;
  max-height: 240px;
}
.detail-rows {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.detail-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}
.detail-row--col {
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}
.detail-links {
  display: flex;
  gap: var(--space-1);
  flex-wrap: wrap;
}
.detail-actions {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: auto;
}
</style>
