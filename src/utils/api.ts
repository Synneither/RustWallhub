/**
 * 后端命令与事件的类型化封装。
 * 函数名与 Rust 侧命令名保持一致，便于对照。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import type {
  ActiveWallpaper,
  AppConfig,
  CleanThumbnailsResult,
  DatabaseStatus,
  DownloadCompletePayload,
  DownloadProgressPayload,
  ImageDownloadedPayload,
  ImageInfo,
  ImageRecord,
  LocalImageList,
  MonitorInfo,
  OrphanFile,
  SlideshowTickPayload,
  Source,
  StatsResponse,
  SyncExportResult,
  SyncImportResult,
  ThumbnailBatch,
  UpdateInfo,
  UpdateProgressPayload,
  WallhavenSearchResult,
  WallhavenSelected,
} from "../types";

/* ════════════ settings ════════════ */

export const getConfig = () => invoke<AppConfig>("get_config");

export const saveSettings = (config: AppConfig) =>
  invoke<void>("save_settings", { config });

export const getStats = () => invoke<StatsResponse>("get_stats");

export const checkDatabases = () => invoke<DatabaseStatus>("check_databases");

export const initDatabases = () => invoke<string[]>("init_databases");

export const checkUpdate = () => invoke<UpdateInfo>("check_update");

export const installUpdate = () => invoke<void>("install_update");

/* ════════════ wallhaven ════════════ */

export const searchWallhaven = (page = 1) =>
  invoke<WallhavenSearchResult>("search_wallhaven", { page });

export const startWallhavenDownload = () =>
  invoke<string>("start_wallhaven_download");

export const downloadWallhavenSelected = (images: WallhavenSelected[]) =>
  invoke<string>("download_wallhaven_selected", { images });

/* ════════════ reddit ════════════ */

export const startRedditDownload = () => invoke<string>("start_reddit_download");

/* ════════════ download ════════════ */

export const recoverDatabaseFiles = (source: Source) =>
  invoke<string>("recover_database_files", { source });

export const downloadMissingImages = (source: Source, images: ImageRecord[]) =>
  invoke<string>("download_missing_images", { source, images });

export const cancelDownloads = () => invoke<void>("cancel_downloads");

/* ════════════ gallery ════════════ */

export interface BrowseOptions {
  offset: number;
  limit: number;
  customDir?: string;
  search?: string;
  sortBy?: string;
}

export const browseImageFiles = (source: Source, opts: BrowseOptions) =>
  invoke<LocalImageList>("browse_image_files", {
    source,
    offset: opts.offset,
    limit: opts.limit,
    customDir: opts.customDir ?? null,
    search: opts.search ?? null,
    sortBy: opts.sortBy ?? null,
  });

export const listFilteredImagePaths = (
  source: Source,
  search?: string,
  sortBy?: string,
) =>
  invoke<string[]>("list_filtered_image_paths", {
    source,
    search: search ?? null,
    sortBy: sortBy ?? null,
  });

export const resolveThumbnails = (
  source: Source,
  filenames: string[],
  dpr = 1,
) => invoke<ThumbnailBatch>("resolve_thumbnails", { source, filenames, dpr });

export const dislikeFile = (source: Source, name: string) =>
  invoke<boolean>("dislike_file", { source, name });

export const dislikeFiles = (source: Source, names: string[]) =>
  invoke<number>("dislike_files", { source, names });

export const deleteOrphanFile = (source: Source, name: string) =>
  invoke<boolean>("delete_orphan_file", { source, name });

export const deleteOrphanFiles = (source: Source, names: string[]) =>
  invoke<number>("delete_orphan_files", { source, names });

export const adoptOrphanFiles = (source: Source, names: string[]) =>
  invoke<number>("adopt_orphan_files", { source, names });

export const cleanThumbnails = () =>
  invoke<CleanThumbnailsResult>("clean_thumbnails");

export const getImageInfo = (source: Source, name: string) =>
  invoke<ImageInfo>("get_image_info", { source, name });

/* ════════════ database ════════════ */

