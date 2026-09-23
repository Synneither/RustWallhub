# RustWallhub 🖼️

**RustWallhub** 是一款桌面壁纸管理器，面向动漫壁纸收藏者与桌面美化爱好者。它把“找图 → 下载 → 浏览 → 设为壁纸 → 维护数据库”的完整流程整合在一个 Tauri 桌面应用中。

后端使用 **Rust + SQLite**，前端使用 **Vue 3 + Vuetify 4**。当前发布产物覆盖 **Windows** 与 **Linux**。

---

## ✨ 功能

| 功能 | 说明 |
|------|------|
| 🔍 **Wallhaven 搜索** | 关键词、分类、纯度、排序、分辨率、比例等条件搜索；条件保存到配置文件 |
| 🖼️ **Wallhaven 大图预览** | 单击卡片勾选，双击或悬停按钮预览原图；可打开来源页或直接下载当前大图 |
| ⬇️ **批量下载** | Wallhaven 按条件批量下载 / 勾选下载；Reddit 按 subreddit 列表批量抓取 |
| 🧵 **Reddit 抓取** | 支持 i.redd.it 直链、gallery 首图、imgur 直链与相册；连续 3 批无新增自动停止 |
| 🗂️ **本地图库** | Wallhaven / Reddit 双源浏览，搜索、排序、分页、孤儿标记，支持浏览主目录内的自定义本地目录 |
| 🖥️ **设置壁纸** | 支持 Noctalia / GNOME / KDE / XFCE / sway / Hyprland / awww(swww) / feh；Windows 与 Noctalia/Hyprland/sway 支持按显示器设置 |
| 🎞️ **壁纸轮播** | 使用当前筛选结果启动轮播，可设置间隔并随时停止 |
| 📋 **缺失检测** | 检测“数据库有记录但磁盘文件不存在”的图片，可选中补下载或全部恢复 |
| 🗑️ **孤儿文件** | 检测“磁盘有文件但数据库无记录”的图片，可批量收养入库或删除 |
| ❤️ **喜好管理** | 删除/不喜欢会写入数据库；缺失恢复时自动跳过已标记记录 |
| 🗑️ **回收站删除** | 删除图片 = 移入系统回收站（Windows 回收站 / Linux XDG Trash），可再恢复；缩略图缓存直接清理 |
| 🧹 **数据库维护** | 库状态、记录浏览、标记缺失、恢复标记、清理孤儿缩略图 |
| 🔄 **自动更新** | 启动时可选检查更新，支持下载安装后自动重启 |
| 🌙 **多主题** | 柔灰暗色（默认）/ 暖白亮色 / 跟随系统 |

---

## 📦 环境要求

