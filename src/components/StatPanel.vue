<script setup lang="ts">
import { computed } from "vue";
import type { DbStats } from "../types";

/** 来源统计面板：总数 / 在库 / 缺失 */
const props = withDefaults(
  defineProps<{
    source: "wallhaven" | "reddit";
    stats: DbStats | null;
    loading?: boolean;
  }>(),
  { loading: false },
);

const meta = computed(() =>
  props.source === "wallhaven"
    ? { label: "Wallhaven", icon: "mdi-image-multiple", color: "var(--accent-primary)" }
    : { label: "Reddit", icon: "mdi-reddit", color: "var(--accent-reddit)" },
);

const items = computed(() => [
  { label: "总记录", value: props.stats?.total },
  { label: "在库", value: props.stats?.love },
  { label: "缺失", value: props.stats?.dislike, warn: (props.stats?.dislike ?? 0) > 0 },
]);
</script>

<template>
  <div class="data-panel stat-panel">
    <div class="stat-panel__header">
      <div class="stat-panel__icon" :style="{ background: `color-mix(in srgb, ${meta.color} 14%, transparent)` }">
        <v-icon :icon="meta.icon" :size="20" :style="{ color: meta.color }" />
      </div>
      <span class="text-heading">{{ meta.label }}</span>
    </div>
    <div class="stat-panel__body">
      <div v-for="it in items" :key="it.label" class="stat-panel__cell">
        <template v-if="loading">
          <div class="shimmer stat-panel__skeleton" />
        </template>
        <template v-else>
          <span class="stat-number stat-panel__value" :class="{ 'stat-panel__value--warn': it.warn }">
            {{ it.value ?? "-" }}
          </span>
        </template>
        <span class="stat-label">{{ it.label }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.stat-panel {
  border-radius: var(--radius-lg);
  padding: var(--space-5) var(--space-6);
}
.stat-panel__header {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  margin-bottom: var(--space-4);
}
.stat-panel__icon {
  width: 34px;
  height: 34px;
  border-radius: var(--radius-md);
  display: flex;
  align-items: center;
  justify-content: center;
}
.stat-panel__body {
  display: flex;
  gap: var(--space-8);
}
.stat-panel__cell {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 64px;
}
.stat-panel__value {
  font-size: 1.625rem;
}
.stat-panel__value--warn {
  color: var(--accent-warning);
}
.stat-panel__skeleton {
  width: 56px;
  height: 26px;
  border-radius: var(--radius-sm);
}
</style>
