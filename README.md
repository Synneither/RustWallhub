# RustWallhub 🖼️

**RustWallhub** 是一个桌面壁纸管理器——从 **Wallhaven** 和 **Reddit r/Animewallpaper** 自动下载动漫壁纸，提供本地浏览、设置壁纸、数据库管理等功能。

基于 **Tauri v2**（Rust 后端 + Vue 3 前端），支持 **Windows / macOS / Linux**。

---

## ✨ 功能

| 功能 | 说明 |
|------|------|
| 🔍 **搜索预览** | 在 Wallhaven 中搜索壁纸，预览缩略图，选中后批量下载 |
| ⬇️ **自动下载** | 从 Wallhaven API / Reddit JSON 批量抓取壁纸，去重保存 |
| 🖼️ **本地图库** | 浏览已下载的壁纸，网格视图，缩略图自适应 DPI |
| 🖥️ **设置壁纸** | 一键设为桌面（支持 GNOME / KDE / XFCE / sway / Hyprland / swww / feh） |
| 📋 **缺失检测** | 检测数据库中有但磁盘缺失的文件，可单独或批量补下载 |
| 🗑️ **孤立文件** | 检测磁盘上有但数据库无记录的文件，可批量入库 |
| ❤️ **喜好管理** | 标记不喜欢的壁纸，补下载时自动跳过 |
| 🌙 **多主题** | 柔灰暗色（默认）/ 暖白亮色，跟随系统主题自动切换 |

---

## 📦 环境要求

- [Rust](https://www.rust-lang.org/) (edition 2021)
- [Deno](https://deno.com/) (前端构建)
- [Tauri CLI](https://v2.tauri.app/start/cli/)

```bash
cargo install tauri-cli --version "^2"
```

## 🚀 快速开始

```bash
cd RustWallhub

# 启动开发模式
cargo tauri dev

# 仅运行前端开发服务器
deno task dev

# 构建发布版本
cargo tauri build
```

## 🧪 测试

```bash
# 运行所有后端测试
cd src-tauri && cargo test

# 运行指定测试
cargo test test_name
```

## 🏗️ 项目结构

```
RustWallhub/
├── src/                  # Vue 3 前端
│   ├── views/            # 页面组件
│   │   ├── Dashboard.vue      # 仪表盘（统计 + 维护）
│   │   ├── WallhavenView.vue  # Wallhaven 搜索/下载
│   │   ├── RedditView.vue     # Reddit 下载
│   │   ├── GalleryView.vue    # 本地图库
│   │   └── SettingsView.vue   # 设置
│   ├── stores/           # 状态管理
│   ├── utils/            # 工具函数
│   ├── assets/           # 样式/资源
│   └── App.vue           # 根组件
├── src-tauri/            # Rust 后端
│   ├── src/
│   │   ├── main.rs       # Tauri 入口
│   │   ├── lib.rs        # 应用逻辑 + Tauri 命令
│   │   ├── config.rs     # 配置管理
│   │   ├── db.rs         # SQLite CRUD
│   │   ├── downloader.rs # HTTP 下载 + 校验
│   │   ├── thumbnail.rs  # 缩略图生成（WebP + DPR）
│   │   ├── wallhaven.rs  # Wallhaven API 客户端
│   │   └── reddit.rs     # Reddit JSON 客户端
│   └── Cargo.toml
├── vite.config.ts        # Vite 配置
├── deno.json             # Deno 任务定义
└── tauri.conf.json       # Tauri 配置
```

## ⚙️ 配置

配置文件路径：`~/.config/rustwallhub/config.json`

主要选项：

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `wallhaven_save_dir` | `~/Pictures/背景/wallhaven` | Wallhaven 壁纸保存目录 |
| `reddit_save_dir` | `~/Pictures/背景/reddit` | Reddit 壁纸保存目录 |
| `wallhaven_categories` | `010` | 分类（1=General 2=Anime 4=People） |
| `wallhaven_purity` | `111` | 纯净度（1=SFW 2=Sketchy 4=NSFW） |
| `wallhaven_sorting` | `toplist` | 排序方式 |
| `wallhaven_max_images` | `100` | 每次最多下载数量 |
| `reddit_max_images` | `100` | Reddit 每次最多下载数量 |
| `wallhaven_api_key` | `""` | Wallhaven API Key（可选，提高速率限制） |

## 🔧 技术栈

- **框架**: Tauri v2
- **前端**: Vue 3 + Vuetify 4 + TypeScript + Vite
- **后端**: Rust + tokio + reqwest + rusqlite + image + rayon
- **数据库**: SQLite
- **缩略图**: WebP 格式，按设备 DPR 自适应
- **主题**: 柔灰暗色 / 暖白亮色，跟随系统

## 📜 许可

MIT
