<script setup lang="ts">
import { computed, inject, onActivated, onMounted, ref } from "vue";
import { appState, dbReady, refreshStats, toast, toastError } from "../stores/app";
import { assetUrl, getActiveWallpaper, startWallhavenDownload, startRedditDownload, stopSlideshow } from "../utils/api";
import StatPanel from "../components/StatPanel.vue";
import ProgressCard from "../components/ProgressCard.vue";
import EmptyState from "../components/EmptyState.vue";

const navigate = inject<(key: string) => void>("navigate", () => {});

const starting = ref<"" | "wallhaven" | "reddit">("");

const slideshow = computed(() => appState.slideshow);
const updateInfo = computed(() => appState.update.info);

const activeSources = computed(() =>
  Object.entries(appState.downloads)
    .filter(([, t]) => t.active || t.lastComplete)
    .map(([s]) => s),
);

const SOURCE_LABEL: Record<string, string> = {
  wallhaven: "Wallhaven 下载",
  reddit: "Reddit 下载",
  all: "补下载任务",
};

async function quickDownload(source: "wallhaven" | "reddit") {
  if (starting.value) return;
  starting.value = source;
  try {
    const msg =
      source === "wallhaven"
        ? await startWallhavenDownload()
        : await startRedditDownload();
    toast(msg, "info");
  } catch (e) {
    toastError(e);
  } finally {
    starting.value = "";
  }
}

async function onStopSlideshow() {
  try {
    const stopped = await stopSlideshow();
    appState.slideshow.running = false;
    appState.slideshow.current = null;
    toast(stopped ? "轮播已停止" : "轮播未在运行", "info");
  } catch (e) {
    toastError(e);
  }
}

onMounted(() => {
  if (dbReady.value) refreshStats();
  loadActiveWallpaper();
});

// KeepAlive 下切走再切回时重载「当前壁纸」，否则在图库换壁纸后返回仍显示旧值。
onActivated(() => {
  loadActiveWallpaper();
});

/* ── 当前壁纸 ── */
const wallpaperPath = ref<string | null>(null);
const wallpaperImgError = ref(false);

const wallpaperName = computed(() => {
  const p = wallpaperPath.value;
  if (!p) return "";
  return p.split(/[\\/]/).pop() ?? p;
});

async function loadActiveWallpaper() {
  try {
    const res = await getActiveWallpaper();
    wallpaperPath.value = res.path;
    wallpaperImgError.value = false;
  } catch {
    wallpaperPath.value = null;
  }
}
</script>

