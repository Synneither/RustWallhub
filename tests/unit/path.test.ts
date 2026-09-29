import { afterEach, describe, expect, it, vi } from "vitest";

/**
 * `pathKey` 是「图库里高亮当前壁纸」的**唯一**正确性来源：图库条目的路径来自 Rust 的
 * `safe_join`（Windows 上是 `\\?\C:\...` 的 verbatim 形式），而系统回报的当前壁纸是
 * 普通形式、大小写还不一定一致。归一化错了不会报错 —— 只会表现为"高亮不生效"，
 * 所以在这里逐条钉死。
 *
 * `IS_WINDOWS` 是模块级常量（读 `navigator.userAgent`），所以必须先 stub 全局再重新
 * 导入模块，否则永远只能测到非 Windows 分支。
 */
async function loadPathModule(userAgent: string) {
  vi.resetModules();
  vi.stubGlobal("navigator", { userAgent });
  return await import("../../src/utils/path");
}

const WINDOWS_UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
const LINUX_UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("pathKey / Windows", () => {
  it("剥掉 verbatim 前缀（safe_join 交出来的就是这个形式）", async () => {
    const { pathKey } = await loadPathModule(WINDOWS_UA);
    expect(pathKey("\\\\?\\C:\\Users\\a\\壁纸 图.png")).toBe("c:/users/a/壁纸 图.png");
  });

  it("verbatim 的 UNC 形式还原成 \\\\server\\share", async () => {
    const { pathKey } = await loadPathModule(WINDOWS_UA);
    expect(pathKey("\\\\?\\UNC\\srv\\share\\a.png")).toBe("//srv/share/a.png");
  });

  it("忽略大小写与分隔符差异 —— 两侧形态不同也必须判为同一张", async () => {
    const { pathKey } = await loadPathModule(WINDOWS_UA);
    const fromDb = pathKey("\\\\?\\C:\\Users\\25149\\Pictures\\Wallhaven_W5X2YQ.png");
    const fromSystem = pathKey("C:\\Users\\25149\\Pictures\\wallhaven_w5x2yq.PNG");
    expect(fromDb).toBe(fromSystem);
  });

  it("去掉结尾多余斜杠", async () => {
    const { pathKey } = await loadPathModule(WINDOWS_UA);
    expect(pathKey("C:\\Users\\a\\")).toBe("c:/users/a");
  });

  it("空值一律给空串", async () => {
    const { pathKey } = await loadPathModule(WINDOWS_UA);
    expect(pathKey(null)).toBe("");
    expect(pathKey(undefined)).toBe("");
    expect(pathKey("   ")).toBe("");
  });
});

describe("pathKey / Linux（大小写敏感，不能一并小写）", () => {
  it("保留大小写", async () => {
    const { pathKey } = await loadPathModule(LINUX_UA);
    expect(pathKey("/home/a/Pictures/A.PNG")).toBe("/home/a/Pictures/A.PNG");
  });

  it("大小写不同就是不同的文件", async () => {
    const { pathKey } = await loadPathModule(LINUX_UA);
    expect(pathKey("/home/a/A.png")).not.toBe(pathKey("/home/a/a.png"));
  });

  it("不剥 verbatim 前缀（Linux 上没有这种路径），但分隔符归一化仍生效", async () => {
    const { pathKey } = await loadPathModule(LINUX_UA);
    expect(pathKey("/home/a/b/")).toBe("/home/a/b");
  });
});

describe("basename", () => {
  it("同时认 / 与 \\（Windows 路径两种都可能出现）", async () => {
    const { basename } = await loadPathModule(WINDOWS_UA);
    expect(basename("C:\\Users\\a\\b.png")).toBe("b.png");
    expect(basename("/home/a/b.png")).toBe("b.png");
  });

  it("空值给空串", async () => {
    const { basename } = await loadPathModule(WINDOWS_UA);
    expect(basename(null)).toBe("");
    expect(basename(undefined)).toBe("");
  });
});
