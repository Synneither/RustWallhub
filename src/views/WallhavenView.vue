<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { logger } from "../utils/logger";

defineProps<{
  downloading: boolean;
}>();

const emit = defineEmits<{
  action: [fn: () => Promise<unknown>];
}>();

// ===== 类型 =====
interface WallhavenImage {
  id: string;
  thumbnail_url: string;
  path: string;
  resolution: string;
  short_url: string;
  file_size: number;
  file_type: string;
}

interface SearchResult {
  images: WallhavenImage[];
  page: number;
  total_pages: number;
  total: number;
}

interface DownloadedImage {
  name: string;
  path: string;
}

interface AppConfig {
  wallhaven_save_dir: string;
  wallhaven_db_path: string;
  wallhaven_api_key: string;
  wallhaven_categories: string;
  wallhaven_purity: string;
  wallhaven_sorting: string;
  wallhaven_order: string;
  wallhaven_top_range: string;
  wallhaven_atleast: string;
  wallhaven_ratios: string;
  wallhaven_q: string;
  wallhaven_max_images: number;
}

// ===== 配置 =====
const config = ref<AppConfig | null>(null);
const showSettings = ref(false);
const saving = ref(false);
const saved = ref(false);

// ===== 搜索 =====
const results = ref<WallhavenImage[]>([]);
const totalResults = ref(0);
const totalPages = ref(1);
const currentPage = ref(1);
const searching = ref(false);
const searched = ref(false);
const error = ref("");

// ===== 选中 =====
const selectedIds = ref<Set<string>>(new Set());

// ===== 下载中预览 =====
const downloadedImages = ref<DownloadedImage[]>([]);
let unlistenImageEvent: (() => void) | null = null;

// ===== Snackbar =====
const localSnackbar = ref(false);
const localSnackbarText = ref("");

// ===== 校验 =====
const requiredRule = (v: string) => !!v || "此项不能为空";
const positiveInt = (v: number) => {
  if (v === undefined || v === null) return true;
  if (typeof v !== "number" || isNaN(v)) return "请输入有效数字";
  if (v < 0) return "不能为负数";
  return true;
};
const resolutionRule = (v: string) => {
  if (!v) return true;
  return /^\d+x\d+$/.test(v) || "格式如 1920x1080";
};

async function loadConfig() {
  try {
    config.value = await invoke<AppConfig>("get_config");
  } catch (e) {
    logger.error("Wallhaven", "配置加载失败", e);
  }
}

async function saveConfig() {
  if (!config.value) return;
  saving.value = true;
  saved.value = false;
  try {
    await invoke("save_settings", { config: config.value });
    saved.value = true;
    setTimeout(() => (saved.value = false), 2000);
    logger.info("Wallhaven", "设置已保存");
  } catch (e) {
    localSnackbarText.value = `保存设置失败: ${e}`;
    localSnackbar.value = true;
  }
  saving.value = false;
}

async function search(page: number = 1) {
  searching.value = true;
  error.value = "";
  try {
    const data = await invoke<SearchResult>("search_wallhaven", { page });
    results.value = data.images;
    totalResults.value = data.total;
    totalPages.value = Math.max(1, data.total_pages);
    currentPage.value = data.page;
    searched.value = true;
    selectedIds.value.clear();
    logger.info("Wallhaven", "搜索完成", { page, total: data.total });
  } catch (e) {
    error.value = String(e);
    logger.error("Wallhaven", "搜索失败", e);
  }
  searching.value = false;
}

function toggleSelect(img: WallhavenImage) {
  const s = new Set(selectedIds.value);
  if (s.has(img.id)) s.delete(img.id);
  else s.add(img.id);
  selectedIds.value = s;
}

function isSelected(img: WallhavenImage): boolean {
  return selectedIds.value.has(img.id);
}

function selectAll() {
  selectedIds.value = new Set(results.value.map(r => r.id));
}

function deselectAll() {
  selectedIds.value = new Set();
}

async function downloadSelected() {
  const selected = results.value.filter(r => selectedIds.value.has(r.id));
  if (selected.length === 0) return;

  downloadedImages.value = [];
  if (!unlistenImageEvent) {
    unlistenImageEvent = await listen<DownloadedImage>("image-downloaded", (e) => {
      if (e.payload.source !== "wallhaven") return;
      const img: DownloadedImage = {
        name: e.payload.name,
        path: convertFileSrc(e.payload.path),
      };
      if (!downloadedImages.value.some(i => i.name === img.name)) {
        downloadedImages.value.unshift(img);
        if (downloadedImages.value.length > 50) {
          downloadedImages.value = downloadedImages.value.slice(0, 50);
        }
      }
    });
  }

  logger.action("Wallhaven", "下载选中", { count: selected.length });
  try {
    await invoke("download_wallhaven_selected", {
      images: selected.map(r => ({
        id: r.id,
        path: r.path,
        resolution: r.resolution,
        short_url: r.short_url,
      })),
    });
  } catch (e) {
    localSnackbarText.value = `下载失败: ${e}`;
    localSnackbar.value = true;
  }
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / (1024 * 1024)).toFixed(1) + " MB";
}

