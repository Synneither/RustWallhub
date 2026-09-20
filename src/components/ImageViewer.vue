<script setup lang="ts">
import { computed, ref, watch, onMounted, onBeforeUnmount } from "vue";
import { assetUrl } from "../utils/api";

/** 全屏深色图片查看器（固定深色，不随主题变化） */
export interface ViewerImage {
  name: string;
  path: string;
  /** 远程图片（如 Wallhaven 原图 URL）走原始地址，不能经过 asset 协议转换 */
  rawUrl?: string;
  /** 快速占位图（如 Wallhaven 的 large 缩略图，已在网格里加载过、命中缓存），
   *  原图在后台加载完再替换，翻页时不会白屏干等 */
  placeholderUrl?: string;
}

const props = defineProps<{
  images: ViewerImage[];
  startIndex: number;
}>();
const emit = defineEmits<{ close: []; "update:index": [value: number] }>();

const index = ref(props.startIndex);
const loading = ref(true);
const error = ref(false);
/** 原图是否已就绪（有 placeholder 时先用占位图顶上） */
const hiResReady = ref(false);
let preload: HTMLImageElement | null = null;
let preloadToken = 0;

const current = computed(() => props.images[index.value] ?? null);
const src = computed(() => {
  const img = current.value;
  if (!img) return "";
  if (img.rawUrl) return hiResReady.value ? img.rawUrl : img.placeholderUrl || img.rawUrl;
  return assetUrl(img.path);
});
/** 有占位图时它已经可见，不必再转圈 */
const showLoading = computed(() => loading.value && !current.value?.placeholderUrl);

/** 后台预载原图；token 防止快速翻页时旧图的回调盖掉新图状态 */
function startPreload(img: (typeof props.images)[number] | null) {
  if (preload) {
    preload.onload = null;
    preload.onerror = null;
    preload = null;
  }
  const token = ++preloadToken;
  if (!img?.rawUrl || img.rawUrl === img.placeholderUrl) {
    hiResReady.value = true;
    return;
  }
  hiResReady.value = false;
  const el = new Image();
  preload = el;
  el.referrerPolicy = "no-referrer";
  el.onload = () => {
    if (token === preloadToken) hiResReady.value = true;
  };
  el.onerror = () => {
    // 原图失败但占位图还在：静默降级，不弹错误覆盖层
    if (token === preloadToken && !img.placeholderUrl) {
      loading.value = false;
      error.value = true;
    }
  };
  el.src = img.rawUrl;
}

function prev() {
  if (index.value > 0) {
    index.value--;
    loading.value = true;
    error.value = false;
  }
}
function next() {
  if (index.value < props.images.length - 1) {
    index.value++;
    loading.value = true;
    error.value = false;
  }
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") emit("close");
  else if (e.key === "ArrowLeft") prev();
  else if (e.key === "ArrowRight") next();
}

function onImgError() {
  loading.value = false;
  // 占位图也挂了才报错，否则等原图结论
  if (!current.value?.placeholderUrl || hiResReady.value) error.value = true;
}

/** 占位图自身加载失败（原图还未就绪）→ 确实无图可看，报错 */
function onPlaceholderError() {
  if (!hiResReady.value) {
    loading.value = false;
    error.value = true;
  }
}

/* 翻页状态回传给父组件：预览里有"下载当前图/选入下载"这类操作，
 * 外部必须知道现在停在第几张，否则会作用到打开时的那一张上。 */
watch(index, (v) => emit("update:index", v));

/* 换图即换原图预载 */
watch(current, (img) => startPreload(img), { immediate: true });

watch(
  () => props.startIndex,
  (v) => {
    if (v === index.value) return;
    index.value = v;
    loading.value = true;
    error.value = false;
  },
);

/* 列表变短时把索引夹回有效范围：下载完成等事件会触发父组件 reload 换页，
 * 若当前停在末尾那张，index 就越界 → current 为 null，舞台全空且计数显示 "13 / 12"。 */
watch(
  () => props.images.length,
  (len) => {
    if (len > 0 && index.value >= len) index.value = len - 1;
  },
);

onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  if (preload) {
    preload.onload = null;
    preload.onerror = null;
    preload = null;
  }
});
</script>

<template>
  <div class="viewer" @click.self="emit('close')">
    <div class="viewer__topbar">
      <span class="viewer__name">{{ current?.name }}</span>
      <span class="viewer__count">{{ index + 1 }} / {{ images.length }}</span>
      <v-spacer />
      <slot name="topbar" :image="current" :index="index" />
      <v-btn icon="mdi-close" variant="text" color="white" @click="emit('close')" />
    </div>

    <button class="viewer__nav viewer__nav--prev" :disabled="index === 0" @click.stop="prev" aria-label="上一张">
      <v-icon icon="mdi-chevron-left" size="36" />
    </button>

    <div class="viewer__stage" @click.self="emit('close')">
      <div v-if="showLoading" class="viewer__loading">
        <v-progress-circular indeterminate color="white" size="40" />
      </div>
      <div v-else-if="error" class="viewer__error">
        <v-icon icon="mdi-image-off-outline" size="36" />
        <span>大图加载失败，可能已被服务器拒绝或图片已失效</span>
      </div>
      <template v-if="current">
        <!-- 占位图与原图叠在同一格：原图就绪后直接盖上去，中间不会闪白 -->
        <img
          v-if="current.placeholderUrl && !hiResReady"
          :key="`ph-${index}`"
          :src="current.placeholderUrl"
          :alt="current.name"
          class="viewer__img"
          referrerpolicy="no-referrer"
          @load="loading = false"
          @error="onPlaceholderError"
          @click.stop
        />
        <img
          v-if="hiResReady || !current.placeholderUrl"
          :key="`hi-${index}`"
          :src="src"
          :alt="current.name"
          class="viewer__img"
          referrerpolicy="no-referrer"
          @load="loading = false"
          @error="onImgError"
          @click.stop
        />
      </template>
    </div>

    <button
      class="viewer__nav viewer__nav--next"
      :disabled="index === images.length - 1"
      @click.stop="next"
      aria-label="下一张"
    >
      <v-icon icon="mdi-chevron-right" size="36" />
    </button>

    <!-- 底部操作区：可选，无内容时不渲染底栏 -->
    <div v-if="$slots.actions" class="viewer__actions">
      <slot name="actions" :image="current" :index="index" />
    </div>
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
  /* 用 grid 单格叠加：占位图和原图占同一格，居中且互不挤压 */
  display: grid;
  grid-template-areas: "pic";
  place-items: center;
  padding: 56px 72px;
}
.viewer__img {
  grid-area: pic;
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
  border-radius: var(--radius-sm);
  box-shadow: 0 8px 40px rgba(0, 0, 0, 0.55);
}
.viewer__loading {
  position: absolute;
}
.viewer__error {
  position: absolute;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-8);
  color: rgba(255, 255, 255, 0.7);
  font-size: 0.8125rem;
  text-align: center;
}
.viewer__actions {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: linear-gradient(to top, rgba(0, 0, 0, 0.55), transparent);
  color: #fff;
  z-index: 2;
}
/* 有底栏时给图片留出空间，避免被挡住 */
.viewer:has(.viewer__actions) .viewer__stage {
  padding-bottom: 92px;
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
