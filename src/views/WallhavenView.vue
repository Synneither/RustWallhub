<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import type { AppConfig, WallhavenImageEntry, WallhavenSearchResult, WallhavenSelected } from "../types";
import {
  downloadWallhavenSelected,
  saveSettings,
  searchWallhaven,
  startWallhavenDownload,
} from "../utils/api";
import { appState, clearNewImages, toast, toastError } from "../stores/app";
import { positiveInt } from "../utils/rules";
import ProgressCard from "../components/ProgressCard.vue";
import NewImagesStrip from "../components/NewImagesStrip.vue";
import EmptyState from "../components/EmptyState.vue";

/* ── 搜索条件（= wallhaven_* 配置，需保存后生效） ── */
const draft = reactive({
  wallhaven_api_key: "",
  wallhaven_q: "",
  wallhaven_categories: "010",
  wallhaven_purity: "100",
  wallhaven_sorting: "toplist",
  wallhaven_top_range: "1y",
  wallhaven_atleast: "1920x1080",
  wallhaven_ratios: "landscape",
  wallhaven_order: "desc",
  wallhaven_max_images: 100,
});

onMounted(() => {
  const c = appState.config;
  if (!c) return;
  draft.wallhaven_api_key = c.wallhaven_api_key;
  draft.wallhaven_q = c.wallhaven_q;
  draft.wallhaven_categories = c.wallhaven_categories;
  draft.wallhaven_purity = c.wallhaven_purity;
  draft.wallhaven_sorting = c.wallhaven_sorting;
  draft.wallhaven_top_range = c.wallhaven_top_range;
  draft.wallhaven_atleast = c.wallhaven_atleast;
  draft.wallhaven_ratios = c.wallhaven_ratios;
  draft.wallhaven_order = c.wallhaven_order;
  draft.wallhaven_max_images = c.wallhaven_max_images;
});

/* 三位开关辅助 */
function flagGet(key: "wallhaven_categories" | "wallhaven_purity", i: number): boolean {
  return draft[key][i] === "1";
}
function flagSet(key: "wallhaven_categories" | "wallhaven_purity", i: number, v: boolean) {
  const arr = draft[key].split("");
  arr[i] = v ? "1" : "0";
  // 至少保留一位
  if (!arr.includes("1")) return;
  draft[key] = arr.join("");
}

const CATEGORY_FLAGS = [
  { label: "General", i: 0 },
  { label: "Anime", i: 1 },
  { label: "People", i: 2 },
];
const PURITY_FLAGS = [
  { label: "SFW", i: 0 },
  { label: "Sketchy", i: 1 },
  { label: "NSFW", i: 2 },
];

const SORTING_ITEMS = [
  { title: "最新", value: "date_added" },
  { title: "相关度", value: "relevance" },
  { title: "随机", value: "random" },
  { title: "浏览量", value: "views" },
  { title: "收藏数", value: "favorites" },
  { title: "排行榜", value: "toplist" },
];
const TOP_RANGE_ITEMS = [
  { title: "1 天", value: "1d" },
  { title: "3 天", value: "3d" },
  { title: "1 周", value: "1w" },
  { title: "1 月", value: "1M" },
  { title: "3 月", value: "3M" },
  { title: "6 月", value: "6M" },
  { title: "1 年", value: "1y" },
];
const ORDER_ITEMS = [
  { title: "降序", value: "desc" },
  { title: "升序", value: "asc" },
];
const RATIO_ITEMS = [
  { title: "不限制", value: "" },
  { title: "横屏", value: "landscape" },
  { title: "竖屏", value: "portrait" },
  { title: "方形", value: "square" },
  { title: "16:9", value: "16x9" },
  { title: "16:10", value: "16x10" },
  { title: "21:9", value: "21x9" },
];
const ATLEAST_ITEMS = ["", "1920x1080", "2560x1440", "2560x1600", "3440x1440", "3840x2160"];

