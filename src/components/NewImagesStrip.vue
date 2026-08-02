<script setup lang="ts">
import { computed } from "vue";
import { appState } from "../stores/app";
import { assetUrl } from "../utils/api";

/** "本次新图"横向预览条：消费全局 newImages（Wallhaven / Reddit 页使用） */
const props = defineProps<{ source: string }>();

const MAX_SHOW = 12;

const images = computed(() =>
  appState.newImages.filter((i) => i.source === props.source),
);
const shown = computed(() => images.value.slice(-MAX_SHOW));
const extra = computed(() => Math.max(0, images.value.length - MAX_SHOW));
</script>

<template>
  <div v-if="images.length > 0" class="new-strip">
    <div class="new-strip__label text-label">
      本次新图 · {{ images.length }}
    </div>
    <div class="new-strip__row">
      <div v-for="img in shown" :key="img.name" class="new-strip__thumb">
        <img :src="assetUrl(img.path)" :alt="img.name" loading="lazy" />
      </div>
      <div v-if="extra > 0" class="new-strip__more text-caption">+{{ extra }}</div>
    </div>
  </div>
</template>

<style scoped>
.new-strip {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.new-strip__row {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  padding-bottom: 2px;
}
.new-strip__thumb {
  width: 96px;
  aspect-ratio: 16 / 10;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: var(--surface-elevated);
  border: var(--border-card);
}
.new-strip__thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.new-strip__more {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  background: var(--surface-elevated);
  color: var(--text-tertiary);
}
</style>
