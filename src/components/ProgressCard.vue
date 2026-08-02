<script setup lang="ts">
import { computed } from "vue";
import { appState, dismissComplete } from "../stores/app";
import { cancelDownloads } from "../utils/api";
import { toastError } from "../stores/app";

/** 下载任务进度卡（仪表盘 / 源页面复用） */
const props = defineProps<{ source: string; title: string }>();

const task = computed(() => appState.downloads[props.source] ?? null);
const percent = computed(() => {
  const t = task.value;
  if (!t || t.total <= 0) return 0;
  return Math.min(100, Math.round((t.done / t.total) * 100));
});

async function onCancel() {
  try {
    await cancelDownloads();
  } catch (e) {
    toastError(e);
  }
}
</script>

<template>
  <div v-if="task && (task.active || task.lastComplete)" class="data-panel progress-panel progress-card">
    <div class="progress-card__head">
      <span class="text-label">{{ title }}</span>
      <v-spacer />
      <template v-if="task.active">
        <span class="text-caption">{{ task.done }} / {{ task.total }}</span>
        <v-btn size="x-small" variant="text" color="error" @click="onCancel">取消</v-btn>
      </template>
      <v-btn
        v-else
        icon="mdi-close"
        size="x-small"
        variant="text"
        @click="dismissComplete(source)"
      />
    </div>
    <v-progress-linear
      :model-value="task.active ? percent : 100"
      :indeterminate="task.active && task.total === 0"
      :color="task.active ? 'primary' : 'success'"
      height="4"
      rounded
    />
    <div class="text-caption progress-card__msg">
      {{ task.active ? task.message : task.lastComplete?.message }}
    </div>
  </div>
</template>

<style scoped>
.progress-card {
  border-radius: var(--radius-lg);
  padding: var(--space-3) var(--space-4);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.progress-card__head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.progress-card__msg {
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