const orderDisabled = computed(
  () => draft.wallhaven_sorting === "toplist" || draft.wallhaven_sorting === "random",
);
const showTopRange = computed(() => draft.wallhaven_sorting === "toplist");
const nsfwWithoutKey = computed(
  () => draft.wallhaven_purity[2] === "1" && !draft.wallhaven_api_key.trim(),
);

/* ── 保存 ── */
const saving = ref(false);
async function persist(): Promise<boolean> {
  if (!appState.config) return false;
  saving.value = true;
  try {
    const next: AppConfig = { ...appState.config, ...draft };
    await saveSettings(next);
    appState.config = next;
    return true;
  } catch (e) {
    toastError(e);
    return false;
  } finally {
    saving.value = false;
  }
}

async function onSaveOnly() {
  if (await persist()) toast("设置已保存", "success");
}

/* ── 搜索 ── */
const searching = ref(false);
const result = ref<WallhavenSearchResult | null>(null);
const searchError = ref("");

async function doSearch(page: number) {
  searching.value = true;
  searchError.value = "";
  try {
    result.value = await searchWallhaven(page);
    selected.clear();
  } catch (e) {
    searchError.value = String(e);
    result.value = null;
  } finally {
    searching.value = false;
  }
}

async function onSaveAndSearch() {
  if (await persist()) await doSearch(1);
}

async function onPage(delta: number) {
  if (!result.value) return;
  const next = result.value.page + delta;
  if (next < 1 || next > result.value.total_pages) return;
  await doSearch(next);
}

/* ── 勾选与下载 ── */
const selected = reactive(new Set<string>());

/** 从 "2560x1440" 解析宽高比，供网格单元格按需定高（竖屏图不再被 16:10 裁切） */
function ratioOf(resolution: string): string {
  const m = /^(\d+)x(\d+)$/.exec(resolution);
  if (!m) return "16 / 10";
  const w = Number(m[1]);
  const h = Number(m[2]);
  return w > 0 && h > 0 ? `${w} / ${h}` : "16 / 10";
}

function toggleSelect(img: WallhavenImageEntry) {
  if (selected.has(img.id)) selected.delete(img.id);
  else selected.add(img.id);
}

const allPageSelected = computed(() => {
  const imgs = result.value?.images ?? [];
  return imgs.length > 0 && imgs.every((i) => selected.has(i.id));
});

function toggleSelectAll() {
  const imgs = result.value?.images ?? [];
  if (allPageSelected.value) {
    imgs.forEach((i) => selected.delete(i.id));
  } else {
    imgs.forEach((i) => selected.add(i.id));
  }
}

const startingSelected = ref(false);
async function onDownloadSelected() {
  if (selected.size === 0 || startingSelected.value) return;
  startingSelected.value = true;
  try {
    if (!(await persist())) return;
    const imgs = (result.value?.images ?? []).filter((i) => selected.has(i.id));
    const payload: WallhavenSelected[] = imgs.map((i) => ({
      id: i.id,
      path: i.path,
      resolution: i.resolution,
      short_url: i.short_url,
    }));
    clearNewImages("wallhaven");
    const msg = await downloadWallhavenSelected(payload);
    toast(msg, "info");
    selected.clear();
  } catch (e) {
    toastError(e);
  } finally {
    startingSelected.value = false;
  }
}

const startingBatch = ref(false);
async function onBatchDownload() {
  if (startingBatch.value) return;
  startingBatch.value = true;
  try {
    if (!(await persist())) return;
    clearNewImages("wallhaven");
    const msg = await startWallhavenDownload();
    toast(msg, "info");
  } catch (e) {
    toastError(e);
  } finally {
    startingBatch.value = false;
  }
}
</script>

