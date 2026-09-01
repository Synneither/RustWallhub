<script setup lang="ts">
import { computed } from "vue";
import type { ImageInfo, LocalImageEntry } from "../types";
import { assetUrl } from "../utils/api";
import { formatBytes } from "../utils/format";

const props = defineProps<{
  /** 抽屉开关（v-model） */
  detailOpen: boolean;
  /** 后端返回的详情元数据 */
  detail: ImageInfo | null;
  /** 触发详情时的原始图库条目（避免用 detail 字段手工拼 LocalImageEntry） */
  entry: LocalImageEntry | null;
  loading: boolean;
  monitorItems: { title: string; value: string }[];
  /** 选中的显示器（v-model） */
  monitor: string;
  settingWallpaper: boolean;
}>();

const emit = defineEmits<{
  (e: "update:detailOpen", v: boolean): void;
  (e: "update:monitor", v: string): void;
  (e: "openViewer", img: LocalImageEntry): void;
  (e: "openLink", url: string | null): void;
  (e: "setWallpaper", path: string, monitor?: string): void;
  (e: "delete", img: LocalImageEntry): void;
}>();

const detailOpen = computed({
  get: () => props.detailOpen,
  set: (v) => emit("update:detailOpen", v),
});
const monitor = computed({
  get: () => props.monitor,
  set: (v) => emit("update:monitor", v),
});
</script>

<template>
  <v-navigation-drawer v-model="detailOpen" location="right" width="360" temporary class="detail-drawer">
    <div v-if="loading" class="async-state"><v-progress-circular indeterminate color="primary" /></div>
    <div v-else-if="detail && entry" class="detail-body">
      <div class="detail-head">
        <span class="text-heading detail-head__name">{{ detail.name }}</span>
        <v-spacer />
        <v-btn icon="mdi-close" variant="text" size="small" aria-label="关闭详情" @click="detailOpen = false" />
      </div>
      <div class="detail-preview">
        <img :src="assetUrl(detail.path)" :alt="detail.name" @click="emit('openViewer', entry)" />
      </div>
      <div class="detail-rows">
        <div class="detail-row"><span class="stat-label">分辨率</span><span class="text-body">{{ detail.resolution ?? (detail.width && detail.height ? `${detail.width}×${detail.height}` : "-") }}</span></div>
        <div class="detail-row"><span class="stat-label">格式</span><span class="text-body">{{ detail.format ?? "-" }}</span></div>
        <div class="detail-row"><span class="stat-label">大小</span><span class="text-body">{{ formatBytes(detail.size) }}</span></div>
        <div class="detail-row"><span class="stat-label">来源</span><span class="text-body">{{ detail.source ?? "未入库" }}</span></div>
        <div v-if="detail.created_at" class="detail-row"><span class="stat-label">入库时间</span><span class="text-body">{{ detail.created_at.slice(0, 16) }}</span></div>
        <div v-if="detail.title" class="detail-row detail-row--col"><span class="stat-label">标题</span><span class="text-body">{{ detail.title }}</span></div>
        <div v-if="detail.source_url || detail.permalink || detail.download_url" class="detail-row detail-row--col">
          <span class="stat-label">链接</span>
          <div class="detail-links">
            <v-btn v-if="detail.source_url" size="x-small" variant="text" color="primary" @click="emit('openLink', detail.source_url)">来源页面</v-btn>
            <v-btn v-if="detail.permalink" size="x-small" variant="text" color="primary" @click="emit('openLink', detail.permalink)">Reddit 帖子</v-btn>
            <v-btn v-if="detail.download_url" size="x-small" variant="text" color="primary" @click="emit('openLink', detail.download_url)">原图 URL</v-btn>
          </div>
        </div>
      </div>
      <div class="detail-actions">
        <v-select
          v-model="monitor"
          :items="monitorItems"
          label="显示器"
          density="compact"
          hide-details
          class="settings-field"
        />
        <v-btn
          color="primary"
          variant="flat"
          prepend-icon="mdi-monitor"
          :loading="settingWallpaper"
          @click="emit('setWallpaper', detail.path, monitor)"
        >
          设为壁纸
        </v-btn>
        <v-btn variant="tonal" color="error" prepend-icon="mdi-delete-outline" @click="emit('delete', entry)">
          删除
        </v-btn>
      </div>
    </div>
  </v-navigation-drawer>
</template>
