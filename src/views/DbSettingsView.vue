<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { AppConfig, ImageRecord, OrphanFile, SyncImportResult } from "../types";
import {
  adoptOrphanFiles,
  checkDatabases,
  cleanThumbnails,
  deleteOrphanFiles,
  downloadMissingImages,
  exportSnapshots,
  importSnapshots,
  listDatabaseImages,
  listMissingImages,
  listOrphanFiles,
  markDislikedFiles,
  ossSyncDownload,
  ossSyncUpload,
  recoverDatabaseFiles,
  restoreAllFiles,
  saveSettings,
  testOssConfig,
} from "../utils/api";
import { appState, askConfirm, dbReady, ensureDatabases, refreshStats, toast, toastError } from "../stores/app";
import { requiredRule } from "../utils/rules";
import { formatBytes, formatDateTime } from "../utils/format";
import StatPanel from "../components/StatPanel.vue";
import EmptyState from "../components/EmptyState.vue";
import ProgressCard from "../components/ProgressCard.vue";

/* ════ 库状态与 db_dir ════ */
const dbDir = ref("");
const savingDir = ref(false);

onMounted(async () => {
  dbDir.value = appState.config?.db_dir ?? "";
  ossEndpoint.value = appState.config?.oss_endpoint ?? "";
  ossBucket.value = appState.config?.oss_bucket ?? "";
  ossAccessKeyId.value = appState.config?.oss_access_key_id ?? "";
  ossAccessKeySecret.value = appState.config?.oss_access_key_secret ?? "";
  ossPrefix.value = appState.config?.oss_prefix ?? "";
  await reloadAll();
});

async function pickDbDir() {
  try {
    const selected = await openDialog({ directory: true, defaultPath: dbDir.value || undefined });
    if (typeof selected === "string") dbDir.value = selected;
  } catch (e) {
    toastError(e);
  }
}

async function onSaveDir() {
  if (!appState.config || savingDir.value) return;
  savingDir.value = true;
  try {
    const next: AppConfig = { ...appState.config, db_dir: dbDir.value };
    await saveSettings(next);
    appState.config = next;
    appState.dbStatus = await checkDatabases();
    toast("数据库目录已保存", "success");
    if (!dbReady.value) {
      const ok = await askConfirm("初始化数据库", "新目录下数据库文件不存在，是否现在创建？", { confirmText: "创建" });
      if (ok) await onInitDatabases();
    } else {
      await reloadAll();
    }
  } catch (e) {
    toastError(e);
  } finally {
    savingDir.value = false;
  }
}

const initializing = ref(false);
async function onInitDatabases() {
  initializing.value = true;
  try {
    const created = await ensureDatabases();
    toast(created.length > 0 ? `已创建数据库：${created.join("、")}` : "数据库已就绪", "success");
    await reloadAll();
  } catch (e) {
    toastError(e);
  } finally {
    initializing.value = false;
  }
}

/* ════ 数据加载 ════ */
const loading = ref(false);
const missingCount = ref(0);
const missing = ref<ImageRecord[]>([]);
const orphans = ref<OrphanFile[]>([]);

async function reloadAll() {
  if (!dbReady.value) return;
  loading.value = true;
  try {
    // 缺失列表与缺失计数来自同一次扫描，直接用列表长度，省掉一次重复 IPC + 磁盘扫描。
    const [m, o] = await Promise.all([listMissingImages("all"), listOrphanFiles("all")]);
    missingCount.value = m.length;
    missing.value = m;
    orphans.value = o;
    await refreshStats();
    await loadRecords();
  } catch (e) {
    toastError(e);
  } finally {
    loading.value = false;
  }
}

/* ════ 缺失文件操作 ════ */
const missingSelected = ref<ImageRecord[]>([]);

