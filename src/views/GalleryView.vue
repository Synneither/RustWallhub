<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { ImageInfo, LocalImageEntry, MonitorInfo, OrphanFile } from "../types";
import {
  adoptOrphanFiles,
  assetUrl,
  browseImageFiles,
  deleteOrphanFile,
  dislikeFile,
  getImageInfo,
  listMonitors,
  listOrphanFiles,
  resolveThumbnails,
  setWallpaper,
  startSlideshow,
  stopSlideshow,
} from "../utils/api";
import { appState, askConfirm, dbReady, toast, toastError } from "../stores/app";
import { formatBytes } from "../utils/format";
import EmptyState from "../components/EmptyState.vue";
import ImageViewer from "../components/ImageViewer.vue";

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
const thumbUrls = reactive<Record<string, string>>({});

let searchTimer: ReturnType<typeof setTimeout> | null = null;
watch(search, (v) => {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    searchDebounced.value = v.trim();
  }, 300);
});

const totalPages = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)));
const orphanCountOnPage = computed(() => images.value.filter((i) => i.is_orphan).length);

function toEntry(o: OrphanFile): LocalImageEntry {
  return { name: o.name, path: o.path, thumb_path: null, size: o.size, is_orphan: true, modified_date: null };
}

async function load() {
  if (!dbReady.value && !customDir.value) return;
  loading.value = true;
  loadError.value = "";
  try {
    if (orphanOnly.value && !customDir.value) {
      const all = (await listOrphanFiles(source.value)).map(toEntry);
      orphanAll.value = all;
      total.value = all.length;
      const start = (page.value - 1) * pageSize.value;
      images.value = all.slice(start, start + pageSize.value);
      await loadThumbs();
    } else {
      const res = await browseImageFiles(source.value, {
        offset: (page.value - 1) * pageSize.value,
        limit: pageSize.value,
        customDir: customDir.value ?? undefined,
        search: searchDebounced.value || undefined,
        sortBy: sortBy.value,
      });
      total.value = res.total;
      images.value = res.images;
      if (customDir.value) {
        // 自定义目录无缩略图管线，直接用原图；同时避免同名文件命中旧缩略图缓存
        for (const img of res.images) thumbUrls[img.name] = assetUrl(img.path);
      } else {
        await loadThumbs();
      }
    }
  } catch (e) {
    loadError.value = String(e);
    images.value = [];
    total.value = 0;
  } finally {
    loading.value = false;
  }
}

async function loadThumbs() {
  const names = images.value.map((i) => i.name);
  if (names.length === 0) return;
  try {
    const dpr = appState.config?.thumbnail_dpr ?? 2;
    const batch = await resolveThumbnails(source.value, names, dpr);
    for (const it of batch.items) {
      thumbUrls[it.name] = assetUrl(it.thumb_path);
    }
  } catch (e) {
    // 缩略图失败不致命，回退原图
    for (const img of images.value) {
      if (!thumbUrls[img.name]) thumbUrls[img.name] = assetUrl(img.path);
    }
  }
}

function thumbOf(img: LocalImageEntry): string {
  return thumbUrls[img.name] ?? assetUrl(img.path);
}

/* 触发重载 */
watch([source, searchDebounced, sortBy, orphanOnly], () => {
  page.value = 1;
  load();
});
watch(customDir, () => {
  // 进入/退出自定义目录时重置易冲突的状态
  orphanOnly.value = false;
  clearSelection();
  page.value = 1;
  load();
});
watch([page, pageSize], () => load());
watch(
  () => appState.galleryEpoch,
  () => load(),
);

onMounted(() => {
  load();
  loadMonitors();
});

/* ════ 多选与批量 ════ */
const selectionMode = ref(false);
const selected = reactive(new Set<string>());

