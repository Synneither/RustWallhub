<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, defineAsyncComponent } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTheme as useVuetifyTheme } from "vuetify";
import { logger } from "./utils/logger";
import { useTheme } from "./stores/theme";

const Dashboard = defineAsyncComponent(() => import("./views/Dashboard.vue"));
const WallhavenView = defineAsyncComponent(() => import("./views/WallhavenView.vue"));
const RedditView = defineAsyncComponent(() => import("./views/RedditView.vue"));
const GalleryView = defineAsyncComponent(() => import("./views/GalleryView.vue"));
const DbSettingsView = defineAsyncComponent(() => import("./views/DbSettingsView.vue"));

// Theme sync
const { theme: appTheme, toggle: toggleTheme } = useTheme();
const vuetifyTheme = useVuetifyTheme();
const themeVars: Record<string, Record<string, string>> = {
  dim: {
    '--surface-deep': '#0a0b0e',
    '--surface-base': '#101218',
    '--surface-card': '#161827',
    '--surface-elevated': '#1c1f32',
    '--surface-hover': '#22263b',
    '--surface-card-rgb': '22, 24, 39',
    '--surface-deep-rgb': '10, 11, 14',
    '--text-primary': '#e2e4ea',
    '--text-secondary': '#8b8fa3',
    '--text-tertiary': '#5c6075',
    '--text-disabled': '#3b3e50',
    '--border-subtle': '#2a2d40',
    '--border-default': '#3a3d52',
  },
  light: {
    '--surface-deep': '#e2dfd8',
    '--surface-base': '#efece6',
    '--surface-card': '#faf8f5',
    '--surface-elevated': '#ffffff',
    '--surface-hover': '#f4f1eb',
    '--surface-card-rgb': '250, 248, 245',
    '--surface-deep-rgb': '226, 223, 216',
    '--text-primary': '#1a1b23',
    '--text-secondary': '#5c6075',
    '--text-tertiary': '#8b8fa3',
    '--text-disabled': '#b0b3c0',
    '--border-subtle': '#d4d2ce',
    '--border-default': '#b0b3c0',
  },
};

function applyTheme(t: 'dim' | 'light') {
  const root = document.documentElement;
  const vars = themeVars[t];
  for (const [key, val] of Object.entries(vars)) {
    root.style.setProperty(key, val);
  }
  root.setAttribute('data-theme', t);
  vuetifyTheme.name.value = t === 'dim' ? 'arknights' : 'light';
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) {
    meta.setAttribute('content', t === 'dim' ? '#101218' : '#efece6');
  }
}

watch(appTheme, (t) => applyTheme(t), { immediate: true });

const currentView = ref("dashboard");

watch(currentView, (to, from) => {
  logger.action("App", "导航切换", { from, to });
});
const drawer = ref(true);
const rail = ref(false);

interface ProgressEvent {
  source: string;
  done: number;
  total: number;
  message: string;
}

interface CompleteEvent {
  source: string;
  success: number;
  total: number;
  message: string;
}

const downloading = ref(false);
const progressMsg = ref("");
const progressDone = ref(0);
const progressTotal = ref(0);
const snackbar = ref(false);
const snackbarText = ref("");

let unlistenProgress: (() => void) | null = null;
let unlistenComplete: (() => void) | null = null;

onMounted(async () => {
  unlistenProgress = await listen<ProgressEvent>("download-progress", (e) => {
    downloading.value = true;
    progressMsg.value = e.payload.message;
    progressDone.value = e.payload.done;
    progressTotal.value = e.payload.total;
    logger.info("App", "下载进度", e.payload);
  });

  unlistenComplete = await listen<CompleteEvent>("download-complete", (e) => {
    downloading.value = false;
    progressMsg.value = "";
    progressDone.value = 0;
    progressTotal.value = 0;
    snackbarText.value = e.payload.message;
    snackbar.value = true;
    logger.info("App", "下载完成", e.payload);
  });
});

onUnmounted(() => {
  if (unlistenProgress) unlistenProgress();
  if (unlistenComplete) unlistenComplete();
});

const navItems = [
  { key: "dashboard", title: "仪表盘", icon: "mdi-view-dashboard-outline", iconActive: "mdi-view-dashboard" },
  { key: "wallhaven", title: "Wallhaven", icon: "mdi-image-search-outline", iconActive: "mdi-image-search" },
  { key: "reddit", title: "Reddit", icon: "mdi-reddit", iconActive: "mdi-reddit" },
  { key: "db", title: "数据库", icon: "mdi-database-cog-outline", iconActive: "mdi-database-cog" },
  { key: "gallery", title: "图库", icon: "mdi-image-multiple-outline", iconActive: "mdi-image-multiple" },
];

function onNavClick(item: { key: string; title: string }) {
  logger.action("App", "菜单点击", { key: item.key, title: item.title, rail: rail.value, drawer: drawer.value });
  drawer.value = true;
  rail.value = false;
  currentView.value = item.key;
}

async function runAction(fn: () => Promise<unknown>) {
  try {
    await fn();
  } catch (e) {
    logger.error("App", "操作失败", e);
    snackbarText.value = `错误: ${e}`;
    snackbar.value = true;
  }
}
</script>

