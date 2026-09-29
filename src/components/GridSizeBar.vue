<script setup lang="ts">
/**
 * 网格密度档位按钮组。
 *
 * 图库统计条与 Wallhaven 结果区工具栏用的是同一套交互（此前两处各抄了一遍同样的 v-for），
 * 抽出来顺便保证两边的选中态样式不会再各自漂移。
 */
import type { DensityOption, GridDensity } from "../composables/useGridDensity";

defineProps<{
  /** v-model，取值为档位的 `value` */
  modelValue: GridDensity;
  items: DensityOption[];
}>();

defineEmits<{ (e: "update:modelValue", v: GridDensity): void }>();
</script>

<template>
  <div class="grid-size">
    <v-btn
      v-for="s in items"
      :key="s.value"
      size="x-small"
      :variant="modelValue === s.value ? 'tonal' : 'text'"
      :color="modelValue === s.value ? 'primary' : undefined"
      @click="$emit('update:modelValue', s.value)"
    >
      {{ s.label }}
    </v-btn>
  </div>
</template>

<style scoped>
/* 只负责按钮组自身的排布；与邻居的间距由调用方（统计条 / 工具栏）决定。 */
.grid-size {
  display: flex;
  align-items: center;
}
</style>
