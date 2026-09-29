#!/usr/bin/env node
/*
 * 渲染自检：把 `vite build` 的产物真正加载起来，用打桩的 Tauri IPC 逐个视图走一遍，
 * 断言每个视图「确实渲染出来了」。
 *
 * 为什么需要它：Vue 的**渲染期**异常只走 `console.error`，**不触发 `pageerror`**。
 * 视图白屏时 DOM 是空的 —— 未知标签 0 个、被裁内容 0 个，任何基于 DOM 的断言都会
 * 得到「全绿」的假象（真实案例：打桩配置漏字段 → 组件里 `draft.x[0]` 抛 TypeError →
 * Wallhaven 页整页白，而当时的布局探针报告「被裁 0」）。所以这里除了"没有报错"，
 * 还必须正向断言"看到了该看到的东西"。
 *
 * 用法：
 *   node tests/render-check.mjs                    # 默认检查 ./dist
 *   node tests/render-check.mjs --root dist-verify # 指定产物目录
 *   node tests/render-check.mjs --headed           # 打开有头浏览器看现场
 *
 * 退出码：0 = 全部通过；1 = 有断言失败（失败清单打在最后）。
 */
import http from "node:http";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "..");

function arg(name, fallback) {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1]
    : fallback;
}
const ROOT = path.resolve(arg("root", path.join(REPO, "dist")));
const STUB = path.resolve(arg("stub", path.join(HERE, "ipc-stub.js")));
const PORT = Number(arg("port", "41761"));
const HEADED = process.argv.includes("--headed");

/** 每个视图：导航项文案 = 该视图 view-header__title 的文案，用它正向断言"切过去了" */
const VIEWS = ["仪表盘", "Wallhaven", "Reddit", "图库", "数据库", "设置"];

/** 1x1 PNG：供 convertFileSrc 打桩返回，图片能真加载，元素才有高度 */
const PNG_1X1 = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==",
  "base64",
);

const MIME = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".webp": "image/webp",
};

function serve() {
  const server = http.createServer((req, res) => {
    let p = decodeURIComponent(req.url.split("?")[0]);
    if (p === "/") p = "/index.html";
    if (p === "/img.png") {
      res.setHeader("Content-Type", "image/png");
      res.end(PNG_1X1);
      return;
    }
    const file = path.join(ROOT, p);
    if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
      res.statusCode = 404;
      res.end("not found");
      return;
    }
    res.setHeader("Content-Type", MIME[path.extname(file)] || "application/octet-stream");
    fs.createReadStream(file).pipe(res);
  });
  return new Promise((resolve) => server.listen(PORT, "127.0.0.1", () => resolve(server)));
}

/**
 * 找一份可用的 chromium。
 *
 * 优先让 Playwright 自己解析（版本配套时最省事）。解析不到时退回扫描
 * `ms-playwright` 目录 —— 本机常常只装了某个 revision（如 chromium-1234），
 * 而 node 侧 playwright 的版本号与它未必对得上。
 */
function findInstalledChromium() {
  const base =
    process.env.PLAYWRIGHT_BROWSERS_PATH ||
    (process.platform === "win32"
      ? path.join(process.env.LOCALAPPDATA || os.homedir(), "ms-playwright")
      : path.join(os.homedir(), ".cache", "ms-playwright"));
  if (!fs.existsSync(base)) return null;
  const candidates = [];
  for (const entry of fs.readdirSync(base)) {
    if (!entry.startsWith("chromium")) continue;
    for (const sub of ["chrome-win64", "chrome-win", "chrome-linux", "chrome-mac"]) {
      for (const exe of ["chrome.exe", "headless_shell.exe", "chrome", "headless_shell", "Chromium"]) {
        const p = path.join(base, entry, sub, exe);
        if (fs.existsSync(p)) candidates.push(p);
      }
    }
  }
  // 带界面的 chrome 优先于 headless_shell
  return candidates.find((p) => !p.includes("headless_shell")) || candidates[0] || null;
}

async function launchBrowser(chromium) {
  try {
    return await chromium.launch({ headless: !HEADED });
  } catch (first) {
    const exe = findInstalledChromium();
    if (!exe) {
      throw new Error(
        `Playwright 解析不到浏览器，ms-playwright 目录里也没有可用的 chromium。\n` +
          `先执行：npx playwright install chromium\n（原始错误：${first.message}）`,
      );
    }
    console.log(`  Playwright 自带的浏览器不可用，改用：${exe}`);
    return await chromium.launch({ headless: !HEADED, executablePath: exe });
  }
}

