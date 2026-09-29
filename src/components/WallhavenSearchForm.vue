<script setup lang="ts">
/**
 * Wallhaven 搜索条件表单。
 *
 * 搜索条件就是下载配置（后端从 config.json 读），所以这块的草稿、校验、保存都在组件内部，
 * 通过 `defineExpose` 把 `persist()` / `validate()` 交给父视图——父视图在发起下载前必须
 * 先落盘，否则下载用的还是旧条件。
 */
import { computed, ref } from "vue";
import { useConfigDraft } from "../composables/useConfigDraft";
import { toast } from "../stores/app";
import { positiveInt } from "../utils/rules";

const props = defineProps<{
  /** 父视图的搜索请求是否进行中（决定「保存并搜索」的 loading） */
  searching: boolean;
}>();
const emit = defineEmits<{ "save-and-search": [] }>();

const WALLHAVEN_DRAFT_KEYS = [
  "wallhaven_api_key",
  "wallhaven_q",
  "wallhaven_categories",
  "wallhaven_purity",
  "wallhaven_sorting",
  "wallhaven_top_range",
  "wallhaven_atleast",
  "wallhaven_ratios",
  "wallhaven_order",
  "wallhaven_max_images",
] as const;

const WALLHAVEN_DEFAULTS = {
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
};

const { draft, saving, persist } = useConfigDraft(WALLHAVEN_DRAFT_KEYS, WALLHAVEN_DEFAULTS);

type TriStateKey = "wallhaven_categories" | "wallhaven_purity";

/** 三位开关（如 "010"）的取值。
 *
 *  兜底是必要的：`draft[key]` 来自后端配置，缺这条字段时 `undefined[0]` 会在**渲染期**
 *  抛错，整个 Wallhaven 页直接白屏（实测：布局探针抓到 `Cannot read properties of
 *  undefined (reading '0')`）。为一条配置的完整性赔上整个页面不划算，所以退默认值。 */
function flags(key: TriStateKey): string {
  const v = draft[key];
  return typeof v === "string" && v.length === 3 ? v : WALLHAVEN_DEFAULTS[key];
}

/* 三位开关辅助 */
function flagGet(key: TriStateKey, i: number): boolean {
  return flags(key)[i] === "1";
}
function flagSet(key: TriStateKey, i: number, v: boolean) {
  const arr = flags(key).split("");
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
  () => flags("wallhaven_purity")[2] === "1" && !(draft.wallhaven_api_key ?? "").trim(),
);

/** v-form 实例句柄，只用到 validate()。 */
const filterForm = ref<{ validate: () => Promise<{ valid: boolean }> } | null>(null);

/** 保存前先过一遍表单校验：字段下有红字却照样保存、最后只看到后端报错（清空数字框
 *  还会变成 serde 的英文原始错误）是之前最容易让人以为"存进去了"的地方。 */
async function validate(): Promise<boolean> {
  const res = await filterForm.value?.validate();
  if (res && !res.valid) {
    toast("搜索条件里有不合法的字段，请按标红提示修正后再保存", "error");
    return false;
  }
  return true;
}

async function onSaveOnly() {
  if (!(await validate())) return;
  if (await persist()) toast("设置已保存", "success");
}

/** 「保存并搜索」不做校验/保存，只发信号：这两步由父视图统一走一遍
 *  （同一段逻辑也被失败重试按钮复用），避免两条路径的校验行为分叉。 */
function onSaveAndSearch() {
  emit("save-and-search");
}

// 父视图发起任何下载前都要先落盘，所以这两个必须对外可见。
defineExpose({ persist, validate });
</script>

<template>
  <v-form ref="filterForm" class="panel-card animate-in" @submit.prevent>
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
        :loading="props.searching"
        @click="onSaveAndSearch"
      >
        保存并搜索
      </v-btn>
    </div>
  </v-form>
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
</style>
