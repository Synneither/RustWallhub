# RustWallhub 前端设计文档

> 版本：2.0 · 2026-08-02
> 本文档完全以后端 36 个 Tauri 命令、8 种事件为依据重新设计前端，不继承旧版界面结构。
> 技术栈：Vue 3.5 + Vuetify 4 + TypeScript（无 vue-router、无 pinia，视图切换与全局状态由 App 层与 reactive store 承担）。

---

## 1. 设计目标与原则

1. **后端能力完全可达**：每个命令在 UI 中都有明确入口；每个事件都有可见的状态反馈。
2. **任务异步化**：下载/恢复是后台任务，UI 不阻塞，进度全部来自 `download-progress` 事件流。
3. **双源对称**：Wallhaven 与 Reddit 是同等的"图片来源"，在导航、统计、图库中对称呈现。
4. **简洁克制**：中性灰表面层级 + 单一主色（蓝）+ Reddit 橙作来源标识色；所有颜色走 `style.css` 设计 token，禁止硬编码。
5. **危险操作必确认**：删除、批量标记、初始化数据库、全量恢复等均需二次确认。

---

## 2. 信息架构

六个一级页面，左侧导航抽屉切换：

| 页面 | 路由态 `view` | 覆盖的后端域 | 核心职责 |
|------|--------------|--------------|----------|
| 仪表盘 | `dashboard` | stats / download / update / slideshow | 全局总览、快捷操作、活动任务 |
| Wallhaven | `wallhaven` | wallhaven 模块 + wallhaven_* 配置 | 搜索条件、在线预览、勾选/批量下载 |
| Reddit | `reddit` | reddit 模块 + reddit_* 配置 | 抓取配置、一键下载 |
| 图库 | `gallery` | gallery 模块 + wallpaper 模块 | 本地图片浏览、壁纸设置、轮播、孤儿管理 |
| 数据库 | `database` | database 模块 + 数据库生命周期 | 库状态、缺失/孤儿/记录管理、统计 |
| 设置 | `settings` | settings 模块 + system | 目录、下载、网络、更新、外观 |

导航项带来源色小圆点：Wallhaven 蓝、Reddit 橙；图库与数据库为中性。

---

## 3. 启动序列（App 层）

```
应用启动
 ├─ get_config            → 写入 config store（目录展示、表单初值）
 ├─ check_databases
 │    ├─ 两库均存在 → 继续
 │    └─ 有缺失 → 模态确认框（列出将创建的库文件路径）
 │         ├─ 确认 → init_databases（Toast 报告实际创建的库）
 │         └─ 取消 → 应用保持可用，但涉及 DB 的页面显示"数据库未初始化"空态
 ├─ get_stats             → 写入 stats store
 ├─ is_slideshow_running  → 恢复轮播指示
 └─ 注册全局事件监听（见 §6）
```

约定：除启动确认框外，任何 DB 命令报错 "unable to open database" 时，经 `friendlyError` 转为引导文案并弹出初始化确认。

---

## 4. 全局状态模型（stores/app.ts，reactive 模块）

```ts
// 配置（与后端 AppConfig 一一对应）
config: AppConfig | null

// 数据库
dbStatus: DatabaseStatus | null      // check_databases 结果
stats: { wallhaven: DbStats; reddit: DbStats } | null

// 下载任务（key = source: "wallhaven" | "reddit" | "all"）
downloads: Record<string, {
  active: boolean
  done: number; total: number
  message: string
  lastComplete?: { success: number; total: number; message: string }
}>

// 轮播
slideshow: { running: boolean; current?: { index: number; total: number; name: string; path: string } }

// 更新
update: { info: UpdateInfo | null; downloading: boolean; progress: number | null }

// UI 基础设施
snackbar: { text: string; color: 'success' | 'error' | 'info'; queue: ... }
confirm: { visible: boolean; title: string; text: string; danger: boolean; resolve }
```

派生约定：
- `dislike` 字段语义 = "love=1 但文件缺失"（后端口径），UI 文案统一写 **"缺失"**，不写"不喜欢"。
- 下载取消为全局单标志：任一任务进行中时 `cancel_downloads` 对所有任务生效，UI 上取消按钮作用于"当前所有进行中的任务"。

---

## 5. API 层（utils/api.ts）

- 全部 36 个 `invoke` 的类型化封装，函数名与后端命令一致。
- 类型定义集中 `src/types.ts`，与后端 serde 结构一一对应（snake_case 字段名原样保留）。
- 事件封装：`onDownloadProgress(cb)` 等 8 个 `listen` 包装，返回 unlisten 函数；App 层统一注册，页面级临时监听自行注册/卸载。
- `assetUrl(path)`：`convertFileSrc` 包装，统一处理 `http://asset.localhost` 前缀。

### 关键调用约束（后端行为决定）