onMounted(loadConfig);
onUnmounted(() => {
  if (unlistenImageEvent) unlistenImageEvent();
});
</script>

<template>
  <div>
    <!-- ===== 搜索栏 + 设置入口 ===== -->
    <v-card class="glass-card source-card animate-in stagger-1">
      <div class="source-card-header" style="background: linear-gradient(135deg, rgba(108,140,255,0.08) 0%, transparent 60%)">
        <div class="source-header-icon" style="background: rgba(108,140,255,0.15)">
          <v-icon color="#6c8cff">mdi-image-search</v-icon>
        </div>
        <div>
          <div class="text-heading">Wallhaven 浏览</div>
          <div class="text-caption">从 Wallhaven 搜索并下载壁纸</div>
        </div>
      </div>
      <v-card-text class="pa-4">
        <div class="d-flex align-center ga-3 flex-wrap">
          <v-btn
            class="gradient-btn"
            :loading="searching"
            @click="search()"
          >
            <v-icon start>mdi-magnify</v-icon>
            搜索壁纸
          </v-btn>

          <v-btn
            variant="text"
            color="grey"
            @click="showSettings = !showSettings"
          >
            <v-icon start size="14">{{ showSettings ? 'mdi-chevron-up' : 'mdi-cog' }}</v-icon>
            搜索参数
          </v-btn>

          <v-chip v-if="searched && !searching" variant="tonal" size="small" color="primary">
            找到 {{ totalResults }} 张
          </v-chip>
        </div>

        <v-alert v-if="error" type="error" variant="tonal" density="compact" class="mt-3">
          {{ error }}
        </v-alert>
      </v-card-text>
    </v-card>

    <!-- ===== 搜索参数（可折叠） ===== -->
    <v-expand-transition>
      <v-card v-if="showSettings && config" class="glass-card mt-3 pa-4 animate-in">
        <div class="settings-grid">
          <v-text-field
            v-model="config.wallhaven_save_dir"
            label="保存目录"
            :rules="[requiredRule]"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model="config.wallhaven_db_path"
            label="数据库路径"
            :rules="[requiredRule]"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model="config.wallhaven_api_key"
            label="API Key（可选）"
            hint="提高速率限制"
            type="password"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model="config.wallhaven_q"
            label="搜索关键词 (q)"
            hint="留空搜索全部，支持标签语法"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model="config.wallhaven_categories"
            label="分类 (categories)"
            hint="位掩码：1=General 2=Anime 4=People，如 010=仅动漫"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model="config.wallhaven_purity"
            label="纯净度 (purity)"
            hint="位掩码：1=SFW 2=Sketchy 4=NSFW，如 110=无NSFW"
            density="compact"
            hide-details="auto"
          />
          <v-select
            v-model="config.wallhaven_sorting"
            label="排序 (sorting)"
            :items="['date_added', 'relevance', 'random', 'views', 'favorites', 'toplist']"
            density="compact"
            hide-details="auto"
          />
          <v-select
            v-model="config.wallhaven_order"
            label="排序方向 (order)"
            :items="[
              { title: '降序 (desc)', value: 'desc' },
              { title: '升序 (asc)', value: 'asc' },
            ]"
            density="compact"
            hide-details="auto"
          />
          <v-select
            v-if="config.wallhaven_sorting === 'toplist'"
            v-model="config.wallhaven_top_range"
            label="时间范围 (topRange)"
            :items="['1d', '3d', '1w', '1M', '3M', '6M', '1y']"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model="config.wallhaven_atleast"
            label="最低分辨率"
            hint="如 1920x1080"
            :rules="[resolutionRule]"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model="config.wallhaven_ratios"
            label="宽高比"
            hint="如 landscape, 16x9"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model.number="config.wallhaven_max_images"
            label="每次最多下载"
            type="number"
            min="0"
            :rules="[positiveInt]"
            density="compact"
            hide-details="auto"
          />
        </div>
        <div class="d-flex align-center mt-3">
          <v-btn
            color="primary"
            variant="tonal"
            size="small"
            :loading="saving"
            @click="saveConfig"
          >
            <v-icon start size="14">mdi-content-save</v-icon>
            保存设置
          </v-btn>
          <v-fade-transition>
            <v-icon v-if="saved" color="success" class="ms-2" size="18">mdi-check-circle</v-icon>
          </v-fade-transition>
        </div>
      </v-card>
    </v-expand-transition>

    <!-- ===== 搜索结果 ===== -->
    <template v-if="searched">
      <div class="d-flex align-center ga-2 mt-4 mb-2 animate-in stagger-2">
        <v-btn
          v-if="results.length > 0 && !downloading"
          size="small"
          variant="tonal"
          color="primary"
          :disabled="selectedIds.size === 0"
          :loading="downloading"
          @click="downloadSelected"
        >
          <v-icon start size="14">mdi-download</v-icon>
          下载选中 ({{ selectedIds.size }})
        </v-btn>

        <v-spacer />

        <v-btn size="x-small" variant="text" @click="selectAll">全选</v-btn>
        <v-btn size="x-small" variant="text" @click="deselectAll">取消</v-btn>

        <v-btn
          size="x-small"
          variant="text"
          icon="mdi-chevron-left"
          :disabled="currentPage <= 1 || searching"
          @click="search(currentPage - 1)"
        />
        <v-chip size="x-small" variant="outlined" color="grey">
          第 {{ currentPage }}/{{ totalPages }} 页
        </v-chip>
        <v-btn
          size="x-small"
          variant="text"
          icon="mdi-chevron-right"
          :disabled="currentPage >= totalPages || searching"
          @click="search(currentPage + 1)"
        />
      </div>

      <div v-if="results.length > 0" class="search-grid animate-in stagger-3">
        <div
          v-for="img in results"
          :key="img.id"
          class="search-item"
          :class="{ 'search-item--selected': isSelected(img) }"
          @click="toggleSelect(img)"
        >
          <img :src="img.thumbnail_url" :alt="img.id" class="search-thumb" loading="lazy" referrerpolicy="no-referrer" />
          <div class="search-overlay">
            <span class="search-res">{{ img.resolution }}</span>
            <span class="search-size">{{ formatFileSize(img.file_size) }}</span>
          </div>
          <div v-if="isSelected(img)" class="search-check">
            <v-icon color="#6c8cff" size="18">mdi-check-circle</v-icon>
          </div>
        </div>
      </div>

      <v-card v-else-if="!searching" class="glass-card mt-4 pa-6 text-center animate-in stagger-3">
        <v-icon size="48" color="grey" class="mb-2">mdi-image-off-outline</v-icon>
        <p class="text-body text-secondary">没有找到匹配的壁纸，请调整搜索参数。</p>
      </v-card>

      <!-- 下载中预览 -->
      <v-card v-if="downloading && downloadedImages.length > 0" class="glass-card mt-4 animate-in">
        <v-card-text class="pa-4">
          <div class="d-flex align-center mb-3">
            <v-progress-circular indeterminate size="18" width="2" color="#6c8cff" class="me-2" />
            <span class="text-body font-weight-medium">已下载 {{ downloadedImages.length }} 张</span>
          </div>
          <div class="download-grid">
            <div v-for="img in downloadedImages" :key="img.name" class="download-thumb">
              <img :src="img.path" :alt="img.name" class="download-thumb-img" loading="lazy" />
            </div>
          </div>
        </v-card-text>
      </v-card>
    </template>

    <v-snackbar v-model="localSnackbar" :timeout="3000" location="bottom" variant="tonal">
      {{ localSnackbarText }}
    </v-snackbar>
  </div>
