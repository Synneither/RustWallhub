<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, nextTick, watch } from "vue";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { logger } from "../utils/logger";

interface LocalImage {
  name: string;
  path: string;
  thumb_path?: string;
  size: number;
  is_orphan?: boolean;
}

interface ListResult {
  images: LocalImage[];
  total: number;
}

interface AppConfig {
  wallhaven_save_dir: string;
  reddit_save_dir: string;
}

interface ThumbnailResult {
  name: string;
  thumb_path: string;
}

const source = ref("wallhaven");
const allImages = ref<LocalImage[]>([]);
const total = ref(0);
const loading = ref(false);
const pageLoading = ref(false);
const selectedIndex = ref(-1);
const saveDir = ref("");
const thumbLoading = ref(false);
const settingWallpaper = ref(false);
const imagesPerPage = ref(12);
const currentPage = ref(1);
const localSnackbar = ref(false);
const localSnackbarText = ref("");
const deletingIndex = ref(-1);
const confirmDelete = ref(false);
const pendingDeleteIndex = ref(-1);
const deletingSelection = ref(false);
const selectedPaths = ref<Set<string>>(new Set());
const selectionMode = ref(false);
const pendingIsOrphan = computed(() => {
  const img = allImages.value[pendingDeleteIndex.value];
  return img?.is_orphan ?? false;
});

const selectedOrphanCount = computed(() => {
  let count = 0;
  for (const name of selectedPaths.value) {
    if (allImages.value.find((i) => i.name === name)?.is_orphan) count++;
  }
  return count;
});
const showOrphansOnly = ref(false);
const dpr = Math.ceil(window.devicePixelRatio || 1);
const cleaningThumbnails = ref(false);
const addingOrphanName = ref("");
const currentWallpaper = ref("");
const galleryToolbar = ref<HTMLElement | null>(null);
const customDir = ref("");
const useCustomDir = ref(false);

const totalPages = computed(() => Math.max(1, Math.ceil(displayImages.value.length / imagesPerPage.value)));

const orphanCount = computed(() =>
  allImages.value.filter((img) => img.is_orphan).length
);

const displayImages = computed(() => {
  if (showOrphansOnly.value) {
    return allImages.value.filter((img) => img.is_orphan);
  }
  return allImages.value;
});

const visibleImages = computed(() => {
  const list = displayImages.value;
  const start = (currentPage.value - 1) * imagesPerPage.value;
  return list.slice(start, start + imagesPerPage.value);
});

const dialogOpen = computed({
  get: () => selectedIndex.value >= 0,
  set: (v: boolean) => {
    if (!v) selectedIndex.value = -1;
  },
});

const selectedImage = computed(() =>
  selectedIndex.value >= 0 && selectedIndex.value < allImages.value.length
    ? allImages.value[selectedIndex.value]
    : null
);

function getAssetUrl(path: string): string {
  if (!path) return '';
  try {
    return convertFileSrc(path);
  } catch {
    return '';
  }
}

function getImageUrl(img: LocalImage): string {
  return getAssetUrl(img.thumb_path || img.path);
}

const imgLoaded = ref<Set<string>>(new Set());

function onImgLoad(img: LocalImage) {
  imgLoaded.value.add(img.name);
}

function onImgError(event: Event, img: LocalImage) {
  const el = event.target as HTMLImageElement;
  // 缩略图失败 → 回退到原图
  if (img.thumb_path && img.path) {
    const url = getAssetUrl(img.path);
    if (url) {
      el.src = url;
      return;
    }
  }
  // 全部失败 → 标记已加载（显示空白而非转圈）
  imgLoaded.value.add(img.name);
}

function getPreviewUrl(img: LocalImage): string {
  try {
    return convertFileSrc(img.path);
  } catch {
    return '';
  }
}

const resizeState = {
  itemWidth: 220,
  itemHeight: 124,
  horizontalGap: 12,
  verticalGap: 12,
  toolbarBottomMargin: 16,
  contentPaddingTop: 12,
  contentPaddingBottom: 120,
};

function updateImagesPerPage() {
  const width = window.innerWidth;
  const height = window.innerHeight;
  const topHeight = galleryToolbar.value?.offsetHeight ?? 0;
  const availableWidth = Math.max(0, width - 32);
  const cols = Math.max(
    1,
    Math.floor((availableWidth + resizeState.horizontalGap) / (resizeState.itemWidth + resizeState.horizontalGap))
  );
  const availableHeight = Math.max(
    0,
    height - topHeight - resizeState.toolbarBottomMargin - resizeState.contentPaddingTop - resizeState.contentPaddingBottom
  );
  const rows = Math.max(
    1,
    Math.floor((availableHeight + resizeState.verticalGap) / (resizeState.itemHeight + resizeState.verticalGap))
  );
  const minRows = 4;
  imagesPerPage.value = Math.max(1, cols * Math.max(rows, minRows));
}

