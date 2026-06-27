<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { logger } from "../utils/logger";

const props = defineProps<{
  downloading: boolean;
  progressMsg: string;
  progressDone: number;
  progressTotal: number;
}>();

const emit = defineEmits<{
  action: [fn: () => Promise<unknown>];
}>();

interface DbStats {
  total: number;
  love: number;
  dislike: number;
}

const whStats = ref<DbStats>({ total: 0, love: 0, dislike: 0 });
const rdStats = ref<DbStats>({ total: 0, love: 0, dislike: 0 });
const localSnackbar = ref(false);
const localSnackbarText = ref("");
const missingCount = ref(0);
const showMissingList = ref(false);
const missingSource = ref("all");
const missingImages = ref<ImageRecord[]>([]);
let unlistenSettings: UnlistenFn | null = null;
const missingListLoading = ref(false);
const dislikeAllLoading = ref(false);

interface ImageRecord {
  id: number;
  name: string;
  hash: string;
  url: string;
  source_url: string;
  resolution: string;
  title?: string | null;
  permalink?: string | null;
  love: number;
  created_at: string;
  source: string;
}

async function loadStats() {
  try {
    const data = await invoke<{ wallhaven: DbStats; reddit: DbStats }>("get_stats");
    whStats.value = data.wallhaven;
    rdStats.value = data.reddit;
    logger.info("Dashboard", "统计已加载", { wh: data.wallhaven, rd: data.reddit });
  } catch (e) {
    logger.error("Dashboard", "统计加载失败", e);
  }
}

const filteredMissing = computed(() => {
  if (missingSource.value === "all") return missingImages.value;
  return missingImages.value.filter((img) => img.source === missingSource.value);
});

async function loadMissingList() {
  missingListLoading.value = true;
  try {
    missingImages.value = await invoke<ImageRecord[]>("list_missing_images", { source: "all" });
    showMissingList.value = true;
    logger.info("Dashboard", "缺失列表已加载", { count: missingImages.value.length });
  } catch (e) {
    logger.error("Dashboard", "加载缺失列表失败", e);
  }
  missingListLoading.value = false;
}

async function downloadAllMissing() {
  if (filteredMissing.value.length === 0) return;
  const bySource: Record<string, ImageRecord[]> = {};
  for (const img of filteredMissing.value) {
    if (!bySource[img.source]) bySource[img.source] = [];
    bySource[img.source].push(img);
  }
  for (const [src, imgs] of Object.entries(bySource)) {
    emit("action", () => invoke("download_missing_images", { source: src, images: imgs }));
  }
}

async function markAllMissingDislike() {
  if (filteredMissing.value.length === 0) return;
  dislikeAllLoading.value = true;
  let count = 0;
  for (const img of filteredMissing.value) {
    try {
      await invoke<boolean>("mark_dislike_image", { source: img.source, name: img.name });
      count++;
    } catch (e) {
      logger.error("Dashboard", "标记失败", { name: img.name, error: e });
    }
  }
  localSnackbarText.value = `已标记 ${count} 张为不喜欢`;
  localSnackbar.value = true;
  logger.action("Dashboard", "全部标记为不喜欢", { count });
  await loadMissingCount();
  await loadMissingList();
  dislikeAllLoading.value = false;
}

function truncate(str: string, len: number): string {
  if (str.length <= len) return str.substring(0, len) + "…";
  return str;
}

async function loadMissingCount() {
  try {
    const count = await invoke<number>("count_missing_images", { source: "all" });
    missingCount.value = count;
    logger.info("Dashboard", "缺失数量", { count });
  } catch (e) {
    logger.error("Dashboard", "缺失数量加载失败", e);
  }
}

watch(() => props.downloading, (now, prev) => {
  if (prev === true && now === false) {
    loadStats();
    loadMissingCount();
    if (showMissingList.value) loadMissingList();
  }
});

onMounted(async () => {
  await loadStats();
  await loadMissingCount();
  unlistenSettings = await listen("settings-changed", () => {
    logger.info("Dashboard", "设置已变更，刷新仪表盘");
    loadStats();
    loadMissingCount();
  });
});

onUnmounted(() => {
  if (unlistenSettings) unlistenSettings();
});

