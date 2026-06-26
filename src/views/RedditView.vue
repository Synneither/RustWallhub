<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { logger } from "../utils/logger";

defineProps<{
  downloading: boolean;
}>();

const emit = defineEmits<{
  action: [fn: () => Promise<unknown>];
}>();

interface DownloadedImage {
  name: string;
  path: string;
}

interface AppConfig {
  reddit_save_dir: string;
  reddit_db_path: string;
  reddit_url: string;
  reddit_max_posts: number;
  reddit_max_images: number;
}

const config = ref<AppConfig | null>(null);
const showSettings = ref(false);
const saving = ref(false);
const saved = ref(false);

const localSnackbar = ref(false);
const localSnackbarText = ref("");
const missingCount = ref(0);
const downloadedImages = ref<DownloadedImage[]>([]);
let alive = true;
let unlistenImageEvent: (() => void) | null = null;

const requiredRule = (v: string) => !!v || "此项不能为空";
const positiveInt = (v: number) => {
  if (v === undefined || v === null) return true;
  if (typeof v !== "number" || isNaN(v)) return "请输入有效数字";
  if (v < 0) return "不能为负数";
  return true;
};

async function loadConfig() {
  try {
    const c = await invoke<AppConfig>("get_config");
    config.value = {
      reddit_save_dir: c.reddit_save_dir,
      reddit_db_path: c.reddit_db_path,
      reddit_url: c.reddit_url,
      reddit_max_posts: c.reddit_max_posts,
      reddit_max_images: c.reddit_max_images,
    };
  } catch (e) {
    logger.error("Reddit", "配置加载失败", e);
  }
}

async function saveConfig() {
  if (!config.value) return;
  saving.value = true;
  saved.value = false;
  try {
    // 只修改 Reddit 相关字段，先获取完整配置再覆盖
    const full = await invoke<AppConfig & Record<string, unknown>>("get_config");
    Object.assign(full, config.value);
    await invoke("save_settings", { config: full });
    saved.value = true;
    setTimeout(() => (saved.value = false), 2000);
    logger.info("Reddit", "设置已保存");
  } catch (e) {
    localSnackbarText.value = `保存设置失败: ${e}`;
    localSnackbar.value = true;
  }
  saving.value = false;
}

async function loadMissingCount() {
  try {
    const count = await invoke<number>("count_missing_images", { source: "reddit" });
    if (!alive) return;
    missingCount.value = count;
    logger.info("Reddit", "缺失数量", count);
  } catch (e) {
    if (!alive) return;
    logger.error("Reddit", "缺失数量加载失败", e);
  }
}

onMounted(async () => {
  alive = true;
  loadConfig();
  loadMissingCount();

  unlistenImageEvent = await listen<DownloadedImage>("image-downloaded", (e) => {
    if (e.payload.source === "reddit") {
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
    }
  });
});

onUnmounted(() => {
  alive = false;
  if (unlistenImageEvent) unlistenImageEvent();
});
</script>

