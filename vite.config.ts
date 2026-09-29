import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [vue()],

  build: {
    rollupOptions: {
      output: {
        // 按依赖来源拆包。**这里刻意不按 vuetify 归堆**：那会把所有 vuetify 模块（不论谁引用）
        // 强制塞进一个 chunk，而入口引用了它 → 整个 chunk 进首屏，懒加载视图里的重组件照样被
        // 首屏背上（实测：放开后首屏 gzip 131.3 → 121.1KB，-7.8%）。代价只是框架代码与业务代码
        // 同属入口 chunk，改业务代码会连带失效框架缓存——桌面端资源在本地磁盘，这个代价很小。
        // @tauri-apps/* 是稳定依赖，独立成块仍划算。
        manualChunks(id: string) {
          if (id.includes("node_modules")) {
            // 注意顺序：vuetify 必须显式返回 undefined，否则会被下面的 vendor 兜住 ——
            // vendor 被入口引用，一样会把懒加载视图的组件拉进首屏。
            if (id.includes("vuetify")) return undefined;
            if (id.includes("@tauri-apps")) return "tauri";
            return "vendor";
          }
          return undefined;
        },
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      // use polling to avoid Deno fs.watch compatibility issues
      ignored: ["**/src-tauri/**"],
      usePolling: true,
    },
  },
}));
