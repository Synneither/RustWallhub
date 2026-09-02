/**
 * 全局应用状态（无 pinia，reactive 单例模块）。
 * App.vue 在挂载时调用 bootstrap() 与 registerGlobalListeners()。
 */
import { reactive, computed, shallowRef } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  DatabaseStatus,
  SlideshowTickPayload,
  StatsResponse,
  UpdateInfo,
} from "../types";
import {
  checkDatabases,
  getConfig,
  getStats,
  initDatabases,
  isSlideshowRunning,
  onDownloadComplete,
  onDownloadProgress,
  onImageDownloaded,
  onSettingsChanged,
  onSlideshowTick,
  onSyncCompleted,
  onSyncFailed,
  onUpdateAvailable,
  onUpdateInstalling,
  onUpdateProgress,
} from "../utils/api";
import { friendlyError } from "../utils/errors";
import { logger } from "../utils/logger";

/* ── 下载任务状态 ── */
export interface DownloadTaskState {
  active: boolean;
  done: number;
  total: number;
  message: string;
  /** 完成后的汇总（保留展示到下一次启动） */
  lastComplete: { success: number; total: number; message: string } | null;
}

/* ── Toast ── */
export interface ToastItem {
  id: number;
  text: string;
  color: "success" | "error" | "info";
}

/* ── 确认框（Promise 化） ── */
interface ConfirmState {
  visible: boolean;
  title: string;
  text: string;
  danger: boolean;
  confirmText: string;
  resolve: ((ok: boolean) => void) | null;
}

interface NewImageEntry {
  source: string;
  name: string;
  path: string;
}

export const appState = reactive({
  /* 启动 */
  booted: false,
  bootError: "" as string,

  /* 配置 / 数据库 / 统计 */
  config: null as AppConfig | null,
  dbStatus: null as DatabaseStatus | null,
  stats: null as StatsResponse | null,

  /* 下载任务：key = source */
  downloads: {} as Record<string, DownloadTaskState>,

  /* 本次会话新下载的图片（各源页面"新图预览条"消费）。
   * shallowRef：整批下载每张都触发一次 push，若用 reactive 数组会被逐元素深度代理；
   * 这里只在替换引用时触发更新，写入时用不可变替换。 */
  newImages: shallowRef<NewImageEntry[]>([]),

  /* 轮播 */
  slideshow: {
    running: false,
    current: null as SlideshowTickPayload | null,
  },

  /* 更新 */
  update: {
    info: null as UpdateInfo | null,
    downloading: false,
    downloaded: 0,
    total: null as number | null,
    installing: false,
  },

  /* UI 基础设施 */
  toasts: [] as ToastItem[],
  confirm: {
    visible: false,
    title: "",
    text: "",
    danger: false,
    confirmText: "确认",
    resolve: null,
  } as ConfirmState,

  /* 图库缓存失效计数：settings-changed 后 +1，图库据此重载 */
  galleryEpoch: 0,
});

/* ── 派生 ── */
export const dbReady = computed(() => {
  const s = appState.dbStatus;
  return !!s && s.wallhaven_exists && s.reddit_exists;
});

export const anyDownloadActive = computed(() =>
  Object.values(appState.downloads).some((d) => d.active),
);

let toastSeq = 0;
export function toast(text: string, color: ToastItem["color"] = "info") {
  const id = ++toastSeq;
  appState.toasts.push({ id, text, color });
  // 错误提示不自动消失，需手动关闭，避免长文案读不完
  if (color !== "error") {
    window.setTimeout(() => dismissToast(id), 3000);
  }
}

export function dismissToast(id: number) {
  const i = appState.toasts.findIndex((t) => t.id === id);
  if (i >= 0) appState.toasts.splice(i, 1);
}

export function toastError(e: unknown) {
  toast(friendlyError(e), "error");
}

/** Promise 化确认框。danger=true 时确认按钮为红色。 */
export function askConfirm(
  title: string,
  text: string,
  opts: { danger?: boolean; confirmText?: string } = {},
): Promise<boolean> {
  return new Promise((resolve) => {
    appState.confirm = {
      visible: true,
      title,
      text,
      danger: opts.danger ?? false,
      confirmText: opts.confirmText ?? "确认",
      resolve,
    };
  });
}

export function settleConfirm(ok: boolean) {
  appState.confirm.visible = false;
  appState.confirm.resolve?.(ok);
  appState.confirm.resolve = null;
}

function taskOf(source: string): DownloadTaskState {
  if (!appState.downloads[source]) {
    appState.downloads[source] = {
      active: false,
      done: 0,
      total: 0,
      message: "",
      lastComplete: null,
    };
  }
  return appState.downloads[source];
}

/* ── 启动序列 ── */
export async function bootstrap() {
  try {
    const [config, status] = await Promise.all([getConfig(), checkDatabases()]);
    appState.config = config;
    appState.dbStatus = status;

    if (status.wallhaven_exists && status.reddit_exists) {
      await refreshStats();
    }
    // 数据库缺失时由 App.vue 弹确认框引导 initDatabases

    appState.slideshow.running = await isSlideshowRunning().catch(() => false);
  } catch (e) {
    logger.error("Bootstrap", "启动初始化失败", e);
    appState.bootError = friendlyError(e);
  } finally {
    appState.booted = true;
  }
}

