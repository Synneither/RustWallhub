<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

defineProps<{
  downloading: boolean;
  progressMsg: string;
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

async function loadStats() {
  try {
    const data = await invoke<{ wallhaven: DbStats; reddit: DbStats }>("get_stats");
    console.log("dashboard stats:", data);
    whStats.value = data.wallhaven;
    rdStats.value = data.reddit;
  } catch (e) {
    console.error("loadStats error:", e);
  }
}

let unlistenDownload: (() => void) | null = null;
let unlistenComplete: (() => void) | null = null;

onMounted(async () => {
  await loadStats();
  unlistenDownload = await listen("download-progress", () => {});
  unlistenComplete = await listen("download-complete", async () => {
    await loadStats();
  });
});

onUnmounted(() => {
  if (unlistenDownload) unlistenDownload();
  if (unlistenComplete) unlistenComplete();
});
</script>

<template>
  <div>
    <v-row>
      <v-col cols="12" md="6">
        <v-card>
          <v-card-title class="d-flex align-center">
            <v-icon class="me-2">mdi-image-search</v-icon>
            Wallhaven
          </v-card-title>
          <v-card-text>
            <v-row>
              <v-col cols="4">
                <div class="text-h5 text-center">{{ whStats.total }}</div>
                <div class="text-caption text-center text-medium-emphasis">总计</div>
              </v-col>
              <v-col cols="4">
                <div class="text-h5 text-center text-success">{{ whStats.love }}</div>
                <div class="text-caption text-center text-medium-emphasis">喜欢</div>
              </v-col>
              <v-col cols="4">
                <div class="text-h5 text-center text-warning">{{ whStats.dislike }}</div>
                <div class="text-caption text-center text-medium-emphasis">不喜欢</div>
              </v-col>
            </v-row>
          </v-card-text>
          <v-card-actions>
            <v-btn
              color="primary"
              variant="flat"
              :disabled="downloading"
              @click="emit('action', () => invoke('start_wallhaven_download'))"
            >
              <v-icon start>mdi-download</v-icon>
              开始下载
            </v-btn>
            <v-btn
              variant="tonal"
              :disabled="downloading"
              @click="emit('action', () => invoke('start_db_download', { source: 'wallhaven' }))"
            >
              <v-icon start>mdi-database-sync</v-icon>
              从数据库补下载
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-col>

      <v-col cols="12" md="6">
        <v-card>
          <v-card-title class="d-flex align-center">
            <v-icon class="me-2">mdi-reddit</v-icon>
            Reddit
          </v-card-title>
          <v-card-text>
            <v-row>
              <v-col cols="4">
                <div class="text-h5 text-center">{{ rdStats.total }}</div>
                <div class="text-caption text-center text-medium-emphasis">总计</div>
              </v-col>
              <v-col cols="4">
                <div class="text-h5 text-center text-success">{{ rdStats.love }}</div>
                <div class="text-caption text-center text-medium-emphasis">喜欢</div>
              </v-col>
              <v-col cols="4">
                <div class="text-h5 text-center text-warning">{{ rdStats.dislike }}</div>
                <div class="text-caption text-center text-medium-emphasis">不喜欢</div>
              </v-col>
            </v-row>
          </v-card-text>
          <v-card-actions>
            <v-btn
              color="primary"
              variant="flat"
              :disabled="downloading"
              @click="emit('action', () => invoke('start_reddit_download'))"
            >
              <v-icon start>mdi-download</v-icon>
              开始下载
            </v-btn>
            <v-btn
              variant="tonal"
              :disabled="downloading"
              @click="emit('action', () => invoke('start_db_download', { source: 'reddit' }))"
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
        <v-card>
          <v-card-title>
            <v-icon class="me-2">mdi-wrench</v-icon>
            维护操作
          </v-card-title>
          <v-card-text>
            <v-btn
              variant="outlined"
              class="me-3"
              :disabled="downloading"
              @click="emit('action', () => invoke('mark_dislike', { source: 'all' }).then(() => loadStats()))"
            >
              <v-icon start>mdi-alert-circle</v-icon>
              标记缺失图片为不喜欢
            </v-btn>
            <v-btn
              variant="outlined"
              :disabled="downloading"
              @click="emit('action', () => invoke('restore_love', { source: 'all' }).then(() => loadStats()))"
            >
              <v-icon start>mdi-check-circle</v-icon>
              全部恢复为喜欢
            </v-btn>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>

    <v-row v-if="downloading" class="mt-4">
      <v-col cols="12">
        <v-card>
          <v-card-text>
            <v-progress-linear
              :indeterminate="true"
              color="primary"
              class="mb-2"
            />
            <div class="text-body-2">{{ progressMsg }}</div>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>
  </div>
</template>