<template>
  <div class="view">
    <div class="view-header">
      <span class="view-header__title">仪表盘</span>
      <span class="view-header__sub">全局状态总览</span>
    </div>

    <!-- 数据库未初始化 -->
    <EmptyState
      v-if="!dbReady"
      icon="mdi-database-alert-outline"
      title="数据库未初始化"
      desc="图库与统计功能需要数据库。请前往「数据库」页面完成初始化。"
    >
      <v-btn color="primary" variant="flat" @click="navigate('database')">
        前往数据库管理
      </v-btn>
    </EmptyState>

    <template v-else>
      <!-- 更新横幅 -->
      <div v-if="updateInfo?.has_update" class="panel-card update-banner animate-in">
        <v-icon icon="mdi-rocket-launch-outline" color="primary" size="22" />
        <div class="update-banner__text">
          <span class="text-body-lg">发现新版本 v{{ updateInfo.version }}</span>
          <span class="text-caption">当前 v{{ updateInfo.current_version }}</span>
        </div>
        <v-spacer />
        <v-btn variant="tonal" color="primary" size="small" @click="navigate('settings')">
          查看更新
        </v-btn>
      </div>

      <!-- 统计 -->
      <div class="dash-stats">
        <StatPanel source="wallhaven" :stats="appState.stats?.wallhaven ?? null" :loading="!appState.stats" class="animate-in stagger-1" />
        <StatPanel source="reddit" :stats="appState.stats?.reddit ?? null" :loading="!appState.stats" class="animate-in stagger-2" />
      </div>

      <!-- 当前壁纸 -->
      <div v-if="wallpaperPath" class="panel-card wallpaper-card animate-in stagger-3">
        <div class="wallpaper-card__thumb">
          <img
            v-if="!wallpaperImgError"
            :src="assetUrl(wallpaperPath)"
            :alt="wallpaperName"
            @error="wallpaperImgError = true"
          />
          <v-icon v-else icon="mdi-image-off-outline" size="28" class="wallpaper-card__thumb-fallback" />
        </div>
        <div class="wallpaper-card__meta">
          <span class="text-label">当前壁纸</span>
          <span class="text-body wallpaper-card__name">{{ wallpaperName }}</span>
          <span class="text-caption wallpaper-card__path">{{ wallpaperPath }}</span>
        </div>
        <v-spacer />
        <v-btn variant="text" size="small" prepend-icon="mdi-image-album" @click="navigate('gallery')">
          去图库换一张
        </v-btn>
      </div>

      <!-- 活动任务 -->
      <div v-if="activeSources.length > 0 || slideshow.running" class="dash-activity">
        <ProgressCard
          v-for="s in activeSources"
          :key="s"
          :source="s"
          :title="SOURCE_LABEL[s] ?? `${s} 下载`"
          class="animate-in"
        />

        <div v-if="slideshow.running" class="data-panel progress-panel slideshow-card animate-in">
          <div class="slideshow-card__head">
            <v-icon icon="mdi-play-circle-outline" size="18" color="primary" />
            <span class="text-label">壁纸轮播中</span>
            <v-spacer />
            <v-btn size="x-small" variant="text" color="error" @click="onStopSlideshow">停止</v-btn>
          </div>
          <div class="text-caption slideshow-card__current">
            <template v-if="slideshow.current">
              {{ slideshow.current.index + 1 }} / {{ slideshow.current.total }} · {{ slideshow.current.name }}
            </template>
            <template v-else>等待下一次切换…</template>
          </div>
        </div>
      </div>

      <!-- 快捷操作 -->
      <div class="panel-card animate-in stagger-3">
        <span class="panel-card__title">快捷操作</span>
        <div class="dash-actions">
          <v-btn color="primary" variant="flat" prepend-icon="mdi-image-album" @click="navigate('gallery')">
            浏览图库
          </v-btn>
          <v-btn
            variant="tonal"
            prepend-icon="mdi-download-outline"
            :loading="starting === 'wallhaven'"
            :disabled="!!starting"
            @click="quickDownload('wallhaven')"
          >
            Wallhaven 下载
          </v-btn>
          <v-btn
            variant="tonal"
            prepend-icon="mdi-reddit"
            :loading="starting === 'reddit'"
            :disabled="!!starting"
            @click="quickDownload('reddit')"
          >
            Reddit 下载
          </v-btn>
          <v-btn variant="text" prepend-icon="mdi-database-outline" @click="navigate('database')">
            数据库管理
          </v-btn>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.dash-stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
  gap: var(--space-4);
}
.dash-activity {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.dash-actions {
  display: flex;
  gap: var(--space-3);
  flex-wrap: wrap;
}
.update-banner {
  flex-direction: row;
  align-items: center;
  gap: var(--space-3);
  border-left: 2px solid var(--accent-primary);
}
.update-banner__text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.wallpaper-card {
  flex-direction: row;
  align-items: center;
  gap: var(--space-4);
}
.wallpaper-card__thumb {
  width: 128px;
  aspect-ratio: 16 / 10;
  flex: none;
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--preview-bg);
  display: flex;
  align-items: center;
  justify-content: center;
}
.wallpaper-card__thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.wallpaper-card__thumb-fallback {
  color: var(--text-tertiary);
}
.wallpaper-card__meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.wallpaper-card__name,
.wallpaper-card__path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.wallpaper-card__path {
  color: var(--text-tertiary);
}
.slideshow-card {
  border-radius: var(--radius-lg);
  padding: var(--space-3) var(--space-4);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.slideshow-card__head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.slideshow-card__current {
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
