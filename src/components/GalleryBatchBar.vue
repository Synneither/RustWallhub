<script setup lang="ts">
/**
 * 图库的批量操作条。纯展示：选中数量、是否孤儿模式、是否忙碌都由父级传入，
 * 点「删除 / 收养入库」只发事件（确认框与调用在父级，那里才有加载与重载逻辑）。
 */
defineProps<{
  /** 已选数量 */
  count: number;
  /** 孤儿模式：多一个「收养入库」 */
  orphanMode: boolean;
  /** 批量动作进行中 */
  busy: boolean;
}>();

defineEmits<{ adopt: []; remove: [] }>();
</script>

<template>
  <div class="gallery-batch animate-in">
    <span class="text-body">已选 {{ count }} 项</span>
    <v-spacer />
    <v-btn v-if="orphanMode" size="small" variant="tonal" :loading="busy" @click="$emit('adopt')">
      收养入库
    </v-btn>
    <v-btn size="small" variant="tonal" color="error" :loading="busy" @click="$emit('remove')">
      删除
    </v-btn>
  </div>
</template>

<style scoped>
.gallery-batch {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-4);
  border-radius: var(--radius-md);
  background: var(--accent-primary-dim);
  border: var(--border-active);
}
</style>