<template>
  <div class="view">
    <div class="view-header">
      <span class="view-header__title">Wallhaven</span>
      <span class="view-header__sub">搜索条件即下载配置，保存后生效</span>
    </div>

    <!-- 搜索条件 -->
    <div class="panel-card animate-in">
      <div class="panel-card__title">
        <v-icon icon="mdi-tune-variant" size="18" color="primary" />
        搜索条件
      </div>

      <div class="wh-row">
        <v-text-field
          v-model="draft.wallhaven_q"
          label="关键词"
          placeholder="如 landscape、anime girl…"
          clearable
          hide-details
          class="settings-field wh-row__q"
        />
        <v-select
          v-model="draft.wallhaven_sorting"
          :items="SORTING_ITEMS"
          label="排序"
          hide-details
          class="settings-field"
          style="max-width: 150px"
        />
        <v-select
          v-if="showTopRange"
          v-model="draft.wallhaven_top_range"
          :items="TOP_RANGE_ITEMS"
          label="排行范围"
          hide-details
          class="settings-field"
          style="max-width: 130px"
        />
        <v-select
          v-model="draft.wallhaven_order"
          :items="ORDER_ITEMS"
          label="顺序"
          hide-details
          :disabled="orderDisabled"
          class="settings-field"
          style="max-width: 110px"
        />
      </div>

      <div class="wh-row wh-row--flags">
        <div class="wh-flag-group">
          <span class="stat-label">分类</span>
          <v-chip
            v-for="f in CATEGORY_FLAGS"
            :key="f.label"
            size="small"
            :variant="flagGet('wallhaven_categories', f.i) ? 'flat' : 'outlined'"
            :color="flagGet('wallhaven_categories', f.i) ? 'primary' : undefined"
            @click="flagSet('wallhaven_categories', f.i, !flagGet('wallhaven_categories', f.i))"
          >
            {{ f.label }}
          </v-chip>
        </div>
        <div class="wh-flag-group">
          <span class="stat-label">纯度</span>
          <v-chip
            v-for="f in PURITY_FLAGS"
            :key="f.label"
            size="small"
            :variant="flagGet('wallhaven_purity', f.i) ? 'flat' : 'outlined'"
            :color="flagGet('wallhaven_purity', f.i) ? 'primary' : undefined"
            @click="flagSet('wallhaven_purity', f.i, !flagGet('wallhaven_purity', f.i))"
          >
            {{ f.label }}
          </v-chip>
        </div>
        <span v-if="nsfwWithoutKey" class="text-caption" style="color: var(--accent-warning)">
          NSFW 内容需要填写 API Key
        </span>
      </div>

      <div class="settings-grid">
        <v-combobox
          v-model="draft.wallhaven_atleast"
          :items="ATLEAST_ITEMS"
          label="最小分辨率"
          hide-details
          class="settings-field"
        />
        <v-select
          v-model="draft.wallhaven_ratios"
          :items="RATIO_ITEMS"
          label="宽高比"
          hide-details
          class="settings-field"
        />
        <v-text-field
          v-model.number="draft.wallhaven_max_images"
          type="number"
          label="批量下载目标张数"
          hide-details
          :rules="[(v: number) => positiveInt(v, { min: 1, max: 10000 })]"
          class="settings-field"
        />
        <v-text-field
          v-model="draft.wallhaven_api_key"
          label="API Key（可选）"
          type="password"
          hide-details
          class="settings-field"
        />
      </div>

      <div class="wh-actions">
        <v-btn variant="tonal" :loading="saving" @click="onSaveOnly">仅保存</v-btn>
        <v-btn
          color="primary"
          variant="flat"
          prepend-icon="mdi-magnify"
          :loading="searching"
          @click="onSaveAndSearch"
        >
          保存并搜索
        </v-btn>
      </div>
    </div>

    <!-- 下载进度 -->
    <ProgressCard source="wallhaven" title="Wallhaven 下载" />

    <!-- 搜索结果 -->
    <div v-if="searching && !result" class="wh-grid">
      <div v-for="i in 12" :key="i" class="wh-cell shimmer" />
    </div>

    <EmptyState
      v-else-if="searchError"
      error
      icon="mdi-cloud-alert-outline"
      title="搜索失败"
      :desc="searchError"
    >
      <v-btn variant="tonal" @click="onSaveAndSearch">重试</v-btn>
    </EmptyState>

    <template v-else-if="result">
      <div class="wh-toolbar">
        <span class="text-caption">
          第 {{ result.page }} / {{ result.total_pages }} 页 · 共 {{ result.total }} 张
          <template v-if="selected.size > 0"> · 已选 {{ selected.size }}</template>
        </span>
        <v-spacer />
        <v-btn size="small" variant="text" @click="toggleSelectAll">
          {{ allPageSelected ? "取消全选" : "全选本页" }}
        </v-btn>
        <v-btn
          size="small"
          variant="text"
          icon="mdi-chevron-left"
          :disabled="result.page <= 1 || searching"
          @click="onPage(-1)"
        />
        <v-btn
          size="small"
          variant="text"
          icon="mdi-chevron-right"
          :disabled="result.page >= result.total_pages || searching"
          @click="onPage(1)"
        />
        <v-btn
          size="small"
          variant="tonal"
          :disabled="selected.size === 0"
          :loading="startingSelected"
          @click="onDownloadSelected"
        >
          下载选中（{{ selected.size }}）
        </v-btn>
        <v-btn
          size="small"
          color="primary"
          variant="flat"
          :loading="startingBatch"
          @click="onBatchDownload"
        >
          按条件批量下载
        </v-btn>
      </div>

      <div class="wh-grid">
        <div
          v-for="img in result.images"
          :key="img.id"
          class="wh-cell wh-cell--clickable"
          :class="{ 'wh-cell--selected': selected.has(img.id) }"
          :style="{ aspectRatio: ratioOf(img.resolution) }"
          @click="toggleSelect(img)"
        >
          <img :src="img.thumbnail_url" :alt="img.id" loading="lazy" />
          <span class="wh-cell__res">{{ img.resolution }}</span>
          <span class="wh-cell__check">
            <v-icon
              :icon="selected.has(img.id) ? 'mdi-checkbox-marked-circle' : 'mdi-checkbox-blank-circle-outline'"
              size="20"
              :color="selected.has(img.id) ? 'primary' : 'white'"
            />
          </span>
        </div>
      </div>

      <EmptyState
        v-if="result.images.length === 0"
        icon="mdi-image-search-outline"
        title="没有符合条件的图片"
        desc="试试放宽分辨率或调整关键词"
      />
    </template>

    <EmptyState
      v-else
      icon="mdi-image-search-outline"
      title="设置条件后开始搜索"
      desc="搜索结果可勾选下载，也可按条件批量下载到本地"
    />

    <NewImagesStrip source="wallhaven" />
  </div>