/** 创建缺失的数据库（用户确认后调用），返回实际创建的库名 */
export async function ensureDatabases(): Promise<string[]> {
  const created = await initDatabases();
  appState.dbStatus = await checkDatabases();
  if (created.length > 0) {
    await refreshStats();
  }
  return created;
}

export async function refreshStats() {
  try {
    appState.stats = await getStats();
  } catch (e) {
    logger.warn("Stats", "统计刷新失败", e);
  }
}

/* ── 全局事件 ── */
let listenersRegistered = false;

/** 多个下载任务可能几乎同时完成；统计刷新与图库重载合并到 400ms 后执行一次。 */
let postDownloadTimer: ReturnType<typeof setTimeout> | null = null;
function schedulePostDownloadRefresh() {
  if (postDownloadTimer) clearTimeout(postDownloadTimer);
  postDownloadTimer = setTimeout(() => {
    postDownloadTimer = null;
    refreshStats();
    appState.galleryEpoch++;
  }, 400);
}

/** 已注册的全局监听器卸载函数，供 unregisterGlobalListeners() 与 HMR 统一解绑。 */
const unlistenFns: UnlistenFn[] = [];

export async function registerGlobalListeners() {
  if (listenersRegistered) return;
  listenersRegistered = true;

  // 逐个 await 并保存卸载函数。此前返回值全部丢弃，dev 下 HMR 重新执行本模块时
  // 监听器会叠加（表现为 toast 重复弹出、galleryEpoch 一次事件自增多次）。
  unlistenFns.push(
    await onDownloadProgress((p) => {
      const t = taskOf(p.source);
      t.active = true;
      t.done = p.done;
      t.total = p.total;
      t.message = p.message;
    }),
  );

  unlistenFns.push(
    await onDownloadComplete((p) => {
      const t = taskOf(p.source);
      t.active = false;
      t.done = p.total;
      t.total = p.total;
      t.message = p.message;
      t.lastComplete = { success: p.success, total: p.total, message: p.message };
      toast(p.message || `下载完成：成功 ${p.success}/${p.total}`, "success");
      schedulePostDownloadRefresh();
    }),
  );

  unlistenFns.push(
    await onImageDownloaded((p) => {
      // newImages 是 shallowRef（reactive 单例里自动解包，读写直接当数组用）。
      // 用不可变替换而非 push，触发 shallowRef 的替换更新，且避免逐元素深代理。
      appState.newImages = [...appState.newImages, { source: p.source, name: p.name, path: p.path }];
      // 预览条最多展示 12 张，这里限制会话内累积数量，避免长任务让数组无限增长。
      const MAX_NEW_IMAGES = 120;
      if (appState.newImages.length > MAX_NEW_IMAGES) {
        appState.newImages = appState.newImages.slice(appState.newImages.length - MAX_NEW_IMAGES);
      }
    }),
  );

  unlistenFns.push(
    await onSettingsChanged(() => {
      appState.galleryEpoch++;
    }),
  );

  unlistenFns.push(
    await onUpdateAvailable((info) => {
      appState.update.info = info;
    }),
  );

  unlistenFns.push(
    await onUpdateProgress((p) => {
      appState.update.downloaded = p.downloaded;
      appState.update.total = p.total;
    }),
  );

  unlistenFns.push(
    await onUpdateInstalling(() => {
      appState.update.installing = true;
    }),
  );

  unlistenFns.push(
    await onSlideshowTick((p) => {
      appState.slideshow.running = true;
      appState.slideshow.current = p;
    }),
  );

  // 启动自动拉取的结果：成功后刷新统计与图库，失败只提示
  unlistenFns.push(
    await onSyncCompleted((msg) => {
      toast(`云端同步：${msg}`, "success");
      refreshStats();
      appState.galleryEpoch++;
    }),
  );

  unlistenFns.push(
    await onSyncFailed((msg) => {
      toast(`云端同步失败：${msg}`, "error");
    }),
  );
}

/** 解绑全部全局监听器并复位注册标志（HMR / 卸载时调用）。 */
export function unregisterGlobalListeners() {
  for (const unlisten of unlistenFns.splice(0)) unlisten();
  listenersRegistered = false;
}

// Vite 热更新会重新执行本模块，此时必须解绑旧监听器，否则与新的那套并存。
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    unregisterGlobalListeners();
  });
}

/** 清除某源的最后完成记录（页面关闭卡片时调用） */
export function dismissComplete(source: string) {
  const t = taskOf(source);
  t.lastComplete = null;
}

/** 清空某源的新图列表（页面卸载或新任务启动时调用） */
export function clearNewImages(source?: string) {
  if (!source) {
    appState.newImages = [];
    return;
  }
  appState.newImages = appState.newImages.filter((i) => i.source !== source);
}
