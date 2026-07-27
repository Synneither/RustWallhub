<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { VForm } from "vuetify/components";
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
  auto_update: boolean;
}

const config = ref<AppConfig | null>(null);
const configError = ref("");
const saving = ref(false);
const saved = ref(false);
const formValid = ref(false);
const formRef = ref<VForm | null>(null);
const localSnackbar = ref(false);
const localSnackbarText = ref("");
const checkingUpdate = ref(false);
const updateInfo = ref<{ has_update: boolean; version: string; current_version: string; body?: string; date?: string } | null>(null);
const installing = ref(false);
const updateProgress = ref<{ downloaded: number; total: number | null } | null>(null);
const updateStatus = ref<"idle" | "downloading" | "installing" | "error">("idle");
const updateError = ref("");
let unlistenUpdateProgress: (() => void) | null = null;
let unlistenUpdateInstalling: (() => void) | null = null;

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

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
  configError.value = "";
  try {
    config.value = await invoke<AppConfig>("get_config");
    logger.info("Settings", "配置已加载");
  } catch (e) {
    configError.value = String(e);
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
      auto_update: true,
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
    localSnackbarText.value = `保存设置失败: ${e}`;
    localSnackbar.value = true;
  }
  saving.value = false;
}

async function checkUpdate() {
  checkingUpdate.value = true;
  updateInfo.value = null;
  logger.action("Settings", "检查应用更新");
  try {
    const info = await invoke<{
      has_update: boolean;
      version: string;
      current_version: string;
      body?: string;
      date?: string;
    }>("check_update");
    updateInfo.value = info;
    logger.info("Settings", "检查更新完成", info);
  } catch (e) {
    logger.error("Settings", "检查更新失败", e);
    localSnackbarText.value = `检查更新失败: ${e}`;
    localSnackbar.value = true;
  }
  checkingUpdate.value = false;
}

async function installUpdate() {
  installing.value = true;
  updateStatus.value = "downloading";
  updateProgress.value = null;
  updateError.value = "";
  logger.action("Settings", "下载并安装更新");
  try {
    await invoke("install_update");
    // App will restart; code below may not execute on Windows
  } catch (e) {
    logger.error("Settings", "更新安装失败", e);
    updateStatus.value = "error";
    updateError.value = String(e);
  }
}

onMounted(async () => {
  loadConfig();
  unlistenUpdateProgress = await listen<{ downloaded: number; total: number | null }>(
    "update-progress",
    (e) => {
      updateProgress.value = e.payload;
    },
  );
  unlistenUpdateInstalling = await listen("update-installing", () => {
    updateStatus.value = "installing";
  });
});

onUnmounted(() => {
  if (unlistenUpdateProgress) unlistenUpdateProgress();
  if (unlistenUpdateInstalling) unlistenUpdateInstalling();
});
</script>