function handleResize() {
  updateImagesPerPage();
  if (currentPage.value > totalPages.value) {
    currentPage.value = totalPages.value;
  }
}

watch(imagesPerPage, () => {
  if (currentPage.value > totalPages.value) {
    currentPage.value = totalPages.value;
  }
});

async function loadImages() {
  if (loading.value) return;
  loading.value = true;
  try {
    const result = await invoke<ListResult>("browse_image_files", {
      source: source.value,
      offset: 0,
      limit: 9999,
      customDir: useCustomDir.value && customDir.value.trim() ? customDir.value.trim() : null,
    });
    allImages.value = result.images;
    // 当前壁纸排最前面，孤立文件其次，其余按名称降序（后端已排好）
    if (currentWallpaper.value) {
      allImages.value.sort((a, b) => {
        if (a.path === currentWallpaper.value) return -1;
        if (b.path === currentWallpaper.value) return 1;
        return 0;
      });
    }
    total.value = result.total;
    currentPage.value = 1;
    selectedPaths.value.clear();
    logger.info("Gallery", "图片已加载", { total: result.total, source: source.value });
    await nextTick();
    // 按需生成可见页面的缩略图
    await requestThumbnails();
  } catch (e) {
    logger.error("Gallery", "加载图片失败", e);
  }
  loading.value = false;
}

async function selectCustomDirectory() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择自定义目录",
    });
    if (selected) {
      customDir.value = selected;
      useCustomDir.value = true;
      resetAndLoad();
    }
  } catch (e) {
    logger.error("Gallery", "目录选择失败", e);
  }
}

async function resetAndLoad() {
  allImages.value = [];
  total.value = 0;
  selectedIndex.value = -1;
  selectedPaths.value.clear();
  await loadCurrentWallpaper();
  await nextTick();
  await loadImages();
}

watch(source, async (to) => {
  saveDir.value = "";
  useCustomDir.value = false;
  logger.action("Gallery", "切换源", { source: to });
  await loadSaveDir();
  await resetAndLoad();
});

async function loadSaveDir() {
  if (useCustomDir.value && customDir.value.trim()) {
    saveDir.value = customDir.value.trim();
    return;
  }
  try {
    const config = await invoke<AppConfig>("get_config");
    saveDir.value = source.value === "wallhaven" ? config.wallhaven_save_dir : config.reddit_save_dir;
  } catch (e) {
    saveDir.value = "";
  }
}

async function requestThumbnails() {
  const capturedSource = source.value;
  const toProcess = visibleImages.value
    .filter(img => !img.thumb_path)
    .map(img => img.name);
  if (toProcess.length === 0) return;

  thumbLoading.value = true;

  try {
    const result = await invoke<{ items: ThumbnailResult[] }>("resolve_thumbnails", {
      source: capturedSource,
      filenames: toProcess,
      dpr: dpr,
    });
    if (source.value !== capturedSource) return;

    const imageMap = new Map(allImages.value.map(img => [img.name, img]));
    for (const item of result.items) {
      const img = imageMap.get(item.name);
      if (img) img.thumb_path = item.thumb_path;
    }
  } catch (e) {
  }

  thumbLoading.value = false;
}

async function deleteImage(index: number) {
  const img = allImages.value[index];
  if (!img) return;
  deletingIndex.value = index;
  try {
    if (img.is_orphan) {
      await invoke<boolean>("delete_orphan_file", {
        source: source.value,
        name: img.name,
      });
      localSnackbarText.value = `已删除: ${img.name}`;
      logger.action("Gallery", "删除孤立文件", { name: img.name });
    } else {
      await invoke<boolean>("dislike_file", {
        source: source.value,
        name: img.name,
      });
      localSnackbarText.value = `已标记为不喜欢: ${img.name}`;
      logger.action("Gallery", "标记为不喜欢", { name: img.name });
    }
    allImages.value.splice(index, 1);
    total.value -= 1;
    if (selectedIndex.value === index) {
      selectedIndex.value = -1;
    } else if (selectedIndex.value > index) {
      selectedIndex.value -= 1;
    }
    localSnackbar.value = true;
  } catch (e) {
    localSnackbarText.value = `操作失败: ${e}`;
    localSnackbar.value = true;
  }
  deletingIndex.value = -1;
}