1. **搜索即配置**：`search_wallhaven` 只收 `page`，搜索条件来自已保存的配置。Wallhaven 页改条件后必须先 `save_settings` 再搜索。UI 策略：条件区与保存按钮一体，"搜索"按钮在条件有未保存改动时先保存再搜索。
2. **图库两阶段加载**：`browse_image_files` 返回 `thumb_path: null` → 对当前页文件名批量 `resolve_thumbnails` → `assetUrl` 显示。切页/搜索/排序时重置。
3. **`Source = all` 用于恢复/补下载会直接报错**：`recover_database_files("all")` 与 `download_missing_images("all")` 现在会明确拒绝。前端"全量恢复"入口必须**串行调用 wallhaven + reddit 两次**并合并提示。
4. **`save_settings` 副作用**：清空图库文件缓存、重建 HTTP client、emit `settings-changed`。保存成功后图库页应失效重载。
5. **显示器设置**：`set_wallpaper(file_path, monitor)`，`monitor` 省略或 `"all"` = 全部显示器；Windows 传 `list_monitors` 返回的 `id`（设备路径）。

---

## 6. 全局事件 → UI 映射

| 事件 | 处理 |
|------|------|
| `download-progress` | 更新 `downloads[source]`；App 底部状态条 + 仪表盘活动卡片显示 |
| `download-complete` | 标记任务结束 → Toast（成功 x/y）→ 静默刷新 `get_stats` + 图库当前页 |
| `image-downloaded` | 追加到对应源页面的"本次新图"预览条（最多展示 12 张，超出计数折叠） |
| `settings-changed` | 重置图库缓存标记；设置页显示"已保存"反馈 |
| `update-available` | 仪表盘 + 设置页出现更新横幅（不打断用户） |
| `update-progress` | 更新下载进度条（total 可能为 null，此时显示不确定态） |
| `update-installing` | 全屏遮罩"正在安装更新，应用将自动重启" |
| `slideshow-tick` | 更新 `slideshow.current`；图库页轮播指示器显示当前张数 |

---

## 7. 页面设计

### 7.1 仪表盘 Dashboard

三段式：
1. **统计区**：两张来源卡（蓝/橙标识），各显示 总数 / 在库（love=1）/ 缺失 三个数字（DataTerminal 面板样式，数字 1.625rem）。
2. **活动区**：进行中的下载任务卡（来源、进度条、done/total、message、取消按钮）；轮播状态卡（运行中显示当前 index/total 与文件名，提供停止）；更新横幅（有更新时：版本号 + 查看更新 → 跳设置页）。
3. **快捷操作**：主按钮"浏览图库"，次按钮"Wallhaven 下载""Reddit 下载""数据库管理"。

空态：数据库未初始化时整页替换为初始化引导空态。

### 7.2 Wallhaven 页

上下两区（可滚动单列）：
1. **搜索条件卡**：关键词 `q`、分类三位开关（general/anime/people）、纯度三位开关（sfw/sketchy/nsfw，NSFW 需 API Key 提示）、排序（date/favorites/toplist/random，toplist 时展开 topRange 选择且禁用 order；random 时禁用 order）、最小分辨率 `atleast`、比例 `ratios`、单次下载目标 `wallhaven_max_images`、API Key（password 输入）。
   - 底部操作条：`保存并搜索`（主按钮，先 `save_settings` 再 `search_wallhaven(1)`）、`仅保存`。
2. **结果区**：在线缩略图网格（`thumbnail_url` 直链，CSP 已允许 `th.wallhaven.cc` / `w.wallhaven.cc`），卡片显示分辨率角标 + 勾选框；单击选择，双击或悬停按钮打开大图预览（原图 URL 直载，可打开来源页 / 直接下载当前大图）；顶栏：`第 x / y 页 · 共 z 张`、分页前后按钮、`全选本页`、`下载选中`（`download_wallhaven_selected`）、`按条件批量下载`（`start_wallhaven_download`，说明文案"最多 100 页直到凑满 N 张"）。
3. 下载中：结果区顶部进度条；`image-downloaded` 累积"本次新图"横向预览条。

### 7.3 Reddit 页

单卡布局：
- 抓取配置：`reddit_url`（带说明：任意 subreddit 列表 URL，自动转 JSON API）、`reddit_max_posts`（每批帖子数）、`reddit_max_images`（目标图片数）、保存目录展示。
- 操作：`保存设置` + `开始下载`（主按钮，`start_reddit_download`）。
- 能力说明卡：支持的图片来源（i.redd.it 直链 / gallery 首图 / imgur），连续 3 批无新增自动停止。
- 进度区与"本次新图"预览条同 Wallhaven 页。

### 7.4 图库 Gallery