async function onDownloadSelectedMissing() {
  if (missingSelected.value.length === 0) return;
  const bySource = groupBySource(missingSelected.value);
  try {
    for (const [source, records] of Object.entries(bySource)) {
      const msg = await downloadMissingImages(source as "wallhaven" | "reddit", records);
      toast(msg, "info");
    }
    missingSelected.value = [];
  } catch (e) {
    toastError(e);
  }
}

async function onRecoverAll() {
  const ok = await askConfirm(
    "全部补下载",
    `将把两个库中所有标记缺失的图片（共 ${missingCount.value} 张）按记录的原 URL 重新下载。\n该任务在后台执行，可随时取消。是否继续？`,
    { confirmText: "开始补下载" },
  );
  if (!ok) return;
  try {
    // 后端 recover 对 "all" 只处理 Reddit，需两源分别调用
    await recoverDatabaseFiles("wallhaven");
    await recoverDatabaseFiles("reddit");
    toast("补下载任务已启动（Wallhaven + Reddit）", "info");
  } catch (e) {
    toastError(e);
  }
}

async function onMarkDisliked() {
  const ok = await askConfirm(
    "标记为不喜欢",
    `将把 ${missingCount.value} 条缺失记录标记为不喜欢（love=0），不再计入缺失。\n此操作不删除任何文件，可通过「恢复所有已标记」撤销。`,
    { danger: true, confirmText: "标记" },
  );
  if (!ok) return;
  try {
    const n = await markDislikedFiles("all");
    toast(`已标记 ${n} 条记录`, "success");
    await reloadAll();
  } catch (e) {
    toastError(e);
  }
}

/* ════ 孤儿文件操作 ════ */
const orphanSelected = ref<OrphanFile[]>([]);

function groupBySource<T extends { source: string }>(items: T[]): Record<string, T[]> {
  const out: Record<string, T[]> = {};
  for (const it of items) {
    (out[it.source] ??= []).push(it);
  }
  return out;
}

async function onAdopt() {
  if (orphanSelected.value.length === 0) return;
  const bySource = groupBySource(orphanSelected.value);
  let total = 0;
  try {
    for (const [source, files] of Object.entries(bySource)) {
      total += await adoptOrphanFiles(
        source as "wallhaven" | "reddit",
        files.map((f) => f.name),
      );
    }
    toast(`已收养 ${total} 个文件入库`, "success");
    orphanSelected.value = [];
    await reloadAll();
  } catch (e) {
    toastError(e);
  }
}

async function onDeleteOrphans() {
  if (orphanSelected.value.length === 0) return;
  const ok = await askConfirm(
    "删除孤儿文件",
    `将永久删除 ${orphanSelected.value.length} 个文件及其缩略图。\n这些文件不在数据库中，删除后无法通过补下载恢复。`,
    { danger: true, confirmText: "删除" },
  );
  if (!ok) return;
  try {
    const bySource = groupBySource(orphanSelected.value);
    let removed = 0;
    for (const [src, files] of Object.entries(bySource)) {
      removed += await deleteOrphanFiles(
        src as "wallhaven" | "reddit",
        files.map((f) => f.name),
      );
    }
    toast(`已删除 ${removed} 个文件`, "success");
    orphanSelected.value = [];
    await reloadAll();
  } catch (e) {
    toastError(e);
  }
}

/* ════ 维护 ════ */
async function onCleanThumbnails() {
  try {
    const r = await cleanThumbnails();
    toast(`已清理孤儿缩略图：Wallhaven ${r.wallhaven} 个，Reddit ${r.reddit} 个`, "success");
  } catch (e) {
    toastError(e);
  }
}

/* ════ 数据同步（快照导出/导入 + OSS） ════ */
const ossEndpoint = ref("");
const ossBucket = ref("");
const ossAccessKeyId = ref("");
const ossAccessKeySecret = ref("");
const ossPrefix = ref("");
const savingOss = ref(false);
const testingOss = ref(false);
const exporting = ref(false);
const importing = ref(false);
const uploading = ref(false);
const cloudDownloading = ref(false);