function useAnimatedNumber(target: { value: number }) {
  const value = ref(0);
  let rafId = 0;

  watch(target, (to) => {
    cancelAnimationFrame(rafId);
    const from = value.value;
    const start = performance.now();
    const duration = 800;

    function tick() {
      const elapsed = performance.now() - start;
      const progress = Math.min(elapsed / duration, 1);
      const eased = 1 - Math.pow(1 - progress, 3);
      value.value = Math.round(from + (to - from) * eased);
      if (progress < 1) rafId = requestAnimationFrame(tick);
    }
    rafId = requestAnimationFrame(tick);
  });

  onUnmounted(() => cancelAnimationFrame(rafId));

  return value;
}

const whTotal = useAnimatedNumber(computed(() => whStats.value.total));
const whLove = useAnimatedNumber(computed(() => whStats.value.love));
const whDislike = useAnimatedNumber(computed(() => whStats.value.dislike));
const rdTotal = useAnimatedNumber(computed(() => rdStats.value.total));
const rdLove = useAnimatedNumber(computed(() => rdStats.value.love));
const rdDislike = useAnimatedNumber(computed(() => rdStats.value.dislike));
</script>

<template>
  <div>
    <v-row class="stagger-cards">
      <v-col cols="12" md="6" class="animate-in stagger-1">
        <v-card
          class="glass-card card-accent-top wh-card card-hover"
          elevation="0"
        >
          <div class="card-bg-icon">
            <v-icon size="112" color="white">mdi-image-search</v-icon>
          </div>
          <v-card-text class="pa-5 pb-4">
            <div class="d-flex align-center mb-4">
              <div class="accent-dot wh-dot" />
              <span class="text-heading font-weight-medium wh-label">Wallhaven</span>
            </div>
            <v-row>
              <v-col cols="4" class="text-center py-0">
                <div class="stat-number text-display">{{ whTotal }}</div>
                <div class="text-caption text-medium-emphasis mt-1">总计</div>
              </v-col>
              <v-col cols="4" class="text-center py-0">
                <div class="stat-number text-display" style="color: #4ade80">{{ whLove }}</div>
                <div class="text-caption text-medium-emphasis mt-1">喜欢</div>
              </v-col>
              <v-col cols="4" class="text-center py-0">
                <div class="stat-number text-display" style="color: #fbbf24">{{ whDislike }}</div>
                <div class="text-caption text-medium-emphasis mt-1">不喜欢</div>
              </v-col>
            </v-row>
          </v-card-text>
          <v-divider style="border-color: rgba(255,255,255,0.06)" />
          <v-card-actions class="pa-4 pt-3">
            <v-btn
              class="gradient-btn text-none px-5"
              color="primary"
              variant="flat"
              :disabled="downloading"
              @click="() => { logger.action('Dashboard', '开始下载', { source: 'wallhaven' }); emit('action', () => invoke('start_wallhaven_download')); }"
            >
              <v-icon start>mdi-download</v-icon>
              开始下载
            </v-btn>
            <v-btn
              variant="tonal"
              class="text-none px-4"
              :disabled="downloading"
              @click="() => { logger.action('Dashboard', '从数据库补下载', { source: 'wallhaven' }); emit('action', () => invoke('start_db_download', { source: 'wallhaven' })); }"
            >
              <v-icon start>mdi-database-sync</v-icon>
              从数据库补下载
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-col>

      <v-col cols="12" md="6" class="animate-in stagger-2">
        <v-card
          class="glass-card card-accent-top rd-card card-hover"
          elevation="0"
        >
          <div class="card-bg-icon">
            <v-icon size="112" color="white">mdi-reddit</v-icon>
          </div>
          <v-card-text class="pa-5 pb-4">
            <div class="d-flex align-center mb-4">
              <div class="accent-dot rd-dot" />
              <span class="text-heading font-weight-medium rd-label">Reddit</span>
            </div>
            <v-row>
              <v-col cols="4" class="text-center py-0">
                <div class="stat-number text-display">{{ rdTotal }}</div>
                <div class="text-caption text-medium-emphasis mt-1">总计</div>
              </v-col>
              <v-col cols="4" class="text-center py-0">
                <div class="stat-number text-display" style="color: #4ade80">{{ rdLove }}</div>
                <div class="text-caption text-medium-emphasis mt-1">喜欢</div>
              </v-col>
              <v-col cols="4" class="text-center py-0">
                <div class="stat-number text-display" style="color: #fbbf24">{{ rdDislike }}</div>
                <div class="text-caption text-medium-emphasis mt-1">不喜欢</div>
              </v-col>
            </v-row>
          </v-card-text>
          <v-divider style="border-color: rgba(255,255,255,0.06)" />
          <v-card-actions class="pa-4 pt-3">
            <v-btn
              class="gradient-btn text-none px-5"
              color="primary"
              variant="flat"
              :disabled="downloading"
              @click="() => { logger.action('Dashboard', '开始下载', { source: 'reddit' }); emit('action', () => invoke('start_reddit_download')); }"
            >
              <v-icon start>mdi-download</v-icon>
              开始下载
            </v-btn>
            <v-btn
              variant="tonal"
              class="text-none px-4"
              :disabled="downloading"
              @click="() => { logger.action('Dashboard', '从数据库补下载', { source: 'reddit' }); emit('action', () => invoke('start_db_download', { source: 'reddit' })); }"
            >
              <v-icon start>mdi-database-sync</v-icon>
              从数据库补下载
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-col>
    </v-row>

    <v-row class="mt-4">
      <v-col cols="12">
        <v-card class="glass-card maintenance-card animate-in stagger-3" elevation="0">
          <v-card-text class="pa-5">
            <div class="d-flex align-center mb-4">
              <v-icon class="me-2" color="text-secondary">mdi-wrench</v-icon>
              <span class="text-heading font-weight-medium text-secondary">维护操作</span>
            </div>
            <div class="d-flex flex-wrap ga-3">
              <v-btn
                variant="tonal"
                color="warning"
                class="text-none px-4"
                :disabled="downloading"
                @click="emit('action', async () => {
                  const count = await invoke('mark_dislike', { source: 'all' }) as number;
                  localSnackbarText.value = `已标记 ${count} 张缺失图片为不喜欢`;
                  localSnackbar.value = true;
                  logger.action('Dashboard', '标记缺失图片为不喜欢', { count });
                  await loadStats();
                  await loadMissingCount();
                })"
              >
                <v-icon start>mdi-alert-circle</v-icon>
                标记缺失图片为不喜欢<template v-if="missingCount > 0">&nbsp;({{ missingCount }})</template>
              </v-btn>
              <v-btn
                variant="tonal"
                color="success"
                class="text-none px-4"
                :disabled="downloading"
                @click="emit('action', async () => {
                  const count = await invoke('restore_love', { source: 'all' }) as number;
                  localSnackbarText.value = `已还原 ${count} 张图片为喜欢`;
                  localSnackbar.value = true;
                  logger.action('Dashboard', '全部恢复为喜欢', { count });
                  await loadStats();
                })"
              >
                <v-icon start>mdi-check-circle</v-icon>
                全部恢复为喜欢
              </v-btn>
            </div>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>

    <v-row v-if="downloading" class="mt-4">
      <v-col cols="12">
        <v-card class="glass-card" elevation="0">
          <v-card-text class="pa-5">
            <v-progress-linear
              :model-value="props.progressTotal > 0 ? (props.progressDone / props.progressTotal) * 100 : 0"
              color="primary"
              height="4"
              rounded
              class="mb-3"
            />
            <div class="text-body-2 text-secondary">{{ progressMsg }}</div>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>

    <!-- 缺失文件列表 -->
    <v-row class="mt-4">
      <v-col cols="12">
        <v-card class="glass-card animate-in stagger-4" elevation="0">
          <v-card-text class="pa-5">
            <div class="d-flex align-center mb-3">
              <v-icon class="me-2" :color="missingCount > 0 ? 'warning' : 'success'">
                {{ missingCount > 0 ? 'mdi-alert-circle' : 'mdi-check-circle' }}
              </v-icon>
              <span class="text-heading font-weight-medium text-secondary">
                缺失文件 <v-chip v-if="missingCount > 0" size="x-small" color="warning" variant="tonal" class="ms-1">{{ missingCount }}</v-chip>
              </span>
              <v-spacer />
              <v-btn
                variant="text"
                size="small"
                color="grey"
                :loading="missingListLoading"
                @click="showMissingList ? (showMissingList = false) : loadMissingList()"
              >
                <v-icon start size="14">{{ showMissingList ? 'mdi-chevron-up' : 'mdi-chevron-down' }}</v-icon>
                {{ showMissingList ? '收起' : '查看详情' }}
              </v-btn>
            </div>

            <v-expand-transition>
              <div v-if="showMissingList">
                <div v-if="missingImages.length === 0 && !missingListLoading" class="text-center pa-4">
                  <v-icon size="40" color="success" class="mb-2">mdi-check-circle-outline</v-icon>
                  <p class="text-body-2 text-secondary">没有缺失文件</p>
                </div>

                <template v-else-if="missingImages.length > 0">
                  <div class="d-flex align-center ga-2 mb-3 flex-wrap">
                    <v-btn-toggle v-model="missingSource" mandatory color="primary" density="compact" rounded="pill">
                      <v-btn value="all" size="x-small">全部</v-btn>
                      <v-btn value="wallhaven" size="x-small">WH</v-btn>
                      <v-btn value="reddit" size="x-small">RD</v-btn>
                    </v-btn-toggle>
                    <v-chip variant="tonal" size="x-small" color="warning">{{ filteredMissing.length }} 张</v-chip>
                    <v-spacer />
                    <v-btn size="x-small" color="error" variant="tonal" :loading="dislikeAllLoading" :disabled="filteredMissing.length === 0" @click="markAllMissingDislike">
                      <v-icon start size="12">mdi-close-circle</v-icon>
                      全部标记不喜欢
                    </v-btn>
                    <v-btn size="x-small" color="primary" variant="flat" class="gradient-btn" :disabled="downloading || filteredMissing.length === 0" @click="downloadAllMissing">
                      <v-icon start size="12">mdi-database-sync</v-icon>
                      全部补下载 ({{ filteredMissing.length }})
                    </v-btn>
                  </div>

                  <div class="missing-file-list">
                    <div v-for="img in filteredMissing" :key="img.id" class="missing-file-item">
                      <v-chip size="x-small" :color="img.source === 'wallhaven' ? '#6c8cff' : '#ff6b35'" variant="tonal" class="me-2">
                        {{ img.source === 'wallhaven' ? 'WH' : 'RD' }}
                      </v-chip>
                      <span class="text-caption missing-file-name">{{ img.name }}</span>
                      <span class="text-caption text-disabled text-truncate missing-file-url" :title="img.url">{{ truncate(img.url, 50) }}</span>
                      <div class="missing-file-actions">
                        <v-btn size="x-small" variant="text" color="error" @click="emit('action', async () => { await invoke('mark_dislike_image', { source: img.source, name: img.name }); await loadMissingCount(); await loadMissingList(); })">
                          <v-icon size="12">mdi-close-circle</v-icon>
                        </v-btn>
                        <v-btn size="x-small" variant="text" color="primary" :disabled="downloading" @click="emit('action', () => invoke('download_missing_images', { source: img.source, images: [img] }))">
                          <v-icon size="12">mdi-download</v-icon>
                        </v-btn>
                      </div>
                    </div>
                  </div>
                </template>

                <div v-if="missingListLoading" class="text-center pa-4">
                  <v-progress-circular indeterminate size="20" width="2" color="primary" />
                </div>
              </div>
            </v-expand-transition>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>

    <v-snackbar v-model="localSnackbar" :timeout="3000" location="bottom" variant="tonal">
      {{ localSnackbarText }}
    </v-snackbar>
  </div>