async function deleteSelected() {
  if (selectedPaths.value.size === 0) return;
  deletingSelection.value = true;
  let count = 0;
  for (const name of selectedPaths.value) {
    try {
      // 检查是否为孤立文件（从当前列表中查找）
      const img = allImages.value.find((i) => i.name === name);
      if (img?.is_orphan) {
        await invoke<boolean>("delete_orphan_file", { source: source.value, name });
        logger.action("Gallery", "批量删除孤立文件", { name });
      } else {
        await invoke<boolean>("dislike_file", { source: source.value, name });
        logger.action("Gallery", "批量标记不喜欢", { name });
      }
      count++;
    } catch (e) {
    }
  }
  localSnackbarText.value = `批量操作完成: ${count} 张`;
  localSnackbar.value = true;
  logger.action("Gallery", "批量操作", { count });
  selectedPaths.value.clear();
  deletingSelection.value = false;
  await resetAndLoad();
}

function toggleSelection(img: LocalImage) {
  if (selectedPaths.value.has(img.name)) {
    selectedPaths.value.delete(img.name);
  } else {
    selectedPaths.value.add(img.name);
  }
}

function isSelected(img: LocalImage): boolean {
  return selectedPaths.value.has(img.name);
}

function shuffleImages() {
  logger.action("Gallery", "随机排序");
  const shuffled = [...allImages.value];
  for (let i = shuffled.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
  }
  allImages.value = shuffled;
  currentPage.value = 1;
  selectedIndex.value = -1;
  selectedPaths.value.clear();
}

async function setAsWallpaper() {
  const img = selectedImage.value;
  if (!img) return;
  settingWallpaper.value = true;
  try {
    const result = await invoke<string>("set_wallpaper", {
      filePath: img.path,
    });
    localSnackbarText.value = result;
    localSnackbar.value = true;
    logger.action("Gallery", "设为壁纸", { name: img.name });
  } catch (e) {
    localSnackbarText.value = `设置壁纸失败: ${e}`;
    localSnackbar.value = true;
  }
  settingWallpaper.value = false;
}

function onDialogKeydown(e: KeyboardEvent) {
  if (!dialogOpen.value) return;
  if (e.key === "ArrowLeft") {
    logger.action("Gallery", "预览导航", { direction: "prev" });
    navigateImage(-1);
  } else if (e.key === "ArrowRight") {
    logger.action("Gallery", "预览导航", { direction: "next" });
    navigateImage(1);
  } else if (e.key === "Escape") {
    selectedIndex.value = -1;
  }
}

function navigateImage(direction: number) {
  const newIndex = selectedIndex.value + direction;
  if (newIndex >= 0 && newIndex < allImages.value.length) {
    selectedIndex.value = newIndex;
    logger.info("Gallery", "切换到图片", { index: newIndex, name: allImages.value[newIndex]?.name });
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / (1024 * 1024)).toFixed(1) + " MB";
}

function isCurrentWallpaper(img: LocalImage): boolean {
  return !!currentWallpaper.value && img.path === currentWallpaper.value;
}

async function loadCurrentWallpaper() {
  try {
    const data = await invoke<{ path: string }>("get_active_wallpaper");
    currentWallpaper.value = data.path || "";
  } catch {
    currentWallpaper.value = "";
  }
}

async function batchAddOrphans() {
  const orphanNames = allImages.value
    .filter((img) => img.is_orphan && selectedPaths.value.has(img.name))
    .map((img) => img.name);
  if (orphanNames.length === 0) return;

  addingOrphanName.value = "batch";
  try {
    const count = await invoke<number>("adopt_orphan_files", {
      source: source.value,
      names: orphanNames,
    });
    localSnackbarText.value = `已入库 ${count} 个孤立文件`;
    localSnackbar.value = true;
    logger.action("Gallery", "批量入库孤立文件", { count });
    // 更新本地状态
    for (const img of allImages.value) {
      if (img.is_orphan && selectedPaths.value.has(img.name)) {
        img.is_orphan = false;
      }
    }
    selectedPaths.value.clear();
  } catch (e) {
    localSnackbarText.value = `入库失败: ${e}`;
    localSnackbar.value = true;
  }
  addingOrphanName.value = "";
}

async function addOrphanFromGallery(img: LocalImage) {
  addingOrphanName.value = img.name;
  try {
    const count = await invoke<number>("adopt_orphan_files", {
      source: source.value,
      names: [img.name],
    });
    if (count > 0) {
      img.is_orphan = false;
      localSnackbarText.value = `已入库: ${img.name}`;
      logger.action("Gallery", "孤立文件已入库", { name: img.name });
    } else {
      localSnackbarText.value = `入库失败或重复: ${img.name}`;
    }
    localSnackbar.value = true;
  } catch (e) {
    localSnackbarText.value = `入库失败: ${e}`;
    localSnackbar.value = true;
  }
  addingOrphanName.value = "";
}

