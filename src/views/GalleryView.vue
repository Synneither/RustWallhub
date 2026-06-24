<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, nextTick, triggerRef, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";

interface LocalImage {
  name: string;
  path: string;
  thumb_path?: string;
  size: number;
}

interface ListResult {
  images: LocalImage[];
  total: number;
}

interface AppConfig {
  wallhaven_save_dir: string;
  reddit_save_dir: string;
}

const source = ref("wallhaven");
const allImages = ref<LocalImage[]>([]);
const total = ref(0);
const loading = ref(false);
const selectedIndex = ref(-1);
const saveDir = ref("");
const thumbLoading = ref(false);
const settingWallpaper = ref(false);
const imagesPerPage = ref(12);
const currentPage = ref(1);
const localSnackbar = ref(false);
const localSnackbarText = ref("");
const deletingIndex = ref(-1);
const deletingSelection = ref(false);
const selectedPaths = ref<Set<string>>(new Set());
const selectionMode = ref(false);
const cleaningThumbnails = ref(false);
const galleryToolbar = ref<HTMLElement | null>(null);
const paginationBar = ref<HTMLElement | null>(null);

const totalPages = computed(() => Math.max(1, Math.ceil(total.value / imagesPerPage.value)));
const visibleImages = computed(() => {
  const start = (currentPage.value - 1) * imagesPerPage.value;
  return allImages.value.slice(start, start + imagesPerPage.value);
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

function getImageUrl(img: LocalImage): string {
  const filePath = img.thumb_path || img.path;
  try {
    return convertFileSrc(filePath);
  } catch {
    return img.path;
  }
}

function getPreviewUrl(img: LocalImage): string {
  try {
    return convertFileSrc(img.path);
  } catch {
    return '';
  }
}

const resizeState = {
  itemWidth: 180,
  itemHeight: 180,
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
  const itemSize = Math.floor((availableWidth - (cols - 1) * resizeState.horizontalGap) / cols);
  const availableHeight = Math.max(
    0,
    height - topHeight - resizeState.toolbarBottomMargin - resizeState.contentPaddingTop - resizeState.contentPaddingBottom
  );
  const rows = Math.max(
    1,
    Math.floor((availableHeight + resizeState.verticalGap) / (itemSize + resizeState.verticalGap))
  );
  const minRows = 6;
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
    const result = await invoke<ListResult>("list_local_images", {
      source: source.value,
      offset: 0,
      limit: 9999,
    });
    allImages.value = result.images;
    total.value = result.total;
    currentPage.value = 1;
    selectedPaths.value.clear();
    await nextTick();
    await requestThumbnails(true);
  } catch (e) {
    console.error("加载图片失败:", e);
  }
  loading.value = false;
}

async function resetAndLoad() {
  allImages.value = [];
  total.value = 0;
  selectedIndex.value = -1;
  selectedPaths.value.clear();
  await nextTick();
  await loadImages();
}

// 切换数据源时重新加载
watch(source, async () => {
  saveDir.value = "";
  await loadSaveDir();
  await resetAndLoad();
});

async function loadSaveDir() {
  try {
    const config = await invoke<AppConfig>("get_config");
    saveDir.value = source.value === "wallhaven" ? config.wallhaven_save_dir : config.reddit_save_dir;
  } catch (e) {
    console.error("加载配置失败:", e);
    saveDir.value = "";
  }
}

interface ThumbnailResult {
  name: string;
  thumb_path: string;
}

async function requestThumbnails(loadAll = false) {
  const targetImages = loadAll ? allImages.value : visibleImages.value;
  const toProcess = targetImages
    .filter(img => !img.thumb_path)
    .map(img => img.name);
  if (toProcess.length === 0) return;

  thumbLoading.value = true;
  const imageMap = new Map(allImages.value.map(img => [img.name, img]));
  try {
    const result = await invoke<{ items: ThumbnailResult[] }>("get_thumbnail_paths", {
      source: source.value,
      filenames: toProcess,
    });
    result.items.forEach(item => {
      const img = imageMap.get(item.name);
      if (img) {
        img.thumb_path = item.thumb_path;
      }
    });
  } catch (e) {
    console.error("批量获取缩略图失败:", e);
  }
  thumbLoading.value = false;
}

async function deleteImage(index: number) {
  const img = allImages.value[index];
  if (!img) return;
  deletingIndex.value = index;
  try {
    await invoke<boolean>("delete_image", {
      source: source.value,
      name: img.name,
    });
    allImages.value.splice(index, 1);
    total.value -= 1;
    if (selectedIndex.value === index) {
      selectedIndex.value = -1;
    } else if (selectedIndex.value > index) {
      selectedIndex.value -= 1;
    }
    localSnackbarText.value = `已删除: ${img.name}`;
    localSnackbar.value = true;
  } catch (e) {
    localSnackbarText.value = `删除失败: ${e}`;
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
      await invoke<boolean>("delete_image", {
        source: source.value,
        name,
      });
      count++;
    } catch (e) {
      console.error(`删除 ${name} 失败:`, e);
    }
  }
  localSnackbarText.value = `批量删除完成: 已删除 ${count} 张`;
  localSnackbar.value = true;
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
  } catch (e) {
    localSnackbarText.value = `设置壁纸失败: ${e}`;
    localSnackbar.value = true;
  }
  settingWallpaper.value = false;
}

