<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { listen } from "@tauri-apps/api/event";
import Dashboard from "./views/Dashboard.vue";
import WallhavenView from "./views/WallhavenView.vue";
import RedditView from "./views/RedditView.vue";
import GalleryView from "./views/GalleryView.vue";
import SettingsView from "./views/SettingsView.vue";

const currentView = ref("dashboard");
const drawer = ref(true);

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
  });

  unlistenComplete = await listen<CompleteEvent>("download-complete", (e) => {
    downloading.value = false;
    progressMsg.value = "";
    progressDone.value = 0;
    progressTotal.value = 0;
    snackbarText.value = e.payload.message;
    snackbar.value = true;
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
  { key: "settings", title: "设置", icon: "mdi-cog" },
];

async function runAction(fn: () => Promise<unknown>) {
  try {
    await fn();
  } catch (e) {
    snackbarText.value = `错误: ${e}`;
    snackbar.value = true;
  }
}
</script>

<template>
  <v-app id="inspire">
    <v-navigation-drawer v-model="drawer" width="220">
      <v-list-item
        title="RustWallhub"
        subtitle="壁纸管理器"
        class="pa-3"
      />
      <v-divider />
      <v-list density="compact" nav>
        <v-list-item
          v-for="item in navItems"
          :key="item.key"
          :prepend-icon="item.icon"
          :title="item.title"
          :active="currentView === item.key"
          @click="currentView = item.key"
        />
      </v-list>
    </v-navigation-drawer>

    <v-app-bar flat density="compact">
      <v-app-bar-nav-icon @click="drawer = !drawer" />
      <v-app-bar-title>{{ navItems.find(i => i.key === currentView)?.title }}</v-app-bar-title>
      <template v-if="downloading">
        <v-progress-circular
          indeterminate
          size="20"
          width="2"
          class="me-2"
        />
        <span class="text-body-2 me-3">{{ progressDone }}/{{ progressTotal }}</span>
      </template>
    </v-app-bar>

    <v-main>
      <v-container fluid>
        <Dashboard
          v-if="currentView === 'dashboard'"
          :downloading="downloading"
          :progress-msg="progressMsg"
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
        <SettingsView v-if="currentView === 'settings'" />
      </v-container>
    </v-main>

    <v-snackbar v-model="snackbar" :timeout="3000" location="bottom">
      {{ snackbarText }}
    </v-snackbar>
  </v-app>
</template>
