<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { AppConfig } from "../types";
import { checkUpdate, installUpdate, saveSettings } from "../utils/api";
import { appState, askConfirm, dbReady, ensureDatabases, toast, toastError } from "../stores/app";
import { positiveInt, requiredRule } from "../utils/rules";
import { useTheme, type Theme } from "../stores/theme";
import { formatBytes } from "../utils/format";

const { theme, userOverride, set: setTheme, resetToSystem } = useTheme();
// 与 store 实际状态派生：手动指定过则高亮对应主题，否则高亮"跟随系统"
const themeChoice = computed<"system" | Theme>(() =>
  userOverride.value ? theme.value : "system",
);

const draft = reactive({
  wallhaven_save_dir: "",
  reddit_save_dir: "",
  thumbnails_dir: "",
  download_concurrency: 6,
  request_timeout: 30,
  thumbnail_dpr: 2,
  proxy_url: "",
  auto_update: true,
});

onMounted(() => {
  const c = appState.config;
  if (!c) return;
  draft.wallhaven_save_dir = c.wallhaven_save_dir;
  draft.reddit_save_dir = c.reddit_save_dir;
  draft.thumbnails_dir = c.thumbnails_dir;
  draft.download_concurrency = c.download_concurrency;
  draft.request_timeout = c.request_timeout;
  draft.thumbnail_dpr = c.thumbnail_dpr;
  draft.proxy_url = c.proxy_url;
  draft.auto_update = c.auto_update;
});

async function pickDir(field: "wallhaven_save_dir" | "reddit_save_dir" | "thumbnails_dir") {
  try {
    const selected = await openDialog({ directory: true, defaultPath: draft[field] || undefined });
    if (typeof selected === "string") draft[field] = selected;
  } catch (e) {
    toastError(e);
  }
}

/* ── 保存 ── */
const saving = ref(false);
const savedFlash = ref(false);

async function onSave() {
  if (!appState.config || saving.value) return;
  saving.value = true;
  try {
    const next: AppConfig = { ...appState.config, ...draft };
    await saveSettings(next);
    appState.config = next;
    savedFlash.value = true;
    window.setTimeout(() => (savedFlash.value = false), 1500);
    toast("设置已保存", "success");
    // save_settings 只建目录不建库：保存后若库缺失，引导初始化
    if (!dbReady.value) {
      const ok = await askConfirm("初始化数据库", "数据库文件不存在，是否现在创建？", { confirmText: "创建" });
      if (ok) {
        const created = await ensureDatabases();
        toast(created.length > 0 ? `已创建数据库：${created.join("、")}` : "数据库已就绪", "success");
      }
    }
  } catch (e) {
    toastError(e);
  } finally {
    saving.value = false;
  }
}

/* ── 更新 ── */
const checking = ref(false);
const installing = ref(false);
const checkDone = ref(false);

const updateInfo = computed(() => appState.update.info);
const updatePercent = computed(() => {
  const u = appState.update;
  if (!u.total || u.total <= 0) return null;
  return Math.min(100, Math.round((u.downloaded / u.total) * 100));
});

async function onCheckUpdate() {
  checking.value = true;
  checkDone.value = false;
  try {
    const info = await checkUpdate();
    appState.update.info = info;
    checkDone.value = true;
    if (!info.has_update) toast("已是最新版本", "success");
  } catch (e) {
    toastError(e);
  } finally {
    checking.value = false;
  }
}

async function onInstall() {
  installing.value = true;
  appState.update.downloading = true;
  appState.update.downloaded = 0;
  appState.update.total = null;
  try {
    await installUpdate();
    // 成功后应用会自动重启（update-installing 遮罩由全局事件驱动）
  } catch (e) {
    toastError(e);
    appState.update.downloading = false;
    installing.value = false;
  }
}

function onThemeChange(v: "system" | Theme) {
  if (v === "system") resetToSystem();
  else setTheme(v);
}
</script>

