<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { logger } from "../utils/logger";

interface AppConfig {
  wallhaven_save_dir: string;
  reddit_save_dir: string;
  db_dir: string;
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
  thumbnails_dir: string;
  download_concurrency: number;
  thumbnail_dpr: number;
  request_timeout: number;
}

const config = ref<AppConfig | null>(null);
const saving = ref(false);
const saved = ref(false);
const formValid = ref(false);
const formRef = ref<any>(null);

const requiredRule = (v: string) => !!v || '此项不能为空';
const positiveInt = (v: number) => {
  if (v === undefined || v === null || v === 0) return true; // 0 = 无限制
  if (typeof v !== 'number' || isNaN(v)) return '请输入有效数字';
  if (v < 1) return '不能小于 1';
  if (v > 100) return '不能超过 100';
  return true;
};
const dprRule = (v: number) => {
  if (v === undefined || v === null) return true;
  const allowed = [1, 2, 3];
  if (!allowed.includes(v)) return '仅支持 1、2、3';
  return true;
};
const timeoutRule = (v: number) => {
  if (!v) return '请输入超时秒数';
  if (v < 5) return '不能低于 5 秒';
  if (v > 120) return '不能超过 120 秒';
  return true;
};
const resolutionRule = (v: string) => {
  if (!v) return true;
  return /^\d+x\d+$/.test(v) || '格式如 1920x1080';
};

async function selectDirectory(field: keyof AppConfig) {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择目录",
    });
    if (selected && config.value) {
      (config.value as any)[field] = selected;
    }
  } catch (e) {
    logger.error("Settings", "目录选择失败", e);
  }
}

async function loadConfig() {
  try {
    config.value = await invoke<AppConfig>("get_config");
    logger.info("Settings", "配置已加载");
  } catch (e) {
    logger.error("Settings", "配置加载失败", e);
    config.value = {
      wallhaven_save_dir: "",
      reddit_save_dir: "",
      wallhaven_db_path: "",
      reddit_db_path: "",
      db_dir: "",
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
      thumbnails_dir: "",
      download_concurrency: 6,
      thumbnail_dpr: 2,
      request_timeout: 30,
    };
  }
}

async function saveSettings() {
  if (!config.value) return;
  saving.value = true;
  saved.value = false;
  logger.action("Settings", "保存设置");
  try {
    await invoke("save_settings", { config: config.value });
    saved.value = true;
    logger.info("Settings", "设置已保存");
    setTimeout(() => (saved.value = false), 2000);
  } catch (e) {
    logger.error("Settings", "保存设置失败", e);
  }
  saving.value = false;
}

onMounted(loadConfig);
</script>