async function cleanThumbnails() {
  cleaningThumbnails.value = true;
  try {
    const result = await invoke<{ wallhaven: number; reddit: number }>("clean_thumbnails");
    const total = result.wallhaven + result.reddit;
    localSnackbarText.value = `已清理 ${total} 个失效缩略图 (Wallhaven: ${result.wallhaven}, Reddit: ${result.reddit})`;
    localSnackbar.value = true;
    logger.action("Gallery", "清理缩略图", { wallhaven: result.wallhaven, reddit: result.reddit });
  } catch (e) {
    localSnackbarText.value = `清理缩略图失败: ${e}`;
    localSnackbar.value = true;
  }
  cleaningThumbnails.value = false;
}

async function prevPage() {
  if (currentPage.value <= 1) return;
  currentPage.value -= 1;
  logger.action("Gallery", "翻页", { page: currentPage.value });
  pageLoading.value = true;
  await nextTick();
  pageLoading.value = false;
  requestThumbnails();
}

async function nextPage() {
  if (currentPage.value >= totalPages.value) return;
  currentPage.value += 1;
  logger.action("Gallery", "翻页", { page: currentPage.value });
  pageLoading.value = true;
  await nextTick();
  pageLoading.value = false;
  requestThumbnails();
}

onMounted(async () => {
  await loadSaveDir();
  await loadCurrentWallpaper();
  updateImagesPerPage();
  window.addEventListener("resize", handleResize);
  await loadImages();
});

onUnmounted(() => {
  window.removeEventListener("resize", handleResize);
});
</script>

