<script setup lang="ts">
import { computed, inject, onActivated, onMounted, ref } from "vue";
import { appState, activeDownloadSources, dbReady, refreshStats, toast, toastError } from "../stores/app";
import { assetUrl, getActiveWallpaper, resolveThumbnails, startRedditDownload } from "../utils/api";
import { basename } from "../utils/path";
import StatPanel from "../components/StatPanel.vue";
import ProgressCard from "../components/ProgressCard.vue";
import EmptyState from "../components/EmptyState.vue";

const navigate = inject<(key: string) => void>("navigate", () => {});

const startingReddit = ref(false);

const updateInfo = computed(() => appState.update.info);

async function startReddit() {
  if (startingReddit.value) return;
  startingReddit.value = true;
  try {
    toast(await startRedditDownload(), "info");
  } catch (e) {
    toastError(e);
  } finally {
    startingReddit.value = false;
  }
}

onMounted(() => {
  if (dbReady.value) refreshStats();
  loadActiveWallpaper();
});

// KeepAlive 下切走再切回时重载「当前壁纸」：它可能在应用外被换掉，返回时不该显示旧值。
onActivated(() => {
  loadActiveWallpaper();
});

/* ── 当前壁纸 ── */
const wallpaperPath = ref<string | null>(null);
const wallpaperImgError = ref(false);
/** 壁纸缩略图地址；解析不到时留空，由模板退回原图。 */
const wallpaperThumb = ref("");

const wallpaperName = computed(() => basename(wallpaperPath.value));

/** 128px 的格子不值得为 4K 原图付出约 33MB 的驻留内存，优先用库里的缩略图。
 *  后端在源文件缺失/非 JPEG 时会把原图路径原样返回（缩略图文件名则是 name__w480.q85.webp），
 *  所以用「返回的文件名与原名不同」判断是否真的拿到了缩略图。 */
async function resolveWallpaperThumb(name: string) {
  wallpaperThumb.value = "";
  if (!name) return;
  const dpr = appState.config?.thumbnail_dpr ?? 2;
  for (const source of ["wallhaven", "reddit"] as const) {
    try {
      const res = await resolveThumbnails(source, [name], dpr);
      const hit = res.items.find((it) => {
        if (it.name !== name) return false;
        const base = basename(it.thumb_path);
        return base !== name;
      });
      if (hit) {
        wallpaperThumb.value = assetUrl(hit.thumb_path);
        return;
      }
    } catch {
      // 该来源里没有这张图，继续试下一个来源
    }
  }
}

async function loadActiveWallpaper() {
  try {
    const res = await getActiveWallpaper();
    // 多显示器各一张时只展示第一张，够表达「当前在用哪张」了。
    wallpaperPath.value = res.paths[0] ?? null;
    wallpaperImgError.value = false;
    await resolveWallpaperThumb(wallpaperName.value);
  } catch {
    wallpaperPath.value = null;
    wallpaperThumb.value = "";
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
            :src="wallpaperThumb || assetUrl(wallpaperPath)"
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
          在图库中查看
        </v-btn>
      </div>

      <!-- 活动任务 -->
      <div v-if="activeDownloadSources.length > 0" class="dash-activity">
        <ProgressCard
          v-for="s in activeDownloadSources"
          :key="s"
          :source="s"
          class="animate-in"
        />
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
            prepend-icon="mdi-reddit"
            :loading="startingReddit"
            @click="startReddit"
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
</style>