async function main() {
  if (!fs.existsSync(path.join(ROOT, "index.html"))) {
    throw new Error(`找不到构建产物：${ROOT}/index.html —— 先跑 vite build（或 deno task build）`);
  }
  if (!fs.existsSync(STUB)) throw new Error(`找不到打桩脚本：${STUB}`);

  const { chromium } = require(path.join(REPO, "node_modules", "playwright"));
  const server = await serve();
  const browser = await launchBrowser(chromium);
  const failures = [];
  const stubSource = fs.readFileSync(STUB, "utf8");

  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const consoleErrors = [];
  page.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(m.text());
  });
  page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e.message}`));

  await page.addInitScript(stubSource);
  await page.goto(`http://127.0.0.1:${PORT}/`, { waitUntil: "load" });
  await page.waitForTimeout(600);

  for (const label of VIEWS) {
    consoleErrors.length = 0;
    const item = page
      .locator(".v-list-item-title", { hasText: new RegExp(`^${label}$`) })
      .first();
    if (!(await item.count())) {
      failures.push(`[${label}] 导航里找不到这一项`);
      continue;
    }
    await item.click();
    await page.waitForTimeout(900);
    // 造出事件驱动的 UI（进度卡、新图条），让它们也进入检查范围
    await page.evaluate(() => window.__pump && window.__pump());
    await page.waitForTimeout(300);

    const r = await page.evaluate(() => {
      const title = document.querySelector(".view-header__title");
      const view = document.querySelector(".app-main .view");
      // Vue 解析不到组件时会留下原始标签（tagName 形如 V-TEXT-FIELD）。
      // 这是「组件没注册 / 没导入」最灵敏的探测器。
      const unresolved = [...document.querySelectorAll("*")]
        .filter((el) => el.tagName.startsWith("V-"))
        .map((el) => el.tagName);
      return {
        title: title ? title.textContent.trim() : null,
        viewClass: view ? view.className : null,
        htmlLength: view ? view.innerHTML.length : 0,
        buttons: document.querySelectorAll(".app-main button").length,
        unresolved: [...new Set(unresolved)],
        outlinedFields: document.querySelectorAll(".app-main .v-field--variant-outlined").length,
        chips: document.querySelectorAll(".app-main .v-chip").length,
      };
    });

    const problems = [];
    if (r.title !== label) problems.push(`标题是 "${r.title}"，期望 "${label}"（视图没切过去或没渲染）`);
    if (r.unresolved.length) problems.push(`有未解析的组件标签：${r.unresolved.join(", ")}`);
    if (r.htmlLength < 300) problems.push(`视图内容只有 ${r.htmlLength} 字符，疑似白屏`);
    if (r.buttons === 0) problems.push("视图里一个按钮都没有，疑似白屏");
    const realErrors = consoleErrors.filter((e) =>
      /TypeError|Cannot read|is not a function|is not defined|Failed to resolve component/i.test(e),
    );
    if (realErrors.length) problems.push(`控制台报错：${realErrors[0].slice(0, 200)}`);

    if (problems.length) {
      failures.push(...problems.map((p) => `[${label}] ${p}`));
      console.log(`  ✗ ${label}：${problems.join("；")}`);
    } else {
      console.log(
        `  ✓ ${label}  title=${r.title}  内容 ${r.htmlLength} 字符  按钮 ${r.buttons}  outlined 输入框 ${r.outlinedFields}`,
      );
    }

    // Vuetify 的 defaults 是按组件名解析的，与"全局注册还是局部导入"无关。
    // 这条断言就是用来证明这一点：搬成局部导入后 outlined/density 这些默认值仍在。
    if (label === "Wallhaven" && r.outlinedFields === 0) {
      failures.push(
        `[${label}] 找不到 .v-field--variant-outlined —— Vuetify 的 defaults（createVuetify 里的 VTextField.variant）失效了`,
      );
    }
  }

  await browser.close();
  server.close();

  if (failures.length) {
    console.error(`\n渲染自检失败，${failures.length} 处问题：`);
    for (const f of failures) console.error("  - " + f);
    process.exitCode = 1;
    return;
  }
  console.log(`\n渲染自检通过：${VIEWS.length} 个视图都正常渲染，且没有未解析的组件标签。`);
}

main().catch((e) => {
  console.error("渲染自检无法执行：" + e.message);
  process.exitCode = 1;
});