- [Rust](https://www.rust-lang.org/)（stable，edition 2021）
- [Deno](https://deno.com/) 2.x（前端依赖与构建）
- [Tauri CLI](https://v2.tauri.app/start/cli/) v2
- Linux 构建还需 WebKitGTK 等系统依赖，参考 [Tauri 官方文档](https://v2.tauri.app/start/prerequisites/)

```bash
cargo install tauri-cli --version "^2"
```

## 🚀 快速开始

```bash
cd RustWallhub

# 安装前端依赖（由 Deno 管理 node_modules）
deno install

# 启动完整桌面应用（Tauri + Vite）
cargo tauri dev

# 仅运行前端开发服务器
deno task dev

# 类型检查 + 前端生产构建
deno task build

# 构建发布版本
cargo tauri build
```

## 🧪 测试

```bash
# 后端格式化检查
cd src-tauri && cargo fmt --check

# 后端 Clippy（CI 使用 -D warnings）
cargo clippy --all-targets -- -D warnings

# 运行所有后端测试
cargo test

# 前端类型检查 + 构建
cd .. && deno task build
```

## 🐧 Linux / Wayland（niri + Noctalia）

Linux 侧按「谁是背景层的真正绘制者」来选壁纸后端，探测顺序即优先级：

| 顺序 | 后端 | 适用场景 |
|------|------|----------|
| 1 | **Noctalia** | v5 走 `noctalia msg wallpaper-set`，v4 走 `qs -c noctalia-shell ipc call wallpaper set` |
| 2 | Hyprland (hyprpaper) | Hyprland 会话 |
| 3 | sway (swaymsg) | sway 会话 |
| 4 | awww / swww | 独立壁纸守护进程（守护进程没在跑时会自动拉起） |
| 5 | KDE / GNOME / XFCE | 桌面环境自身的壁纸设置 |
| 6 | feh | 仅 X11 有意义 |

**niri 本身不画壁纸**，所以要么跑 Noctalia（推荐，壁纸和调色板主题会一起变），要么跑 `awww-daemon`。走 Noctalia 时壁纸由外壳绘制，不要再另外启动 awww/swaybg 抢背景层。

「当前壁纸」读取顺序：`noctalia msg wallpaper-get` → Noctalia v4 缓存 JSON → awww/swww 守护进程缓存 → Noctalia `settings.toml`。

### AppImage

```bash
chmod +x rustwallhub_*.AppImage && ./rustwallhub_*.AppImage
```

遇到 `dlopen(): error loading libfuse.so.2` 时，装上 `fuse2`，或改用解包运行：`./rustwallhub_*.AppImage --appimage-extract-and-run`。

### 桌面集成（装进启动器）

AppImage 不会被启动器自动收录，所以先手动装一次桌面项：

```bash
./rustwallhub_*.AppImage --install-desktop     # 写入 ~/.local/share/applications/rustwallhub.desktop + 图标
./rustwallhub_*.AppImage --uninstall-desktop   # 卸载
```

装好后启动器/dock 里就能搜到 RustWallhub。桌面项里的 `StartupWMClass=rustwallhub` 与应用的 Wayland `app-id` 一致，dock 才能把窗口和图标对上。

### 界面缩放

WebKitGTK 基于 GTK3，不支持 `wp_fractional_scale_v1`。niri 这类用非整数缩放（1.25 / 1.5）的合成器下，界面会由合成器整体放大而发虚。**设置 → 外观 → 界面缩放** 可以在应用侧补偿（80% – 200%，保存后立即生效）。

### 自动更新

Linux 上的自动更新走 Tauri updater，有两个前提，不满足时应用会直接给出中文提示：

- **必须是从 AppImage 运行的实例**。`.deb` 装的实例走的是 `pkexec dpkg -i`，而 niri 默认没有 polkit 认证代理，这条路走不通 —— 请用包管理器升级。
- **AppImage 所在目录必须可写**，因为更新过程会先把当前 AppImage 挪走后写入新文件。放在 `/opt`、`/usr/local/bin` 这类 root 所有的目录会失败，放进 `$HOME` 下的目录（例如 `~/Applications`）即可。

### 日志与回收站

- 日志同时写 stderr 与 `~/.local/state/rustwallhub/logs/rustwallhub.log`（超过 2 MiB 滚动，保留一份 `.1` 备份）。从启动器启动时看这个文件。
- 删除走回收站：有 `gio` 时用 `gio trash`，否则用内置的 XDG Trash 实现（`~/.local/share/Trash`）。移入回收站失败会直接报错，**不会退化成永久删除**。

### niri 窗口规则

niri 默认平铺，桌面工具类窗口建议浮动打开。先 `niri msg windows` 确认真实 `app-id`（AppImage 下通常是 `rustwallhub`），再写进 `~/.config/niri/config.kdl`：

```kdl
window-rule {
    match app-id="rustwallhub"
    open-floating true
    default-column-width { proportion 0.8; }
}
```

### NVIDIA 显卡

WebKitGTK 在专有 NVIDIA 驱动上有已知的空白窗口 / resize 崩溃问题。应用会在**检测到专有 NVIDIA 驱动**时按会话类型自动补上兼容项，并在日志里打印一行 `[linux-env] ...`：

- Wayland（niri 等）：`__NV_DISABLE_EXPLICIT_SYNC=1`（驱动 ≥ 560 且装了 egl-wayland2 时自动跳过，保留快速路径）
- X11：`WEBKIT_DISABLE_DMABUF_RENDERER=1`

自己显式设置过的变量不会被覆盖；排查问题时可临时 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 再试。

### 排查

从终端启动可以看到日志（`RUST_LOG=info` 是默认值）：

```bash
RUST_LOG=info ./rustwallhub_*.AppImage
```

- 设壁纸报「未检测到可用的壁纸后端」：确认 Noctalia 在跑，或 `awww-daemon` 已启动。
- 文件/目录选择框是**进程内 GTK 对话框**（`tauri-plugin-dialog` 默认 gtk3 后端），不需要 `xdg-desktop-portal`；打不开先看日志。
- 打开外链/来源页走 `xdg-open`（自带 `gio open` 等回退），都没装时补一个 `xdg-utils`。
- 明明装了 `noctalia` / `awww` 却探测不到：GUI 启动时继承的 `PATH` 可能被裁剪，应用会额外查找 `~/.local/bin`、`~/.nix-profile/bin`、`/run/current-system/sw/bin` 等目录。

## 🏗️ 项目结构

```
RustWallhub/
├── src/                          # Vue 3 前端
│   ├── views/
│   │   ├── DashboardView.vue     # 仪表盘：统计、当前壁纸、活动任务、快捷操作
│   │   ├── WallhavenView.vue     # Wallhaven 搜索、大图预览、勾选/批量下载
│   │   ├── RedditView.vue        # Reddit 抓取配置与下载
│   │   ├── GalleryView.vue       # 本地图库、详情、壁纸、轮播、孤儿管理
│   │   ├── DbSettingsView.vue    # 数据库状态、缺失/孤儿/记录管理
│   │   └── SettingsView.vue      # 存储、下载、网络、更新、外观
│   ├── components/               # 进度卡、统计面板、图片查看器等
│   ├── stores/                   # 全局 reactive store 与主题
│   ├── utils/                    # API 封装、校验、格式化、错误处理
│   └── assets/                   # 设计 token 与图标字体子集
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── main.rs               # 二进制入口
│   │   ├── lib.rs                # Tauri Builder、命令注册
│   │   ├── config.rs             # 配置加载/保存/路径归一化
│   │   ├── db/                   # SQLite schema、缓存连接、CRUD、统计、快照同步
│   │   ├── downloader.rs         # HTTP 下载、流式大小限制、并发与退避重试
│   │   ├── oss.rs                # 阿里云 OSS V1 签名（手写，无 SDK 依赖）
│   │   ├── thumbnail.rs          # WebP 缩略图（DPR 1x/2x/3x）
│   │   ├── wallhaven.rs          # Wallhaven API 客户端
│   │   ├── reddit.rs             # Reddit JSON 客户端与 imgur 解析
│   │   ├── wallpaper.rs          # 各桌面环境壁纸设置与轮播
│   │   ├── linux_env.rs          # Linux/WebKit 启动期兼容项（NVIDIA、会话类型）
│   │   ├── exec.rs               # 外部命令查找（PATH + ~/.local/bin、Nix profile 等）
│   │   ├── trash.rs             # 回收站（Windows SHFileOperationW / Linux gio+XDG Trash）
│   │   ├── logging.rs            # 日志 tee 到文件 + 滚动
│   │   ├── desktop_entry.rs      # --install-desktop / --uninstall-desktop
│   │   ├── state.rs              # 应用状态、事件 payload、安全路径
│   │   └── commands/             # settings/gallery/database/download/...
│   ├── capabilities/             # Tauri capability
│   ├── tauri.conf.json           # CSP、asset scope、updater、窗口
│   └── Cargo.toml
├── public/fonts/                 # 自托管字体：MDI 图标子集 + UI 字体（Space Grotesk / Rajdhani）
├── scripts/
│   ├── generate_mdi_subset.py    # 重新生成 MDI 图标子集
│   └── fetch_webfonts.py         # 重新拉取并自托管 UI 字体
├── vite.config.ts
├── deno.json                     # Deno 任务（dev/build/tauri）
└── package.json
```

## ⚙️ 配置

配置文件路径（各平台）：
- Linux：`~/.config/rustwallhub/config.json`
- Windows：`%APPDATA%\rustwallhub\config.json`
- macOS：`~/Library/Application Support/rustwallhub/config.json`

首次启动会自动生成默认配置。以下为主要配置项：

### 存储

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `wallhaven_save_dir` | `~/Pictures/背景/wallhaven` | Wallhaven 图片保存目录 |
| `reddit_save_dir` | `~/Pictures/背景/reddit` | Reddit 图片保存目录 |
| `thumbnails_dir` | 系统缓存目录 + `rustwallhub/thumbnails` | WebP 缩略图目录 |
| `db_dir` | 系统数据目录 + `rustwallhub` | 数据库目录，两个 DB 文件由此目录派生 |

> 数据库文件固定为 `{db_dir}/wallhaven_images.db` 和 `{db_dir}/reddit_images.db`。

### Wallhaven

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `wallhaven_api_key` | `""` | API Key（可选，用于提高速率限制与 NSFW） |
| `wallhaven_q` | `""` | 搜索关键词 |
| `wallhaven_categories` | `010` | 分类：General / Anime / People |
| `wallhaven_purity` | `111` | 纯度：SFW / Sketchy / NSFW |
| `wallhaven_sorting` | `toplist` | date_added / relevance / random / views / favorites / toplist |
| `wallhaven_top_range` | `1y` | 排行榜时间范围 |
| `wallhaven_order` | `desc` | 排序方向 |
| `wallhaven_atleast` | `1920x1080` | 最小分辨率 |
| `wallhaven_ratios` | `landscape` | 宽高比 |
| `wallhaven_max_images` | `100` | 按条件批量下载目标张数 |

### Reddit

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `reddit_url` | r/Animewallpaper Desktop 筛选页 | 任意 subreddit 列表 URL，自动转 JSON API |
| `reddit_max_posts` | `100` | 每批抓取帖子数 |
| `reddit_max_images` | `100` | 目标图片数 |

### 下载与网络

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `download_concurrency` | `6` | 并发下载数，范围 1-100 |
| `request_timeout` | `30` | HTTP 请求超时（秒），范围 5-120 |
| `proxy_url` | `""` | HTTP/HTTPS 代理，留空直连 |
| `thumbnail_dpr` | `2` | 缩略图清晰度：1x(240) / 2x(480) / 3x(720) |
| `auto_update` | `true` | 启动时自动检查更新 |

## 🔧 技术栈与实现要点

- **框架**：Tauri v2
- **前端**：Vue 3 + Vuetify 4（按需注册组件）+ TypeScript + Vite
- **后端**：Rust + tokio + reqwest + rusqlite + image + rayon
- **数据库**：SQLite（WAL、连接缓存、统计短缓存、旧 schema 自动清理冗余索引）
- **下载**：流式大小限制（单图 256MB）、分批下载、批量事务入库、进度事件节流
- **缩略图**：WebP + DPR 适配，按需惰性生成
- **图标**：Material Design Icons 子集化，仅保留实际使用图标（约 5KB woff2）
- **主题**：柔灰暗色 / 暖白亮色 / 跟随系统
- **更新**：Tauri updater，Release 资产含 `latest.json`

## 📦 发布与更新

Release workflow 在推送 `v*` 标签时构建：

- Windows：`.msi`、`-setup.exe`
- Linux：`.deb`、`.AppImage`

所有资产均带签名校验文件，并生成自动更新所需的 `latest.json`。

## 📜 许可

MIT