1. **顶栏**：来源切换（Wallhaven / Reddit 分段按钮）、搜索框（文件名包含，防抖 300ms）、排序下拉（默认/名称/大小/日期）、刷新。
2. **统计条**：`共 N 张 · 第 x/y 页`；存在孤儿时显示 `含 M 个孤儿文件` 警示 chip（点击筛选孤儿——前端过滤当前列表）。
3. **网格**：缩略图卡片（asset URL），孤儿文件带橙色角标；hover 浮层操作：设为壁纸 / 详情 / 删除。多选模式：点击进入选择态，底栏批量操作（批量删除=dislike，孤儿批量收养/删除）。
4. **分页**：页码 + 每页数量（24/48/96）。
5. **详情抽屉**：大图预览 + `get_image_info` 元数据（尺寸/格式/大小/来源链接[ opener 打开 ]/入库时间/标题[reddit]）；操作：设为壁纸（含显示器选择：`list_monitors` 下拉，"全部显示器"为默认）、删除/不喜欢。
6. **轮播控制条**（页首或悬浮）：间隔秒数输入（≥5）、`使用当前筛选结果启动轮播`（取当前源全部文件名 → `start_slideshow`）、运行中显示 tick 信息与停止按钮。
7. **全屏查看器**：固定深色（`--preview-bg`），左右切换、Esc 关闭、快捷键 ←/→。

空态分级：目录为空（引导去下载）/ 搜索无结果 / 数据库未初始化。

### 7.5 数据库页

1. **库状态卡**：`db_dir` 可编辑（保存后两个 db 文件路径强制派生为 `{db_dir}/*.db`，展示为只读文本）；每库存在状态徽标；缺失时提供"创建数据库"按钮（确认后 `init_databases`）。
2. **统计卡**：两库 total / love=1 / 缺失。
3. **缺失文件管理**：`count_missing_images` 计数 + `list_missing_images` 表格（名称、分辨率、入库时间、来源）；操作：`补下载选中`（`download_missing_images`）、`全部补下载`（`recover_database_files` × 两源，确认框）、`标记为不喜欢`（`mark_disliked_files`，确认框）。
4. **孤儿文件管理**：`list_orphan_files` 表格（名称、大小、来源）；操作：`收养入库`（`adopt_orphan_files`）、`删除`（确认框）。
5. **维护区**：`清理孤儿缩略图`（`clean_thumbnails` 结果显示）、`恢复所有已标记`（`restore_all_files`，确认框）。
6. **记录浏览**：`list_database_images` 分页表格（全部字段含 love 状态、source_url/permalink 外链）。

### 7.6 设置页

分组表单（单页滚动，底部 sticky 保存条）：
1. **存储**：wallhaven / reddit 保存目录（dialog 选目录）、缩略图目录。
2. **下载**：并发数（1-100）、请求超时（5-120s）、缩略图 DPR（1-3）。
3. **网络**：代理 URL（留空直连，示例 `http://127.0.0.1:7890`）。
4. **更新**：自动检查开关；当前版本；`检查更新` / `下载并安装`（进度条 + 安装遮罩）。
5. **外观**：主题切换（跟随系统 / 深色 / 浅色）。

校验：全部走 `utils/rules.ts`；保存成功 Toast + `settings-changed` 处理。

---

## 8. 组件清单

| 组件 | 说明 |
|------|------|
| `StatPanel.vue` | 来源统计面板（图标 + 三个数字），替代旧 DataTerminal |
| `EmptyState.vue` | 空态（图标、标题、描述、动作插槽），用全局 `.gallery-empty` 类族 |
| `ConfirmDialog.vue` | 全局确认框，Promise 化（store 提供 `askConfirm()`） |
| `ImageViewer.vue` | 全屏深色查看器（预览 + 元信息 + 键盘导航） |
| `ProgressCard.vue` | 下载任务进度卡（仪表盘/源页复用） |
| `NewImagesStrip.vue` | "本次新图"横向预览条（Wallhaven/Reddit 复用） |

---

## 9. 视觉与交互规范

- **Token 优先**：颜色/间距/圆角/阴影全部引用 `style.css` 变量；透明度变体一律 `color-mix`。
- **来源色**：Wallhaven `--accent-primary`（蓝）、Reddit `--accent-reddit`（橙），仅用于标识与徽标，不作为大面积填充。
- **按钮层级**：每屏主操作唯一实心（`variant="flat" color="primary"`）；危险操作用 `color="error"` 且必弹确认。
- **数字**：统计数字 `.stat-number`（1.625rem，tabular-nums）。
- **加载态**：网格用 shimmer 骨架；命令进行中按钮 loading 且禁用。
- **Toast**：成功 `accent-success` / 失败 `accent-error` / 信息中性；底部居中，3s。
- **无障碍**：焦点环、`prefers-reduced-motion` 降级（token 已实现，保持）。

---

## 10. 后端已知口径的前端对策

| 后端行为 | 前端对策 |
|----------|----------|
| `DbStats.dislike` 实为缺失数 | 文案统一为"缺失" |
| `browse_image_files.modified_date` 为近似换算 | 仅作展示，排序依赖后端 `sort_by`，不做二次精确化 |
| `recover_database_files("all")` 返回错误 | 全量恢复 = 两源串行调用，结果合并提示 |
| `download-progress` 与 `download-complete` 的 total 口径不同（恢复流程） | 进度条按 progress 事件渲染，完成提示用 complete 事件数字，互不混用 |
| `is_slideshow_running` 可能假阳性 | 启动时查询仅用于恢复指示；收到 tick 才视为活跃运行 |
| `save_settings` 不建库 | 保存后若库缺失，主动弹初始化确认 |