<template>
  <div class="gallery-root">
    <div class="gallery-toolbar glass-card" ref="galleryToolbar">
      <div class="toolbar-inner">
        <v-btn-toggle v-model="source" :mandatory="!useCustomDir" color="primary" density="compact" rounded="pill" class="toolbar-source-toggle">
          <v-btn value="wallhaven" prepend-icon="mdi-image-search" size="small">Wallhaven</v-btn>
          <v-btn value="reddit" prepend-icon="mdi-reddit" size="small">Reddit</v-btn>
        </v-btn-toggle>

        <v-btn
          variant="text"
          size="small"
          :color="useCustomDir ? 'primary' : 'grey'"
          @click="useCustomDir = !useCustomDir; if (!useCustomDir) { source = 'wallhaven'; resetAndLoad(); }"
          class="toolbar-action-btn"
        >
          <v-icon start size="14">mdi-folder-open</v-icon>
          {{ useCustomDir ? '退出自定义' : '自定义目录' }}
        </v-btn>

        <template v-if="useCustomDir">
          <v-text-field
            v-model="customDir"
            label="目录路径"
            density="compact"
            hide-details
            variant="outlined"
            class="toolbar-dir-input"
            style="max-width: 280px;"
            append-inner-icon="mdi-folder-open"
            @click:append-inner="selectCustomDirectory"
            @keydown.enter="resetAndLoad"
          />
          <v-btn
            variant="tonal"
            size="small"
            color="primary"
            @click="resetAndLoad"
            class="toolbar-action-btn"
          >
            <v-icon size="14">mdi-magnify</v-icon>
          </v-btn>
        </template>
        <template v-else>
          <v-chip v-if="saveDir" size="x-small" variant="outlined" class="toolbar-dir-chip text-truncate">
            <v-icon start size="11">mdi-folder</v-icon>
            {{ saveDir }}
          </v-chip>
        </template>

        <div class="toolbar-actions">
          <v-btn variant="text" color="default" :loading="loading" @click="() => { logger.action('Gallery', '刷新'); resetAndLoad(); }" icon="mdi-refresh" size="small" class="toolbar-action-btn" />
          <v-btn variant="text" color="default" @click="shuffleImages" size="small" class="toolbar-action-btn">
            <v-icon start size="14">mdi-shuffle</v-icon>
            随机
          </v-btn>
          <v-btn variant="text" :color="selectionMode ? 'warning' : 'default'" @click="() => { const newMode = !selectionMode; logger.action('Gallery', '选择模式', { mode: newMode }); selectionMode = newMode; selectedPaths.clear(); }" size="small" class="toolbar-action-btn">
            <v-icon start size="14">{{ selectionMode ? 'mdi-close' : 'mdi-checkbox-multiple-marked-outline' }}</v-icon>
            {{ selectionMode ? '退出选择' : '批量选择' }}
          </v-btn>
          <v-btn v-if="selectionMode && selectedOrphanCount > 0" color="success" variant="text" :loading="addingOrphanName !== ''" @click="batchAddOrphans" size="small" class="toolbar-action-btn">
            <v-icon start size="14">mdi-database-plus</v-icon>
            入库 ({{ selectedOrphanCount }})
          </v-btn>
          <v-btn v-if="selectionMode && selectedPaths.size > 0" color="error" variant="text" :loading="deletingSelection" @click="deleteSelected" size="small" class="toolbar-action-btn">
            <v-icon start size="14">mdi-close-circle</v-icon>
            标记不喜欢 ({{ selectedPaths.size }})
          </v-btn>
        </div>

        <div class="toolbar-meta">
          <v-btn
            variant="text"
            size="x-small"
            :color="showOrphansOnly ? 'orange' : 'grey'"
            @click="showOrphansOnly = !showOrphansOnly; currentPage = 1"
            class="toolbar-action-btn"
          >
            <v-icon start size="13">mdi-file-remove</v-icon>
            孤立文件
            <v-chip
              v-if="orphanCount > 0"
              size="x-small"
              :color="showOrphansOnly ? 'orange' : 'grey'"
              variant="tonal"
              class="ms-1"
              style="height: 16px !important; font-size: 0.5625rem"
            >
              {{ orphanCount }}
            </v-chip>
          </v-btn>
          <v-btn variant="text" size="x-small" color="grey" :loading="cleaningThumbnails" @click="cleanThumbnails" class="toolbar-action-btn">
            <v-icon start size="12">mdi-broom</v-icon>
            清理缩略图
          </v-btn>
          <v-chip color="primary" variant="tonal" size="x-small" class="toolbar-chip">
            {{ visibleImages.length }} / {{ showOrphansOnly ? orphanCount : total }}
          </v-chip>
          <v-chip color="secondary" variant="tonal" size="x-small" class="toolbar-chip">
            第 {{ currentPage }}/{{ totalPages }} 页
          </v-chip>
          <v-chip v-if="pageLoading" color="primary" variant="outlined" size="x-small" class="toolbar-chip">
            <v-progress-circular indeterminate size="10" width="2" class="me-1" />
            加载中
          </v-chip>
          <v-chip v-if="thumbLoading" color="warning" variant="outlined" size="x-small" class="toolbar-chip">
            <v-progress-circular indeterminate size="10" width="2" class="me-1" />
            缩略图中
          </v-chip>
        </div>
      </div>
    </div>

    <div class="gallery-content">
      <div v-if="loading && allImages.length === 0" class="gallery-grid">
        <div v-for="n in 6" :key="n" class="gallery-skeleton shimmer" />
      </div>

      <div v-else-if="allImages.length > 0" class="gallery-grid-wrapper" :class="{ 'gallery-grid-wrapper--loading': pageLoading }">
        <div class="gallery-grid" :class="{ 'gallery-grid--fade': pageLoading }">
          <div
            v-for="(img, i) in visibleImages"
            :key="img.path"
            class="gallery-item"
            :class="{ 'gallery-item-selected': isSelected(img) }"
            @click="selectionMode ? toggleSelection(img) : (selectedIndex = (currentPage - 1) * imagesPerPage + i)"
          >
            <div class="gallery-thumb-wrap">
              <img
                :src="getImageUrl(img)"
                :alt="img.name"
                class="gallery-img"
                :class="{ 'gallery-img--visible': imgLoaded.has(img.name) }"
                decoding="async"
                loading="lazy"
                @load="onImgLoad(img)"
                @error="onImgError($event, img)"
              />
              <div v-if="!imgLoaded.has(img.name)" class="gallery-thumb-loading">
                <v-progress-circular indeterminate size="28" width="2" color="#6c8cff" />
              </div>
            </div>
            <div class="gallery-overlay">
              <span class="overlay-name">{{ img.name }}</span>
              <span class="overlay-size">{{ formatSize(img.size) }}</span>
            </div>
            <div class="gallery-badges">
              <div v-if="img.is_orphan" class="orphan-badge">
                <v-icon size="11">mdi-file-remove</v-icon>
                <span>孤立</span>
              </div>
              <div v-if="isCurrentWallpaper(img)" class="current-wallpaper-badge">
                <v-icon size="12">mdi-wallpaper</v-icon>
                <span>当前</span>
              </div>
            </div>
            <div class="gallery-item-actions">
              <div v-if="img.is_orphan" class="gallery-orphan-actions">
                <v-btn
                  icon="mdi-database-plus"
                  size="x-small"
                  color="success"
                  variant="flat"
                  class="gallery-action-btn"
                  :loading="addingOrphanName === img.name"
                  @click.stop="addOrphanFromGallery(img)"
                />
              </div>
              <v-btn
                v-if="!useCustomDir"
                icon="mdi-delete"
                size="x-small"
                color="error"
                variant="flat"
                class="gallery-action-btn"
                @click.stop="pendingDeleteIndex = (currentPage - 1) * imagesPerPage + i; confirmDelete = true"
              />
            </div>
            <div v-if="selectionMode" class="gallery-checkbox" @click.stop="toggleSelection(img)">
              <v-icon :color="isSelected(img) ? 'primary' : 'white'">
                {{ isSelected(img) ? 'mdi-checkbox-marked-circle' : 'mdi-checkbox-blank-circle-outline' }}
              </v-icon>
            </div>
          </div>
        </div>
      </div>

      <div v-else class="gallery-empty">
        <div class="empty-icon-wrap">
          <v-icon size="72" color="grey" class="empty-icon">mdi-image-off-outline</v-icon>
        </div>
        <p class="empty-title">暂无本地图片</p>
        <p class="empty-desc">请先从 Wallhaven 或 Reddit 下载壁纸</p>
        <div v-if="saveDir" class="empty-dir">
          <v-chip variant="outlined" color="grey" size="small">
            <v-icon start size="14">mdi-folder</v-icon>
            {{ saveDir }}
          </v-chip>
        </div>
      </div>
    </div>

    <div v-if="allImages.length > 0" class="gallery-pagination-bar">
      <v-btn icon="mdi-chevron-left" variant="text" size="small" :disabled="currentPage <= 1" @click="prevPage" />
      <span class="pagination-info">第 <strong>{{ currentPage }}</strong> / {{ totalPages }} 页</span>
      <v-btn icon="mdi-chevron-right" variant="text" size="small" :disabled="currentPage >= totalPages" @click="nextPage" />
    </div>

    <v-dialog v-model="dialogOpen" max-width="90vw" scrollable @keydown="onDialogKeydown" transition="dialog-transition">
      <v-card v-if="selectedImage" class="preview-card">
        <div class="preview-navbar">
          <div class="navbar-left">
            <v-btn icon="mdi-chevron-left" variant="text" color="white" size="small" :disabled="selectedIndex <= 0" @click="navigateImage(-1)" />
            <span class="navbar-position">{{ selectedIndex + 1 }} / {{ allImages.length }}</span>
            <v-btn icon="mdi-chevron-right" variant="text" color="white" size="small" :disabled="selectedIndex >= allImages.length - 1" @click="navigateImage(1)" />
          </div>
          <v-btn icon="mdi-close" variant="text" color="white" size="small" @click="selectedIndex = -1" />
        </div>
        <div class="preview-image-wrap">
          <v-img :src="selectedImage ? getPreviewUrl(selectedImage) : ''" max-height="70vh" contain class="preview-image">
            <template v-slot:placeholder>
              <div class="d-flex align-center justify-center fill-height">
                <v-progress-circular indeterminate size="40" width="3" color="white" />
              </div>
            </template>
          </v-img>
        </div>
        <div class="preview-actions">
          <div class="preview-info">
            <span class="info-name">{{ selectedImage.name }}</span>
            <span class="info-size">{{ formatSize(selectedImage.size) }}</span>
          </div>
          <div class="preview-buttons">
            <v-btn color="primary" variant="tonal" prepend-icon="mdi-wallpaper" :loading="settingWallpaper" @click="setAsWallpaper" size="small" class="preview-btn">
              设为壁纸
            </v-btn>
            <v-btn color="warning" variant="tonal" prepend-icon="mdi-close-circle" :loading="deletingIndex >= 0" @click="pendingDeleteIndex = selectedIndex; confirmDelete = true" size="small" class="preview-btn">
              不喜欢
            </v-btn>
          </div>
        </div>
      </v-card>
    </v-dialog>

    <!-- 确认对话框 -->
    <v-dialog v-model="confirmDelete" max-width="380">
      <v-card class="pa-4" style="border-radius: 16px;">
        <v-card-title class="text-h6 d-flex align-center pa-0 pb-3">
          <v-icon :color="pendingIsOrphan ? 'error' : 'warning'" class="me-2">mdi-alert-circle</v-icon>
          {{ pendingIsOrphan ? '确认删除' : '确认标记为不喜欢' }}
        </v-card-title>
        <v-card-text class="pa-0 pb-4 text-body-2 text-secondary">
          <template v-if="pendingIsOrphan">
            该文件<strong>无数据库记录</strong>，删除后无法通过补下载恢复。<br />
            直接删除文件本身。
          </template>
          <template v-else>
            将删除文件并标记为不喜欢，之后<strong>不再补下载</strong>。<br />
            可通过「全部恢复为喜欢」还原。
          </template>
        </v-card-text>
        <v-card-actions class="pa-0 d-flex justify-end ga-2">
          <v-btn variant="text" color="grey" @click="confirmDelete = false">取消</v-btn>
          <v-btn :color="pendingIsOrphan ? 'error' : 'warning'" variant="tonal" @click="confirmDelete = false; deleteImage(pendingDeleteIndex)">
            <v-icon start size="16">{{ pendingIsOrphan ? 'mdi-delete' : 'mdi-close-circle' }}</v-icon>
            {{ pendingIsOrphan ? '确定删除' : '确定标记' }}
          </v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-snackbar v-model="localSnackbar" :timeout="3000" location="bottom" variant="tonal">
      {{ localSnackbarText }}
    </v-snackbar>
  </div>