async function onSaveOss() {
  if (!appState.config || savingOss.value) return;
  savingOss.value = true;
  try {
    const next: AppConfig = {
      ...appState.config,
      oss_endpoint: ossEndpoint.value.trim(),
      oss_bucket: ossBucket.value.trim(),
      oss_access_key_id: ossAccessKeyId.value.trim(),
      oss_access_key_secret: ossAccessKeySecret.value.trim(),
      oss_prefix: ossPrefix.value.trim(),
    };
    await saveSettings(next);
    appState.config = next;
    toast("OSS 配置已保存", "success");
  } catch (e) {
    toastError(e);
  } finally {
    savingOss.value = false;
  }
}

async function onTestOss() {
  if (testingOss.value) return;
  testingOss.value = true;
  try {
    const msg = await testOssConfig();
    toast(msg, "success");
  } catch (e) {
    toastError(e);
  } finally {
    testingOss.value = false;
  }
}

async function onExport() {
  if (exporting.value) return;
  try {
    const dir = await openDialog({
      directory: true,
      title: "选择快照导出目录",
      defaultPath: appState.config?.db_dir || undefined,
    });
    if (typeof dir !== "string") return;
    exporting.value = true;
    const r = await exportSnapshots(dir);
    const names = [r.wallhaven ? "Wallhaven" : null, r.reddit ? "Reddit" : null]
      .filter(Boolean)
      .join("、");
    toast(`已导出 ${names} 快照到 ${dir}`, "success");
  } catch (e) {
    toastError(e);
  } finally {
    exporting.value = false;
  }
}

async function onImport() {
  if (importing.value) return;
  try {
    const dir = await openDialog({
      directory: true,
      title: "选择包含快照文件的目录",
      defaultPath: appState.config?.db_dir || undefined,
    });
    if (typeof dir !== "string") return;

    const whPath = `${dir}/wallhaven_images.db`;
    const rdPath = `${dir}/reddit_images.db`;
    const ok = await askConfirm(
      "从快照导入",
      `将合并 ${dir} 下的快照到本地数据库：\n新记录会被插入，快照中标记喜欢的记录会恢复本地同条记录。\n本地已有数据不会被删除。是否继续？`,
      { confirmText: "导入" },
    );
    if (!ok) return;

    importing.value = true;
    const r = await importSnapshots(whPath, rdPath);
    toast(importResultText(r), "success");
    await reloadAll();
  } catch (e) {
    toastError(e);
  } finally {
    importing.value = false;
  }
}

async function onUpload() {
  if (uploading.value) return;
  uploading.value = true;
  try {
    const msg = await ossSyncUpload();
    toast(msg, "success");
  } catch (e) {
    toastError(e);
  } finally {
    uploading.value = false;
  }
}

async function onCloudDownload() {
  if (cloudDownloading.value) return;
  const ok = await askConfirm(
    "从云端拉取",
    "将下载 OSS 上的快照并合并到本地数据库：\n新记录会被插入，云端标记喜欢的记录会恢复本地同条记录。\n本地已有数据不会被删除。是否继续？",
    { confirmText: "拉取并合并" },
  );
  if (!ok) return;
  cloudDownloading.value = true;
  try {
    const r = await ossSyncDownload();
    toast(importResultText(r), "success");
    await reloadAll();
  } catch (e) {
    toastError(e);
  } finally {
    cloudDownloading.value = false;
  }
}

function importResultText(r: SyncImportResult): string {
  const parts: string[] = [];
  if (r.wallhaven) {
    parts.push(`Wallhaven 新增 ${r.wallhaven.inserted} 条、恢复 ${r.wallhaven.loved} 条`);
  }
  if (r.reddit) {
    parts.push(`Reddit 新增 ${r.reddit.inserted} 条、恢复 ${r.reddit.loved} 条`);
  }
  return parts.length > 0 ? parts.join("；") : "没有可导入的内容";
}