// 预览时键盘导航
function onDialogKeydown(e: KeyboardEvent) {
  if (!dialogOpen.value) return;
  if (e.key === "ArrowLeft") {
    navigateImage(-1);
  } else if (e.key === "ArrowRight") {
    navigateImage(1);
  } else if (e.key === "Escape") {
    selectedIndex.value = -1;
  }
}

function navigateImage(direction: number) {
  const newIndex = selectedIndex.value + direction;
  if (newIndex >= 0 && newIndex < allImages.value.length) {
    selectedIndex.value = newIndex;
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / (1024 * 1024)).toFixed(1) + " MB";
}



async function cleanThumbnails() {
  cleaningThumbnails.value = true;
  try {
    const result = await invoke<{ wallhaven: number; reddit: number }>("clean_thumbnails");
    const total = result.wallhaven + result.reddit;
    localSnackbarText.value = `已清理 ${total} 个失效缩略图 (Wallhaven: ${result.wallhaven}, Reddit: ${result.reddit})`;
    localSnackbar.value = true;
  } catch (e) {
    localSnackbarText.value = `清理缩略图失败: ${e}`;
    localSnackbar.value = true;
  }
  cleaningThumbnails.value = false;
}

async function prevPage() {
  if (currentPage.value <= 1) return;
  currentPage.value -= 1;
  await nextTick();
  await requestThumbnails();
}

async function nextPage() {
  if (currentPage.value >= totalPages.value) return;
  currentPage.value += 1;
  await nextTick();
  await requestThumbnails();
}

