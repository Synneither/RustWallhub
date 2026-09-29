import { describe, expect, it } from "vitest";
import { formatBytes, formatDateTime } from "../../src/utils/format";
import { densityItems } from "../../src/composables/useGridDensity";

describe("formatBytes", () => {
  it("按 1024 进制进位", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1024 * 1024)).toBe("1.0 MB");
    expect(formatBytes(1024 ** 3)).toBe("1.0 GB");
  });

  it("大于 100 时不再保留小数", () => {
    expect(formatBytes(100 * 1024)).toBe("100 KB");
    expect(formatBytes(999 * 1024)).toBe("999 KB");
  });

  it("非法值与负数给占位符", () => {
    expect(formatBytes(-1)).toBe("-");
    expect(formatBytes(Number.NaN)).toBe("-");
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe("-");
  });
});

describe("formatDateTime", () => {
  it("空值给占位符", () => {
    expect(formatDateTime(null)).toBe("-");
    expect(formatDateTime(undefined)).toBe("-");
    expect(formatDateTime("")).toBe("-");
  });

  it("解析不了时退回原始串的前 16 个字符（比显示 Invalid Date 强）", () => {
    expect(formatDateTime("not-a-date-whatsoever")).toBe("not-a-date-whats");
  });

  it("输出形状固定 —— 与运行机器的时区无关", () => {
    // 后端存的是 UTC 时间串，用本地时区渲染；断言形状而不是具体钟点，
    // 否则测试会在 CI（UTC）和开发机（+08:00）之间来回翻。
    expect(formatDateTime("2026-08-02 12:00:00")).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
  });

  it("已带时区的串不能再拼 Z（拼了会变成 +08:00Z 这种非法串）", () => {
    // +08:00 形式：若被拼成 "...+08:00Z" 会解析失败并退回 raw.slice(0,16)，
    // 于是输出里会带上 "T"；正常解析出来的形状里不带 T。
    const out = formatDateTime("2026-08-02T12:00:00+08:00");
    expect(out).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
    expect(out).not.toContain("T");

    // Z 结尾同理
    const z = formatDateTime("2026-08-02T12:00:00Z");
    expect(z).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
  });

  it("带 T 的 ISO 串与等价的空格串得到同一结果", () => {
    expect(formatDateTime("2026-08-02T12:00:00")).toBe(formatDateTime("2026-08-02 12:00:00"));
  });
});

describe("densityItems", () => {
  it("标签与顺序固定，数值由调用方给 —— 两个视图不会再各写一份档位表", () => {
    const items = densityItems({ compact: "120px", normal: "170px", large: "240px" });
    expect(items.map((i) => i.value)).toEqual(["compact", "normal", "large"]);
    expect(items.map((i) => i.label)).toEqual(["紧凑", "标准", "大图"]);
    expect(items.map((i) => i.min)).toEqual(["120px", "170px", "240px"]);
  });

  it("按档位取值，不会因为写反键名而错位", () => {
    const items = densityItems({ compact: "a", normal: "b", large: "c" });
    expect(items.find((i) => i.value === "large")?.min).toBe("c");
  });
});
