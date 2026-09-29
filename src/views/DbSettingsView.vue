<script setup lang="ts">
import { onActivated, onDeactivated, onMounted, ref, shallowRef, watch } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { AppConfig, ImageRecord, OrphanFile } from "../types";
import {
  adoptOrphanFiles,
  checkDatabases,
  cleanThumbnails,
  deleteMissingRecords,
  deleteOrphanFiles,
  downloadMissingImages,
  listMissingImages,
  listOrphanFiles,
  markDislikedFiles,
  refreshFileCaches,
  recoverDatabaseFiles,
  restoreAllFiles,
  saveSettings,
} from "../utils/api";
import { appState, askConfirm, dbReady, ensureDatabases, refreshStats, toast, toastError } from "../stores/app";
import { useAsyncAction } from "../composables/useAsyncAction";
import { requiredRule } from "../utils/rules";
import { formatBytes, formatDateTime } from "../utils/format";
import StatPanel from "../components/StatPanel.vue";
import EmptyState from "../components/EmptyState.vue";
import ProgressCard from "../components/ProgressCard.vue";
import DbSyncPanel from "../components/DbSyncPanel.vue";
import RecordBrowserPanel from "../components/RecordBrowserPanel.vue";

// Vuetify 组件按需局部导入（见 main.ts 的注册策略说明）：只在这个视图/组件里用到，
// 挂全局注册会让首屏无条件背上它们。
import { VChip } from "vuetify/components/VChip";
import { VDataTable } from "vuetify/components/VDataTable";
import { VTab, VTabs } from "vuetify/components/VTabs";
import { VTextField } from "vuetify/components/VTextField";
import { VWindow, VWindowItem } from "vuetify/components/VWindow";

/* ════ 库状态与 db_dir ════ */
const dbDir = ref("");
// savingDir / initializing 由下面各自的 useAsyncAction 提供

// KeepAlive 下该视图只在首次挂载时跑一次 onMounted，之后切走再切回不会重新加载。
// 用 viewActive 标志 + onActivated/onDeactivated + 监听 galleryEpoch，保证：
// 1) 每次切回本页都刷新；2) 本页可见时下载完成/补下载（galleryEpoch++）也会刷新。
let viewActive = false;
let hasActivated = false;

// 「上次重载时看到的 galleryEpoch」。缺失/孤儿列表都是全盘扫描（listMissingImages +
// listOrphanFiles），大图库下代价明显，所以只在数据真的变了时才重扫：
// 切回本页时若 epoch 没变，就沿用已有列表，不再打一次全盘 IO。
let lastLoadedEpoch = -1;
// reloadAll 的竞态守卫：onMounted 与 onActivated 可能挨得极近，
// 后发起的请求完成时可能覆盖先完成的结果，用序号保证只认最后一次。
let reloadSeq = 0;

onMounted(async () => {
  dbDir.value = appState.config?.db_dir ?? "";
  viewActive = true;
  await reloadAll();
});

onActivated(() => {
  viewActive = true;
  // 首次挂载时 onActivated 紧跟 onMounted 触发，跳过那一次避免重复加载；
  // 之后每次从其他页面切回来，仅在数据确实变化（epoch 不同）时才全量重扫。
  if (hasActivated && lastLoadedEpoch !== appState.galleryEpoch) reloadAll();
  hasActivated = true;
});
onDeactivated(() => {
  viewActive = false;
});
watch(
  () => appState.galleryEpoch,
  () => {
    if (viewActive) reloadAll();
  },
);

async function pickDbDir() {
  try {
    const selected = await openDialog({ directory: true, defaultPath: dbDir.value || undefined });
    if (typeof selected === "string") dbDir.value = selected;
  } catch (e) {
    toastError(e);
  }
}

/** 创建库文件。放在 onSaveDir 之前：后者在确认后会调用它。 */
const { run: onInitDatabases, loading: initializing } = useAsyncAction(async () => {
  const created = await ensureDatabases();
  toast(created.length > 0 ? `已创建数据库：${created.join("、")}` : "数据库已就绪", "success");
  await reloadAll();
});

const { run: onSaveDir, loading: savingDir } = useAsyncAction(async () => {
  if (!appState.config) return;
  const next: AppConfig = { ...appState.config, db_dir: dbDir.value };
  await saveSettings(next);
  appState.config = next;
  appState.dbStatus = await checkDatabases();
  toast("数据库目录已保存", "success");
  if (!dbReady.value) {
    const ok = await askConfirm("初始化数据库", "新目录下数据库文件不存在，是否现在创建？", {
      confirmText: "创建",
    });
    if (ok) await onInitDatabases();
  } else {
    await reloadAll();
  }
});

/* ════ 数据加载 ════ */
const loading = ref(false);