async function onRestoreAll() {
  const ok = await askConfirm(
    "恢复所有已标记",
    "将把两个库中所有 love=0 的记录恢复为正常（love=1）。是否继续？",
    { confirmText: "恢复" },
  );
  if (!ok) return;
  try {
    const n = await restoreAllFiles("all");
    toast(`已恢复 ${n} 条记录`, "success");
    await reloadAll();
  } catch (e) {
    toastError(e);
  }
}

/* ════ 记录浏览 ════ */
const recordSource = ref<"wallhaven" | "reddit">("wallhaven");
const records = ref<ImageRecord[]>([]);
const recordPage = ref(1);
const RECORD_PAGE_SIZE = 20;
const recordsLoading = ref(false);

const recordTotal = computed(() =>
  recordSource.value === "wallhaven"
    ? (appState.stats?.wallhaven.total ?? 0)
    : (appState.stats?.reddit.total ?? 0),
);
const recordTotalPages = computed(() =>
  Math.max(1, Math.ceil(recordTotal.value / RECORD_PAGE_SIZE)),
);

async function loadRecords() {
  recordsLoading.value = true;
  try {
    records.value = await listDatabaseImages(
      recordSource.value,
      RECORD_PAGE_SIZE,
      (recordPage.value - 1) * RECORD_PAGE_SIZE,
    );
  } catch (e) {
    toastError(e);
  } finally {
    recordsLoading.value = false;
  }
}

async function onRecordSourceChange(s: "wallhaven" | "reddit") {
  recordSource.value = s;
  recordPage.value = 1;
  await loadRecords();
}

async function onRecordPage(delta: number) {
  const next = recordPage.value + delta;
  if (next < 1 || next > recordTotalPages.value) return;
  recordPage.value = next;
  await loadRecords();
}

const tab = ref<"missing" | "orphan" | "records">("missing");
</script>

