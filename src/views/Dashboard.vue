<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { logger } from "../utils/logger";
import DataTerminal from "../components/DataTerminal.vue";

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
      await invoke<boolean>("dislike_file", { source: img.source, name: img.name });
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
  if (str.length <= len) return str;
  return str.substring(0, len) + "…";
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
    <!-- Arknights 数据面板 - 双终端布局 -->
    <v-row class="stagger-cards">
      <DataTerminal
        badge="WH" title="Wallhaven" accent-color="#3b82f6"
        :total="whTotal" :love="whLove" :dislike="whDislike"
        :disabled="downloading" :stagger="1"
        @start-download="logger.action('Dashboard', '开始下载', { source: 'wallhaven' }); emit('action', () => invoke('start_wallhaven_download'))"
        @recover-files="logger.action('Dashboard', '下载所有喜欢的文件', { source: 'wallhaven' }); emit('action', () => invoke('recover_database_files', { source: 'wallhaven' }))"
      />
      <DataTerminal
        badge="RD" title="Reddit" accent-color="#f97316"
        :total="rdTotal" :love="rdLove" :dislike="rdDislike"
        :disabled="downloading" :stagger="2"
        @start-download="logger.action('Dashboard', '开始下载', { source: 'reddit' }); emit('action', () => invoke('start_reddit_download'))"
        @recover-files="logger.action('Dashboard', '下载所有喜欢的文件', { source: 'reddit' }); emit('action', () => invoke('recover_database_files', { source: 'reddit' }))"
      />
    </v-row>

    <!-- 下载进度面板 -->
    <v-row v-if="downloading" class="mt-4">
      <v-col cols="12">
        <div class="data-panel progress-panel animate-in stagger-3">
          <div class="panel-header">
            <div class="panel-header-left">
              <span class="panel-title">传输队列</span>
            </div>
            <div class="panel-progress-text">
              <span class="stat-number" style="font-size: 0.75rem; color: var(--accent-primary);">
                {{ Math.round(progressTotal > 0 ? (progressDone / progressTotal) * 100 : 0) }}%
              </span>
            </div>
          </div>
          <div class="progress-bar-container">
            <div
              class="progress-bar-fill"
              :style="{ width: progressTotal > 0 ? (progressDone / progressTotal) * 100 + '%' : '0%' }"
            />
          </div>
          <div class="progress-meta">
            <span class="panel-subtitle">{{ progressMsg }}</span>
            <span class="stat-label">{{ progressDone }}/{{ progressTotal }}</span>
          </div>
        </div>
      </v-col>
    </v-row>

    <!-- 维护操作面板 -->
    <v-row class="mt-4">
      <v-col cols="12">
        <div class="data-panel operations-panel animate-in stagger-3">
          <div class="panel-header">
            <div class="panel-header-left">
              <v-icon size="16" color="#f59e0b" class="me-2">mdi-wrench</v-icon>
              <span class="panel-title">维护操作</span>
            </div>
          </div>

          <div class="panel-ops-grid">
            <button
              class="ops-btn ops-btn--warning"
              :disabled="downloading"
              @click="emit('action', async () => {
                const count = await invoke<number>('mark_disliked_files', { source: 'all' });
                localSnackbarText.value = `已标记 ${count} 张缺失图片为不喜欢`;
                localSnackbar.value = true;
                logger.action('Dashboard', '标记缺失图片为不喜欢', { count });
                await loadStats();
                await loadMissingCount();
              })"
            >
              <v-icon size="14">mdi-alert-circle</v-icon>
              <span>标记缺失为不喜欢 <template v-if="missingCount > 0">({{ missingCount }})</template></span>
            </button>
            <button
              class="ops-btn ops-btn--success"
              :disabled="downloading"
              @click="emit('action', async () => {
                const count = await invoke<number>('restore_all_files', { source: 'all' });
                localSnackbarText.value = `已还原 ${count} 张图片为喜欢`;
                localSnackbar.value = true;
                logger.action('Dashboard', '全部恢复为喜欢', { count });
                await loadStats();
              })"
            >
              <v-icon size="14">mdi-check-circle</v-icon>
              <span>全部恢复为喜欢</span>
            </button>
          </div>
        </div>
      </v-col>
    </v-row>

    <!-- 缺失文件终端 -->
    <v-row class="mt-4">
      <v-col cols="12">
        <div class="data-panel animate-in stagger-4">
          <div class="panel-header">
            <div class="panel-header-left">
              <v-icon size="16" :color="missingCount > 0 ? '#f59e0b' : '#10b981'" class="me-2">
                {{ missingCount > 0 ? 'mdi-alert-circle' : 'mdi-check-circle' }}
              </v-icon>
              <span class="panel-title">缺失文件</span>
              <span v-if="missingCount > 0" class="missing-badge">{{ missingCount }}</span>
            </div>
            <button
              class="panel-toggle-btn"
              @click="showMissingList ? (showMissingList = false) : loadMissingList()"
            >
              <span>{{ showMissingList ? '收起' : '查看详情' }}</span>
              <v-icon size="12">{{ showMissingList ? 'mdi-chevron-up' : 'mdi-chevron-down' }}</v-icon>
            </button>
          </div>

          <v-expand-transition>
            <div v-if="showMissingList">
              <div class="panel-divider" />

              <div v-if="missingImages.length === 0 && !missingListLoading" class="panel-empty">
                <v-icon size="28" color="#10b981">mdi-check-circle-outline</v-icon>
                <span class="panel-subtitle">没有缺失文件</span>
              </div>

              <template v-else-if="missingImages.length > 0">
                <div class="missing-toolbar">
                  <div class="missing-filter-group">
                    <button
                      class="filter-chip"
                      :class="{ 'filter-chip--active': missingSource === 'all' }"
                      @click="missingSource = 'all'"
                    >全部</button>
                    <button
                      class="filter-chip"
                      :class="{ 'filter-chip--active': missingSource === 'wallhaven' }"
                      @click="missingSource = 'wallhaven'"
                    >WH</button>
                    <button
                      class="filter-chip"
                      :class="{ 'filter-chip--active': missingSource === 'reddit' }"
                      @click="missingSource = 'reddit'"
                    >RD</button>
                    <span class="filter-count">{{ filteredMissing.length }} 张</span>
                  </div>
                  <div class="missing-bulk-actions">
                    <button
                      class="bulk-btn bulk-btn--danger"
                      :disabled="filteredMissing.length === 0"
                      :class="{ 'bulk-btn--loading': dislikeAllLoading }"
                      @click="markAllMissingDislike"
                    >
                      <v-icon size="12">mdi-close-circle</v-icon>
                      <span>全部标记不喜欢</span>
                    </button>
                    <button
                      class="bulk-btn bulk-btn--primary"
                      :disabled="downloading || filteredMissing.length === 0"
                      @click="downloadAllMissing"
                    >
                      <v-icon size="12">mdi-database-sync</v-icon>
                      <span>全部补下载 ({{ filteredMissing.length }})</span>
                    </button>
                  </div>
                </div>

                <div class="missing-list">
                  <div v-for="img in filteredMissing" :key="img.id" class="data-row missing-item">
                    <div
                      class="missing-source-tag"
                      :class="img.source === 'wallhaven' ? 'source-wh' : 'source-rd'"
                    >
                      {{ img.source === 'wallhaven' ? 'WH' : 'RD' }}
                    </div>
                    <span class="missing-name">{{ img.name }}</span>
                    <span class="missing-url" :title="img.url">{{ truncate(img.url, 45) }}</span>
                    <div class="missing-actions">
                      <button
                        class="row-action-btn row-action-btn--danger"
                        :title="'标记不喜欢'"
                        @click="emit('action', async () => { await invoke('dislike_file', { source: img.source, name: img.name }); await loadMissingCount(); await loadMissingList(); })"
                      >
                        <v-icon size="12">mdi-close-circle</v-icon>
                      </button>
                      <button
                        class="row-action-btn row-action-btn--primary"
                        :disabled="downloading"
                        :title="'补下载'"
                        @click="emit('action', () => invoke('download_missing_images', { source: img.source, images: [img] }))"
                      >
                        <v-icon size="12">mdi-download</v-icon>
                      </button>
                    </div>
                  </div>
                </div>
              </template>

              <div v-if="missingListLoading" class="panel-loading">
                <div class="loading-spinner" />
                <span class="panel-subtitle">扫描中...</span>
              </div>
            </div>
          </v-expand-transition>
        </div>
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

