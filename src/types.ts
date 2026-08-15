/**
 * 与后端 serde 结构一一对应的类型定义。
 * 字段名保持 snake_case，与 Rust 侧序列化结果一致。
 */

/** 图片来源枚举（serde snake_case） */
export type Source = "wallhaven" | "reddit" | "all";

/* ── 配置 ── */

export interface AppConfig {
  // Wallhaven
  wallhaven_save_dir: string;
  wallhaven_db_path: string;
  wallhaven_api_key: string;
  wallhaven_categories: string; // "010" 三位开关: general/anime/people
  wallhaven_purity: string; // "111" 三位开关: sfw/sketchy/nsfw
  wallhaven_sorting: string; // date_added | relevance | random | views | favorites | toplist
  wallhaven_top_range: string; // 1d | 3d | 1w | 1M | 3M | 6M | 1y
  wallhaven_atleast: string; // 如 "1920x1080"
  wallhaven_ratios: string; // landscape | portrait | square | 16x9 ...
  wallhaven_q: string;
  wallhaven_order: string; // desc | asc（toplist 时后端不下发）
  wallhaven_max_images: number;
  // Reddit
  reddit_save_dir: string;
  reddit_db_path: string;
  reddit_url: string;
  reddit_max_posts: number;
  reddit_max_images: number;
  // 通用
  thumbnails_dir: string;
  db_dir: string;
  download_concurrency: number; // 1-100
  thumbnail_dpr: number; // 1-3
  request_timeout: number; // 5-120 秒
  auto_update: boolean;
  proxy_url: string;
}

/* ── settings 模块 ── */

export interface DbStats {
  total: number;
  love: number;
  /** 注意：后端语义为 "love=1 且文件缺失" 的数量，UI 文案用"缺失" */
  dislike: number;
}

export interface StatsResponse {
  wallhaven: DbStats;
  reddit: DbStats;
}

export interface DatabaseStatus {
  wallhaven_exists: boolean;
  reddit_exists: boolean;
  wallhaven_path: string;
  reddit_path: string;
}

export interface UpdateInfo {
  has_update: boolean;
  version: string;
  current_version: string;
  body: string | null;
  date: string | null;
}

/* ── wallhaven 模块 ── */

export interface WallhavenImageEntry {
  id: string;
  thumbnail_url: string;
  path: string;
  resolution: string;
  short_url: string;
  file_size: number;
  file_type: string;
}

export interface WallhavenSearchResult {
  images: WallhavenImageEntry[];
  page: number;
  total_pages: number;
  total: number;
}

export interface WallhavenSelected {
  id: string;
  path: string;
  resolution: string;
  short_url: string;
}

/* ── gallery 模块 ── */

export interface LocalImageEntry {
  name: string;
  path: string;
  thumb_path: string | null; // 后端恒为 null，需 resolve_thumbnails
  size: number;
  is_orphan: boolean;
  modified_date: string | null; // 近似换算，仅展示用
}

export interface LocalImageList {
  images: LocalImageEntry[];
  total: number;
}

export interface ThumbnailItem {
  name: string;
  thumb_path: string;
}

export interface ThumbnailBatch {
  items: ThumbnailItem[];
}

export interface CleanThumbnailsResult {
  wallhaven: number;
  reddit: number;
}

export interface ImageInfo {
  name: string;
  path: string;
  size: number;
  resolution: string | null;
  format: string | null; // "Jpeg" | "Png" | ...
  width: number | null;
  height: number | null;
  source_url: string | null;
  download_url: string | null;
  title: string | null;
  permalink: string | null;
  source: string | null;
  created_at: string | null;
}

/* ── database 模块 ── */

export interface ImageRecord {
  id: number;
  name: string;
  hash: string;
  url: string;
  source_url: string;
  resolution: string;
  title: string | null;
  permalink: string | null;
  love: number; // 1=正常 0=不喜欢/已标记缺失
  created_at: string;
  source: string; // "wallhaven" | "reddit"
}

export interface OrphanFile {
  name: string;
  path: string;
  size: number;
  source: string;
}

/* ── system / wallpaper ── */

export interface ActiveWallpaper {
  path: string | null;
}

export interface MonitorInfo {
  id: string;
  name: string;
  is_primary: boolean;
  width: number;
  height: number;
}

/* ── 事件 payload ── */

export interface DownloadProgressPayload {
  source: string;
  done: number;
  total: number;
  message: string;
}

export interface DownloadCompletePayload {
  source: string;
  success: number;
  total: number;
  message: string;
}

export interface ImageDownloadedPayload {
  source: string;
  name: string;
  path: string;
}

export interface UpdateProgressPayload {
  downloaded: number;
  total: number | null;
}

export interface SlideshowTickPayload {
  index: number;
  total: number;
  name: string;
  path: string;
}