<template>
  <div class="view settings-view">
    <div class="view-header">
      <span class="view-header__title">设置</span>
      <span class="view-header__sub">存储、下载、网络与更新</span>
    </div>

    <v-form class="settings-form" @submit.prevent>
      <!-- 存储 -->
      <div class="panel-card animate-in">
        <div class="panel-card__title"><v-icon icon="mdi-folder-outline" size="18" color="primary" />存储</div>
        <div
          v-for="f in [
            { key: 'wallhaven_save_dir', label: 'Wallhaven 保存目录' },
            { key: 'reddit_save_dir', label: 'Reddit 保存目录' },
            { key: 'thumbnails_dir', label: '缩略图目录' },
          ]"
          :key="f.key"
          class="dir-field"
        >
          <v-text-field
            v-model="draft[f.key as keyof typeof draft]"
            :label="f.label"
            :rules="[requiredRule]"
            hide-details
            class="settings-field"
            readonly
          />
          <v-btn variant="tonal" @click="pickDir(f.key as 'wallhaven_save_dir' | 'reddit_save_dir' | 'thumbnails_dir')">
            选择
          </v-btn>
        </div>
      </div>

      <!-- 下载 -->
      <div class="panel-card animate-in stagger-1">
        <div class="panel-card__title"><v-icon icon="mdi-download-outline" size="18" color="primary" />下载</div>
        <div class="settings-grid">
          <v-text-field
            v-model.number="draft.download_concurrency"
            type="number"
            label="并发下载数"
            hint="1 - 100"
            persistent-hint
            :rules="[(v: number) => positiveInt(v, { min: 1, max: 100 })]"
            class="settings-field"
          />
          <v-text-field
            v-model.number="draft.request_timeout"
            type="number"
            label="请求超时（秒）"
            hint="5 - 120"
            persistent-hint
            :rules="[(v: number) => positiveInt(v, { min: 5, max: 120 })]"
            class="settings-field"
          />
          <v-select
            v-model.number="draft.thumbnail_dpr"
            :items="[
              { title: '1x（240px）', value: 1 },
              { title: '2x（480px）', value: 2 },
              { title: '3x（720px）', value: 3 },
            ]"
            label="缩略图清晰度"
            hint="越高越清晰，占用空间越大"
            persistent-hint
            class="settings-field"
          />
        </div>
      </div>

      <!-- 网络 -->
      <div class="panel-card animate-in stagger-2">
        <div class="panel-card__title"><v-icon icon="mdi-web" size="18" color="primary" />网络</div>
        <v-text-field
          v-model="draft.proxy_url"
          label="HTTP 代理"
          placeholder="http://127.0.0.1:7890"
          hint="留空表示直连；保存后立即生效"
          persistent-hint
          clearable
          class="settings-field"
        />
      </div>

      <!-- 更新 -->
      <div class="panel-card animate-in stagger-3">
        <div class="panel-card__title"><v-icon icon="mdi-rocket-launch-outline" size="18" color="primary" />更新</div>
        <div class="update-row">
          <v-switch
            v-model="draft.auto_update"
            label="启动时自动检查更新"
            color="primary"
            hide-details
            density="compact"
          />
          <v-spacer />
          <span class="text-caption">当前版本 v{{ updateInfo?.current_version ?? appState.update.info?.current_version ?? "-" }}</span>
          <v-btn variant="tonal" :loading="checking" @click="onCheckUpdate">检查更新</v-btn>
        </div>

        <div v-if="updateInfo?.has_update" class="update-available">
          <div class="update-available__head">
            <v-icon icon="mdi-tag-outline" size="18" color="primary" />
            <span class="text-body-lg">新版本 v{{ updateInfo.version }}</span>
            <span v-if="updateInfo.date" class="text-caption">{{ updateInfo.date.slice(0, 10) }}</span>
          </div>
          <p v-if="updateInfo.body" class="text-caption update-available__body">{{ updateInfo.body }}</p>
          <div v-if="appState.update.downloading" class="update-progress">
            <v-progress-linear
              :model-value="updatePercent ?? 0"
              :indeterminate="updatePercent === null"
              color="primary"
              height="4"
              rounded
            />
            <span class="text-caption">
              正在下载 {{ formatBytes(appState.update.downloaded) }}<template v-if="appState.update.total"> / {{ formatBytes(appState.update.total) }}</template>
            </span>
          </div>
          <v-btn
            v-else
            color="primary"
            variant="flat"
            :loading="installing"
            @click="onInstall"
          >
            下载并安装（自动重启）
          </v-btn>
        </div>
        <div v-else-if="checkDone" class="text-caption">已是最新版本</div>
      </div>

      <!-- 外观 -->
      <div class="panel-card animate-in stagger-4">
        <div class="panel-card__title"><v-icon icon="mdi-palette-outline" size="18" color="primary" />外观</div>
        <div class="theme-row">
          <v-btn
            v-for="t in [
              { key: 'system', label: '跟随系统', icon: 'mdi-monitor' },
              { key: 'dim', label: '深色', icon: 'mdi-weather-night' },
              { key: 'light', label: '浅色', icon: 'mdi-white-balance-sunny' },
            ]"
            :key="t.key"
            :variant="(t.key === 'system' ? themeChoice === 'system' : theme === t.key && themeChoice !== 'system') ? 'flat' : 'outlined'"
            :color="(t.key === 'system' ? themeChoice === 'system' : theme === t.key && themeChoice !== 'system') ? 'primary' : undefined"
            :prepend-icon="t.icon"
            size="small"
            @click="onThemeChange(t.key as 'system' | Theme)"
          >
            {{ t.label }}
          </v-btn>
        </div>
      </div>
    </v-form>

    <!-- 保存条 -->
    <div class="settings-save-bar">
      <v-btn color="primary" variant="flat" min-width="200" :loading="saving" @click="onSave">
        <v-icon v-if="savedFlash" icon="mdi-check" class="saved-icon" start />
        {{ savedFlash ? "已保存" : "保存全部设置" }}
      </v-btn>
    </div>
  </div>
</template>

<style scoped>
.settings-view {
  padding-bottom: 0;
}
.settings-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
  padding-bottom: var(--space-5);
}
.dir-field {
  display: flex;
  gap: var(--space-3);
  align-items: center;
}
.dir-field .settings-field {
  flex: 1;
}
.update-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}
.update-available {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-4);
  border-radius: var(--radius-md);
  background: var(--accent-primary-dim);
  align-items: flex-start;
}
.update-available__head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.update-available__body {
  max-height: 120px;
  overflow-y: auto;
  white-space: pre-wrap;
}
.update-progress {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.theme-row {
  display: flex;
  gap: var(--space-2);
}
.settings-save-bar {
  margin: 0 calc(-1 * var(--space-8));
}
</style>