<template>
  <div v-if="config" class="settings-root">
    <v-alert
      v-if="configError"
      type="warning"
      variant="tonal"
      density="compact"
      class="mb-3 animate-in"
    >
      <div class="d-flex align-center justify-space-between">
        <span>配置加载失败，已使用默认设置: {{ configError }}</span>
        <v-btn size="x-small" variant="text" color="warning" prepend-icon="mdi-refresh" @click="loadConfig">
          重试
        </v-btn>
      </div>
    </v-alert>
    <v-form v-model="formValid" ref="formRef">
    <v-card class="glass-card settings-card animate-in stagger-1">
      <div class="settings-card-header wh-header-bg">
        <div class="settings-header-icon wh-header-icon">
          <v-icon color="#3b82f6">mdi-image-search</v-icon>
        </div>
        <div>
          <div class="text-heading">Wallhaven 设置</div>
          <div class="text-caption">保存目录与 API 配置</div>
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

        <div class="settings-group-label">API 配置</div>
        <div class="settings-group">
          <v-row>
            <v-col cols="12" sm="6" md="4">
              <v-text-field
                v-model="config.wallhaven_api_key"
                label="API Key（可选）"
                hint="提高 API 速率限制"
                type="password"
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
          <v-icon color="#f97316">mdi-reddit</v-icon>
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
            <v-col cols="12" sm="6" md="4">
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
            <v-col cols="12" sm="6" md="4">
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
      <div class="settings-card-header adv-header-bg">
        <div class="settings-header-icon adv-header-icon">
          <v-icon color="#c9a94e">mdi-tune-variant</v-icon>
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
            <v-col cols="12" sm="6" md="4">
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
            <v-col cols="12" sm="6" md="4">
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
            <v-col cols="12" sm="6" md="4">
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
            <v-col cols="12" sm="6" md="4">
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

        <div class="settings-group-label">应用更新</div>
        <div class="settings-group">
          <v-row align="center">
            <v-col cols="12" sm="6">
              <v-switch
                v-model="config.auto_update"
                label="启动时自动检查更新"
                color="primary"
                hide-details
                class="settings-field"
              />
            </v-col>
            <v-col cols="12" sm="6" class="d-flex align-center">
              <v-btn
                variant="tonal"
                size="small"
                color="primary"
                :loading="checkingUpdate"
                @click="checkUpdate"
              >
                <v-icon start size="16">mdi-update</v-icon>
                立即检查
              </v-btn>
            </v-col>
          </v-row>
          <v-row v-if="updateInfo">
            <v-col cols="12">
              <v-alert
                :type="updateInfo.has_update ? 'info' : 'success'"
                variant="tonal"
                density="compact"
                class="mt-2"
              >
                <template v-if="updateInfo.has_update">
                  <div class="d-flex align-center justify-space-between flex-wrap gap-2">
                    <span>发现新版本 <strong>{{ updateInfo.version }}</strong>（当前 {{ updateInfo.current_version }}）</span>
                    <v-btn
                      v-if="!installing"
                      variant="flat"
                      size="small"
                      color="primary"
                      @click="installUpdate"
                    >
                      <v-icon start size="16">mdi-download</v-icon>
                      立即更新
                    </v-btn>
                  </div>
                </template>
                <template v-else>
                  当前已是最新版本 <strong>{{ updateInfo.current_version }}</strong>
                </template>
              </v-alert>

              <!-- Download progress -->
              <div v-if="installing && updateStatus === 'downloading'" class="mt-3">
                <div class="d-flex align-center justify-space-between mb-1">
                  <span class="text-caption text-secondary">
                    <v-icon size="14" class="me-1">mdi-download</v-icon>
                    正在下载更新...
                  </span>
                  <span class="text-caption text-secondary" v-if="updateProgress">
                    {{ formatBytes(updateProgress.downloaded) }}{{ updateProgress.total ? ' / ' + formatBytes(updateProgress.total) : '' }}
                  </span>
                </div>
                <v-progress-linear
                  :model-value="updateProgress && updateProgress.total ? (updateProgress.downloaded / updateProgress.total) * 100 : 0"
                  color="primary"
                  height="6"
                  rounded
                  :indeterminate="!updateProgress || !updateProgress.total"
                />
              </div>

              <!-- Installing -->
              <v-alert v-if="installing && updateStatus === 'installing'" type="info" variant="tonal" density="compact" class="mt-2">
                <v-icon size="16" class="me-1 animate-spin">mdi-cog</v-icon>
                下载完成，正在安装更新，应用将自动重启...
              </v-alert>

              <!-- Error -->
              <v-alert v-if="updateStatus === 'error'" type="error" variant="tonal" density="compact" class="mt-2">
                更新失败：{{ updateError }}
                <v-btn variant="text" size="x-small" color="error" class="ms-2" @click="installing = false; updateStatus = 'idle'">
                  重试
                </v-btn>
              </v-alert>
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

    <v-snackbar v-model="localSnackbar" :timeout="3000" location="bottom" variant="tonal">
      {{ localSnackbarText }}
    </v-snackbar>
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
  background: linear-gradient(135deg, rgba(59,130,246,0.08) 0%, transparent 60%);
}
.rd-header-bg {
  background: linear-gradient(135deg, rgba(249,115,22,0.08) 0%, transparent 60%);
}
.adv-header-bg {
  background: linear-gradient(135deg, rgba(201,169,78,0.08) 0%, transparent 60%);
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
  background: rgba(59,130,246,0.15);
}
.rd-header-icon {
  background: rgba(249,115,22,0.15);
}
.adv-header-icon {
  background: rgba(201,169,78,0.15);
}

.animate-spin {
  animation: spin 1.5s linear infinite;
}
@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

</style>