/* ── Arknights 数据终端面板 ── */
.data-panel {
  position: relative;
  background: rgba(var(--surface-card-rgb), 0.55) !important;
  backdrop-filter: blur(16px) saturate(140%);
  -webkit-backdrop-filter: blur(16px) saturate(140%);
  border: var(--border-card);
  border-radius: var(--radius-md);
  overflow: hidden;
  box-shadow: var(--shadow-sm);
}

.operations-panel {
  border-left: 2px solid rgba(245, 158, 11, 0.4) !important;
}

.progress-panel {
  border-left: 2px solid rgba(59, 130, 246, 0.4) !important;
}

/* ── 面板头部 ── */
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px 12px;
}

.panel-header-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.panel-header-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.panel-title {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.875rem;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: 0.03em;
}

.panel-subtitle {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.625rem;
  font-weight: 500;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-tertiary);
}

/* ── 信号指示器 ── */
.panel-signal {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  opacity: 0.6;
}
/* ── 统计数据行 ── */
.panel-stats {
  display: flex;
  align-items: center;
  padding: 4px 16px 16px;
}

.stat-cell {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}

.stat-value {
  font-size: 1.75rem;
  font-weight: 700;
  color: var(--text-primary);
}

.stat-label {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.625rem;
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-tertiary);
}