</template>

<style scoped>
.gallery-root {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

.gallery-toolbar {
  position: sticky;
  top: 0;
  z-index: 10;
  margin: 0 16px 16px;
  border-radius: 12px;
  overflow: hidden;
  flex-shrink: 0;
}

.toolbar-inner {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  flex-wrap: wrap;
}

.toolbar-source-toggle {
  flex-shrink: 0;
}

.toolbar-source-toggle .v-btn {
  font-size: 0.8125rem;
  letter-spacing: 0.01em;
}

.toolbar-dir-chip {
  max-width: 200px;
  opacity: 0.7;
  font-size: 0.6875rem;
}

.toolbar-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

.toolbar-action-btn {
  opacity: 0.75;
  transition: opacity 0.2s var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1));
}

.toolbar-action-btn:hover {
  opacity: 1;
}

.toolbar-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
  flex-shrink: 0;
}

.toolbar-chip {
  font-size: 0.6875rem;
  height: 22px !important;
}

.gallery-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.gallery-grid-wrapper {
  flex: 1;
  overflow: auto;
  padding: 12px 16px 24px;
  box-sizing: border-box;
  transition: opacity 0.2s ease;
}

.gallery-grid-wrapper--loading {
  opacity: 0.5;
  pointer-events: none;
}

.gallery-grid--fade {
  transition: opacity 0.15s ease;
}

