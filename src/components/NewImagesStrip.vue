<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { appState } from "../stores/app";
import type { Source } from "../types";
import { assetUrl, resolveThumbnails } from "../utils/api";
import { useThumbCache } from "../composables/useThumbCache";

/** "本次新图"横向预览条：消费全局 newImages（Wallhaven / Reddit 页使用） */
const props = defineProps<{ source: Source }>();

const MAX_SHOW = 12;

const images = computed(() =>
  appState.newImages.filter((i) => i.source === props.source),
);
const shown = computed(() => images.value.slice(-MAX_SHOW));
const extra = computed(() => Math.max(0, images.value.length - MAX_SHOW));

/* 96px 的小格子没必要加载 4K 原图，优先取后端已生成好的缩略图。
 * 缩略图缓存（LRU + 上限）已收敛到 useThumbCache。 */
const thumbCache = useThumbCache(200);
let thumbSeq = 0;
let thumbTimer: ReturnType<typeof setTimeout> | null = null;
/** 是否已经问过后端一次。在拿到结果前不拿原图兜底，否则 96px 格子会先拉 4K 原图。 */
const thumbsResolved = ref(false);

/** 只请求缓存里没有的名字：下载过程中窗口会持续滚动，否则同几个名字会被反复请求。 */
async function loadThumbs() {
  const names = shown.value.map((i) => i.name).filter((n) => thumbCache.get(n) === undefined);
  if (names.length === 0) {
    thumbsResolved.value = true;
    return;
  }
  const seq = ++thumbSeq;
  try {
    const dpr = appState.config?.thumbnail_dpr ?? 2;
    const batch = await resolveThumbnails(props.source, names, dpr);
    if (seq !== thumbSeq) return;
    thumbCache.cache(batch.items.map((it) => [it.name, assetUrl(it.thumb_path)] as const));
  } catch {
    // 失败时退回到原图，保证预览条可用
  } finally {
    if (seq === thumbSeq) thumbsResolved.value = true;
  }
}

/* 每下载完一张图，store 都会不可变替换 appState.newImages，而窗口一滚动 watch 键就变。
 * 直接发请求的话批量下载 500 张就是 500 次 IPC，所以合并到 300ms 后只发一次，
 * 并在真正发起前再查一遍缓存。 */
watch(
  () => shown.value.map((i) => i.name).join("\n"),
  () => {
    if (thumbTimer) clearTimeout(thumbTimer);
    thumbTimer = setTimeout(() => {
      thumbTimer = null;
      void loadThumbs();
    }, 300);
  },
  { immediate: true },
);

/* 切换来源后旧缩略图不能跨源复用（文件名可能重名）。 */
watch(
  () => props.source,
  () => {
    thumbCache.clear();
    thumbsResolved.value = false;
  },
);

onBeforeUnmount(() => {
  if (thumbTimer) clearTimeout(thumbTimer);
});

function thumbOf(name: string, path: string): string {
  const cached = thumbCache.get(name);
  if (cached) return cached;
  // 还没问过后端就先给空串，让格子保持占位样式，避免为 96px 拉整张原图。
  return thumbsResolved.value ? assetUrl(path) : "";
}

/** 把已解析到的 src 一起算好，避免模板里重复调用 thumbOf。 */
const tiles = computed(() =>
  shown.value.map((i) => ({ name: i.name, src: thumbOf(i.name, i.path) })),
);
</script>

<template>
  <div v-if="images.length > 0" class="new-strip">
    <div class="new-strip__label text-label">
      本次新图 · {{ images.length }}
    </div>
    <div class="new-strip__row">
      <div v-for="t in tiles" :key="t.name" class="new-strip__thumb">
        <img
          v-if="t.src"
          :src="t.src"
          :alt="t.name"
          loading="lazy"
          decoding="async"
        />
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
