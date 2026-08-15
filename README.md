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
| 🖥️ **设置壁纸** | 支持 GNOME / KDE / XFCE / sway / Hyprland / swww / feh；Windows 支持多显示器 |
| 🎞️ **壁纸轮播** | 使用当前筛选结果启动轮播，可设置间隔并随时停止 |
| 📋 **缺失检测** | 检测“数据库有记录但磁盘文件不存在”的图片，可选中补下载或全部恢复 |
| 🗑️ **孤儿文件** | 检测“磁盘有文件但数据库无记录”的图片，可批量收养入库或删除 |
| ❤️ **喜好管理** | 删除/不喜欢会写入数据库；缺失恢复时自动跳过已标记记录 |
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
│   │   ├── db.rs                 # SQLite schema、缓存连接、CRUD、统计
│   │   ├── downloader.rs         # HTTP 下载、流式大小限制、重试、MD5
│   │   ├── thumbnail.rs          # WebP 缩略图（DPR 1x/2x/3x）
│   │   ├── wallhaven.rs          # Wallhaven API 客户端
│   │   ├── reddit.rs             # Reddit JSON 客户端与 imgur 解析
│   │   ├── wallpaper.rs          # 各桌面环境壁纸设置与轮播
│   │   ├── state.rs              # 应用状态、事件 payload、安全路径
│   │   └── commands/             # settings/gallery/database/download/...
│   ├── capabilities/             # Tauri capability
│   ├── tauri.conf.json           # CSP、asset scope、updater、窗口
│   └── Cargo.toml
├── public/fonts/                 # 自生成的 MDI 图标子集 woff2
├── scripts/generate_mdi_subset.py # 重新生成图标子集
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