// 表头定义提到模块级常量：写成内联字面量的话每次渲染都会生成新数组，
// v-data-table 会当成 props 变化重新计算列。
const MISSING_HEADERS = [
  { title: "文件名", key: "name" },
  { title: "来源", key: "source", width: 100 },
  { title: "分辨率", key: "resolution", width: 110 },
  { title: "入库时间", key: "created_at", width: 150 },
];
const ORPHAN_HEADERS = [
  { title: "文件名", key: "name" },
  { title: "来源", key: "source", width: 100 },
  { title: "大小", key: "size", width: 100 },
];
const missingCount = ref(0);
/* 这两个列表大图库下可能上千条（含 hash/url/title），且只有整批替换、没有原地改动，
 * 所以用 shallowRef：避免逐字段深度代理。下面的 xxxSelected 会被 v-data-table 原地增删，
 * 必须保持 ref。 */
const missing = shallowRef<ImageRecord[]>([]);
const orphans = shallowRef<OrphanFile[]>([]);

async function reloadAll() {
  if (!dbReady.value) return;
  const seq = ++reloadSeq;
  loading.value = true;
  try {
    // 先让后端丢弃目录列表/统计缓存（它们有短 TTL，不清的话「刷新」可能返回
    // 几秒前的旧结果）。清完后下面这几条命令仍然共用**同一次**重新扫描，
    // 所以既拿到了新鲜数据，又不会像以前那样把目录扫 3 遍。
    await refreshFileCaches();
    // 缺失列表与缺失计数来自同一次扫描，直接用列表长度，省掉一次重复 IPC + 磁盘扫描。
    const [m, o] = await Promise.all([listMissingImages("all"), listOrphanFiles("all")]);
    if (seq !== reloadSeq) return; // 已有更新的重载在跑，丢弃这次结果
    missingCount.value = m.length;
    missing.value = m;
    orphans.value = o;
    await refreshStats();
    await recordBrowser.value?.load();
    lastLoadedEpoch = appState.galleryEpoch;
  } catch (e) {
    if (seq !== reloadSeq) return;
    toastError(e);
  } finally {
    if (seq === reloadSeq) loading.value = false;
  }
}

/* ════ 缺失文件操作 ════ */
const missingSelected = ref<ImageRecord[]>([]);

/** 补下载全部缺失（两个源各调一次；后端 recover 对 "all" 只处理 Reddit）。 */
const { run: runRecoverAll, loading: recoveringAll } = useAsyncAction(async () => {
  await recoverDatabaseFiles("wallhaven");
  await recoverDatabaseFiles("reddit");
  toast("补下载任务已启动（Wallhaven + Reddit）", "info");
});

/** 批量标记缺失记录为不喜欢（love=0），不删文件。 */
const { run: runMarkDisliked, loading: markingDisliked } = useAsyncAction(async () => {
  const n = await markDislikedFiles("all");
  toast(`已标记 ${n} 条记录`, "success");
  await reloadAll();
});

/** 批量补下载选中的缺失文件，按来源分组串行调用。 */
const { run: onDownloadSelectedMissing, loading: downloadingMissing } = useAsyncAction(
  async () => {
    if (missingSelected.value.length === 0) return;
    const bySource = groupBySource(missingSelected.value);
    for (const [source, records] of Object.entries(bySource)) {
      const msg = await downloadMissingImages(source as "wallhaven" | "reddit", records);
      toast(msg, "info");
    }
    missingSelected.value = [];
  },
);

/** 永久删除选中的缺失记录（连同残留缩略图）。与「标记为不喜欢」不同，这一步不可撤销。 */
const { run: runDeleteMissing, loading: deletingMissing } = useAsyncAction(async () => {
  const bySource = groupBySource(missingSelected.value);
  let removed = 0;
  for (const [source, records] of Object.entries(bySource)) {
    removed += await deleteMissingRecords(
      source as "wallhaven" | "reddit",
      records.map((r) => r.name),
    );
  }
  toast(`已删除 ${removed} 条记录`, "success");
  missingSelected.value = [];
  await reloadAll();
});

/** 收养选中的孤儿文件入库。 */
const { run: runAdopt, loading: adopting } = useAsyncAction(async () => {
  const bySource = groupBySource(orphanSelected.value);
  let total = 0;
  for (const [source, files] of Object.entries(bySource)) {
    total += await adoptOrphanFiles(
      source as "wallhaven" | "reddit",
      files.map((f) => f.name),
    );
  }
  toast(`已收养 ${total} 个文件入库`, "success");
  orphanSelected.value = [];
  await reloadAll();
});

/** 永久删除选中的孤儿文件及其缩略图。 */
const { run: runDeleteOrphans, loading: deletingOrphans } = useAsyncAction(async () => {
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
});

async function onRecoverAll() {
  const ok = await askConfirm(
    "全部补下载",
    `将把两个库中所有标记缺失的图片（共 ${missingCount.value} 张）按记录的原 URL 重新下载。\n该任务在后台执行，可随时取消。是否继续？`,
    { confirmText: "开始补下载" },
  );
  if (!ok) return;
  await runRecoverAll();
}

