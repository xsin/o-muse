# PRD — Product Requirements Document: O Muse

> 读者：产品与实现团队、Agent 执行者（配合 `brd.md` / `tdd.md`）。
> 本文件聚焦**做什么**与**体验规格**；商业背景见 `brd.md`，技术约束见 `tdd.md`。

## 1. 产品概述

本地桌面助手，由 **Tauri 2 壳 + React19/TS7 前端**承载。核心是一个**工具箱（App Marketplace 落地形态）**首页：左侧常驻导航栏，右侧展示 App 卡片；点击 App 在其左栏新增标签页，并支持**多个 App 同时打开**。

## 2. 术语（锁定）

见 `GLOSSARY.md`：
- **App**：O Muse 自身功能单元（可被启动/常驻/多开）。
- **Audio Plugin**：被扫描管理的第三方音频格式（AU/VST/VST3/AAX/CLAP），**不是** App。
- **工具箱**：v1 首页左栏实际标签；**App Marketplace**：战略愿景名。

## 3. 用户故事（v1）

- 作为音乐人，我希望打开 O Muse 看到一个**工具箱**，点开"音频分轨"即可拖入歌曲分离人声/伴奏。
- 作为音乐人，我希望在"Kontakt 库管理"里**只读盘点**我的音源库、看到诊断（缺失路径/重复 SNPID/断卷）。
- 我希望**同时打开**分轨和库管理两个 App，并在左栏切换，不必退出重进。
- 作为潜在 Pro 用户，我希望输入/激活 license_key 后，Pro 专属能力（写入/修复、自定义目录）被解锁——通过 **Dodo Payments** 校验。

## 4. 功能需求

### FR-1 工具箱首页（启动器）
- 左栏默认选中"**工具箱**"，右栏为 App 卡片网格。
- 卡片来自**轻量 App Registry**（前端常量/薄模块）：字段含 `id, name, route, icon, enabled, minVersion`。
- 卡片：`StemDeck`（音频分轨）、`Kontakt Library Manager`（库管理）为**启用态**；`Audio Plugin Manager` 为 **"即将推出"**（disabled，v1.1 启用）。

### FR-2 多开工作区（关键体验，来自 Q12）
- 左栏**常驻**；点击 App 卡片 → 在左栏新增该 App 的标签页项（图标 + 名称 + 关闭按钮）。
- 支持**同时打开多个 App**，每个 App 在右侧以独立面板呈现（多窗格/标签内容并存，非模态）。
- 关闭标签页仅关闭该 App 视图，不退出应用；"工具箱"项始终存在，点回显示卡片网格。
- 导航模型类比：左栏 = 常驻坞 + 已开 App 标签；右侧 = 当前激活 App 的内容。

### FR-3 App 1 — 音频分轨（StemDeck）
- 文件拖放区、`POST /separate` 触发、SSE 进度（`GET /status/{id}`）、结果列出（`GET /stems/{id}`）。
- 通过 **Tauri Sidecar** 启动 Python (FastAPI + demucs) 进程；壳退出时终止该进程。

### FR-4 App 2 — Kontakt 库管理（只读，KLM 逻辑 Rust 重写）
- 扫描 macOS 上 Service Center XML / Kontakt plist / installed products JSON。
- 诊断：路径存在性、重复 SNPID、断卷（`/Volumes/` 不可访问）。
- v1 **仅读取**，写入/修复为 Pro（见 FR-6）。

### FR-5 App 3 — 音频插件管理（v1.1，v1 仅占位）
- 标准目录自动发现（AU/VST/VST3/AAX/CLAP，macOS 路径见 `tdd.md`）。
- 元数据提取 + SQLite 增量缓存；Pro 解锁自定义目录。

### FR-6 Free / Pro 边界（来自 plan §五）
| 功能 | Free | Pro |
|---|---|---|
| 分轨 | ✅ | ✅ |
| Kontakt 库只读盘点 | ✅ | ✅ |
| Kontakt 库写入/修复/重排 | ❌ | ✅ |
| 插件标准目录扫描 | ✅ | ✅ |
| 插件自定义目录 | ❌ | ✅ |
| 插件元数据完整视图 | ✅ | ✅ |
| 插件预设管理（v1.2+） | ❌ | ✅ |