onMounted(async () => {
  await loadSaveDir();
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
    <v-card class="gallery-toolbar mb-4" elevation="2" ref="galleryToolbar" style="margin: 0 0 16px;">
      <v-card-text class="d-flex align-center py-3" style="flex-wrap: wrap; gap: 8px;">
        <v-btn-toggle v-model="source" mandatory color="primary" density="comfortable">
          <v-btn value="wallhaven" prepend-icon="mdi-image-search">Wallhaven</v-btn>
          <v-btn value="reddit" prepend-icon="mdi-reddit">Reddit</v-btn>
        </v-btn-toggle>
        <v-chip v-if="saveDir" size="small" variant="outlined" class="text-truncate" style="max-width: 300px;">
          <v-icon start size="small">mdi-folder</v-icon>
          {{ saveDir }}
        </v-chip>
        <v-btn variant="tonal" color="primary" :loading="loading" @click="resetAndLoad" icon="mdi-refresh" />
        <v-btn variant="tonal" color="secondary" @click="shuffleImages">
          <v-icon start>mdi-shuffle</v-icon>
          随机
        </v-btn>
        <v-btn variant="tonal" :color="selectionMode ? 'warning' : 'default'" @click="selectionMode = !selectionMode; selectedPaths.clear()">
          <v-icon start>{{ selectionMode ? 'mdi-close' : 'mdi-checkbox-multiple-marked-outline' }}</v-icon>
          {{ selectionMode ? '退出选择' : '批量选择' }}
        </v-btn>
        <v-btn v-if="selectionMode && selectedPaths.size > 0" color="error" variant="tonal" :loading="deletingSelection" @click="deleteSelected">
          <v-icon start>mdi-delete</v-icon>
          删除选中 ({{ selectedPaths.size }})
        </v-btn>
        <v-spacer />
        <v-btn variant="text" size="small" color="grey" :loading="cleaningThumbnails" @click="cleanThumbnails">
          <v-icon start size="small">mdi-broom</v-icon>
          清理缩略图
        </v-btn>
        <v-chip color="primary" variant="tonal" size="small">
          {{ visibleImages.length }} / {{ total }} 张图片
        </v-chip>
        <v-chip color="secondary" variant="tonal" size="small">
          第 {{ currentPage }} 页 / 共 {{ totalPages }} 页
        </v-chip>
        <v-chip v-if="thumbLoading" color="warning" variant="outlined" size="small">
          <v-progress-circular indeterminate size="12" width="2" class="me-1" />
          缩略图中…
        </v-chip>
      </v-card-text>
    </v-card>

    <div class="gallery-content">
      <div v-if="loading && allImages.length === 0" class="gallery-grid">
        <div v-for="n in 6" :key="n" class="gallery-skeleton">
          <v-skeleton-loader type="image" class="fill-height" />
        </div>
      </div>

          <div v-else-if="allImages.length > 0" class="gallery-grid-wrapper">
        <div class="gallery-grid">
          <div v-for="(img, i) in visibleImages" :key="img.path" class="gallery-item" :class="{ 'gallery-item-selected': isSelected(img) }" @click="selectionMode ? toggleSelection(img) : (selectedIndex = (currentPage - 1) * imagesPerPage + i)">
            <img :src="getImageUrl(img)" :alt="img.name" class="gallery-img" decoding="async" loading="lazy" />
            <div class="gallery-overlay">
              <div class="text-caption text-white text-truncate px-2">{{ img.name }}</div>
            </div>
            <div v-if="selectionMode" class="gallery-checkbox" @click.stop="toggleSelection(img)">
              <v-checkbox :model-value="isSelected(img)" density="compact" hide-details color="primary" />
            </div>
          </div>
        </div>
      </div>

      <v-card v-else class="text-center pa-12" elevation="0">
        <v-icon size="80" color="grey-lighten-1" class="mb-4">mdi-image-off-outline</v-icon>
        <p class="text-h6 text-medium-emphasis">暂无本地图片</p>
        <p class="text-body-2 text-medium-emphasis mt-2">请先从 Wallhaven 或 Reddit 下载壁纸</p>
        <div v-if="saveDir" class="mt-6">
          <v-chip variant="outlined" color="grey">
            <v-icon start>mdi-folder</v-icon>
            {{ saveDir }}
          </v-chip>
        </div>
      </v-card>
    </div>

    <div v-if="allImages.length > 0" class="gallery-pagination-bar" ref="paginationBar">
      <div class="d-flex flex-wrap align-center justify-center gap-3">
        <v-btn variant="outlined" color="primary" :disabled="currentPage <= 1" @click="prevPage">上一页</v-btn>
        <v-chip color="secondary" variant="tonal" size="small">第 {{ currentPage }} 页 / 共 {{ totalPages }} 页</v-chip>
        <v-btn variant="outlined" color="primary" :disabled="currentPage >= totalPages" @click="nextPage">下一页</v-btn>
      </div>
    </div>

    <v-dialog v-model="dialogOpen" max-width="90vw" scrollable @keydown="onDialogKeydown">
      <v-card v-if="selectedImage" class="preview-card" elevation="8">
        <div class="preview-navbar d-flex align-center px-3 py-2 bg-grey-darken-3">
          <v-btn icon="mdi-chevron-left" variant="text" color="white" :disabled="selectedIndex <= 0" @click="navigateImage(-1)" />
          <span class="text-white mx-2">{{ selectedIndex + 1 }} / {{ allImages.length }}</span>
          <v-btn icon="mdi-chevron-right" variant="text" color="white" :disabled="selectedIndex >= allImages.length - 1" @click="navigateImage(1)" />
          <v-spacer />
          <v-btn icon="mdi-close" variant="text" color="white" @click="selectedIndex = -1" />
        </div>
        <v-img :src="selectedImage ? getPreviewUrl(selectedImage) : ''" max-height="70vh" contain class="bg-grey-darken-4">
          <template v-slot:placeholder>
            <div class="d-flex align-center justify-center fill-height">
              <v-progress-circular indeterminate size="48" color="white" />
            </div>
          </template>
        </v-img>
        <v-card-actions class="pa-3 bg-grey-darken-3">
          <div class="flex-grow-1">
            <div class="text-body-1 text-white">{{ selectedImage.name }}</div>
            <div class="text-caption text-grey-lighten-1">{{ formatSize(selectedImage.size) }}</div>
          </div>
          <v-btn color="primary" variant="tonal" prepend-icon="mdi-wallpaper" :loading="settingWallpaper" @click="setAsWallpaper">设为壁纸</v-btn>
          <v-btn color="error" variant="tonal" prepend-icon="mdi-delete" :loading="deletingIndex >= 0" @click="deleteImage(selectedIndex)">删除</v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <v-snackbar v-model="localSnackbar" :timeout="3000" location="bottom">{{ localSnackbarText }}</v-snackbar>
  </div>
</template>

<style scoped>
.gallery-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 12px;
}

.gallery-item {
  position: relative;
  border-radius: 8px;
  overflow: hidden;
  cursor: pointer;
  transition: transform 0.2s ease, box-shadow 0.2s ease;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
  background: #1e1e1e;
  aspect-ratio: 1 / 1;
  min-height: 180px;
}

.gallery-item:hover {
  transform: translateY(-4px);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
}

.gallery-item-selected {
  outline: 3px solid rgb(var(--v-theme-primary));
  outline-offset: -3px;
  box-shadow: 0 0 12px rgba(var(--v-theme-primary), 0.4);
}

.gallery-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.gallery-overlay {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  padding: 6px 0;
  background: linear-gradient(to bottom, transparent 50%, rgba(0,0,0,0.7));
}

.gallery-checkbox {
  position: absolute;
  top: 6px;
  left: 6px;
  z-index: 2;
  background: rgba(0, 0, 0, 0.4);
  border-radius: 4px;
}

.gallery-skeleton {
  aspect-ratio: 1;
  border-radius: 8px;
  overflow: hidden;
}

.gallery-root {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.gallery-toolbar {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--v-theme-surface);
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
}

.gallery-sentinel {
  display: flex;
  justify-content: center;
  align-items: center;
  padding: 24px;
  gap: 8px;
}

.gallery-pagination-bar {
  position: static;
  z-index: 1;
  margin: 0 16px 16px;
  padding: 12px 16px;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.92);
  backdrop-filter: blur(12px);
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.12);
}

@media (prefers-color-scheme: dark) {
  .gallery-pagination-bar {
    background: rgba(20, 20, 20, 0.95);
  }
}

.preview-card {
  overflow: hidden;
  border-radius: 12px;
}

.preview-navbar {
  user-select: none;
}
</style>