<template>
  <div v-if="config" class="settings-root">
    <v-form v-model="formValid" ref="formRef">
    <v-card class="glass-card settings-card animate-in stagger-1">
      <div class="settings-card-header wh-header-bg">
        <div class="settings-header-icon wh-header-icon">
          <v-icon color="#6c8cff">mdi-image-search</v-icon>
        </div>
        <div>
          <div class="text-heading">Wallhaven 设置</div>
          <div class="text-caption">API 配置、搜索参数与下载限制</div>
        </div>
      </div>
      <v-card-text class="pa-6 pt-4">
        <div class="settings-group-label">下载配置</div>
        <div class="settings-group">
          <v-text-field
            v-model="config.wallhaven_save_dir"
            label="图片保存目录"
            class="settings-field"
            :rules="[requiredRule]"
            append-inner-icon="mdi-folder-open"
            @click:append-inner="selectDirectory('wallhaven_save_dir')"
          />
        </div>

        <div class="settings-group-label">搜索参数</div>
        <div class="settings-group">
          <v-row>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model="config.wallhaven_categories"
                label="类别"
                hint="1=General, 2=Anime, 4=People"
                persistent-hint
                class="settings-field"
              />
            </v-col>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model="config.wallhaven_purity"
                label="纯净度"
                hint="1=SFW, 2=Sketchy, 4=NSFW"
                persistent-hint
                class="settings-field"
              />
            </v-col>
            <v-col cols="6" sm="4">
              <v-select
                v-model="config.wallhaven_sorting"
                label="排序方式"
                :items="['date_added', 'relevance', 'random', 'views', 'favorites', 'toplist']"
                class="settings-field"
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
                class="settings-field"
              />
            </v-col>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model="config.wallhaven_atleast"
                label="最低分辨率"
                hint="例如: 1920x1080"
                persistent-hint
                :rules="[resolutionRule]"
                class="settings-field"
              />
            </v-col>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model="config.wallhaven_ratios"
                label="宽高比"
                hint="例如: landscape, 16x9"
                persistent-hint
                class="settings-field"
              />
            </v-col>
          </v-row>
        </div>

        <div class="settings-group-label">下载限制</div>
        <div class="settings-group">
          <v-row>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model="config.wallhaven_api_key"
                label="API Key（可选）"
                hint="提高 API 速率限制"
                type="password"
                class="settings-field"
              />
            </v-col>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model.number="config.wallhaven_max_images"
                label="最大下载数量"
                type="number"
                min="1"
                max="500"
                :rules="[positiveInt]"
                class="settings-field"
              />
            </v-col>
          </v-row>
        </div>
      </v-card-text>
    </v-card>

    <v-card class="glass-card settings-card animate-in stagger-2">
      <div class="settings-card-header rd-header-bg">
        <div class="settings-header-icon rd-header-icon">
          <v-icon color="#ff6b35">mdi-reddit</v-icon>
        </div>
        <div>
          <div class="text-heading">Reddit 设置</div>
          <div class="text-caption">抓取配置与下载限制</div>
        </div>
      </div>
      <v-card-text class="pa-6 pt-4">
        <div class="settings-group-label">下载配置</div>
        <div class="settings-group">
          <v-row>
            <v-col cols="12" sm="6">
              <v-text-field
                v-model="config.reddit_save_dir"
                label="图片保存目录"
                class="settings-field"
                :rules="[requiredRule]"
                append-inner-icon="mdi-folder-open"
                @click:append-inner="selectDirectory('reddit_save_dir')"
              />
            </v-col>
          </v-row>
        </div>

        <div class="settings-group-label">下载限制</div>
        <div class="settings-group">
          <v-row>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model.number="config.reddit_max_posts"
                label="最大抓取帖子数"
                type="number"
                min="1"
                max="500"
                :rules="[positiveInt]"
                class="settings-field"
              />
            </v-col>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model.number="config.reddit_max_images"
                label="最大下载数量"
                type="number"
                min="1"
                max="500"
                :rules="[positiveInt]"
                class="settings-field"
              />
            </v-col>
          </v-row>
        </div>
      </v-card-text>
    </v-card>

    <v-card class="glass-card settings-card animate-in stagger-3">
      <div class="settings-card-header db-header-bg">
        <div class="settings-header-icon db-header-icon">
          <v-icon color="#43e97b">mdi-database</v-icon>
        </div>
        <div>
          <div class="text-heading">数据库设置</div>
          <div class="text-caption">统一管理 Wallhaven 和 Reddit 数据库存储目录</div>
        </div>
      </div>
      <v-card-text class="pa-6 pt-4">
        <div class="settings-group-label">存储位置</div>
        <div class="settings-group">
          <v-text-field
            v-model="config.db_dir"
            label="数据库目录"
            hint="存放 wallhaven_images.db 和 reddit_images.db"
            persistent-hint
            class="settings-field"
            :rules="[requiredRule]"
            append-inner-icon="mdi-folder-open"
            @click:append-inner="selectDirectory('db_dir')"
          />
        </div>
      </v-card-text>
    </v-card>

    <v-card class="glass-card settings-card animate-in stagger-4">
      <div class="settings-card-header adv-header-bg">
        <div class="settings-header-icon adv-header-icon">
          <v-icon color="#a78bfa">mdi-tune-variant</v-icon>
        </div>
        <div>
          <div class="text-heading">高级设置</div>
          <div class="text-caption">下载、缩略图与网络参数</div>
        </div>
      </div>
      <v-card-text class="pa-6 pt-4">
        <div class="settings-group-label">下载与网络</div>
        <div class="settings-group">
          <v-row>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model.number="config.download_concurrency"
                label="并发下载数"
                type="number"
                min="1"
                max="20"
                hint="同时下载的文件数 (1-20)"
                persistent-hint
                :rules="[positiveInt]"
                class="settings-field"
              />
            </v-col>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model.number="config.request_timeout"
                label="请求超时(秒)"
                type="number"
                min="5"
                max="120"
                hint="单个 HTTP 请求超时 (5-120s)"
                persistent-hint
                :rules="[timeoutRule]"
                class="settings-field"
              />
            </v-col>
          </v-row>
        </div>

        <div class="settings-group-label">缩略图</div>
        <div class="settings-group">
          <v-row>
            <v-col cols="6" sm="4">
              <v-select
                v-model.number="config.thumbnail_dpr"
                label="缩略图质量"
                :items="[
                  { title: '1x (省空间)', value: 1 },
                  { title: '2x (推荐)', value: 2 },
                  { title: '3x (高清)', value: 3 },
                ]"
                hint="质量越高占用存储越多"
                persistent-hint
                :rules="[dprRule]"
                class="settings-field"
              />
            </v-col>
            <v-col cols="6" sm="4">
              <v-text-field
                v-model="config.thumbnails_dir"
                label="缩略图存储目录"
                hint="留空使用默认缓存路径"
                persistent-hint
                class="settings-field"
                append-inner-icon="mdi-folder-open"
                @click:append-inner="selectDirectory('thumbnails_dir')"
              />
            </v-col>
          </v-row>
        </div>
      </v-card-text>
    </v-card>
    </v-form>

    <div class="settings-save-bar">
      <v-btn
        class="gradient-btn"
        size="large"
        variant="flat"
        :loading="saving"
        :disabled="!formValid"
        @click="saveSettings"
      >
        <v-icon start>mdi-content-save</v-icon>
        保存设置
      </v-btn>
      <v-fade-transition>
        <v-icon
          v-if="saved"
          color="success"
          class="ms-3 saved-icon"
        >
          mdi-check-circle
        </v-icon>
      </v-fade-transition>
    </div>
  </div>
