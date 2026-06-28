<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { logger } from "../utils/logger";
import { VForm } from "vuetify/components";

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

interface DbStats {
  total: number;
  love: number;
  dislike: number;
}

const config = ref<AppConfig | null>(null);
const saving = ref(false);
const saved = ref(false);
const formValid = ref(false);
const formRef = ref<VForm | null>(null);
const whStats = ref<DbStats>({ total: 0, love: 0, dislike: 0 });
const rdStats = ref<DbStats>({ total: 0, love: 0, dislike: 0 });

const requiredRule = (v: string) => !!v || "此项不能为空";

const whPathExists = ref(false);
const rdPathExists = ref(false);
const whSize = ref("");
const rdSize = ref("");

async function selectDirectory() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择数据库目录",
    });
    if (selected && config.value) {
      config.value.db_dir = selected;
      checkDbFiles();
    }
  } catch (e) {
    logger.error("DbSettings", "目录选择失败", e);
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / 1024 / 1024).toFixed(1) + " MB";
}

async function checkDbFiles() {
  if (!config.value) return;
  try {
    const files: { name: string; path: string; size: number }[] = await invoke("scan_directory", { dir: config.value.db_dir });
    const whFile = files.find((f) => f.name === "wallhaven_images.db");
    const rdFile = files.find((f) => f.name === "reddit_images.db");
    whPathExists.value = !!whFile;
    rdPathExists.value = !!rdFile;
    whSize.value = whFile ? formatSize(whFile.size) : "";
    rdSize.value = rdFile ? formatSize(rdFile.size) : "";
  } catch {
    whPathExists.value = false;
    rdPathExists.value = false;
  }
}

async function loadDbStats() {
  try {
    const stats: { wallhaven: DbStats; reddit: DbStats } = await invoke("get_stats");
    whStats.value = stats.wallhaven;
    rdStats.value = stats.reddit;
  } catch (e) {
    logger.error("DbSettings", "加载统计失败", e);
  }
}

async function saveSettings() {
  if (!config.value) return;
  saving.value = true;
  saved.value = false;
  logger.action("DbSettings", "保存数据库设置");
  try {
    await invoke("save_settings", { config: config.value });
    saved.value = true;
    logger.info("DbSettings", "设置已保存");
    await loadDbStats();
    await checkDbFiles();
    setTimeout(() => (saved.value = false), 2000);
  } catch (e) {
    logger.error("DbSettings", "保存设置失败", e);
  }
  saving.value = false;
}

async function initDatabase() {
  if (!config.value) return;
  logger.action("DbSettings", "初始化数据库");
  try {
    await invoke("save_settings", { config: config.value });
    await loadDbStats();
    await checkDbFiles();
    logger.info("DbSettings", "数据库已初始化");
  } catch (e) {
    logger.error("DbSettings", "初始化失败", e);
  }
}