export const listDatabaseImages = (source: Source, limit: number, offset: number) =>
  invoke<ImageRecord[]>("list_database_images", { source, limit, offset });

export const listOrphanFiles = (source: Source) =>
  invoke<OrphanFile[]>("list_orphan_files", { source });

export const markDislikedFiles = (source: Source) =>
  invoke<number>("mark_disliked_files", { source });

export const restoreAllFiles = (source: Source) =>
  invoke<number>("restore_all_files", { source });

export const listMissingImages = (source: Source) =>
  invoke<ImageRecord[]>("list_missing_images", { source });

/* ════════════ sync ════════════ */

export const exportSnapshots = (dir: string) =>
  invoke<SyncExportResult>("export_snapshots", { dir });

export const importSnapshots = (
  wallhavenPath: string | null,
  redditPath: string | null,
) =>
  invoke<SyncImportResult>("import_snapshots", {
    wallhavenPath,
    redditPath,
  });

export const ossSyncUpload = () => invoke<string>("oss_sync_upload");

export const ossSyncDownload = () =>
  invoke<SyncImportResult>("oss_sync_download");

export const testOssConfig = () => invoke<string>("test_oss_config");

/* ════════════ system ════════════ */

export const getActiveWallpaper = () =>
  invoke<ActiveWallpaper>("get_active_wallpaper");

/* ════════════ wallpaper ════════════ */

export const setWallpaper = (filePath: string, monitor?: string) =>
  invoke<string>("set_wallpaper", {
    filePath,
    monitor: monitor ?? null,
  });

export const startSlideshow = (filePaths: string[], intervalSecs: number) =>
  invoke<void>("start_slideshow", { filePaths, intervalSecs });

export const stopSlideshow = () => invoke<boolean>("stop_slideshow");

export const isSlideshowRunning = () => invoke<boolean>("is_slideshow_running");

export const listMonitors = () => invoke<MonitorInfo[]>("list_monitors");

/* ════════════ asset URL ════════════ */

/** 本地文件路径 → WebView 可显示的 asset URL */
export function assetUrl(path: string): string {
  return convertFileSrc(path);
}

/* ════════════ 事件监听 ════════════ */

export const onDownloadProgress = (
  cb: (p: DownloadProgressPayload) => void,
): Promise<UnlistenFn> => listen("download-progress", (e) => cb(e.payload as DownloadProgressPayload));

export const onDownloadComplete = (
  cb: (p: DownloadCompletePayload) => void,
): Promise<UnlistenFn> => listen("download-complete", (e) => cb(e.payload as DownloadCompletePayload));

export const onImageDownloaded = (
  cb: (p: ImageDownloadedPayload) => void,
): Promise<UnlistenFn> => listen("image-downloaded", (e) => cb(e.payload as ImageDownloadedPayload));

export const onSettingsChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("settings-changed", () => cb());

export const onUpdateAvailable = (cb: (p: UpdateInfo) => void): Promise<UnlistenFn> =>
  listen("update-available", (e) => cb(e.payload as UpdateInfo));

export const onUpdateProgress = (
  cb: (p: UpdateProgressPayload) => void,
): Promise<UnlistenFn> => listen("update-progress", (e) => cb(e.payload as UpdateProgressPayload));

export const onUpdateInstalling = (cb: () => void): Promise<UnlistenFn> =>
  listen("update-installing", () => cb());

export const onSlideshowTick = (
  cb: (p: SlideshowTickPayload) => void,
): Promise<UnlistenFn> => listen("slideshow-tick", (e) => cb(e.payload as SlideshowTickPayload));

/* ════════════ 自动同步（启动拉取的结果） ════════════ */

/** 启动自动拉取成功，payload 是结果摘要文本 */
export const onSyncCompleted = (cb: (msg: string) => void): Promise<UnlistenFn> =>
  listen("sync-completed", (e) => cb(e.payload as string));

/** 启动自动拉取失败，payload 是错误文本 */
export const onSyncFailed = (cb: (msg: string) => void): Promise<UnlistenFn> =>
  listen("sync-failed", (e) => cb(e.payload as string));