</template>

<style scoped>
.source-card { overflow: hidden; }
.source-card-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 20px 24px 16px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}
.source-header-icon {
  width: 40px; height: 40px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.settings-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 12px;
}

.search-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 10px;
}

.search-item {
  position: relative;
  border-radius: var(--radius-md);
  overflow: hidden;
  cursor: pointer;
  transition: transform 0.25s var(--ease-out), box-shadow 0.25s var(--ease-out);
  box-shadow: 0 2px 8px rgba(0,0,0,0.2);
  aspect-ratio: 16 / 9;
  background: var(--surface-card);
}
.search-item:hover {
  transform: scale(1.03);
  box-shadow: 0 8px 24px rgba(0,0,0,0.4);
}
.search-item--selected {
  outline: 2px solid var(--accent-primary);
  outline-offset: -2px;
}
.search-thumb {
  width: 100%; height: 100%;
  object-fit: cover; display: block;
}
.search-overlay {
  position: absolute; bottom: 0; left: 0; right: 0;
  padding: 8px;
  background: linear-gradient(to top, rgba(0,0,0,0.7), transparent);
  display: flex; gap: 8px; align-items: center;
  opacity: 0; transition: opacity 0.2s;
}
.search-item:hover .search-overlay { opacity: 1; }
.search-res, .search-size { font-size: 0.6875rem; color: rgba(255,255,255,0.85); }
.search-check {
  position: absolute; top: 6px; right: 6px;
  filter: drop-shadow(0 1px 3px rgba(0,0,0,0.5));
}

.download-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(80px, 1fr));
  gap: 6px;
  max-height: 280px;
  overflow-y: auto;
}
.download-thumb {
  aspect-ratio: 16 / 9;
  border-radius: 4px;
  overflow: hidden;
  background: var(--surface-card);
}
.download-thumb-img {
  width: 100%; height: 100%;
  object-fit: cover; display: block;
}
</style>
