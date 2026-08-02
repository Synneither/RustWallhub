/**
 * 把后端错误转成用户可读的中文提示。
 */
export function friendlyError(e: unknown): string {
  const msg = String(e);
  if (msg.includes("unable to open database"))
    return "数据库不存在，请先在弹窗中确认创建，或在设置中检查数据库目录";
  return msg;
}