.stat-divider {
  width: 1px;
  height: 32px;
  background: var(--border-subtle);
  flex-shrink: 0;
}

/* ── 面板分隔线 ── */
.panel-divider {
  height: 1px;
  background: linear-gradient(
    90deg,
    var(--accent-primary) 0%,
    var(--border-subtle) 30%,
    transparent 100%
  );
  opacity: 0.3;
}

/* ── 操作按钮 ── */
.panel-actions {
  display: flex;
  gap: 8px;
  padding: 12px 16px;
}

.panel-action-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 14px;
  border: none;
  border-radius: var(--radius-sm);
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.75rem;
  font-weight: 600;
  letter-spacing: 0.03em;
  cursor: pointer;
  transition: all 0.2s var(--ease-out);
}
.panel-action-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.panel-action-btn--primary {
  background: linear-gradient(135deg, var(--accent-primary) 0%, #2563eb 100%);
  color: #fff;
}
.panel-action-btn--primary:hover:not(:disabled) {
  box-shadow: var(--shadow-glow-strong);
  transform: translateY(-1px);
}
.panel-action-btn--primary:active:not(:disabled) {
  transform: scale(0.97);
}

.panel-action-btn--ghost {
  background: transparent;
  color: var(--text-secondary);
  border: var(--border-card);
}
.panel-action-btn--ghost:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-primary);
  border-color: var(--border-default);
}

/* ── 进度面板 ── */
.panel-progress-text {
  display: flex;
  align-items: center;
}

.progress-bar-container {
  height: 3px;
  background: rgba(255, 255, 255, 0.05);
  margin: 0 16px;
  border-radius: 2px;
  overflow: hidden;
}

.progress-bar-fill {
  height: 100%;
  background: linear-gradient(90deg, var(--accent-primary), #60a5fa);
  border-radius: 2px;
  transition: width 0.3s ease;
}

.progress-meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 16px 14px;
}

/* ── 维护操作 ── */
.panel-ops-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding: 12px 16px 16px;
}

.ops-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  border-radius: var(--radius-sm);
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.75rem;
  font-weight: 600;
  letter-spacing: 0.02em;
  cursor: pointer;
  transition: all 0.2s var(--ease-out);
  border: var(--border-card);
  background: transparent;
  color: var(--text-secondary);
}
.ops-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.ops-btn--warning {
  border-color: rgba(245, 158, 11, 0.2);
  color: #f59e0b;
}
.ops-btn--warning:hover:not(:disabled) {
  background: rgba(245, 158, 11, 0.08);
  border-color: rgba(245, 158, 11, 0.35);
}

