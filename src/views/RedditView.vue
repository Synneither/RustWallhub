<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import type { AppConfig } from "../types";
import { saveSettings } from "../utils/api";
import { startRedditDownload } from "../utils/api";
import { appState, clearNewImages, toast, toastError } from "../stores/app";
import { positiveInt, requiredRule } from "../utils/rules";
import ProgressCard from "../components/ProgressCard.vue";
import NewImagesStrip from "../components/NewImagesStrip.vue";

const draft = reactive<Pick<AppConfig, "reddit_url" | "reddit_max_posts" | "reddit_max_images">>({
  reddit_url: "",
  reddit_max_posts: 100,
  reddit_max_images: 100,
});

const REDDIT_DRAFT_KEYS = [
  "reddit_url",
  "reddit_max_posts",
  "reddit_max_images",
] as const;

function isDirty(): boolean {
  const c = appState.config;
  if (!c) return false;
  return REDDIT_DRAFT_KEYS.some((key) => draft[key] !== c[key]);
}

const saving = ref(false);
const starting = ref(false);
const formValid = ref(false);

onMounted(() => {
  const c = appState.config;
  if (c) {
    draft.reddit_url = c.reddit_url;
    draft.reddit_max_posts = c.reddit_max_posts;
    draft.reddit_max_images = c.reddit_max_images;
  }
});

async function persist(): Promise<boolean> {
  if (!appState.config) return false;
  if (!isDirty()) return true;
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

async function onSave() {
  if (await persist()) toast("设置已保存", "success");
}

async function onStart() {
  if (starting.value) return;
  starting.value = true;
  try {
    if (!(await persist())) return;
    clearNewImages("reddit");
    const msg = await startRedditDownload();
    toast(msg, "info");
  } catch (e) {
    toastError(e);
  } finally {
    starting.value = false;
  }
}
</script>

<template>
  <div class="view">
    <div class="view-header">
      <span class="view-header__title">Reddit</span>
      <span class="view-header__sub">从 subreddit 抓取图片</span>
    </div>

    <v-form v-model="formValid" class="panel-card source-card animate-in" @submit.prevent>
      <div class="source-card-header source-header--reddit" style="padding: 0 0 4px; border-bottom: none">
        <div class="source-header-icon">
          <v-icon icon="mdi-reddit" size="22" color="var(--accent-reddit)" />
        </div>
        <div>
          <div class="text-heading">抓取配置</div>
          <div class="text-caption">修改后需保存，下载按已保存的配置执行</div>
        </div>
      </div>

      <v-text-field
        v-model="draft.reddit_url"
        label="Subreddit 列表 URL"
        hint="任意 reddit.com 列表页地址，后端自动转 JSON API 抓取"
        persistent-hint
        :rules="[requiredRule]"
        class="settings-field"
      />

      <div class="settings-grid">
        <v-text-field
          v-model.number="draft.reddit_max_posts"
          type="number"
          label="每批帖子数"
          :rules="[(v: number) => positiveInt(v, { min: 1, max: 1000 })]"
          class="settings-field"
        />
        <v-text-field
          v-model.number="draft.reddit_max_images"
          type="number"
          label="目标图片数"
          hint="凑满该数量后停止"
          persistent-hint
          :rules="[(v: number) => positiveInt(v, { min: 1, max: 10000 })]"
          class="settings-field"
        />
      </div>

      <div class="panel-card__hint">
        支持的图片来源：i.redd.it 直链（jpg/png/webp）、Reddit gallery（取首图）、imgur（直链与相册封面）。
        连续 3 批没有新增图片时自动停止；每批间隔 2 秒。保存目录：{{ appState.config?.reddit_save_dir ?? "-" }}
      </div>

      <div class="reddit-actions">
        <v-btn variant="tonal" :loading="saving" @click="onSave">保存设置</v-btn>
        <v-btn
          color="primary"
          variant="flat"
          prepend-icon="mdi-download-outline"
          :loading="starting"
          :disabled="!formValid"
          @click="onStart"
        >
          保存并开始下载
        </v-btn>
      </div>
    </v-form>

    <ProgressCard source="reddit" title="Reddit 下载" class="animate-in stagger-2" />

    <NewImagesStrip source="reddit" class="animate-in stagger-3" />
  </div>
</template>

<style scoped>
.reddit-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-3);
}
</style>