### FR-7 订阅激活（Dodo，v1 即接入，ADR-0003）
- 设置/Pro 引导入口：输入 `license_key`；`license::current_tier()` 经 Dodo API 校验推导 `Free/Pro`。
- 无 key / 校验失败 → `Free`；Pro 专属 Command 入口统一拦截返回错误。

## 5. 非功能需求

- **平台**：v1 macOS only；v1.5 Windows（架构已抽象，见 ADR-0002）。
- **本地优先 / 隐私**：不向第三方上传用户音频或库数据；仅 Dodo 校验所需最小信息出网。
- **性能**：v1.1 插件扫描 50 文件首扫 < 2s、缓存 < 200ms（来自 plan Task 10）。
- **可维护性**：React+TS 便于 Agent 维护；App Registry 让新增工具走同一渲染路径。

## 6. 首页设计规格（Homepage Spec）

> 视觉语言参照 `examples/02-dashboard.html`：左侧栏（brand + 分组导航 + 当前项高亮）、右侧主内容、亮/暗主题、`--brand` 主色、卡片化布局。

### 6.1 布局
- 两栏栅格：`grid-template-columns: 232px 1fr`（同参考 dashboard）。
- 左栏：品牌区（logo + "O Muse"）→ 导航项"**工具箱**"（默认 `on`）→ 已开 App 标签列表（动态追加）→ 底部"设置"。
- 右栏：当"工具箱"激活时，显示 App 卡片网格；当某 App 标签激活时，显示该 App 内容面板。

### 6.2 卡片组件（App Card）
- 图标（SVG，可用 `assets/logo.svg` 占位或各 App 图标）、名称、一句话描述、状态徽标（`可用` / `即将推出`）。
- 启用态卡片：点击 → 左栏追加标签 + 右侧切换至该 App 面板。
- "即将推出"卡片：`disabled`，显示锁/沙漏图标，不可点击。

### 6.3 交互
- 多开：左栏可同时存在 StemDeck、Kontakt Library Manager 等多个标签；点击切换；✕ 关闭。
- 主题：沿用参考 dashboard 的 `data-theme` + `localStorage` 持久化与亮/暗切换。

### 6.4 实现映射（v1 前端）
- 原 plan Task 4 的"顶部三页签"**改为**本规格的"左栏工具箱 + 多开标签页"（来自 Q4/Q12）。
- 路由用 `react-router-dom` v7；全局状态用 `zustand` v5（记录 `openApps: AppId[]`、`activeApp: AppId | 'market'`）。

## 7. 国际化与主题（i18n / Dark Mode）

### FR-8 国际化（i18n）
- 支持 **简体中文（zh-CN）** 与 **英文（en-US）** 两套文案，默认 **跟随系统区域**（macOS `prefers-languages` / 浏览器/OS 语言）。
- 语言解析优先级：用户手动选择（持久化到 `tauri-plugin-store` 的 `ui_lang`）> 系统区域（`zh-CN`→中文，其他→英文）> 回退 `en-US`。
- 壳层所有用户可见文案（导航项、卡片名/描述、状态徽标、Pro 提示、设置等）**不得硬编码字符串**，统一走 i18n 资源（`src/locales/{en,zh}.json`）。Rust 侧日志/错误文案可保留英文（用户不可见为主），但前端展示字符串必须可翻译。
- **App Registry** 的 `name`/`sub`/`desc` 从 i18n 资源按 `id` 取（如 `apps.stemdeck.name`），不写死在 `appRegistry.ts`。
- 语言切换入口放在左栏底部「设置」（v1 引入设置面板，或复用顶部）。切换即时生效、无需重启。

### FR-9 暗夜模式（Dark Mode）
- 支持 **亮 / 暗** 双主题，默认 **跟随系统**（`prefers-color-scheme`）；用户可手动切换并持久化（同 `ui_theme` 键）。
- 视觉实现沿用参考 dashboard 的 CSS 变量方案（`[data-theme="dark"]` + 统一 `--n-*`/`--brand` 令牌），原型 `prototype/home.html` 已实现亮/暗切换与系统跟随。
- 主题仅作用于前端外壳；Tauri 原生窗口（`titleBarStyle`、托盘）在 v1 跟随系统外观，深色窗口装饰留 v1.x 微调。

## 8. 依赖文档
- `brd.md`（商业/范围）、`tdd.md`（技术/版本/目录/命令）、`GLOSSARY.md`、`docs/adr/`、`handsoff.md`（执行进度）。
