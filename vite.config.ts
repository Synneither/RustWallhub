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
        // 按依赖来源拆包：vuetify 体量最大（当前约 290KB / gzip 93KB），单独成块可与
        // 业务代码分开缓存。桌面端资源都在本地磁盘，加载耗时可忽略，分块主要是为了
        // 改业务代码时不必重新打包框架、构建更快。
        // @tauri-apps/* 是稳定依赖，同理独立成块。
        manualChunks(id: string) {
          if (id.includes("node_modules")) {
            if (id.includes("vuetify")) return "vuetify";
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
