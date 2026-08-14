<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, provide, ref, watch } from "vue";
import { useTheme as useVuetifyTheme } from "vuetify";
import { useTheme } from "./stores/theme";
import {
  appState,
  anyDownloadActive,
  askConfirm,
  bootstrap,
  dismissToast,
  ensureDatabases,
  registerGlobalListeners,
  toast,
} from "./stores/app";
import ConfirmDialog from "./components/ConfirmDialog.vue";

/* 视图懒加载 */
const DashboardView = defineAsyncComponent(() => import("./views/DashboardView.vue"));
const WallhavenView = defineAsyncComponent(() => import("./views/WallhavenView.vue"));
const RedditView = defineAsyncComponent(() => import("./views/RedditView.vue"));
const GalleryView = defineAsyncComponent(() => import("./views/GalleryView.vue"));
const DbSettingsView = defineAsyncComponent(() => import("./views/DbSettingsView.vue"));
const SettingsView = defineAsyncComponent(() => import("./views/SettingsView.vue"));

type ViewKey = "dashboard" | "wallhaven" | "reddit" | "gallery" | "database" | "settings";

const NAV: { key: ViewKey; label: string; icon: string; dot?: string }[] = [
  { key: "dashboard", label: "仪表盘", icon: "mdi-view-dashboard-outline" },
  { key: "wallhaven", label: "Wallhaven", icon: "mdi-image-multiple-outline", dot: "var(--accent-primary)" },
  { key: "reddit", label: "Reddit", icon: "mdi-reddit", dot: "var(--accent-reddit)" },
  { key: "gallery", label: "图库", icon: "mdi-image-album" },
  { key: "database", label: "数据库", icon: "mdi-database-outline" },
  { key: "settings", label: "设置", icon: "mdi-cog-outline" },
];

const currentView = ref<ViewKey>("dashboard");

const viewComponent = computed(() => {
  switch (currentView.value) {
    case "wallhaven": return WallhavenView;
    case "reddit": return RedditView;
    case "gallery": return GalleryView;
    case "database": return DbSettingsView;
    case "settings": return SettingsView;
    default: return DashboardView;
  }
});

function navigate(key: string) {
  currentView.value = key as ViewKey;
}

/** 供子页面跳转（如仪表盘快捷操作） */
provide("navigate", navigate);

/* ── 主题 ── */
const { theme, toggle } = useTheme();
const vuetifyTheme = useVuetifyTheme();

watch(
  theme,
  (t) => {
    vuetifyTheme.global.name.value = t === "dim" ? "arknights" : "light";
    document.documentElement.dataset.theme = t === "dim" ? "dim" : "light";
  },
  { immediate: true },
);

/* ── 启动 ── */
const dbPromptShown = ref(false);

onMounted(async () => {
  await registerGlobalListeners();
  await bootstrap();

  const s = appState.dbStatus;
  if (s && (!s.wallhaven_exists || !s.reddit_exists) && !dbPromptShown.value) {
    dbPromptShown.value = true;
    const missing: string[] = [];
    if (!s.wallhaven_exists) missing.push(s.wallhaven_path);
    if (!s.reddit_exists) missing.push(s.reddit_path);
    const ok = await askConfirm(
      "初始化数据库",
      `以下数据库文件不存在：\n${missing.join("\n")}\n\n是否现在创建？（不创建则图库与统计功能不可用）`,
      { confirmText: "创建" },
    );
    if (ok) {
      try {
        const created = await ensureDatabases();
        toast(created.length > 0 ? `已创建数据库：${created.join("、")}` : "数据库已就绪", "success");
      } catch (e) {
        toast(String(e), "error");
      }
    }
  }
});
</script>

