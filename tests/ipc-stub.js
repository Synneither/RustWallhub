/*
 * Tauri IPC 打桩：把 `vite build` 产物放进浏览器里跑，不需要后端。
 *
 * 由 `tests/render-check.mjs` 通过 `page.addInitScript()` 注入，所以必须用真正的 JS
 * （不经过打包），且不能引用模块导入。
 *
 * ⚠ 两条硬约束，违反就会得到"假绿"：
 * 1. `get_config` 必须把**每个**前端会读的字段都给上。`useConfigDraft` 会把
 *    `appState.config` 的每个 key 拷进草稿，漏掉的 key 变成 undefined；组件里若有
 *    `draft.x[0]` 这类字符串索引，就会在**渲染期**抛 TypeError、整页白屏——而白屏下
 *    任何断言都会"通过"（DOM 是空的，没有未知标签、没有裁剪）。
 * 2. `convertFileSrc` 必须返回同源 URL（这里用本脚本提供的 /img.png）。返回资产协议
 *    时图片会 404，元素高度为 0，所有尺寸断言都会失真。
 */
(() => {
  let seq = 0;
  const callbacks = {};
  const handlers = {};

  const files = [];
  for (let i = 1; i <= 96; i++) {
    const n = "wallpaper_" + String(i).padStart(3, "0") + ".jpg";
    files.push({
      name: n,
      path: "D:\\Wallpapers\\wallhaven\\" + n,
      thumb_path: null,
      size: 1234567 + i * 100,
      is_orphan: i % 17 === 0,
      modified_date: "2026-09-1" + (i % 9) + " 12:00",
    });
  }
  const mk = (i, source, love, resolution) => ({
    id: i,
    name: "wallpaper_" + String(i).padStart(3, "0") + ".jpg",
    hash: "h" + i,
    url: "https://example.invalid/" + i,
    source_url: "",
    resolution: resolution || "3840x2160",
    title: null,
    permalink: null,
    love: love === undefined ? 1 : love,
    created_at: "2026-09-10 12:00:00",
    source,
  });
  const results = Array.from({ length: 48 }, (_, i) => ({
    id: "wh" + i,
    path: "https://w.wallhaven.cc/full/" + i + ".jpg",
    thumbnail_url: "/img.png",
    resolution: i % 4 === 0 ? "1080x1920" : "2560x1440",
    short_url: "https://wallhaven.cc/w/wh" + i,
    file_size: 1234567,
    category: "anime",
    colors: [],
  }));

  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) {
      const id = ++seq;
      callbacks[id] = cb;
      return id;
    },
    convertFileSrc: () => "/img.png",
    invoke(cmd, args) {
      switch (cmd) {
        case "get_config":
          return Promise.resolve({
            thumbnails_dir: "D:\\Wallpapers\\thumbs",
            db_dir: "D:\\Wallpapers\\db",
            download_concurrency: 4,
            thumbnail_dpr: 2,
            request_timeout: 30,
            auto_update: false,
            wallhaven_save_dir: "D:\\Wallpapers\\wallhaven",
            wallhaven_db_path: "D:\\Wallpapers\\db\\wallhaven_images.db",
            wallhaven_q: "",
            wallhaven_categories: "010",
            wallhaven_purity: "100",
            wallhaven_sorting: "toplist",
            wallhaven_top_range: "1y",
            wallhaven_atleast: "1920x1080",
            wallhaven_ratios: "landscape",
            wallhaven_order: "desc",
            wallhaven_api_key: "",
            wallhaven_max_images: 24,
            reddit_save_dir: "D:\\Wallpapers\\reddit",
            reddit_db_path: "D:\\Wallpapers\\db\\reddit_images.db",
            reddit_subreddit: "Animewallpaper",
            reddit_sort: "hot",
            reddit_time_range: "week",
            reddit_max_images: 100,
            oss_auto_upload_on_exit: true,
            oss_auto_download_on_start: false,
          });
        case "check_databases":
          return Promise.resolve({
            wallhaven_exists: true,
            reddit_exists: true,
            wallhaven_path: "D:\\Wallpapers\\db\\wallhaven_images.db",
            reddit_path: "D:\\Wallpapers\\db\\reddit_images.db",
          });
        case "get_stats":
          return Promise.resolve({
            wallhaven: { total: 4821, love: 4790, dislike: 37 },
            reddit: { total: 1204, love: 1180, dislike: 12 },
          });
        case "list_database_images":
          return Promise.resolve(
            Array.from({ length: 20 }, (_, i) => mk(i + 1, "wallhaven", i % 7 === 0 ? 0 : 1)),
          );
        case "list_missing_images":
          return Promise.resolve(
            Array.from({ length: 23 }, (_, i) => mk(i + 1, i % 4 === 0 ? "reddit" : "wallhaven")),
          );
        case "list_orphan_files":
          return Promise.resolve(
            Array.from({ length: 9 }, (_, i) => ({
              name: "orphan_" + String(i + 1).padStart(2, "0") + ".jpg",
              path: "D:\\Wallpapers\\wallhaven\\orphan_" + String(i + 1).padStart(2, "0") + ".jpg",
              size: 823456 + i * 4096,
              source: i % 3 === 0 ? "reddit" : "wallhaven",
            })),
          );
        case "browse_image_files":
          return Promise.resolve({ images: files, total: files.length });
        case "resolve_thumbnails":
          return Promise.resolve({ items: [] });
        case "get_active_wallpaper":
          return Promise.resolve({ paths: ["D:\\Wallpapers\\wallhaven\\wallpaper_007.jpg"] });
        case "search_wallhaven":
          return Promise.resolve({ images: results, page: 1, total_pages: 12, total: 576 });
        case "refresh_file_caches":
        case "check_update":
        case "plugin:event|unlisten":
          return Promise.resolve(null);
        case "plugin:event|listen": {
          const id = args.handler;
          (handlers[args.event] = handlers[args.event] || []).push(id);
          return Promise.resolve(id);
        }
        default:
          return Promise.resolve(null);
      }
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };

  /** 手动派发后端事件，用来造「进度卡 / 新图条」这些只在事件驱动下才出现的 UI */
  window.__fire = (event, payload) => {
    for (const id of handlers[event] || []) callbacks[id]({ event, id, payload });
  };
  window.__pump = () => {
    for (const source of ["wallhaven", "reddit"]) {
      window.__fire("download-progress", {
        source,
        done: 8,
        total: 37,
        message: "补下载中 8/37：mountains_at_sunset_3840x2160.jpg",
      });
      for (let i = 1; i <= 12; i++) {
        window.__fire("image-downloaded", {
          source,
          name: "new_" + source + "_" + i + ".jpg",
          path: "D:\\Wallpapers\\" + source + "\\new_" + source + "_" + i + ".jpg",
        });
      }
    }
  };
})();
