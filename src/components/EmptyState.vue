<script setup lang="ts">
/** 通用空态（图库/搜索页/数据库页复用），样式走全局 .gallery-empty 类族 */
withDefaults(
  defineProps<{
    icon: string;
    title: string;
    desc?: string;
    small?: boolean;
    error?: boolean;
  }>(),
  { desc: "", small: false, error: false },
);
</script>

<template>
  <div class="gallery-empty" :class="{ 'gallery-empty--error': error }">
    <div class="empty-icon-wrap" :class="{ 'empty-icon-wrap--sm': small }">
      <v-icon :icon="icon" :size="small ? 28 : 44" class="empty-icon" :color="error ? 'error' : undefined" />
    </div>
    <p class="empty-title">{{ title }}</p>
    <p v-if="desc" class="empty-desc">{{ desc }}</p>
    <div v-if="$slots.default" class="empty-actions">
      <slot />
    </div>
  </div>
</template>