<template>
  <div class="view">
    <div class="view-header">
      <span class="view-header__title">数据库</span>
      <span class="view-header__sub">库状态、缺失与孤儿文件管理</span>
    </div>

    <!-- 库状态 -->
    <div class="panel-card animate-in">
      <div class="panel-card__title"><v-icon icon="mdi-database-outline" size="18" color="primary" />库状态</div>
      <div class="dir-field">
        <v-text-field
          v-model="dbDir"
          label="数据库目录"
          hint="两个数据库文件路径由该目录派生"
          persistent-hint
          :rules="[requiredRule]"
          class="settings-field"
        />
        <v-btn variant="tonal" @click="pickDbDir">选择</v-btn>
        <v-btn color="primary" variant="flat" :loading="savingDir" @click="onSaveDir">保存</v-btn>
      </div>
      <div class="db-paths">
        <div class="db-path-row">
          <v-icon
            :icon="appState.dbStatus?.wallhaven_exists ? 'mdi-check-circle' : 'mdi-alert-circle-outline'"
            size="16"
            :color="appState.dbStatus?.wallhaven_exists ? 'success' : 'warning'"
          />
          <span class="text-caption db-path-row__path">{{ appState.dbStatus?.wallhaven_path ?? "-" }}</span>
        </div>
        <div class="db-path-row">
          <v-icon
            :icon="appState.dbStatus?.reddit_exists ? 'mdi-check-circle' : 'mdi-alert-circle-outline'"
            size="16"
            :color="appState.dbStatus?.reddit_exists ? 'success' : 'warning'"
          />
          <span class="text-caption db-path-row__path">{{ appState.dbStatus?.reddit_path ?? "-" }}</span>
        </div>
      </div>
      <div v-if="!dbReady">
        <v-btn color="primary" variant="flat" :loading="initializing" @click="onInitDatabases">
          创建数据库
        </v-btn>
      </div>
    </div>

    <EmptyState
      v-if="!dbReady"
      icon="mdi-database-off-outline"
      title="数据库未初始化"
      desc="创建数据库后，统计、缺失与孤儿文件管理才可用"
    />

    <template v-else>
      <!-- 统计 -->
      <div class="db-stats">
        <StatPanel source="wallhaven" :stats="appState.stats?.wallhaven ?? null" :loading="!appState.stats" class="animate-in stagger-1" />
        <StatPanel source="reddit" :stats="appState.stats?.reddit ?? null" :loading="!appState.stats" class="animate-in stagger-2" />
      </div>

      <ProgressCard source="wallhaven" title="Wallhaven 补下载" />
      <ProgressCard source="reddit" title="Reddit 补下载" />

      <!-- 管理区 -->
      <div class="panel-card animate-in stagger-3">
        <v-tabs v-model="tab" density="compact" color="primary">
          <v-tab value="missing">
            缺失文件
            <v-chip v-if="missingCount > 0" size="x-small" color="warning" class="ml-2">{{ missingCount }}</v-chip>
          </v-tab>
          <v-tab value="orphan">
            孤儿文件
            <v-chip v-if="orphans.length > 0" size="x-small" class="ml-2">{{ orphans.length }}</v-chip>
          </v-tab>
          <v-tab value="records">全部记录</v-tab>
        </v-tabs>

        <v-window v-model="tab">
          <!-- 缺失文件 -->
          <v-window-item value="missing">
            <div class="tab-actions">
              <v-btn size="small" variant="tonal" icon="mdi-refresh" :loading="loading" @click="reloadAll" />
              <v-spacer />
              <v-btn size="small" variant="tonal" :disabled="missingSelected.length === 0" @click="onDownloadSelectedMissing">
                补下载选中（{{ missingSelected.length }}）
              </v-btn>
              <v-btn size="small" variant="tonal" :disabled="missingCount === 0" @click="onRecoverAll">
                全部补下载
              </v-btn>
              <v-btn size="small" variant="tonal" color="error" :disabled="missingCount === 0" @click="onMarkDisliked">
                标记为不喜欢
              </v-btn>
            </div>
            <EmptyState
              v-if="missing.length === 0 && !loading"
              small
              icon="mdi-check-circle-outline"
              title="没有缺失文件"
              desc="数据库记录与磁盘文件一致"
            />
            <v-data-table
              v-else
              v-model="missingSelected"
              :items="missing"
              :loading="loading"
              show-select
              item-value="name"
              density="compact"
              class="db-table"
              :headers="[
                { title: '文件名', key: 'name' },
                { title: '来源', key: 'source', width: 100 },
                { title: '分辨率', key: 'resolution', width: 110 },
                { title: '入库时间', key: 'created_at', width: 150 },
              ]"
              :items-per-page="10"
            >
              <template #[`item.source`]="{ item }">
                <v-chip size="x-small" :color="item.source === 'wallhaven' ? 'primary' : undefined" variant="tonal">
                  {{ item.source }}
                </v-chip>
              </template>
              <template #[`item.created_at`]="{ item }">
                <span class="text-caption">{{ formatDateTime(item.created_at) }}</span>
              </template>
            </v-data-table>
          </v-window-item>

          <!-- 孤儿文件 -->
          <v-window-item value="orphan">
            <div class="tab-actions">
              <v-btn size="small" variant="tonal" icon="mdi-refresh" :loading="loading" @click="reloadAll" />
              <v-spacer />
              <v-btn size="small" variant="tonal" :disabled="orphanSelected.length === 0" @click="onAdopt">
                收养入库（{{ orphanSelected.length }}）
              </v-btn>
              <v-btn size="small" variant="tonal" color="error" :disabled="orphanSelected.length === 0" @click="onDeleteOrphans">
                删除（{{ orphanSelected.length }}）
              </v-btn>
            </div>
            <EmptyState
              v-if="orphans.length === 0 && !loading"
              small
              icon="mdi-folder-check-outline"
              title="没有孤儿文件"
              desc="保存目录中的文件都已在数据库中登记"
            />
            <v-data-table
              v-else
              v-model="orphanSelected"
              :items="orphans"
              :loading="loading"
              show-select
              item-value="path"
              density="compact"
              class="db-table"
              :headers="[
                { title: '文件名', key: 'name' },
                { title: '来源', key: 'source', width: 100 },
                { title: '大小', key: 'size', width: 100 },
              ]"
              :items-per-page="10"
            >
              <template #[`item.source`]="{ item }">
                <v-chip size="x-small" :color="item.source === 'wallhaven' ? 'primary' : undefined" variant="tonal">
                  {{ item.source }}
                </v-chip>
              </template>
              <template #[`item.size`]="{ item }">
                <span class="text-caption">{{ formatBytes(item.size) }}</span>
              </template>
            </v-data-table>
          </v-window-item>

          <!-- 全部记录 -->
          <v-window-item value="records">
            <div class="tab-actions">
              <v-btn-toggle :model-value="recordSource" mandatory density="compact" color="primary" @update:model-value="onRecordSourceChange">
                <v-btn value="wallhaven" size="small">Wallhaven</v-btn>
                <v-btn value="reddit" size="small">Reddit</v-btn>
              </v-btn-toggle>
              <v-spacer />
              <span class="text-caption">第 {{ recordPage }} / {{ recordTotalPages }} 页 · 共 {{ recordTotal }} 条</span>
              <v-btn size="small" variant="text" icon="mdi-chevron-left" :disabled="recordPage <= 1 || recordsLoading" @click="onRecordPage(-1)" />
              <v-btn size="small" variant="text" icon="mdi-chevron-right" :disabled="recordPage >= recordTotalPages || recordsLoading" @click="onRecordPage(1)" />
            </div>
            <v-data-table
              :items="records"
              :loading="recordsLoading"
              density="compact"
              class="db-table"
              :headers="[
                { title: '文件名', key: 'name' },
                { title: '状态', key: 'love', width: 80 },
                { title: '分辨率', key: 'resolution', width: 110 },
                { title: '入库时间', key: 'created_at', width: 150 },
              ]"
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
          </v-window-item>
        </v-window>
      </div>

      <!-- 维护 -->
      <div class="panel-card animate-in stagger-4">
        <div class="panel-card__title"><v-icon icon="mdi-wrench-outline" size="18" color="primary" />维护</div>
        <div class="maint-actions">
          <v-btn variant="tonal" prepend-icon="mdi-image-off-outline" @click="onCleanThumbnails">
            清理孤儿缩略图
          </v-btn>
          <v-btn variant="tonal" prepend-icon="mdi-restore" @click="onRestoreAll">
            恢复所有已标记
          </v-btn>
        </div>
      </div>

      <!-- 数据同步 -->
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
          <v-text-field
            v-model="ossBucket"
            label="Bucket"
            density="compact"
            class="settings-field"
          />
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
          <v-btn variant="tonal" prepend-icon="mdi-cloud-download-outline" :loading="cloudDownloading" @click="onCloudDownload">
            从云端拉取并合并
          </v-btn>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.dir-field {
  display: flex;
  gap: var(--space-3);
  align-items: flex-start;
}
.dir-field .settings-field {
  flex: 1;
}
.db-paths {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.db-path-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.db-path-row__path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.db-stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
  gap: var(--space-4);
}
.tab-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-3) 0;
}
.db-table {
  background: transparent !important;
}
.maint-actions {
  display: flex;
  gap: var(--space-3);
  flex-wrap: wrap;
}
.sync-desc {
  color: var(--text-secondary);
  margin-bottom: var(--space-3);
}
.sync-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: var(--space-2) var(--space-3);
}
.sync-oss-actions {
  display: flex;
  gap: var(--space-3);
  margin-top: var(--space-2);
}
</style>
