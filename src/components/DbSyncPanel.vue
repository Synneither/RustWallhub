<script setup lang="ts">
/**
 * 数据同步卡：OSS 配置 + 快照导出/导入 + 云端上传/拉取。
 *
 * 从 DbSettingsView 抽出来时顺带修了两件事：
 * 1. 「弹窗要用户操作、loading 属于异步动作」原先混在一起（按钮会在目录选择框还开着的
 *    时候就转圈）。现在统一成「先完成用户交互（选目录 / 二次确认），再交给 useAsyncAction」，
 *    手动 try/catch/finally 也就随之消失了。
 * 2. 导入/拉取成功后不再由本组件清缓存，而是发 `imported` 让父视图统一重载（缺失、孤儿、
 *    统计、记录浏览四处都要刷新）。
 */
import { onMounted, ref } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { AppConfig, SyncImportResult } from "../types";
import {
  exportSnapshots,
  importSnapshots,
  ossSyncDownload,
  ossSyncUpload,
  saveSettings,
  testOssConfig,
} from "../utils/api";
import { appState, askConfirm, toast } from "../stores/app";
import { useAsyncAction } from "../composables/useAsyncAction";

// Vuetify 组件按需局部导入（见 main.ts 的注册策略说明）：只在这个视图/组件里用到，
// 挂全局注册会让首屏无条件背上它们。
import { VDivider } from "vuetify/components/VDivider";
import { VSwitch } from "vuetify/components/VSwitch";
import { VTextField } from "vuetify/components/VTextField";

const emit = defineEmits<{ imported: [] }>();

const ossEndpoint = ref("");
const ossBucket = ref("");
const ossAccessKeyId = ref("");
const ossAccessKeySecret = ref("");
const ossPrefix = ref("");
const ossAutoUpload = ref(false);
const ossAutoDownload = ref(false);

onMounted(() => {
  ossEndpoint.value = appState.config?.oss_endpoint ?? "";
  ossBucket.value = appState.config?.oss_bucket ?? "";
  ossAccessKeyId.value = appState.config?.oss_access_key_id ?? "";
  ossAccessKeySecret.value = appState.config?.oss_access_key_secret ?? "";
  ossPrefix.value = appState.config?.oss_prefix ?? "";
  ossAutoUpload.value = appState.config?.oss_auto_upload_on_exit ?? false;
  ossAutoDownload.value = appState.config?.oss_auto_download_on_start ?? false;
});

const { run: onSaveOss, loading: savingOss } = useAsyncAction(async () => {
  if (!appState.config) return;
  const next: AppConfig = {
    ...appState.config,
    oss_endpoint: ossEndpoint.value.trim(),
    oss_bucket: ossBucket.value.trim(),
    oss_access_key_id: ossAccessKeyId.value.trim(),
    oss_access_key_secret: ossAccessKeySecret.value.trim(),
    oss_prefix: ossPrefix.value.trim(),
    oss_auto_upload_on_exit: ossAutoUpload.value,
    oss_auto_download_on_start: ossAutoDownload.value,
  };
  await saveSettings(next);
  appState.config = next;
  toast("OSS 配置已保存", "success");
});

const { run: onTestOss, loading: testingOss } = useAsyncAction(async () => {
  toast(await testOssConfig(), "success");
});

/** 目录选择器属于用户交互，先选完再进入异步动作，免得按钮在弹窗开着时就转圈。 */
async function pickSnapshotDir(title: string, defaultPath?: string): Promise<string | null> {
  const dir = await openDialog({ directory: true, title, defaultPath });
  return typeof dir === "string" ? dir : null;
}

const { run: runExport, loading: exporting } = useAsyncAction(async (dir: string) => {
  const r = await exportSnapshots(dir);
  const names = [r.wallhaven ? "Wallhaven" : null, r.reddit ? "Reddit" : null]
    .filter(Boolean)
    .join("、");
  toast(`已导出 ${names} 快照到 ${dir}`, "success");
});

async function onExport() {
  // 刻意不把默认目录设成 db_dir：快照文件名与数据库同名，选到那里会覆盖数据库
  // （后端会拒绝，这里只是不把用户往坑里带）。
  const dir = await pickSnapshotDir("选择快照导出目录（请勿选择数据库所在目录）");
  if (!dir) return;
  await runExport(dir);
}

const { run: runImport, loading: importing } = useAsyncAction(async (dir: string) => {
  const r = await importSnapshots(`${dir}/wallhaven_images.db`, `${dir}/reddit_images.db`);
  toast(importResultText(r), "success");
  emit("imported");
});