.ops-btn--success {
  border-color: rgba(16, 185, 129, 0.2);
  color: #10b981;
}
.ops-btn--success:hover:not(:disabled) {
  background: rgba(16, 185, 129, 0.08);
  border-color: rgba(16, 185, 129, 0.35);
}

/* ── 缺失文件标记 ── */
.missing-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: var(--radius-sm);
  background: rgba(245, 158, 11, 0.15);
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.625rem;
  font-weight: 700;
  color: #f59e0b;
  line-height: 1;
}

.panel-toggle-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: all 0.15s ease;
}
.panel-toggle-btn:hover {
  color: var(--text-secondary);
  background: rgba(255, 255, 255, 0.03);
}

/* ── 空状态 ── */
.panel-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 24px;
}

/* ── 加载状态 ── */
.panel-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 16px;
}

.loading-spinner {
  width: 16px;
  height: 16px;
  border: 2px solid var(--border-subtle);
  border-top-color: var(--accent-primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}
@keyframes spin {
  to { transform: rotate(360deg); }
}

/* ── 缺失文件工具栏 ── */
.missing-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 16px;
  flex-wrap: wrap;
  gap: 8px;
}

.missing-filter-group {
  display: flex;
  align-items: center;
  gap: 4px;
}

.filter-chip {
  padding: 3px 10px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: all 0.15s ease;
}
.filter-chip:hover {
  color: var(--text-secondary);
  background: rgba(255, 255, 255, 0.03);
}
.filter-chip--active {
  color: var(--accent-primary);
  background: rgba(59, 130, 246, 0.1);
}

.filter-count {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--text-tertiary);
  margin-left: 6px;
}

.missing-bulk-actions {
  display: flex;
  gap: 6px;
}

.bulk-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border-radius: var(--radius-sm);
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.6875rem;
  font-weight: 600;
  letter-spacing: 0.02em;
  cursor: pointer;
  transition: all 0.15s ease;
  border: var(--border-card);
  background: transparent;
}
.bulk-btn:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}
.bulk-btn--danger {
  color: #ef4444;
  border-color: rgba(239, 68, 68, 0.15);
}
.bulk-btn--danger:hover:not(:disabled) {
  background: rgba(239, 68, 68, 0.08);
}
.bulk-btn--primary {
  color: var(--accent-primary);
  border-color: rgba(59, 130, 246, 0.2);
}
.bulk-btn--primary:hover:not(:disabled) {
  background: rgba(59, 130, 246, 0.08);
}

/* ── 缺失文件列表 ── */
.missing-list {
  display: flex;
  flex-direction: column;
  max-height: 320px;
  overflow-y: auto;
  padding: 0 16px 12px;
  gap: 2px;
}

.missing-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  background: rgba(255, 255, 255, 0.015);
  transition: background 0.15s;
  border-bottom: 1px solid rgba(255, 255, 255, 0.025);
}
.missing-item:hover {
  background: rgba(255, 255, 255, 0.035);
}

.missing-source-tag {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.625rem;
  font-weight: 700;
  padding: 1px 6px;
  border-radius: 2px;
  flex-shrink: 0;
}
.source-wh {
  background: rgba(59, 130, 246, 0.12);
  color: #3b82f6;
}
.source-rd {
  background: rgba(249, 115, 22, 0.12);
  color: #f97316;
}

.missing-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: 'Inter', system-ui, sans-serif;
  font-size: 0.75rem;
  color: var(--text-primary);
}

.missing-url {
  max-width: 200px;
  flex-shrink: 0;
  font-family: 'Inter', system-ui, sans-serif;
  font-size: 0.6875rem;
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.missing-actions {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity 0.15s;
}
.missing-item:hover .missing-actions {
  opacity: 1;
}

.row-action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  cursor: pointer;
  transition: all 0.15s ease;
}
.row-action-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}
.row-action-btn--danger { color: #ef4444; }
.row-action-btn--danger:hover:not(:disabled) { background: rgba(239, 68, 68, 0.1); }
.row-action-btn--primary { color: var(--accent-primary); }
.row-action-btn--primary:hover:not(:disabled) { background: rgba(59, 130, 246, 0.1); }
</style>