</template>

<style scoped>
.stagger-cards {
  position: relative;
}

.wh-card {
  background: linear-gradient(135deg, rgba(108,140,255,0.08) 0%, rgba(var(--surface-card-rgb), 0.7) 100%) !important;
}
.rd-card {
  background: linear-gradient(135deg, rgba(255,107,53,0.08) 0%, rgba(var(--surface-card-rgb), 0.7) 100%) !important;
}

.wh-dot {
  background: #6c8cff;
  box-shadow: 0 0 8px #6c8cff;
}
.rd-dot {
  background: #ff6b35;
  box-shadow: 0 0 8px #ff6b35;
}

.wh-label {
  color: #6c8cff;
}
.rd-label {
  color: #ff6b35;
}

.card-bg-icon {
  position: absolute;
  right: -8px;
  bottom: -8px;
  opacity: 0.04;
  pointer-events: none;
  line-height: 1;
}

.maintenance-card {
  border-left: 3px solid var(--accent-warning);
}

.missing-file-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 320px;
  overflow-y: auto;
}
.missing-file-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.02);
  transition: background 0.15s;
}
.missing-file-item:hover {
  background: rgba(255, 255, 255, 0.05);
}
.missing-file-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  color: var(--text-primary);
}
.missing-file-url {
  max-width: 200px;
  flex-shrink: 0;
}
.missing-file-actions {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity 0.15s;
}
.missing-file-item:hover .missing-file-actions {
  opacity: 1;
}
</style>