</template>

<style scoped>
.settings-root {
  padding-bottom: 80px;
}

.settings-card {
  overflow: hidden;
}

.settings-card-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 20px 24px 16px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}

.wh-header-bg {
  background: linear-gradient(135deg, rgba(108,140,255,0.08) 0%, transparent 60%);
}
.rd-header-bg {
  background: linear-gradient(135deg, rgba(255,107,53,0.08) 0%, transparent 60%);
}
.db-header-bg {
  background: linear-gradient(135deg, rgba(67,233,123,0.08) 0%, transparent 60%);
}
.adv-header-bg {
  background: linear-gradient(135deg, rgba(167,139,250,0.08) 0%, transparent 60%);
}

.settings-header-icon {
  width: 40px;
  height: 40px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.wh-header-icon {
  background: rgba(108,140,255,0.15);
}
.rd-header-icon {
  background: rgba(255,107,53,0.15);
}
.db-header-icon {
  background: rgba(67,233,123,0.15);
}
.adv-header-icon {
  background: rgba(167,139,250,0.15);
}

.settings-group-label {
  font-size: 0.75rem;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  padding: 16px 0 8px;
  font-weight: 600;
}
.settings-group-label:first-of-type {
  padding-top: 8px;
}

.settings-group {
  padding: 0 0 4px;
}

.settings-field :deep(.v-field) {
  border-color: rgba(255, 255, 255, 0.1);
  transition: border-color 0.2s, box-shadow 0.2s;
}
.settings-field :deep(.v-field--focused) {
  border-color: var(--accent-primary);
  box-shadow: 0 0 0 1px rgba(108, 140, 255, 0.2);
}

.settings-save-bar {
  position: sticky;
  bottom: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  background: rgba(15, 15, 17, 0.9);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  z-index: 5;
}

.saved-icon {
  animation: saved-pop 0.4s cubic-bezier(0.16, 1, 0.3, 1);
}

@keyframes saved-pop {
  0% {
    transform: scale(0) rotate(-30deg);
  }
  50% {
    transform: scale(1.2) rotate(5deg);
  }
  100% {
    transform: scale(1) rotate(0deg);
  }
}
</style>