.gallery-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 10px;
}

.gallery-item {
  position: relative;
  border-radius: var(--radius-md);
  overflow: hidden;
  cursor: pointer;
  transition: transform 0.3s var(--ease-out), box-shadow 0.3s var(--ease-out);
  box-shadow: var(--shadow-sm), 0 0 0 1px rgba(108, 140, 255, 0.03);
  background: var(--surface-card);
  aspect-ratio: 16 / 9;
}

.gallery-item:hover {
  transform: scale(1.03);
  box-shadow: var(--shadow-lg), 0 0 0 1px rgba(108, 140, 255, 0.1);
}

.gallery-item:active {
  transform: scale(0.97);
}

.gallery-item-selected {
  outline: 2px solid #6c8cff;
  outline-offset: -2px;
  box-shadow: 0 0 16px rgba(108, 140, 255, 0.25), 0 0 0 1px rgba(108, 140, 255, 0.15);
}

.gallery-item-selected:hover {
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.3), 0 0 20px rgba(108, 140, 255, 0.35);
}

.gallery-item-actions {
  position: absolute;
  top: 6px;
  right: 6px;
  z-index: 3;
  opacity: 0;
  transition: opacity 0.2s var(--ease-out);
  display: flex;
  gap: 4px;
}

.gallery-item:hover .gallery-item-actions {
  opacity: 1;
}

.gallery-badges {
  position: absolute;
  top: 6px;
  left: 6px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  z-index: 4;
  pointer-events: none;
}

.orphan-badge {
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 2px 6px;
  border-radius: 5px;
  background: rgba(255, 152, 0, 0.85);
  backdrop-filter: blur(4px);
  font-size: 0.625rem;
  color: white;
  white-space: nowrap;
}

.current-wallpaper-badge {
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 2px 6px;
  border-radius: 5px;
  background: rgba(108, 140, 255, 0.85);
  backdrop-filter: blur(4px);
  font-size: 0.625rem;
  color: white;
  white-space: nowrap;
}

.gallery-action-btn {
  min-width: 28px !important;
  width: 28px !important;
  height: 28px !important;
  background: rgba(0, 0, 0, 0.6) !important;
  backdrop-filter: blur(4px);
  border-radius: 6px !important;
}
.gallery-orphan-actions .gallery-action-btn:hover {
  background: rgba(76, 175, 80, 0.8) !important;
}
.gallery-action-btn:hover {
  background: rgba(200, 200, 200, 0.4) !important;
}

