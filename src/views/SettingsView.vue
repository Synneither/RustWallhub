<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";

interface AppConfig {
  wallhaven_save_dir: string;
  reddit_save_dir: string;
  wallhaven_db_path: string;
  reddit_db_path: string;
  wallhaven_api_key: string;
  wallhaven_categories: string;
  wallhaven_purity: string;
  wallhaven_sorting: string;
  wallhaven_top_range: string;
  wallhaven_atleast: string;
  wallhaven_ratios: string;
  wallhaven_max_images: number;
  reddit_url: string;
  reddit_max_posts: number;
  reddit_max_images: number;
}

const config = ref<AppConfig | null>(null);
const saving = ref(false);
const saved = ref(false);

async function loadConfig() {
  try {
    config.value = await invoke<AppConfig>("get_config");
  } catch {
    // 使用默认值
    config.value = {
      wallhaven_save_dir: "",
      reddit_save_dir: "",
      wallhaven_db_path: "wallhaven_images.db",
      reddit_db_path: "reddit_images.db",
      wallhaven_api_key: "",
      wallhaven_categories: "010",
      wallhaven_purity: "111",
      wallhaven_sorting: "toplist",
      wallhaven_top_range: "1y",
      wallhaven_atleast: "1920x1080",
      wallhaven_ratios: "landscape",
      wallhaven_max_images: 100,
      reddit_url: "",
      reddit_max_posts: 100,
      reddit_max_images: 100,
    };
  }
}

async function saveSettings() {
  if (!config.value) return;
  saving.value = true;
  saved.value = false;
  try {
    await invoke("save_settings", { config: config.value });
    saved.value = true;
    setTimeout(() => (saved.value = false), 2000);
  } catch (e) {
    console.error("保存设置失败:", e);
  }
  saving.value = false;
}

onMounted(loadConfig);
</script>

<template>
  <div v-if="config">
    <v-card class="mb-4">
      <v-card-title>
        <v-icon class="me-2">mdi-image-search</v-icon>
        Wallhaven 设置
      </v-card-title>
      <v-card-text>
        <v-text-field
          v-model="config.wallhaven_api_key"
          label="API Key（可选）"
          hint="提高 API 速率限制"
          type="password"
          class="mb-2"
        />
        <v-row>
          <v-col cols="12" sm="6">
            <v-text-field
              v-model="config.wallhaven_save_dir"
              label="图片保存目录"
            />
          </v-col>
          <v-col cols="12" sm="6">
            <v-text-field
              v-model="config.wallhaven_db_path"
              label="数据库路径"
            />
          </v-col>
        </v-row>
        <v-row>
          <v-col cols="6" sm="4">
            <v-text-field
              v-model="config.wallhaven_categories"
              label="类别"
              hint="1=General, 2=Anime, 4=People（可组合如 010）"
              persistent-hint
            />
          </v-col>
          <v-col cols="6" sm="4">
            <v-text-field
              v-model="config.wallhaven_purity"
              label="纯净度"
              hint="1=SFW, 2=Sketchy, 4=NSFW（可组合如 111）"
              persistent-hint
            />
          </v-col>
          <v-col cols="6" sm="4">
            <v-select
              v-model="config.wallhaven_sorting"
              label="排序方式"
              :items="['date_added', 'relevance', 'random', 'views', 'favorites', 'toplist']"
            />
          </v-col>
        </v-row>
        <v-row>
          <v-col cols="6" sm="4">
            <v-select
              v-if="config.wallhaven_sorting === 'toplist'"
              v-model="config.wallhaven_top_range"
              label="排序范围"
              :items="['1d', '3d', '1w', '1M', '3M', '6M', '1y']"
            />
          </v-col>
          <v-col cols="6" sm="4">
            <v-text-field
              v-model="config.wallhaven_atleast"
              label="最低分辨率"
              hint="例如: 1920x1080"
              persistent-hint
            />
          </v-col>
          <v-col cols="6" sm="4">
            <v-text-field
              v-model="config.wallhaven_ratios"
              label="宽高比"
              hint="例如: landscape, 16x9, 21x9"
              persistent-hint
            />
          </v-col>
        </v-row>
        <v-row>
          <v-col cols="6" sm="4">
            <v-text-field
              v-model.number="config.wallhaven_max_images"
              label="最大下载数量"
              type="number"
              min="1"
              max="500"
            />
          </v-col>
        </v-row>
      </v-card-text>
    </v-card>

    <v-card class="mb-4">
      <v-card-title>
        <v-icon class="me-2">mdi-reddit</v-icon>
        Reddit 设置
      </v-card-title>
      <v-card-text>
        <v-row>
          <v-col cols="12" sm="6">
            <v-text-field
              v-model="config.reddit_save_dir"
              label="图片保存目录"
            />
          </v-col>
          <v-col cols="12" sm="6">
            <v-text-field
              v-model="config.reddit_db_path"
              label="数据库路径"
            />
          </v-col>
        </v-row>
        <v-row>
          <v-col cols="6" sm="4">
            <v-text-field
              v-model.number="config.reddit_max_posts"
              label="最大抓取帖子数"
              type="number"
              min="1"
              max="500"
            />
          </v-col>
          <v-col cols="6" sm="4">
            <v-text-field
              v-model.number="config.reddit_max_images"
              label="最大下载数量"
              type="number"
              min="1"
              max="500"
            />
          </v-col>
        </v-row>
      </v-card-text>
    </v-card>

    <div class="d-flex align-center">
      <v-btn
        color="primary"
        size="large"
        :loading="saving"
        @click="saveSettings"
      >
        <v-icon start>mdi-content-save</v-icon>
        保存设置
      </v-btn>
      <v-fade-transition>
        <v-icon v-if="saved" color="success" class="ms-3">mdi-check-circle</v-icon>
      </v-fade-transition>
    </div>
  </div>
</template>
