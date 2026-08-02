/** 展示格式化工具 */

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "-";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

/** "2026-08-02 12:00:00" / ISO 字符串 → "2026-08-02 12:00"（失败原样返回） */
export function formatDateTime(raw: string | null | undefined): string {
  if (!raw) return "-";
  const normalized = raw.includes("T") ? raw : raw.replace(" ", "T");
  const d = new Date(normalized.endsWith("Z") ? normalized : normalized + "Z");
  if (isNaN(d.getTime())) return raw.slice(0, 16);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 秒数 → 友好时长（用于轮播间隔展示等） */
export function formatSeconds(secs: number): string {
  if (secs < 60) return `${secs} 秒`;
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return s === 0 ? `${m} 分钟` : `${m} 分 ${s} 秒`;
}