<template>
  <v-app id="inspire">
    <!-- Arknights 风格侧边导航 -->
    <v-navigation-drawer
      v-model="drawer"
      :rail="rail"
      :width="240"
      rail-width="64"
      class="ark-drawer"
      @click="rail = false"
    >
      <div class="drawer-header">
        <div class="drawer-logo-area">
          <div class="drawer-logo-hex">
            <svg width="28" height="32" viewBox="0 0 28 32" fill="none">
              <polygon points="14,0 28,8 28,24 14,32 0,24 0,8" fill="rgba(59,130,246,0.15)" stroke="#3b82f6" stroke-width="1.5" />
              <polygon points="14,6 22,10 22,22 14,26 6,22 6,10" fill="rgba(59,130,246,0.1)" stroke="#3b82f6" stroke-width="0.8" />
            </svg>
          </div>
          <div class="drawer-logo-text" :class="{ 'drawer-logo-text--hidden': rail }">
            <span class="drawer-title">RustWallhub</span>
            <span class="drawer-subtitle">壁纸管理器</span>
          </div>
        </div>
        <div class="drawer-accent-line" />
      </div>

      <v-divider class="drawer-divider" />

      <div class="drawer-nav-wrapper">
        <div
          v-for="item in navItems"
          :key="item.key"
          class="drawer-nav-item"
          :class="{ 'drawer-nav-item--active': currentView === item.key }"
          @click="onNavClick(item)"
        >
          <div class="nav-item-indicator" v-if="currentView === item.key" />
          <v-icon
            :size="rail ? 20 : 20"
            class="nav-item-icon"
            :class="{ 'nav-item-icon--active': currentView === item.key }"
          >
            {{ currentView === item.key ? item.iconActive : item.icon }}
          </v-icon>
          <span
            class="nav-item-label"
            :class="{ 'nav-item-label--active': currentView === item.key }"
            v-show="!rail"
          >
            {{ item.title }}
          </span>
          <div
            class="nav-item-glow"
            v-if="currentView === item.key && !rail"
          />
        </div>
      </div>

      <template v-slot:append>
        <div class="drawer-footer">
          <v-btn
            :icon="rail ? 'mdi-chevron-right' : 'mdi-chevron-left'"
            variant="text"
            size="small"
            class="drawer-toggle-btn"
            @click.stop="rail = !rail"
          />
        </div>
      </template>
    </v-navigation-drawer>

    <!-- Arknights 风格顶栏 -->
    <v-app-bar flat density="compact" class="ark-topbar">
      <v-app-bar-nav-icon @click="drawer = !drawer" class="topbar-nav-icon" />
      <div v-if="downloading" class="topbar-dot" :style="{ background: progressMsg?.includes('Reddit') ? '#f97316' : '#3b82f6' }" />
      <v-app-bar-title class="topbar-title">
        {{ navItems.find(i => i.key === currentView)?.title }}
      </v-app-bar-title>

      <template v-if="downloading">
        <div class="topbar-progress-ring">
          <svg width="18" height="18" viewBox="0 0 18 18">
            <circle cx="9" cy="9" r="7.5" fill="none" stroke="rgba(59,130,246,0.15)" stroke-width="1.5" />
            <circle cx="9" cy="9" r="7.5" fill="none" stroke="#3b82f6" stroke-width="1.5"
              stroke-dasharray="47.12"
              :stroke-dashoffset="progressTotal > 0 ? 47.12 * (1 - progressDone / progressTotal) : 47.12"
              stroke-linecap="round"
              transform="rotate(-90 9 9)"
              style="transition: stroke-dashoffset 0.3s ease"
            />
          </svg>
        </div>
      </template>

      <v-btn
        :icon="appTheme === 'dim' ? 'mdi-weather-night' : 'mdi-weather-sunny'"
        variant="text"
        size="small"
        class="topbar-icon-btn"
        @click="toggleTheme"
      />

      <template v-if="downloading">
        <v-btn
          color="error"
          variant="tonal"
          size="small"
          class="topbar-cancel-btn"
          @click="async () => { logger.action('App', '取消下载'); try { await invoke('cancel_downloads'); } catch (e) { logger.error('App', '取消下载失败', e); } }"
        >
          <v-icon start size="14">mdi-stop-circle-outline</v-icon>
          取消
        </v-btn>
      </template>
    </v-app-bar>

    <v-main class="hex-bg">
      <div class="view-wrapper">
        <v-progress-linear
          v-if="downloading"
          :model-value="progressTotal > 0 ? (progressDone / progressTotal) * 100 : 0"
          height="2"
          color="#3b82f6"
        />
        <Transition name="view-fade" mode="out-in">
          <div :key="currentView" class="view-content">
            <Dashboard
              v-if="currentView === 'dashboard'"
              :downloading="downloading"
              :progress-msg="progressMsg"
              :progress-done="progressDone"
              :progress-total="progressTotal"
              @action="runAction"
            />
            <WallhavenView
              v-if="currentView === 'wallhaven'"
              :downloading="downloading"
              @action="runAction"
            />
            <RedditView
              v-if="currentView === 'reddit'"
              :downloading="downloading"
              @action="runAction"
            />
            <GalleryView v-if="currentView === 'gallery'" />
            <DbSettingsView v-if="currentView === 'db'" />
          </div>
        </Transition>
      </div>
    </v-main>

    <v-snackbar v-model="snackbar" :timeout="3000" location="bottom" class="ark-snackbar">
      {{ snackbarText }}
    </v-snackbar>
  </v-app>
