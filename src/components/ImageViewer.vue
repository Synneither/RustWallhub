<script setup lang="ts">
import { computed, ref, watch, onMounted, onBeforeUnmount } from "vue";
import { assetUrl } from "../utils/api";

/** 全屏深色图片查看器（固定深色，不随主题变化） */
export interface ViewerImage {
  name: string;
  path: string;
}

const props = defineProps<{
  images: ViewerImage[];
  startIndex: number;
}>();
const emit = defineEmits<{ close: [] }>();

const index = ref(props.startIndex);
const loading = ref(true);

const current = computed(() => props.images[index.value] ?? null);
const src = computed(() => (current.value ? assetUrl(current.value.path) : ""));

function prev() {
  if (index.value > 0) {
    index.value--;
    loading.value = true;
  }
}
function next() {
  if (index.value < props.images.length - 1) {
    index.value++;
    loading.value = true;
  }
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") emit("close");
  else if (e.key === "ArrowLeft") prev();
  else if (e.key === "ArrowRight") next();
}

watch(
  () => props.startIndex,
  (v) => {
    index.value = v;
    loading.value = true;
  },
);

onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div class="viewer" @click.self="emit('close')">
    <div class="viewer__topbar">
      <span class="viewer__name">{{ current?.name }}</span>
      <span class="viewer__count">{{ index + 1 }} / {{ images.length }}</span>
      <v-spacer />
      <v-btn icon="mdi-close" variant="text" color="white" @click="emit('close')" />
    </div>

    <button class="viewer__nav viewer__nav--prev" :disabled="index === 0" @click.stop="prev" aria-label="上一张">
      <v-icon icon="mdi-chevron-left" size="36" />
    </button>

    <div class="viewer__stage" @click.self="emit('close')">
      <div v-if="loading" class="viewer__loading">
        <v-progress-circular indeterminate color="white" size="40" />
      </div>
      <img
        v-if="current"
        :key="current.path"
        :src="src"
        :alt="current.name"
        class="viewer__img"
        @load="loading = false"
        @error="loading = false"
        @click.stop
      />
    </div>

    <button
      class="viewer__nav viewer__nav--next"
      :disabled="index === images.length - 1"
      @click.stop="next"
      aria-label="下一张"
    >
      <v-icon icon="mdi-chevron-right" size="36" />
    </button>
  </div>
</template>

<style scoped>
.viewer {
  position: fixed;
  inset: 0;
  z-index: 2400;
  background: var(--preview-bg);
  display: flex;
  align-items: center;
  justify-content: center;
}
.viewer__topbar {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: linear-gradient(to bottom, rgba(0, 0, 0, 0.45), transparent);
  color: #fff;
  z-index: 2;
}
.viewer__name {
  font-size: 0.875rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 60%;
}
.viewer__count {
  font-size: 0.75rem;
  color: rgba(255, 255, 255, 0.65);
}
.viewer__stage {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 56px 72px;
}
.viewer__img {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
  border-radius: var(--radius-sm);
  box-shadow: 0 8px 40px rgba(0, 0, 0, 0.55);
}
.viewer__loading {
  position: absolute;
}
.viewer__nav {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 2;
  width: 48px;
  height: 48px;
  border: none;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--preview-surface);
  color: #fff;
  cursor: pointer;
  opacity: 0.85;
  transition: opacity 0.15s, background 0.15s;
}
.viewer__nav:hover:not(:disabled) {
  opacity: 1;
  background: #2a2a2e;
}
.viewer__nav:disabled {
  opacity: 0.25;
  cursor: default;
}
.viewer__nav--prev {
  left: 16px;
}
.viewer__nav--next {
  right: 16px;
}
</style>
