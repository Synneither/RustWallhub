<script setup lang="ts">
import { computed } from "vue";
import { appState, dismissComplete, toast } from "../stores/app";
import { cancelDownloads } from "../utils/api";
import { useAsyncAction } from "../composables/useAsyncAction";
import type { Source } from "../types";

/** 下载任务进度卡（仪表盘 / 源页面 / 图库 / 设置页复用） */
const props = defineProps<{ source: Source; title?: string }>();

/** 调用方不传 title 时按来源取默认标题，避免各页面各自维护一份映射。 */
const DEFAULT_TITLE: Record<Source, string> = {
  wallhaven: "Wallhaven 下载",
  reddit: "Reddit 下载",
  all: "补下载任务",
};
const label = computed(() => props.title ?? DEFAULT_TITLE[props.source]);

const task = computed(() => appState.downloads[props.source] ?? null);
const percent = computed(() => {
  const t = task.value;
  if (!t || t.total <= 0) return 0;
  return Math.min(100, Math.round((t.done / t.total) * 100));
});

/** 取消只作用于本卡对应的来源。以前这里发的是一条无参数的 cancel_downloads，
 *  后端会把**所有**来源的取消标志一起置位，于是在 Wallhaven 卡上点一下会把
 *  同时在跑的 Reddit 任务也杀掉。 */
const { run: onCancel, loading: cancelling } = useAsyncAction(async () => {
  await cancelDownloads(props.source);
  toast(`${label.value}：已请求取消`, "info");
});
</script>

<template>
  <div v-if="task && (task.active || task.lastComplete)" class="data-panel progress-panel progress-card">
    <div class="progress-card__head">
      <span class="text-label">{{ label }}</span>
      <v-spacer />
      <template v-if="task.active">
        <span class="text-caption">{{ task.done }} / {{ task.total }}</span>
        <v-btn
          size="x-small"
          variant="text"
          color="error"
          :loading="cancelling"
          title="只取消这个来源的下载"
          @click="onCancel"
        >
          取消
        </v-btn>
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
  /* 「数据库」「设置」等页面的根 .view 是 height:100% 的纵向 flex 容器，内容高于
     视口时子项会被压缩。普通 .panel-card 的 overflow 可见，min-height:auto 会解析
     成内容高度，压不动；本卡继承 .data-panel 的 overflow:hidden，自动最小尺寸是 0，
     于是它一个人把全部负空间吸收掉——实测在 1440×1000 下被压成 26px 高，进度条与
     文案（height:0）全被裁掉，看起来就是一条空白细条。
     固定 flex:none 后不再参与压缩，改由 .view 正常滚动（其余卡片本来就在滚）。 */
  flex: none;
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
