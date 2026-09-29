/**
 * 跨平台的路径比较。
 *
 * 后端给出的路径形态并不统一：图库条目来自 `safe_join`，在 Windows 上是 verbatim
 * 形式（`\\?\C:\...`）；而系统回报的「当前壁纸」是普通形式（`C:\...`）、还可能大小写
 * 不同。要判断「这张图就是当前壁纸」，必须先把两边归一化再比。
 */

const IS_WINDOWS = /Windows/i.test(navigator.userAgent);

/**
 * 归一化成可比较的 key：剥掉 verbatim 前缀、统一分隔符、去掉结尾多余斜杠，
 * Windows 下再忽略大小写（Linux/macOS 的路径是大小写敏感的，不能一并小写）。
 */
export function pathKey(path: string | null | undefined): string {
  if (!path) return "";
  let p = path.trim();
  // `\\?\C:\x` → `C:\x`；`\\?\UNC\srv\share` → `\\srv\share`
  if (p.startsWith("\\\\?\\UNC\\")) p = `\\\\${p.slice(8)}`;
  else if (p.startsWith("\\\\?\\")) p = p.slice(4);

  p = p.replace(/\\/g, "/");
  if (p.length > 1) p = p.replace(/\/+$/, "");

  return IS_WINDOWS ? p.toLowerCase() : p;
}

/** 取路径的最后一段（同时认 `/` 与 `\`，因为 Windows 路径两种都可能出现）。 */
export function basename(path: string | null | undefined): string {
  if (!path) return "";
  return path.split(/[\\/]/).pop() ?? "";
}