</template>

<style>
/* ── Arknights 侧边导航 ── */
.ark-drawer {
  background: var(--surface-deep) !important;
  border-right: 1px solid rgba(59, 130, 246, 0.08) !important;
  transition: width 0.2s var(--ease-out) !important;
}

.drawer-header {
  padding: 20px 16px 12px;
  position: relative;
}

.drawer-logo-area {
  display: flex;
  align-items: center;
  gap: 10px;
}

.drawer-logo-hex {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 36px;
}

.drawer-logo-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  overflow: hidden;
  transition: opacity 0.15s ease;
}

.drawer-logo-text--hidden {
  opacity: 0;
  width: 0;
}

.drawer-title {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 1.1rem;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: 0.02em;
  line-height: 1.2;
}

.drawer-subtitle {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.65rem;
  font-weight: 500;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-tertiary);
}

.drawer-accent-line {
  height: 1px;
  background: linear-gradient(90deg, var(--accent-primary) 0%, transparent 70%);
  margin: 12px 0 0;
  opacity: 0.4;
}

.drawer-divider {
  border-color: rgba(255, 255, 255, 0.04) !important;
  margin: 0 12px !important;
}

.drawer-nav-wrapper {
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.drawer-nav-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: all 0.2s var(--ease-out);
  user-select: none;
  overflow: hidden;
}

.drawer-nav-item:hover {
  background: rgba(255, 255, 255, 0.03);
}

.drawer-nav-item--active {
  background: rgba(59, 130, 246, 0.08) !important;
}

.nav-item-indicator {
  position: absolute;
  left: 0;
  top: 50%;
  transform: translateY(-50%);
  width: 3px;
  height: 20px;
  background: var(--accent-primary);
  border-radius: 0 2px 2px 0;
  box-shadow: 0 0 8px rgba(59, 130, 246, 0.4);
}

.nav-item-icon {
  color: var(--text-tertiary);
  transition: color 0.2s ease;
  flex-shrink: 0;
  z-index: 1;
}

.nav-item-icon--active {
  color: var(--accent-primary) !important;
}

.drawer-nav-item:hover .nav-item-icon {
  color: var(--text-secondary);
}

.nav-item-label {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
  font-size: 0.8125rem;
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--text-secondary);
  transition: color 0.2s ease;
  z-index: 1;
}

.nav-item-label--active {
  color: var(--text-primary);
}

.drawer-nav-item:hover .nav-item-label {
  color: var(--text-primary);
}

.nav-item-glow {
  position: absolute;
  right: -20px;
  top: 50%;
  transform: translateY(-50%);
  width: 80px;
  height: 40px;
  background: radial-gradient(ellipse, rgba(59, 130, 246, 0.08), transparent);
  pointer-events: none;
}

.drawer-footer {
  display: flex;
  justify-content: center;
  padding: 8px;
  border-top: 1px solid rgba(255, 255, 255, 0.04);
  margin: 0 12px;
}

.drawer-toggle-btn {
  color: var(--text-tertiary) !important;
}

/* ── Arknights 顶栏 ── */
.ark-topbar {
  background: rgba(var(--surface-deep-rgb), 0.85) !important;
  backdrop-filter: blur(16px) saturate(140%);
  -webkit-backdrop-filter: blur(16px) saturate(140%);
  border-bottom: 1px solid rgba(59, 130, 246, 0.06) !important;
  transition: background 0.2s;
}

.topbar-nav-icon {
  color: var(--text-secondary) !important;
}

.topbar-title {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif !important;
  font-size: 0.875rem !important;
  font-weight: 700 !important;
  letter-spacing: 0.04em !important;
  color: var(--text-primary) !important;
}

.topbar-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  margin-left: 8px;
  flex-shrink: 0;
  box-shadow: 0 0 6px currentColor;
  animation: pulse 1.5s infinite;
}

.topbar-progress-ring {
  display: flex;
  align-items: center;
  margin-right: 4px;
}

.topbar-icon-btn {
  color: var(--text-tertiary) !important;
  margin-right: 4px;
}
.topbar-icon-btn:hover {
  color: var(--text-primary) !important;
}

.topbar-cancel-btn {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif !important;
  font-weight: 600 !important;
  letter-spacing: 0.03em !important;
  margin-right: 8px;
}

/* ── 视图容器 ── */
.view-wrapper {
  position: relative;
  min-height: 100%;
}

.view-content {
  padding: 20px 24px;
  position: relative;
  z-index: 1;
}

/* ── Snackbar ── */
.ark-snackbar {
  font-family: 'Rajdhani', 'Inter', system-ui, sans-serif;
}

/* ── 脉冲动画 ── */
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}
</style>