function toggleSelect(name: string) {
  if (selected.has(name)) selected.delete(name);
  else selected.add(name);
}
function clearSelection() {
  selected.clear();
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
  let done = 0;
  try {
    for (const name of names) {
      try {
        if (isOrphanMode) await deleteOrphanFile(source.value, name);
        else await dislikeFile(source.value, name);
        done++;
      } catch (e) {
        toastError(e);
      }
    }
    toast(`已处理 ${done} / ${names.length} 张`, done === names.length ? "success" : "info");
    clearSelection();
    await load();
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

async function openDetail(img: LocalImageEntry) {
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
    } else {
      const res = await browseImageFiles(source.value, {
        offset: 0,
        limit: Math.max(total.value, 1),
        customDir: customDir.value ?? undefined,
        search: searchDebounced.value || undefined,
        sortBy: sortBy.value,
      });
      paths = res.images.map((i) => i.path);
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
      <div v-for="i in 12" :key="i" class="gallery-card shimmer" />
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

    <div v-else class="gallery-grid">
      <div
        v-for="img in images"
        :key="img.name"
        class="gallery-card"
        :class="{ 'gallery-card--selected': selectionMode && selected.has(img.name) }"
        @click="onCardClick(img)"
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
    <v-navigation-drawer v-model="detailOpen" location="right" width="360" temporary class="detail-drawer">
      <div v-if="detailLoading" class="async-state"><v-progress-circular indeterminate color="primary" /></div>
      <div v-else-if="detail" class="detail-body">
        <div class="detail-head">
          <span class="text-heading detail-head__name">{{ detail.name }}</span>
          <v-spacer />
          <v-btn icon="mdi-close" variant="text" size="small" @click="detailOpen = false" />
        </div>
        <div class="detail-preview">
          <img :src="assetUrl(detail.path)" :alt="detail.name" @click="openViewerFor({ name: detail!.name, path: detail!.path, thumb_path: null, size: detail!.size, is_orphan: false, modified_date: null })" />
        </div>
        <div class="detail-rows">
          <div class="detail-row"><span class="stat-label">分辨率</span><span class="text-body">{{ detail.resolution ?? (detail.width && detail.height ? `${detail.width}×${detail.height}` : "-") }}</span></div>
          <div class="detail-row"><span class="stat-label">格式</span><span class="text-body">{{ detail.format ?? "-" }}</span></div>
          <div class="detail-row"><span class="stat-label">大小</span><span class="text-body">{{ formatBytes(detail.size) }}</span></div>
          <div class="detail-row"><span class="stat-label">来源</span><span class="text-body">{{ detail.source ?? "未入库" }}</span></div>
          <div v-if="detail.created_at" class="detail-row"><span class="stat-label">入库时间</span><span class="text-body">{{ detail.created_at.slice(0, 16) }}</span></div>
          <div v-if="detail.title" class="detail-row detail-row--col"><span class="stat-label">标题</span><span class="text-body">{{ detail.title }}</span></div>
          <div v-if="detail.source_url || detail.permalink || detail.download_url" class="detail-row detail-row--col">
            <span class="stat-label">链接</span>
            <div class="detail-links">
              <v-btn v-if="detail.source_url" size="x-small" variant="text" color="primary" @click="onOpenLink(detail.source_url)">来源页面</v-btn>
              <v-btn v-if="detail.permalink" size="x-small" variant="text" color="primary" @click="onOpenLink(detail.permalink)">Reddit 帖子</v-btn>
              <v-btn v-if="detail.download_url" size="x-small" variant="text" color="primary" @click="onOpenLink(detail.download_url)">原图 URL</v-btn>
            </div>
          </div>
        </div>
        <div class="detail-actions">
          <v-select
            v-model="monitorChoice"
            :items="monitorItems"
            label="显示器"
            density="compact"
            hide-details
            class="settings-field"
          />
          <v-btn
            color="primary"
            variant="flat"
            prepend-icon="mdi-monitor"
            :loading="settingWallpaper"
            @click="onSetWallpaper(detail.path, monitorChoice)"
          >
            设为壁纸
          </v-btn>
          <v-btn variant="tonal" color="error" prepend-icon="mdi-delete-outline" @click="onDeleteSingle({ name: detail.name, path: detail.path, thumb_path: null, size: detail.size, is_orphan: !detail.source, modified_date: null })">
            删除
          </v-btn>
        </div>
      </div>
    </v-navigation-drawer>

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
