<script setup lang="ts">
import { computed } from "vue";
import type { ImageInfo, LocalImageEntry } from "../types";
import { assetUrl } from "../utils/api";
import { formatBytes } from "../utils/format";

const props = defineProps<{
  /** 抽屉开关（v-model） */
  detailOpen: boolean;
  /** 后端返回的详情元数据 */
  detail: ImageInfo | null;
  /** 触发详情时的原始图库条目（避免用 detail 字段手工拼 LocalImageEntry） */
  entry: LocalImageEntry | null;
  loading: boolean;
  monitorItems: { title: string; value: string }[];
  /** 选中的显示器（v-model） */
  monitor: string;
  settingWallpaper: boolean;
  /** 可选：小尺寸预览地址（缩略图）。抽屉里的预览最高只有 240px，
   *  不传的话会退回原图，为一个小方块付整张 4K 图的解码代价。 */
  previewSrc?: string;
}>();

const emit = defineEmits<{
  (e: "update:detailOpen", v: boolean): void;
  (e: "update:monitor", v: string): void;
  (e: "openViewer", img: LocalImageEntry): void;
  (e: "openLink", url: string | null): void;
  (e: "setWallpaper", path: string, monitor?: string): void;
  (e: "delete", img: LocalImageEntry): void;
}>();

const detailOpen = computed({
  get: () => props.detailOpen,
  set: (v) => emit("update:detailOpen", v),
});
const monitor = computed({
  get: () => props.monitor,
  set: (v) => emit("update:monitor", v),
});
/** 预览优先用缩略图；`previewSrc` 未提供时退回原图，
 *  detail 还没回来之前先用 entry.path，这样抽屉一打开就能看到图。 */
const previewImage = computed(() => {
  if (props.previewSrc) return props.previewSrc;
  if (props.detail) return assetUrl(props.detail.path);
  return props.entry ? assetUrl(props.entry.path) : "";
});
</script>

<template>
  <v-navigation-drawer v-model="detailOpen" location="right" width="360" temporary class="detail-drawer">
    <!-- 只有连 entry 都还没有时才整屏转圈：entry 里已经有 name/path，预览可以立刻显示，
         之前用全屏 spinner 盖住整块，等于让用户白等一次 IPC。 -->
    <div v-if="!entry" class="async-state"><v-progress-circular indeterminate color="primary" /></div>
    <div v-else class="detail-body">
      <div class="detail-head">
        <span class="text-heading detail-head__name">{{ detail?.name ?? entry.name }}</span>
        <v-spacer />
        <v-btn icon="mdi-close" variant="text" size="small" aria-label="关闭详情" @click="detailOpen = false" />
      </div>
      <div class="detail-preview">
        <img :src="previewImage" :alt="detail?.name ?? entry.name" @click="emit('openViewer', entry)" />
      </div>
      <div v-if="loading && !detail" class="detail-loading">
        <v-progress-linear indeterminate height="2" color="primary" />
        <span class="text-caption">正在读取详情…</span>
      </div>
      <div v-else-if="detail" class="detail-rows">
        <div class="detail-row"><span class="stat-label">分辨率</span><span class="text-body">{{ detail.resolution ?? (detail.width && detail.height ? `${detail.width}×${detail.height}` : "-") }}</span></div>
        <div class="detail-row"><span class="stat-label">格式</span><span class="text-body">{{ detail.format ?? "-" }}</span></div>
        <div class="detail-row"><span class="stat-label">大小</span><span class="text-body">{{ formatBytes(detail.size) }}</span></div>
        <div class="detail-row"><span class="stat-label">来源</span><span class="text-body">{{ detail.source ?? "未入库" }}</span></div>
        <div v-if="detail.created_at" class="detail-row"><span class="stat-label">入库时间</span><span class="text-body">{{ detail.created_at.slice(0, 16) }}</span></div>
        <div v-if="detail.title" class="detail-row detail-row--col"><span class="stat-label">标题</span><span class="text-body">{{ detail.title }}</span></div>
        <div v-if="detail.source_url || detail.permalink || detail.download_url" class="detail-row detail-row--col">
          <span class="stat-label">链接</span>
          <div class="detail-links">
            <v-btn v-if="detail.source_url" size="x-small" variant="text" color="primary" @click="emit('openLink', detail.source_url)">来源页面</v-btn>
            <v-btn v-if="detail.permalink" size="x-small" variant="text" color="primary" @click="emit('openLink', detail.permalink)">Reddit 帖子</v-btn>
            <v-btn v-if="detail.download_url" size="x-small" variant="text" color="primary" @click="emit('openLink', detail.download_url)">原图 URL</v-btn>
          </div>
        </div>
      </div>
      <div class="detail-actions">
        <v-select
          v-model="monitor"
          :items="monitorItems"
          label="显示器"
          density="compact"
          hide-details
          class="settings-field"
        />
        <v-btn
          color="primary"
          variant="flat"
          prepend-icon="mdi-monitor"
          :loading="settingWallpaper"
          @click="emit('setWallpaper', entry.path, monitor)"
        >
          设为壁纸
        </v-btn>
        <v-btn variant="tonal" color="error" prepend-icon="mdi-delete-outline" @click="emit('delete', entry)">
          删除
        </v-btn>
      </div>
    </div>
  </v-navigation-drawer>
</template>

<!-- 这些 .detail-* 样式原先写在 GalleryView 的 <style scoped> 里，但作用元素都在本组件
     内部：scoped 只会把作用域标记加到自己的模板元素和子组件根节点上，所以那边一条都没命中
     （抽屉的内部布局和预览图的 max-height 一直没生效）。放到这里才是正确的归属。 -->
<style scoped>
.detail-drawer {
  background: var(--surface-card) !important;
  border-left: 1px solid var(--border-subtle);
}
.detail-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-4);
  height: 100%;
  overflow-y: auto;
}
.detail-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.detail-head__name {
  font-size: 0.9375rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.detail-preview {
  border-radius: var(--radius-md);
  overflow: hidden;
  /* flex:none 不可省。.detail-body 是 height:100% 的纵向 flex 容器，而本块 overflow:hidden
     会让 flex 的自动最小尺寸解析为 0 —— 抽屉内容高于视口时负空间全落到它身上：实测
     1440x640 被压到 136px（预览图内容 184px，裁掉 48px）、1440x520 只剩 16px，整块预览消失。
     更隐蔽的是 .detail-body 因此永远算"没有溢出"，也就永远不会滚动。关掉收缩后由它正常滚动。 */
  flex: none;
  background: var(--preview-bg);
  cursor: zoom-in;
}
.detail-preview img {
  width: 100%;
  display: block;
  object-fit: contain;
  max-height: 240px;
}
.detail-loading {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  color: var(--text-tertiary);
}
.detail-rows {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.detail-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}
.detail-row--col {
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}
.detail-links {
  display: flex;
  gap: var(--space-1);
  flex-wrap: wrap;
}
.detail-actions {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: auto;
}
</style>
