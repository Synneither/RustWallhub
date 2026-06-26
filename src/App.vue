<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTheme as useVuetifyTheme } from "vuetify";
import { logger } from "./utils/logger";
import { useTheme } from "./stores/theme";
import { defineAsyncComponent } from "vue";

const Dashboard = defineAsyncComponent(() => import("./views/Dashboard.vue"));
const WallhavenView = defineAsyncComponent(() => import("./views/WallhavenView.vue"));
const RedditView = defineAsyncComponent(() => import("./views/RedditView.vue"));
const GalleryView = defineAsyncComponent(() => import("./views/GalleryView.vue"));

// Theme sync
const { theme: appTheme, toggle: toggleTheme } = useTheme();
const vuetifyTheme = useVuetifyTheme();
const themeVars: Record<string, Record<string, string>> = {
  dim: {
    '--surface-deep': '#18181b',
    '--surface-base': '#1e1f23',
    '--surface-card': '#27282d',
    '--surface-elevated': '#2f3036',
    '--surface-hover': '#37383e',
    '--surface-card-rgb': '39, 40, 45',
    '--surface-deep-rgb': '24, 24, 27',
    '--text-primary': '#e4e5e9',
    '--text-secondary': '#9ca0ab',
    '--text-tertiary': '#6b6f7a',
    '--text-disabled': '#4a4d55',
    '--border-subtle': '#383940',
    '--border-default': '#484a50',
  },
  light: {
    '--surface-deep': '#e8e5df',
    '--surface-base': '#f5f2ed',
    '--surface-card': '#ffffff',
    '--surface-elevated': '#faf8f5',
    '--surface-hover': '#f0ede8',
    '--surface-card-rgb': '255, 255, 255',
    '--surface-deep-rgb': '232, 229, 223',
    '--text-primary': '#1c1d22',
    '--text-secondary': '#6b6f7a',
    '--text-tertiary': '#9ca0ab',
    '--text-disabled': '#bcc0c8',
    '--border-subtle': '#d4d2ce',
    '--border-default': '#c0bdb8',
  },
};