async function loadConfig() {
  try {
    config.value = await invoke<AppConfig>("get_config");
    logger.info("DbSettings", "配置已加载");
    await loadDbStats();
    await checkDbFiles();
  } catch (e) {
    logger.error("DbSettings", "配置加载失败", e);
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

onMounted(loadConfig);
</script>

<template>
  <div v-if="config" class="db-settings-root">
    <v-form v-model="formValid" ref="formRef">
      <v-card class="glass-card db-card animate-in stagger-1">
        <div class="db-card-header">
          <div class="db-header-icon">
            <v-icon color="#43e97b" size="28">mdi-database-cog</v-icon>
          </div>
          <div>
            <div class="text-heading">数据库目录</div>
            <div class="text-caption">统一管理 Wallhaven 和 Reddit 数据库存储位置</div>
          </div>
        </div>
        <v-card-text class="pa-6 pt-4">
          <div class="settings-group-label">存储路径</div>
          <div class="settings-group">
            <v-text-field
              v-model="config.db_dir"
              label="数据库目录"
              hint="存放 wallhaven_images.db 和 reddit_images.db"
              persistent-hint
              class="settings-field"
              :rules="[requiredRule]"
              append-inner-icon="mdi-folder-open"
              @click:append-inner="selectDirectory"
            />
          </div>

          <div class="settings-group-label">数据库文件</div>
          <div class="db-file-list">
            <div class="db-file-item">
              <div class="db-file-left">
                <v-icon color="#6c8cff" size="20">mdi-database</v-icon>
                <div class="db-file-info">
                  <span class="db-file-name">wallhaven_images.db</span>
                  <span class="db-file-meta">Wallhaven 数据库</span>
                </div>
              </div>
              <div class="db-file-right">
                <v-chip v-if="whPathExists" size="x-small" color="success" variant="flat">已存在 {{ whSize }}</v-chip>
                <v-chip v-else size="x-small" color="warning" variant="flat">未创建</v-chip>
              </div>
            </div>
            <div class="db-file-item">
              <div class="db-file-left">
                <v-icon color="#ff6b35" size="20">mdi-database</v-icon>
                <div class="db-file-info">
                  <span class="db-file-name">reddit_images.db</span>
                  <span class="db-file-meta">Reddit 数据库</span>
                </div>
              </div>
              <div class="db-file-right">
                <v-chip v-if="rdPathExists" size="x-small" color="success" variant="flat">已存在 {{ rdSize }}</v-chip>
                <v-chip v-else size="x-small" color="warning" variant="flat">未创建</v-chip>
              </div>
            </div>
          </div>
        </v-card-text>
      </v-card>

      <v-card class="glass-card db-card animate-in stagger-2">
        <div class="db-card-header db-stats-header">
          <div class="db-header-icon">
            <v-icon color="#a78bfa" size="28">mdi-chart-box-outline</v-icon>
          </div>
          <div>
            <div class="text-heading">数据库概览</div>
            <div class="text-caption">当前数据库中的图片统计</div>
          </div>
        </div>
        <v-card-text class="pa-6 pt-4">
          <v-row>
            <v-col cols="12" sm="6">
              <div class="stat-box wh-stat-box">
                <div class="stat-box-header">
                  <v-icon color="#6c8cff" size="16">mdi-image-search</v-icon>
                  <span>Wallhaven</span>
                </div>
                <div class="stat-numbers">
                  <div class="stat-item">
                    <span class="stat-value">{{ whStats.total }}</span>
                    <span class="stat-label">总计</span>
                  </div>
                  <div class="stat-item">
                    <span class="stat-value stat-love">{{ whStats.love }}</span>
                    <span class="stat-label">可用</span>
                  </div>
                  <div class="stat-item">
                    <span class="stat-value stat-dislike">{{ whStats.dislike }}</span>
                    <span class="stat-label">缺失</span>
                  </div>
                </div>
              </div>
            </v-col>
            <v-col cols="12" sm="6">
              <div class="stat-box rd-stat-box">
                <div class="stat-box-header">
                  <v-icon color="#ff6b35" size="16">mdi-reddit</v-icon>
                  <span>Reddit</span>
                </div>
                <div class="stat-numbers">
                  <div class="stat-item">
                    <span class="stat-value">{{ rdStats.total }}</span>
                    <span class="stat-label">总计</span>
                  </div>
                  <div class="stat-item">
                    <span class="stat-value stat-love">{{ rdStats.love }}</span>
                    <span class="stat-label">可用</span>
                  </div>
                  <div class="stat-item">
                    <span class="stat-value stat-dislike">{{ rdStats.dislike }}</span>
                    <span class="stat-label">缺失</span>
                  </div>
                </div>
              </div>
            </v-col>
          </v-row>
        </v-card-text>
      </v-card>
    </v-form>

    <div class="db-settings-save-bar">
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
      <v-btn
        class="ms-3"
        variant="outlined"
        color="#a78bfa"
        size="large"
        @click="initDatabase"
      >
        <v-icon start>mdi-database-refresh</v-icon>
        初始化数据库
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
.db-settings-root {
  padding-bottom: 80px;
}

.db-card {
  overflow: hidden;
}

.db-card-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 20px 24px 16px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  background: linear-gradient(135deg, rgba(67, 233, 123, 0.06) 0%, transparent 60%);
}

.db-stats-header {
  background: linear-gradient(135deg, rgba(167, 139, 250, 0.06) 0%, transparent 60%);
}

.db-header-icon {
  width: 40px;
  height: 40px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  background: rgba(67, 233, 123, 0.15);
}

.db-stats-header .db-header-icon {
  background: rgba(167, 139, 250, 0.15);
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

.db-file-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.db-file-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.06);
}

.db-file-left {
  display: flex;
  align-items: center;
  gap: 12px;
}

.db-file-info {
  display: flex;
  flex-direction: column;
}

.db-file-name {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-primary);
  font-family: "SF Mono", "Fira Code", monospace;
}

.db-file-meta {
  font-size: 0.75rem;
  color: var(--text-tertiary);
}

.stat-box {
  padding: 16px;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(255, 255, 255, 0.02);
}

.stat-box-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 12px;
}

.stat-numbers {
  display: flex;
  gap: 24px;
}

.stat-item {
  display: flex;
  flex-direction: column;
  align-items: center;
}

.stat-value {
  font-size: 1.5rem;
  font-weight: 700;
  color: var(--text-primary);
}

.stat-value.stat-love {
  color: #43e97b;
}

.stat-value.stat-dislike {
  color: #ff6b35;
}

.stat-label {
  font-size: 0.6875rem;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.db-settings-save-bar {
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
  0% { transform: scale(0) rotate(-30deg); }
  50% { transform: scale(1.2) rotate(5deg); }
  100% { transform: scale(1) rotate(0deg); }
}
</style>
