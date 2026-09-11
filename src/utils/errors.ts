/**
 * 把后端错误转成用户可读的中文提示。
 */
export function friendlyError(e: unknown): string {
  // Tauri invoke 失败时 reject 出来的常是纯字符串，但也可能是 Error 或空值，
  // 这里统一抽出可读文本，避免把 "[object Object]" 之类直接甩给用户。
  let msg: string;
  if (typeof e === "string") {
    msg = e;
  } else if (e instanceof Error) {
    msg = e.message;
  } else if (e && typeof e === "object" && "message" in e) {
    msg = String((e as { message: unknown }).message);
  } else {
    msg = e === null || e === undefined ? "" : String(e);
  }

  if (msg.includes("unable to open database"))
    return "数据库不存在，请先在弹窗中确认创建，或在设置中检查数据库目录";
  if (msg.includes("No such file or directory") || msg.includes("os error 2"))
    return "文件或目录不存在，请检查路径设置";
  if (msg.includes("Access is denied") || msg.includes("os error 5"))
    return "没有访问权限，请检查文件是否被其他程序占用";
  if (msg.toLowerCase().includes("timeout"))
    return "请求超时，请检查网络或代理设置";

  // 有内容就带上通用前缀，方便用户区分「这是应用报的错」和「这是系统原文」。
  return msg ? `操作失败：${msg}` : "操作失败，请重试";
}