function applyTheme(t: 'dim' | 'light') {
  const root = document.documentElement;
  const vars = themeVars[t];
  for (const [key, val] of Object.entries(vars)) {
    root.style.setProperty(key, val);
  }
  root.setAttribute('data-theme', t);
  vuetifyTheme.name.value = t === 'dim' ? 'dark' : 'light';
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) {
    meta.setAttribute('content', t === 'dim' ? '#1e1f23' : '#f5f2ed');
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
  { key: "dashboard", title: "仪表盘", icon: "mdi-view-dashboard" },
  { key: "wallhaven", title: "Wallhaven", icon: "mdi-image-search" },
  { key: "reddit", title: "Reddit", icon: "mdi-reddit" },
  { key: "gallery", title: "图库", icon: "mdi-image-multiple" },
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
    <v-navigation-drawer
      v-model="drawer"
      :rail="rail"
      :width="240"
      rail-width="64"
      class="sidebar-drawer"
      @click="rail = false"
    >
      <div class="sidebar-header">
        <div class="sidebar-logo">{{ rail ? 'RW' : 'RustWallhub' }}</div>
        <div v-if="!rail" class="sidebar-subtitle">壁纸管理器</div>
        <div class="sidebar-header-accent" />
      </div>
      <v-divider class="sidebar-divider" />
      <v-list density="compact" nav class="sidebar-nav">
        <v-list-item
          v-for="item in navItems"
          :key="item.key"
          :prepend-icon="item.icon"
          :title="item.title"
          :active="currentView === item.key"
          :color="currentView === item.key ? '#6c8cff' : undefined"
          variant="text"
          class="nav-item"
          @click="onNavClick(item)"
        />
      </v-list>
      <template v-slot:append>
        <div class="sidebar-rail-toggle">
          <v-btn
            :icon="rail ? 'mdi-chevron-right' : 'mdi-chevron-left'"
            variant="text"
            size="small"
            color="grey"
            @click.stop="rail = !rail"
          />
        </div>
      </template>
    </v-navigation-drawer>

    <v-app-bar flat density="compact" class="top-bar">
      <v-app-bar-nav-icon @click="drawer = !drawer" />
      <div v-if="downloading" class="download-indicator" :style="{ background: progressMsg?.includes('Reddit') ? '#ff6b35' : '#6c8cff' }" />
      <v-app-bar-title class="top-bar-title">
        {{ navItems.find(i => i.key === currentView)?.title }}
      </v-app-bar-title>

      <v-btn
        :icon="appTheme === 'dim' ? 'mdi-weather-night' : 'mdi-weather-sunny'"
        variant="text"
        size="small"
        class="theme-toggle-btn"
        @click="toggleTheme"
      />

      <template v-if="downloading">
        <v-btn
          color="error"
          variant="tonal"
          size="small"
          class="mr-3"
          @click="async () => { logger.action('App', '取消下载'); try { await invoke('cancel_download'); } catch (e) { logger.error('App', '取消下载失败', e); } }"
        >
          <v-icon start size="14">mdi-stop-circle</v-icon>
          取消
        </v-btn>
      </template>
    </v-app-bar>

    <v-main>
      <div class="view-wrapper">
        <v-progress-linear
          v-if="downloading"
          :model-value="progressTotal > 0 ? (progressDone / progressTotal) * 100 : 0"
          height="3"
          color="#6c8cff"
        />
        <Transition name="view-fade" mode="out-in">
          <div :key="currentView">
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
          </div>
        </Transition>
      </div>
    </v-main>

    <v-snackbar v-model="snackbar" :timeout="3000" location="bottom">
      {{ snackbarText }}
    </v-snackbar>
  </v-app>
</template>

<style>
.sidebar-drawer {
  background: var(--surface-base) !important;
  border-right: 1px solid rgba(255, 255, 255, 0.06) !important;
  transition: width 0.2s var(--ease-out) !important;
}

.sidebar-header {
  padding: 20px 16px 12px;
  position: relative;
}

.sidebar-logo {
  font-size: 1.125rem;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: -0.01em;
  transition: all 0.2s var(--ease-out);
}

.sidebar-subtitle {
  font-size: 0.6875rem;
  letter-spacing: 0.05em;
  color: var(--text-secondary);
  margin-top: 2px;
}

.sidebar-header-accent {
  height: 2px;
  background: linear-gradient(90deg, var(--accent-primary), transparent);
  margin: 12px 0 0;
  opacity: 0.4;
  border-radius: 1px;
}

.sidebar-divider {
  border-color: rgba(255, 255, 255, 0.06) !important;
  margin: 0 12px !important;
}

.sidebar-nav {
  padding: 8px !important;
}

.nav-item {
  border-radius: 8px;
  margin-bottom: 2px;
  transition: all 0.25s var(--ease-out);
  position: relative;
}

.nav-item:hover {
  background: rgba(255, 255, 255, 0.04);
}

.nav-item.v-list-item--active {
  background: linear-gradient(90deg, rgba(108, 140, 255, 0.12) 0%, transparent 100%) !important;
  box-shadow: inset 3px 0 0 var(--accent-primary), 0 0 0 1px rgba(108, 140, 255, 0.06);
}

.nav-item.v-list-item--active::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 8px;
  box-shadow: 0 0 16px rgba(108, 140, 255, 0.08);
  pointer-events: none;
}

.sidebar-rail-toggle {
  display: flex;
  justify-content: center;
  padding: 8px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  margin: 0 12px;
}

.view-wrapper {
  position: relative;
  min-height: 100%;
}

.top-bar {
  background: rgba(var(--surface-card-rgb), 0.85) !important;
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border-bottom: 1px solid rgba(255, 255, 255, 0.06) !important;
  transition: background 0.2s;
}

.top-bar-title {
  font-size: 0.875rem !important;
  font-weight: 600 !important;
  color: var(--text-primary) !important;
}

.download-indicator {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-left: 8px;
  flex-shrink: 0;
  animation: pulse 1.5s infinite;
}
</style>