async function onMarkDisliked() {
  const ok = await askConfirm(
    "标记为不喜欢",
    `将把 ${missingCount.value} 条缺失记录标记为不喜欢（love=0），不再计入缺失。\n此操作不删除任何文件，可通过「恢复所有已标记」撤销。`,
    { danger: true, confirmText: "标记" },
  );
  if (!ok) return;
  await runMarkDisliked();
}

async function onDeleteMissingRecords() {
  if (missingSelected.value.length === 0) return;
  const ok = await askConfirm(
    "删除记录",
    `将从数据库中永久删除选中的 ${missingSelected.value.length} 条记录，并清理它们的缩略图。\n文件本身已不在磁盘上，删除后无法补下载，也无法撤销。`,
    { danger: true, confirmText: "删除" },
  );
  if (!ok) return;
  await runDeleteMissing();
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

const onAdopt = async () => {
  if (orphanSelected.value.length === 0) return;
  await runAdopt();
};

async function onDeleteOrphans() {
  if (orphanSelected.value.length === 0) return;
  const ok = await askConfirm(
    "删除孤儿文件",
    `将永久删除 ${orphanSelected.value.length} 个文件及其缩略图。\n这些文件不在数据库中，删除后无法通过补下载恢复。`,
    { danger: true, confirmText: "删除" },
  );
  if (!ok) return;
  await runDeleteOrphans();
}

/* ════ 维护 ════ */
const { run: onCleanThumbnails, loading: cleaningThumbs } = useAsyncAction(async () => {
  const r = await cleanThumbnails();
  toast(`已清理孤儿缩略图：Wallhaven ${r.wallhaven} 个，Reddit ${r.reddit} 个`, "success");
});

/** 恢复所有 love=0 的记录（不碰文件）。
 *  二次确认属用户交互，先确认再进入异步动作——否则按钮会在确认框还开着的时候就转圈。 */
const { run: runRestoreAll, loading: restoringAll } = useAsyncAction(async () => {
  const n = await restoreAllFiles("all");
  toast(`已恢复 ${n} 条记录`, "success");
  await reloadAll();
});

async function onRestoreAll() {
  const ok = await askConfirm(
    "恢复所有已标记",
    "将把两个库中所有 love=0 的记录恢复为正常（love=1）。是否继续？",
    { confirmText: "恢复" },
  );
  if (!ok) return;
  await runRestoreAll();
}

/* ════ 记录浏览 ════ */
/** 「全部记录」标签页：分页与取数都在组件内部，写库后调 load() 重新取当前页。 */
const recordBrowser = ref<{ load: () => Promise<void> } | null>(null);

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
              <v-btn size="small" variant="tonal" :disabled="missingSelected.length === 0" @click="onDownloadSelectedMissing" :loading="downloadingMissing">
                补下载选中（{{ missingSelected.length }}）
              </v-btn>
              <v-btn size="small" variant="tonal" :disabled="missingCount === 0" @click="onRecoverAll" :loading="recoveringAll">
                全部补下载
              </v-btn>
              <v-btn
                size="small"
                variant="tonal"
                color="error"
                title="只删除选中的记录，不可撤销"
                :disabled="missingSelected.length === 0"
                @click="onDeleteMissingRecords"
                :loading="deletingMissing"
              >
                删除记录（{{ missingSelected.length }}）
              </v-btn>
              <v-btn size="small" variant="tonal" color="error" :disabled="missingCount === 0" @click="onMarkDisliked" :loading="markingDisliked">
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
              return-object
              density="compact"
              class="db-table"
              :headers="MISSING_HEADERS"
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
              <v-btn size="small" variant="tonal" :disabled="orphanSelected.length === 0" @click="onAdopt" :loading="adopting">
                收养入库（{{ orphanSelected.length }}）
              </v-btn>
              <v-btn size="small" variant="tonal" color="error" :disabled="orphanSelected.length === 0" @click="onDeleteOrphans" :loading="deletingOrphans">
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
              return-object
              density="compact"
              class="db-table"
              :headers="ORPHAN_HEADERS"
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
            <!-- 分页与取数都在组件内部；写库后由 reloadAll 调它的 load() -->
            <RecordBrowserPanel ref="recordBrowser" />
          </v-window-item>
        </v-window>
      </div>

      <!-- 维护 -->
      <div class="panel-card animate-in stagger-4">
        <div class="panel-card__title"><v-icon icon="mdi-wrench-outline" size="18" color="primary" />维护</div>
        <div class="maint-actions">
          <v-btn variant="tonal" prepend-icon="mdi-image-off-outline" @click="onCleanThumbnails" :loading="cleaningThumbs">
            清理孤儿缩略图
          </v-btn>
          <v-btn variant="tonal" prepend-icon="mdi-restore" :loading="restoringAll" @click="onRestoreAll">
            恢复所有已标记
          </v-btn>
        </div>
      </div>

      <!-- 数据同步（OSS 配置、快照导出/导入、云端上传拉取）都在组件内部；
           导入或拉取成功后它发 imported，由这里统一重载缺失/孤儿/统计/记录 -->
      <DbSyncPanel @imported="reloadAll" />
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
</style>
