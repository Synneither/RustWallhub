import { describe, expect, it } from "vitest";
import { positiveInt, requiredRule } from "../../src/utils/rules";

describe("requiredRule", () => {
  it("空串给提示", () => {
    expect(requiredRule("")).toBe("此项不能为空");
  });
  it("有值放行", () => {
    expect(requiredRule("D:\\Wallpapers")).toBe(true);
  });
});

describe("positiveInt", () => {
  it("默认最小值 1", () => {
    expect(positiveInt(1)).toBe(true);
    expect(positiveInt(0)).toBe("不能小于 1");
  });

  it("清空字段要报错，而不是把空值当合法", () => {
    expect(positiveInt(undefined as unknown as number)).toBe("请输入有效数字");
    expect(positiveInt(null as unknown as number)).toBe("请输入有效数字");
    expect(positiveInt(Number.NaN)).toBe("请输入有效数字");
  });

  it("allowZero 时 0 合法、负数另给文案", () => {
    expect(positiveInt(0, { allowZero: true })).toBe(true);
    expect(positiveInt(-1, { allowZero: true })).toBe("不能为负数");
  });

  it("max 只在传了的时候生效", () => {
    expect(positiveInt(10000)).toBe(true);
    expect(positiveInt(10001, { max: 10000 })).toBe("不能超过 10000");
  });
});
