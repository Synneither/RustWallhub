import { test, expect } from "@playwright/test";
import { execSync, spawn } from "child_process";

let devServer: ReturnType<typeof spawn>;

test.describe("App smoke tests", () => {
  test.beforeAll(async () => {
    // Start Vite dev server
    devServer = spawn("deno", ["task", "dev"], {
      cwd: process.cwd(),
      stdio: "ignore",
    });
    // Wait for server to be ready
    for (let i = 0; i < 30; i++) {
      try {
        await fetch("http://127.0.0.1:1420");
        break;
      } catch {
        await new Promise((r) => setTimeout(r, 1000));
      }
    }
  });

  test.afterAll(() => {
    devServer?.kill();
  });

  test("page loads without error", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));

    await page.goto("http://127.0.0.1:1420");
    await page.waitForLoadState("networkidle");

    // The app mounts with a nav drawer
    const title = page.locator(".v-navigation-drawer");
    await expect(title.first()).toBeVisible({ timeout: 10000 });

    // No uncaught JS errors (allow Tauri invoke errors since backend isn't running)
    const realErrors = errors.filter(
      (e) => !e.includes("invoke") && !e.includes("__TAURI__")
    );
    expect(realErrors).toEqual([]);
  });

  test("navigates between views", async ({ page }) => {
    await page.goto("http://127.0.0.1:1420");
    await page.waitForLoadState("networkidle");

    // Click Gallery nav item
    const galleryBtn = page.locator(".v-list-item").filter({ hasText: "画廊" });
    if (await galleryBtn.count() > 0) {
      await galleryBtn.first().click();
      await page.waitForTimeout(500);
      // Gallery view should be visible
      await expect(page.locator(".gallery-root")).toBeVisible({ timeout: 5000 });
    }

    // Click Settings nav item
    const settingsBtn = page.locator(".v-list-item").filter({ hasText: "设置" });
    if (await settingsBtn.count() > 0) {
      await settingsBtn.first().click();
      await page.waitForTimeout(500);
    }
  });
});
