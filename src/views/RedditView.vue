<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";

defineProps<{
  downloading: boolean;
}>();

const emit = defineEmits<{
  action: [fn: () => Promise<unknown>];
}>();
</script>

<template>
  <div>
    <v-card>
      <v-card-title>
        <v-icon class="me-2">mdi-reddit</v-icon>
        Reddit 下载
      </v-card-title>
      <v-card-text>
        <p class="text-body-2 mb-4">
          从 Reddit r/Animewallpaper 抓取动漫壁纸。自动过滤 Desktop 标签的帖子。
        </p>
        <v-alert type="info" variant="tonal" class="mb-4">
          支持 Reddit 图片直链、Imgur 相册、Reddit 画廊格式。
        </v-alert>
      </v-card-text>
      <v-card-actions>
        <v-btn
          color="primary"
          size="large"
          variant="flat"
          :disabled="downloading"
          @click="emit('action', () => invoke('start_reddit_download'))"
        >
          <v-icon start>mdi-download</v-icon>
          开始下载
        </v-btn>
        <v-btn
          variant="tonal"
          size="large"
          :disabled="downloading"
          @click="emit('action', () => invoke('start_db_download', { source: 'reddit' }))"
        >
          <v-icon start>mdi-database-sync</v-icon>
          从数据库补下载
        </v-btn>
      </v-card-actions>
    </v-card>

    <v-card class="mt-4">
      <v-card-title>
        <v-icon class="me-2">mdi-wrench</v-icon>
        Reddit 维护
      </v-card-title>
      <v-card-text>
        <v-btn
          variant="outlined"
          class="me-3"
          :disabled="downloading"
          @click="emit('action', () => invoke('mark_dislike', { source: 'reddit' }).then(() => undefined))"
        >
          <v-icon start>mdi-alert-circle</v-icon>
          标记缺失图片为不喜欢
        </v-btn>
        <v-btn
          variant="outlined"
          :disabled="downloading"
          @click="emit('action', () => invoke('restore_love', { source: 'reddit' }).then(() => undefined))"
        >
          <v-icon start>mdi-check-circle</v-icon>
          全部恢复为喜欢
        </v-btn>
      </v-card-text>
    </v-card>
  </div>
</template>
