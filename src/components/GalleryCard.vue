<script setup lang="ts">
/**
 * 图库网格里的单张卡片。
 *
 * 卡片的样式（含 `content-visibility` / `contain-intrinsic-size` / `aspect-ratio`）**必须
 * 跟着卡片走**：scoped 样式管不到子组件内部元素，留在 GalleryView 里会全部失效。
 * 网格那侧的 `grid-auto-rows: min-content` 与 `--grid-cell-ph` 由父级提供，CSS 变量会继承。
 */
import type { LocalImageEntry } from "../types";

defineProps<{
  img: LocalImageEntry;
  /** 已解析好的缩略图地址（未命中缓存时是原图地址） */
  thumbSrc: string;
  /** 当前是否处于多选模式 */
  selectionMode: boolean;
  /** 这张是否被选中 */
  selected: boolean;
  /** 这张是不是当前桌面壁纸 */
  isCurrent: boolean;
  /** 是否在「来源库」模式下浏览：孤儿角标、选择态、hover 操作都只在这个模式下有意义
   *  （浏览任意自定义目录时既没有数据库也没有缩略图管线） */
  dbMode: boolean;
}>();

defineEmits<{
  /** 点击/回车/空格：多选模式下切换选中，否则打开查看器 */
  activate: [img: LocalImageEntry];
  detail: [img: LocalImageEntry];
  remove: [img: LocalImageEntry];
}>();
</script>

<template>
  <div
    class="gallery-card gallery-cell"
    :class="{
      'gallery-card--selected': selectionMode && selected,
      'gallery-card--current': isCurrent,
    }"
    role="button"
    tabindex="0"
    :aria-label="img.name"
    :aria-pressed="selectionMode ? selected : undefined"
    @click="$emit('activate', img)"
    @keydown.enter.prevent="$emit('activate', img)"
    @keydown.space.prevent="$emit('activate', img)"
  >
    <!-- decoding="async"：让浏览器把图片解码放到后台线程，滚动时不卡主线程
         （一页最多 96 张，同步解码会造成明显掉帧）。 -->
    <img :src="thumbSrc" :alt="img.name" loading="lazy" decoding="async" />
    <span v-if="img.is_orphan && dbMode" class="gallery-card__orphan">孤儿</span>
    <span v-if="isCurrent && !selectionMode" class="gallery-card__current" title="这张就是当前桌面壁纸">
      当前壁纸
    </span>

    <!-- 选择态角标 -->
    <span v-if="selectionMode && dbMode" class="gallery-card__check">
      <v-icon
        :icon="selected ? 'mdi-checkbox-marked-circle' : 'mdi-checkbox-blank-circle-outline'"
        size="20"
        :color="selected ? 'primary' : 'white'"
      />
    </span>

    <!-- hover 操作 -->
    <div v-if="!selectionMode" class="gallery-card__overlay" @click.stop>
      <template v-if="dbMode">
        <v-btn
          icon="mdi-information-outline"
          size="x-small"
          variant="flat"
          class="overlay-btn"
          title="详情"
          @click="$emit('detail', img)"
        />
        <v-btn
          icon="mdi-delete-outline"
          size="x-small"
          variant="flat"
          class="overlay-btn overlay-btn--danger"
          title="删除"
          @click="$emit('remove', img)"
        />
      </template>
    </div>
  </div>
</template>

<style scoped>
/* 几何（尺寸/圆角/占位属性）在 style.css 的 .gallery-cell 里与骨架共用；
   这里只放卡片特有的交互样式。 */
.gallery-card {
  cursor: pointer;
}
.gallery-card img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  transition: transform 0.2s var(--ease-out);
}
.gallery-card:hover img {
  transform: scale(1.03);
}
.gallery-card--selected {
  border-color: var(--accent-primary);
}
/* 当前桌面壁纸：常驻边框 + 一圈克制的辉光，扫一眼就能在图里认出来。
   放在 --selected 之前：多选是眼下更即时的操作态，两者同时命中时让选择态胜出。 */
.gallery-card--current {
  border-color: var(--accent-success);
  box-shadow:
    0 0 0 1px color-mix(in srgb, var(--accent-success) 45%, transparent),
    0 0 14px color-mix(in srgb, var(--accent-success) 28%, transparent);
}
.gallery-card__current {
  position: absolute;
  right: 6px;
  bottom: 6px;
  padding: 1px 7px;
  border-radius: var(--radius-full);
  font-size: 0.625rem;
  background: color-mix(in srgb, var(--accent-success) 88%, black);
  color: #fff;
}
.gallery-card__orphan {
  position: absolute;
  left: 6px;
  top: 6px;
  padding: 1px 7px;
  border-radius: var(--radius-full);
  font-size: 0.625rem;
  background: color-mix(in srgb, var(--accent-reddit) 85%, black);
  color: #fff;
}
.gallery-card__check {
  position: absolute;
  right: 6px;
  top: 6px;
  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.6));
}
.gallery-card__overlay {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  justify-content: center;
  gap: 6px;
  padding: 18px 6px 8px;
  background: linear-gradient(to top, rgba(0, 0, 0, 0.72), transparent);
  opacity: 0;
  transition: opacity 0.15s;
}
.gallery-card:hover .gallery-card__overlay {
  opacity: 1;
}
.overlay-btn {
  background: rgba(30, 30, 34, 0.9) !important;
  color: #fff !important;
}
.overlay-btn--danger {
  color: var(--accent-error) !important;
}
</style>
