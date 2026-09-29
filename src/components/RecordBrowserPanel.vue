<script setup lang="ts">
/**
 * 「全部记录」标签页：按来源分页浏览数据库里的记录（含 love=0 的墓碑记录）。
 *
 * 从 DbSettingsView 抽出来：这一页的数据只依赖 appState.stats 与自己的分页状态，
 * 与缺失/孤儿列表没有交集。
 *
 * **必须自己挂载时取一次数据**：父视图的 reloadAll 里那句 `recordBrowser.value?.load()`
 * 在首次加载时是空操作——本组件在 `v-if="dbReady"` 里，父视图 onMounted 跑 reloadAll 时
 * 它还没挂载（抽出来之前是直接调 loadRecords()，不存在这个问题）。少了这句，这一页会一直
 * 停在「没有数据」。之后父视图写库时它一定已经挂载，走 ref 调用即可。
 */
import { computed, onMounted, ref } from "vue";
import type { ImageRecord } from "../types";
import { listDatabaseImages } from "../utils/api";
import { appState, toastError } from "../stores/app";
import { formatDateTime } from "../utils/format";

// Vuetify 组件按需局部导入（见 main.ts 的注册策略说明）：只在这个视图/组件里用到，
// 挂全局注册会让首屏无条件背上它们。
import { VBtnToggle } from "vuetify/components/VBtnToggle";
import { VChip } from "vuetify/components/VChip";
import { VDataTable } from "vuetify/components/VDataTable";

const RECORD_PAGE_SIZE = 20;
// 表头定义提到模块级常量：写成内联字面量的话每次渲染都会生成新数组，
// v-data-table 会当成 props 变化重新计算列。
const RECORD_HEADERS = [
  { title: "文件名", key: "name" },
  { title: "状态", key: "love", width: 80 },
  { title: "分辨率", key: "resolution", width: 110 },
  { title: "入库时间", key: "created_at", width: 150 },
];

const recordSource = ref<"wallhaven" | "reddit">("wallhaven");
const records = ref<ImageRecord[]>([]);
const recordPage = ref(1);
const recordsLoading = ref(false);

const recordTotal = computed(() =>
  recordSource.value === "wallhaven"
    ? (appState.stats?.wallhaven.total ?? 0)
    : (appState.stats?.reddit.total ?? 0),
);
const recordTotalPages = computed(() =>
  Math.max(1, Math.ceil(recordTotal.value / RECORD_PAGE_SIZE)),
);

/** 记录列表竞态控制：快速切来源/翻页时，慢响应不得覆盖新响应。 */
let recordSeq = 0;

async function load() {
  const seq = ++recordSeq;
  recordsLoading.value = true;
  try {
    const res = await listDatabaseImages(
      recordSource.value,
      RECORD_PAGE_SIZE,
      (recordPage.value - 1) * RECORD_PAGE_SIZE,
    );
    if (seq !== recordSeq) return;
    records.value = res;
  } catch (e) {
    if (seq !== recordSeq) return;
    toastError(e);
  } finally {
    if (seq === recordSeq) recordsLoading.value = false;
  }
}

async function onSourceChange(s: "wallhaven" | "reddit") {
  recordSource.value = s;
  recordPage.value = 1;
  await load();
}

async function onPage(delta: number) {
  const next = recordPage.value + delta;
  if (next < 1 || next > recordTotalPages.value) return;
  recordPage.value = next;
  await load();
}

defineExpose({ load });

onMounted(load);
</script>

<template>
  <div class="tab-actions">
    <v-btn-toggle
      :model-value="recordSource"
      mandatory
      density="compact"
      color="primary"
      @update:model-value="onSourceChange"
    >
      <v-btn value="wallhaven" size="small">Wallhaven</v-btn>
      <v-btn value="reddit" size="small">Reddit</v-btn>
    </v-btn-toggle>
    <v-spacer />
    <span class="text-caption">第 {{ recordPage }} / {{ recordTotalPages }} 页 · 共 {{ recordTotal }} 条</span>
    <v-btn
      size="small"
      variant="text"
      icon="mdi-chevron-left"
      :disabled="recordPage <= 1 || recordsLoading"
      @click="onPage(-1)"
    />
    <v-btn
      size="small"
      variant="text"
      icon="mdi-chevron-right"
      :disabled="recordPage >= recordTotalPages || recordsLoading"
      @click="onPage(1)"
    />
  </div>
  <v-data-table
    :items="records"
    :loading="recordsLoading"
    density="compact"
    class="db-table"
    :headers="RECORD_HEADERS"
    :items-per-page="20"
    hide-default-footer
  >
    <template #[`item.love`]="{ item }">
      <v-chip size="x-small" :color="item.love === 1 ? 'success' : 'error'" variant="tonal">
        {{ item.love === 1 ? "正常" : "标记" }}
      </v-chip>
    </template>
    <template #[`item.created_at`]="{ item }">
      <span class="text-caption">{{ formatDateTime(item.created_at) }}</span>
    </template>
  </v-data-table>
</template>