<template>
  <v-app>
    <!-- 左侧导航 -->
    <v-navigation-drawer permanent width="208" class="app-nav">
      <div class="app-nav__brand">
        <v-icon icon="mdi-image-frame" size="22" color="primary" />
        <span class="text-heading">RustWallhub</span>
      </div>

      <v-list density="compact" nav class="app-nav__list">
        <v-list-item
          v-for="item in NAV"
          :key="item.key"
          :active="currentView === item.key"
          active-color="primary"
          rounded="sm"
          @click="navigate(item.key)"
        >
          <template #prepend>
            <v-icon :icon="item.icon" size="20" />
          </template>
          <v-list-item-title class="text-body">{{ item.label }}</v-list-item-title>
          <template v-if="item.dot" #append>
            <span class="app-nav__dot" :style="{ background: item.dot }" />
          </template>
        </v-list-item>
      </v-list>

      <template #append>
        <div class="app-nav__footer">
          <v-btn
            :icon="theme === 'dim' ? 'mdi-white-balance-sunny' : 'mdi-weather-night'"
            variant="text"
            size="small"
            @click="toggle"
          />
          <v-spacer />
          <transition name="view-fade">
            <div v-if="anyDownloadActive" class="app-nav__busy">
              <v-progress-circular indeterminate size="14" width="2" color="primary" />
              <span class="text-small">下载中</span>
            </div>
          </transition>
        </div>
      </template>
    </v-navigation-drawer>

    <!-- 主内容 -->
    <v-main class="app-main">
      <div v-if="!appState.booted" class="async-state">
        <v-progress-circular indeterminate color="primary" />
      </div>
      <div v-else-if="appState.bootError" class="async-state async-state--error">
        {{ appState.bootError }}
      </div>
      <transition v-else name="view-fade" mode="out-in">
        <!-- KeepAlive：切页不销毁组件，保留搜索结果/图库页码等状态 -->
        <KeepAlive>
          <component :is="viewComponent" :key="currentView" />
        </KeepAlive>
      </transition>
    </v-main>

    <!-- Toast -->
    <div class="toast-stack">
      <transition-group name="view-fade">
        <div
          v-for="t in appState.toasts"
          :key="t.id"
          class="toast"
          :class="`toast--${t.color}`"
        >
          <span class="toast__text">{{ t.text }}</span>
          <button class="toast__close" aria-label="关闭" @click="dismissToast(t.id)">
            <v-icon icon="mdi-close" size="14" />
          </button>
        </div>
      </transition-group>
    </div>

    <!-- 全局确认框 -->
    <ConfirmDialog />

    <!-- 更新安装遮罩 -->
    <v-overlay :model-value="appState.update.installing" persistent class="d-flex align-center justify-center">
      <div class="update-mask">
        <v-progress-circular indeterminate color="primary" size="44" />
        <p class="text-heading mt-4">正在安装更新</p>
        <p class="text-caption">安装完成后应用将自动重启</p>
      </div>
    </v-overlay>
  </v-app>
</template>

<style scoped>
.app-nav {
  background: var(--surface-deep) !important;
  border-right: 1px solid var(--border-subtle);
}
.app-nav__brand {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-5) var(--space-4) var(--space-4);
}
.app-nav__list {
  padding: 0 var(--space-2);
}
.app-nav__dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}
.app-nav__footer {
  display: flex;
  align-items: center;
  padding: var(--space-3) var(--space-4);
  border-top: 1px solid var(--border-subtle);
}
.app-nav__busy {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-secondary);
}
.app-main {
  background: var(--surface-base);
  height: 100vh;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.toast-stack {
  position: fixed;
  bottom: 20px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 2500;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  pointer-events: none;
}
.toast {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px 8px 18px;
  border-radius: var(--radius-full);
  font-size: 0.8125rem;
  background: var(--surface-elevated);
  color: var(--text-primary);
  border: var(--border-card);
  box-shadow: var(--shadow-lg);
  pointer-events: auto;
  max-width: min(560px, 80vw);
}
.toast__text {
  overflow-wrap: anywhere;
}
.toast__close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  flex: none;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
}
.toast__close:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}
.toast--success {
  border-color: color-mix(in srgb, var(--accent-success) 45%, transparent);
}
.toast--error {
  border-color: color-mix(in srgb, var(--accent-error) 45%, transparent);
  color: var(--accent-error);
}

.update-mask {
  display: flex;
  flex-direction: column;
  align-items: center;
  background: var(--surface-elevated);
  border: var(--border-card);
  border-radius: var(--radius-xl);
  padding: 36px 48px;
}
</style>
