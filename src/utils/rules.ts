/**
 * 共享表单校验规则（各视图统一使用，保证同字段同语义）。
 */

export const requiredRule = (v: string) => !!v || "此项不能为空";

export interface IntRuleOptions {
  /** 最小值，默认 1 */
  min?: number;
  /** 最大值，不传不限制 */
  max?: number;
  /** 允许 0（0 表示无限制），此时最小值为 0 */
  allowZero?: boolean;
}

export function positiveInt(v: number, opts: IntRuleOptions = {}): true | string {
  // null/undefined 不再放行：字段被清空时应提示，而不是把空值当合法。
  if (v === undefined || v === null) return "请输入有效数字";
  if (typeof v !== "number" || isNaN(v)) return "请输入有效数字";
  const min = opts.allowZero ? 0 : opts.min ?? 1;
  if (v < min) return opts.allowZero ? "不能为负数" : `不能小于 ${min}`;
  if (opts.max !== undefined && v > opts.max) return `不能超过 ${opts.max}`;
  return true;
}