.gallery-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  opacity: 0;
  transition: opacity 0.3s ease, transform 0.35s cubic-bezier(0.16, 1, 0.3, 1);
}

.gallery-img--visible {
  opacity: 1;
}

.gallery-thumb-wrap {
  width: 100%;
  height: 100%;
  position: relative;
  overflow: hidden;
}

.gallery-thumb-loading {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--surface-card);
  z-index: 1;
}

.gallery-item:hover .gallery-img {
  transform: scale(1.08);
}

.gallery-overlay {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  padding: 10px 10px 8px;
  background: linear-gradient(to top, rgba(0, 0, 0, 0.75) 0%, transparent 100%);
  opacity: 0;
  transform: translateY(4px);
  transition: opacity 0.25s cubic-bezier(0.16, 1, 0.3, 1), transform 0.25s cubic-bezier(0.16, 1, 0.3, 1);
}

.gallery-item:hover .gallery-overlay {
  opacity: 1;
  transform: translateY(0);
}

.overlay-name {
  color: var(--text-primary);
  font-size: 0.75rem;
  line-height: 1.3;
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.overlay-size {
  color: rgba(255, 255, 255, 0.6);
  font-size: 0.6875rem;
  line-height: 1.4;
}

.gallery-checkbox {
  position: absolute;
  top: 8px;
  left: 8px;
  z-index: 2;
  cursor: pointer;
  opacity: 0.85;
  transition: opacity 0.2s ease;
}

.gallery-checkbox:hover {
  opacity: 1;
}

.gallery-skeleton {
  aspect-ratio: 16 / 9;
  border-radius: 10px;
  overflow: hidden;
  background: var(--surface-card);
}

.gallery-skeleton.shimmer {
  background: linear-gradient(90deg, var(--surface-card) 25%, var(--surface-hover) 50%, var(--surface-card) 75%);
  background-size: 200% 100%;
  animation: shimmer 1.5s infinite;
}

.gallery-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  padding: 60px 24px;
  gap: 4px;
}

.empty-icon-wrap {
  width: 100px;
  height: 100px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: rgba(42, 43, 48, 0.5);
  margin-bottom: 12px;
}

.empty-icon {
  opacity: 0.5;
}

.empty-title {
  font-size: 1.125rem;
  font-weight: 600;
  color: var(--text-secondary);
  margin: 0;
}

.empty-desc {
  font-size: 0.8125rem;
  color: var(--text-disabled);
  margin: 4px 0 0;
}

.empty-dir {
  margin-top: 16px;
}

.gallery-pagination-bar {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  margin: 0 16px 16px;
  padding: 8px 20px;
  border-radius: 100px;
  background: rgba(var(--surface-card-rgb), 0.35);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border: 1px solid rgba(128, 128, 128, 0.12);
  box-shadow: 0 1px 8px rgba(0, 0, 0, 0.06);
  flex-shrink: 0;
  width: fit-content;
  margin-left: auto;
  margin-right: auto;
}

.pagination-info {
  font-size: 0.8125rem;
  color: var(--text-secondary);
  white-space: nowrap;
}

.pagination-info strong {
  color: var(--text-primary);
  font-weight: 600;
}

.preview-card {
  overflow: hidden;
  border-radius: 16px;
  background: rgba(var(--surface-deep-rgb), 0.92);
  backdrop-filter: blur(30px);
  -webkit-backdrop-filter: blur(30px);
  border: 1px solid rgba(255, 255, 255, 0.06);
}

.preview-navbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  user-select: none;
  background: rgba(0, 0, 0, 0.3);
  border-bottom: 1px solid rgba(255, 255, 255, 0.04);
}

.navbar-left {
  display: flex;
  align-items: center;
  gap: 4px;
}

.navbar-position {
  color: var(--text-secondary);
  font-size: 0.8125rem;
  font-weight: 500;
  letter-spacing: 0.02em;
  min-width: 60px;
  text-align: center;
}

.preview-image-wrap {
  background: var(--surface-base);
}

.preview-image {
  background: var(--surface-base);
}

.preview-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  background: rgba(0, 0, 0, 0.3);
  border-top: 1px solid rgba(255, 255, 255, 0.04);
  gap: 12px;
}

.preview-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
  border-left: 2px solid rgba(108, 140, 255, 0.3);
  padding-left: 12px;
}

.info-name {
  color: var(--text-primary);
  font-size: 0.875rem;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.info-size {
  color: var(--text-disabled);
  font-size: 0.75rem;
}

.preview-buttons {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.preview-btn {
  font-size: 0.8125rem;
}
</style>
