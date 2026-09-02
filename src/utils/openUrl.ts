import { openUrl } from "@tauri-apps/plugin-opener";
import { toastError } from "../stores/app";

/** 允许打开外链的协议白名单：只放 http/https，拒绝 file://、smb:// 等危险协议。 */
const SAFE_PROTOCOLS = ["http:", "https:"];

export function isSafeUrl(url: string): boolean {
  try {
    return SAFE_PROTOCOLS.includes(new URL(url).protocol);
  } catch {
    return false;
  }
}

/**
 * 安全打开外链：校验协议白名单后再调用系统 openUrl。
 * 数据库里存的 source_url/permalink/download_url 来自不可信网络，不能直接打开。
 */
export async function openUrlSafe(url: string | null): Promise<void> {
  if (!url) return;
  if (!isSafeUrl(url)) {
    toastError(new Error(`拒绝打开不安全的链接：${url.slice(0, 80)}`));
    return;
  }
  try {
    await openUrl(url);
  } catch (e) {
    toastError(e);
  }
}