async function onImport() {
  const dir = await pickSnapshotDir("选择包含快照文件的目录", appState.config?.db_dir || undefined);
  if (!dir) return;
  const ok = await askConfirm(
    "从快照导入",
    `将合并 ${dir} 下的快照到本地数据库：\n新记录会被插入，快照中标记喜欢的记录会恢复本地同条记录。\n本地已有数据不会被删除。是否继续？`,
    { confirmText: "导入" },
  );
  if (!ok) return;
  await runImport(dir);
}

const { run: onUpload, loading: uploading } = useAsyncAction(async () => {
  toast(await ossSyncUpload(), "success");
});

const { run: runCloudDownload, loading: cloudDownloading } = useAsyncAction(async () => {
  const r = await ossSyncDownload();
  toast(importResultText(r), "success");
  emit("imported");
});

async function onCloudDownload() {
  const ok = await askConfirm(
    "从云端拉取",
    "将下载 OSS 上的快照并合并到本地数据库：\n新记录会被插入，云端标记喜欢的记录会恢复本地同条记录。\n本地已有数据不会被删除。是否继续？",
    { confirmText: "拉取并合并" },
  );
  if (!ok) return;
  await runCloudDownload();
}

function importResultText(r: SyncImportResult): string {
  if (r.skipped) return "云端快照无变化，已跳过";
  const parts: string[] = [];
  if (r.wallhaven) {
    parts.push(`Wallhaven 新增 ${r.wallhaven.inserted} 条、恢复 ${r.wallhaven.loved} 条`);
  }
  if (r.reddit) {
    parts.push(`Reddit 新增 ${r.reddit.inserted} 条、恢复 ${r.reddit.loved} 条`);
  }
  return parts.length > 0 ? parts.join("；") : "没有可导入的内容";
}
</script>

<template>
  <div class="panel-card animate-in stagger-4">
    <div class="panel-card__title">
      <v-icon icon="mdi-cloud-sync-outline" size="18" color="primary" />数据同步
    </div>
    <p class="sync-desc text-body-2">
      快照导出为单文件（VACUUM INTO），合并按记录进行：新记录插入、喜欢的记录恢复，本地数据不会被删除。多设备同步推荐上传 OSS 后在另一台电脑拉取。
    </p>

    <div class="sync-grid">
      <v-text-field
        v-model="ossEndpoint"
        label="OSS Endpoint"
        placeholder="oss-cn-beijing.aliyuncs.com"
        density="compact"
        class="settings-field"
      />
      <v-text-field v-model="ossBucket" label="Bucket" density="compact" class="settings-field" />
      <v-text-field
        v-model="ossAccessKeyId"
        label="AccessKey ID"
        hint="建议使用 RAM 子账号，仅授权本前缀读写"
        persistent-hint
        density="compact"
        class="settings-field"
      />
      <v-text-field
        v-model="ossAccessKeySecret"
        label="AccessKey Secret"
        type="password"
        density="compact"
        class="settings-field"
      />
      <v-text-field
        v-model="ossPrefix"
        label="对象前缀（可选）"
        placeholder="rustwallhub/"
        density="compact"
        class="settings-field"
      />
    </div>
    <div class="sync-switches">
      <v-switch
        v-model="ossAutoUpload"
        color="primary"
        density="compact"
        hide-details
        label="退出应用时自动上传快照"
      />
      <v-switch
        v-model="ossAutoDownload"
        color="primary"
        density="compact"
        hide-details
        label="启动时自动从云端拉取并合并"
      />
    </div>

    <div class="sync-oss-actions">
      <v-btn color="primary" variant="flat" :loading="savingOss" @click="onSaveOss">保存配置</v-btn>
      <v-btn variant="tonal" prepend-icon="mdi-lan-check" :loading="testingOss" @click="onTestOss">
        测试连接
      </v-btn>
    </div>

    <v-divider class="my-4" />

    <div class="maint-actions">
      <v-btn variant="tonal" prepend-icon="mdi-file-export-outline" :loading="exporting" @click="onExport">
        导出到文件夹
      </v-btn>
      <v-btn variant="tonal" prepend-icon="mdi-file-import-outline" :loading="importing" @click="onImport">
        从文件夹导入
      </v-btn>
      <v-btn variant="tonal" prepend-icon="mdi-cloud-upload-outline" :loading="uploading" @click="onUpload">
        上传到云端
      </v-btn>
      <v-btn
        variant="tonal"
        prepend-icon="mdi-cloud-download-outline"
        :loading="cloudDownloading"
        @click="onCloudDownload"
      >
        从云端拉取并合并
      </v-btn>
    </div>
  </div>
</template>

<style scoped>
.sync-desc {
  color: var(--text-secondary);
  margin-bottom: var(--space-3);
}
.sync-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: var(--space-2) var(--space-3);
}
.sync-switches {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: var(--space-2);
}
.sync-oss-actions {
  display: flex;
  gap: var(--space-3);
  margin-top: var(--space-2);
}
</style>