</template>

<style scoped>
.wh-row {
  display: flex;
  gap: var(--space-3);
  flex-wrap: wrap;
}
.wh-row__q {
  flex: 1;
  min-width: 220px;
}
.wh-row--flags {
  align-items: center;
  gap: var(--space-5);
}
.wh-flag-group {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.wh-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-3);
}
.wh-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}
.wh-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: var(--space-2);
  align-items: start;
}
.wh-cell {
  position: relative;
  aspect-ratio: 16 / 10; /* 默认值，实际由内联 style 按真实分辨率覆盖 */
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--surface-elevated);
  border: 2px solid transparent;
  min-height: 90px;
}
.wh-cell--clickable {
  cursor: pointer;
  transition: border-color 0.15s, transform 0.15s;
}
.wh-cell--clickable:hover {
  transform: translateY(-1px);
}
.wh-cell--selected {
  border-color: var(--accent-primary);
}
.wh-cell img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.wh-cell__res {
  position: absolute;
  left: 6px;
  bottom: 6px;
  padding: 1px 7px;
  border-radius: var(--radius-full);
  font-size: 0.625rem;
  background: rgba(0, 0, 0, 0.6);
  color: #fff;
}
.wh-cell__check {
  position: absolute;
  right: 6px;
  top: 6px;
  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.6));
}
</style>