<template>
  <div>
    <v-card class="glass-card source-card animate-in stagger-1">
      <div class="source-card-header" style="background: linear-gradient(135deg, rgba(255,107,53,0.08) 0%, transparent 60%)">
        <div class="source-header-icon" style="background: rgba(255,107,53,0.15)">
          <v-icon color="#ff6b35">mdi-reddit</v-icon>
        </div>
        <div>
          <div class="text-heading">Reddit 下载</div>
          <div class="text-caption">从 Reddit r/Animewallpaper 抓取动漫壁纸</div>
        </div>
      </div>
      <v-card-text class="pa-4">
        <v-btn
          class="gradient-btn"
          size="large"
          variant="flat"
          :disabled="downloading"
          @click="() => { downloadedImages.value = []; logger.action('Reddit', '开始下载'); emit('action', () => invoke('start_reddit_download')); }"
        >
          <v-icon start>mdi-download</v-icon>
          开始下载
        </v-btn>
        <v-btn
          variant="tonal"
          size="large"
          class="ms-3"
          style="background: rgba(255,255,255,0.06)"
          :disabled="downloading"
          @click="() => { logger.action('Reddit', '从数据库补下载'); emit('action', () => invoke('start_db_download', { source: 'reddit' })); }"
        >
          <v-icon start>mdi-database-sync</v-icon>
          从数据库补下载
        </v-btn>

        <v-btn
          variant="text"
          color="grey"
          class="ms-2"
          @click="showSettings = !showSettings"
        >
          <v-icon start size="14">{{ showSettings ? 'mdi-chevron-up' : 'mdi-cog' }}</v-icon>
          Reddit 设置
        </v-btn>
      </v-card-text>
    </v-card>

    <!-- 设置面板 -->
    <v-expand-transition>
      <v-card v-if="showSettings && config" class="glass-card mt-3 pa-4">
        <div class="settings-grid">
          <v-text-field
            v-model="config.reddit_save_dir"
            label="保存目录"
            :rules="[requiredRule]"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model="config.reddit_db_path"
            label="数据库路径"
            :rules="[requiredRule]"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model="config.reddit_url"
            label="Reddit URL"
            hint="支持 flair 过滤"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model.number="config.reddit_max_posts"
            label="最大抓取帖子数"
            type="number"
            min="0"
            :rules="[positiveInt]"
            density="compact"
            hide-details="auto"
          />
          <v-text-field
            v-model.number="config.reddit_max_images"
            label="最大下载数量"
            type="number"
            min="0"
            :rules="[positiveInt]"
            density="compact"
            hide-details="auto"
          />
        </div>
        <div class="d-flex align-center mt-3">
          <v-btn color="primary" variant="tonal" size="small" :loading="saving" @click="saveConfig">
            <v-icon start size="14">mdi-content-save</v-icon>
            保存设置
          </v-btn>
          <v-fade-transition>
            <v-icon v-if="saved" color="success" class="ms-2" size="18">mdi-check-circle</v-icon>
          </v-fade-transition>
        </div>
      </v-card>
    </v-expand-transition>

    <!-- 下载中预览 -->
    <v-card v-if="downloading && downloadedImages.length > 0" class="glass-card mt-4 animate-in stagger-2">
      <v-card-text class="pa-4">
        <div class="d-flex align-center mb-3">
          <v-progress-circular indeterminate size="18" width="2" color="#ff6b35" class="me-2" />
          <span class="text-body font-weight-medium">已下载 {{ downloadedImages.length }} 张</span>
        </div>
        <div class="download-grid">
          <div v-for="img in downloadedImages" :key="img.name" class="download-thumb">
            <img :src="img.path" :alt="img.name" class="download-thumb-img" loading="lazy" />
          </div>
        </div>
      </v-card-text>
    </v-card>

    <v-card class="glass-card mt-4 animate-in stagger-3">
      <v-card-title class="font-weight-bold pa-4 pb-0">
        <div class="d-flex align-center">
          <v-icon class="me-2" color="text-secondary">mdi-wrench</v-icon>
          <span class="text-heading text-secondary">维护</span>
        </div>
      </v-card-title>
      <v-card-text>
        <v-btn variant="tonal" color="warning" class="me-3" :disabled="downloading"
          @click="emit('action', async () => {
            const count = await invoke('mark_dislike', { source: 'reddit' }) as number;
            localSnackbarText.value = `已标记 ${count} 张缺失图片为不喜欢`;
            localSnackbar.value = true;
            logger.action('Reddit', '标记缺失图片为不喜欢', { count });
            await loadMissingCount();
          })">
          <v-icon start>mdi-alert-circle</v-icon>
          标记缺失图片为不喜欢<template v-if="missingCount > 0">&nbsp;({{ missingCount }})</template>
        </v-btn>
        <v-btn variant="tonal" color="success" :disabled="downloading"
          @click="emit('action', async () => {
            const count = await invoke('restore_love', { source: 'reddit' }) as number;
            localSnackbarText.value = `已还原 ${count} 张图片为喜欢`;
            localSnackbar.value = true;
            logger.action('Reddit', '全部恢复为喜欢', { count });
          })">
          <v-icon start>mdi-check-circle</v-icon>
          全部恢复为喜欢
        </v-btn>
      </v-card-text>
    </v-card>

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
