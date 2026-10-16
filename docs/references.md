# 外部参考记录

本文件记录 O Muse 开发过程中参考过的外部项目。所有参考均限于
"研究功能与设计思路"，未复制任何源代码。本文件用于证明独立实现
过程，不具有法律约束力，但保留以备查证。

---

## StemDeck
- **URL**: https://github.com/stemdeckapp/stemdeck
- **许可证**: Apache License 2.0
- **参考方面**:
  - 多轨波形编辑器的交互设计（缩放、静音、独奏、推子的用户流程）
  - Python sidecar 进程的生命周期管理思路（启动、健康检查、退出清理）
  - 后端 watchdog 机制的架构模式
- **使用方式**: 仅作为设计参考，未复制任何代码。实现完全独立。
- **拉取 commit（已删除，仅存档）**: 5f9405ca68ac31b31ae07ac416141d4d1f7b5f3a
- **拉取日期**: 2026-10-15

---

## Demucs
- **URL**: https://github.com/facebookresearch/demucs
- **许可证**: MIT License
- **使用方式**: 作为 Python 依赖直接安装使用（`pip install demucs`）。
  本项目的分轨能力完全来自 Demucs 模型，StemDeck 只是 Demucs 的一个
  使用范例。
- **依赖引入位置**: python-backend/requirements.txt

---

## KLM - Kontakt Library Manager
- **URL**: https://github.com/cloinse/klm
- **许可证**: MIT License
- **参考方面**:
  - Kontakt 库元数据的解析逻辑（Service Center XML、plist、JSON 的结构）
  - 安全文件操作模型（不安装后台服务、不调用 sudo、直接写用户偏好）
- **使用方式**: 仅作为逻辑参考，用 Rust 独立重写。未复制 KLM 的 Flutter/Dart 源码。

---

## Tauri
- **URL**: https://github.com/tauri-apps/tauri
- **许可证**: MIT / Apache 2.0 双许可
- **使用方式**: 作为桌面应用框架直接依赖。

---

## 其他依赖
（后续新增依赖时，在此追加记录，注明 URL、许可证、用途。）

### FastAPI
- **URL**: https://github.com/fastapi/fastapi
- **许可证**: MIT License
- **使用方式**: Python 分轨后端（Task 2）的 Web 框架，提供 `/separate`、`/status`、`/stems`、`/health` 接口。作为依赖直接安装（`python-backend/requirements.txt`）。

### Uvicorn
- **URL**: https://github.com/encode/uvicorn
- **许可证**: BSD License
- **使用方式**: 运行 FastAPI 分轨后端的 ASGI 服务器（`python-backend/requirements.txt`）。

### librosa
- **URL**: https://github.com/librosa/librosa
- **许可证**: ISC License
- **使用方式**: 音频分析辅助库（`python-backend/requirements.txt`），Task 2 暂未直接使用，预留给后续响度/标准化处理。

### pyloudnorm
- **URL**: https://github.com/csteinmetz1/pyloudnorm
- **许可证**: MIT License
- **使用方式**: 响度测量辅助库（`python-backend/requirements.txt`），预留给后续分轨后响度对齐。

### reqwest
- **URL**: https://github.com/seanmonstar/reqwest
- **许可证**: MIT / Apache 2.0 双许可
- **使用方式**: Rust 端向 Python sidecar 发起 blocking HTTP 请求的客户端（Task 2，`src-tauri/Cargo.toml`）。

### quick-xml
- **URL**: https://github.com/tafia/quick-xml
- **许可证**: MIT License
- **使用方式**: Rust 端解析 Native Instruments Service Center XML（`<Application>` 条目）的 XML 解析库（Task 3，`src-tauri/Cargo.toml`）。

### plist
- **URL**: https://github.com/rustsec/plist-rs
- **许可证**: MIT License
- **使用方式**: Rust 端解析 Kontakt 偏好 plist（已注册库与路径）的 plist 解析库（Task 3，`src-tauri/Cargo.toml`）。

### 前端依赖（Task 4，仓库根 package.json）
- **@tauri-apps/api** (`^2.1.1`)：Apache-2.0 OR MIT；Tauri 前端桥接（invoke/events）。
- **@tauri-apps/plugin-shell** (`^2.0.1`)：Apache-2.0 OR MIT；sidecar 进程管理（前端侧）。
- **@tauri-apps/plugin-store** (`^2.0.1`)：Apache-2.0 OR MIT；持久化 store（license_key / ui_lang / ui_theme）。
- **i18next** (`^23.16.4`)：MIT；国际化内核（ADR-0004）。
- **i18next-browser-languagedetector** (`^8.0.0`)：MIT；浏览器语言检测。
- **react** (`^18.3.1`) / **react-dom** (`^18.3.1`)：MIT；UI 框架（回退栈，见 handsoff Task 4 偏差）。
- **react-i18next** (`^14.1.3`)：MIT；React i18n 绑定。
- **react-router-dom** (`^6.27.0`)：MIT；左栏多开视图路由。
- **zustand** (`^4.5.5`)：MIT；轻量全局 store（openApps/activeView/theme/lang）。
- **dev**：`@tauri-apps/cli` (`^2.1.0`, Apache-2.0 OR MIT)、`typescript` (`^5.6.3`, Apache-2.0)、`vite` (`^5.4.10`, MIT)、`@vitejs/plugin-react` (`^4.3.3`, MIT)、`@types/react`/`@types/react-dom` (`^18.3.x`, MIT)。

> 注：npm 缓存置于仓库内 `.npm-cache/`（沙箱拒写 `~/.npm`）。前端栈已于 v1.3 升级至最新（React 19 / Vite 8 / TS 7 / Tailwind 4 / HeroUI 3）。

### rusqlite（Task 8，v1.1）
- **URL**: https://github.com/rusqlite/rusqlite
- **许可证**: MIT License
- **使用方式**: 插件元数据增量缓存的 SQLite 驱动（`src-tauri/Cargo.toml`，feature `bundled` 自带 SQLite 源码）。
- **决策说明**: 缓存层选用 `rusqlite` 而非 `tauri-plugin-sql`，以保持 `core/` 无 Tauri 依赖，使缓存逻辑可在单元测试中直接建库验证。`tauri-plugin-sql` 已于 v1.3 移除：它与 `rusqlite` 都声明链接原生 `sqlite3`，Cargo 不允许同一 `links` 值出现两份；且该插件从未被实际调用。

### tauri-plugin-dialog（Task 9，v1.1）
- **URL**: https://github.com/tauri-apps/plugins-workspace
- **许可证**: MIT License
- **使用方式**: 前端"添加自定义目录"的目录选择对话框（Rust `tauri-plugin-dialog 2.2.0` + npm `@tauri-apps/plugin-dialog ^2.8.3`）。

### @tauri-apps/plugin-dialog（Task 9，v1.1，前端）
- **许可证**: MIT
- **使用方式**: `PluginManagerPage` 中调用 `open({directory:true})` 选择插件目录。

### flate2 / tar（v1.2）
- **URL**: https://github.com/rust-lang/flate2 、 https://github.com/alexcrichton/tar-rs
- **许可证**: MIT / Apache 2.0 双许可
- **使用方式**: 纯 Rust 解压自包含 CPython 归档（`src-tauri/Cargo.toml`）。用 Rust 库而非外部 `tar` 进程，符合"外部进程统一走 tauri-plugin-shell"的约定。

### libc（v1.2）
- **URL**: https://github.com/rust-lang/libc
- **许可证**: MIT / Apache 2.0 双许可
- **使用方式**: 通过 `statfs` 估算磁盘可用空间，安装分轨运行时前做余量检查（`src-tauri/Cargo.toml`）。

### python-build-standalone（v1.2，运行时来源）
- **URL**: https://github.com/astral-sh/python-build-standalone
- **许可证**: Python-2.0 / PSF（CPython 自身许可）
- **使用方式**: 按需下载自包含 CPython 3.12.15 作为分轨运行时；**按固定 SHA256 校验**后才使用（见 `core/stem_engine/runtime.rs`）。不随安装包分发，仅在用户启用分轨时下载。

### htdemucs 模型权重（v1.2，运行时资产）
- **URL**: https://dl.fbaipublicfiles.com/demucs/hybrid_transformer/955717e8-8726e21a.th
- **许可证**: MIT（Demucs 项目）
- **使用方式**: 由 Rust 端主动下载并按固定 SHA256 校验，落位到 `<runtime>/torch-home/hub/checkpoints/`，使分轨装完即可离线运行。

### HeroUI v3（`@heroui/react` 3.2.6，v1.3）
- **URL**: https://github.com/heroui-inc/heroui
- **许可证**: MIT
- **使用方式**: 全站 UI 组件库（表格 / 模态 / 表单 / 反馈等）。基于 React Aria（无障碍）+ Tailwind CSS v4，无 CSS-in-JS 运行时。
- **说明**: 要求 React ≥19 与 Tailwind ≥4，故本版本一并升级了 React 与样式栈。

### Tailwind CSS v4（`tailwindcss` / `@tailwindcss/vite` 4.3.3，v1.3）
- **URL**: https://github.com/tailwindlabs/tailwindcss
- **许可证**: MIT
- **使用方式**: 原子化样式引擎（Vite 插件形态）。HeroUI 的样式基于它构建。
- **说明**: v4 采用 CSS-first 配置（`@import "@heroui/styles"`），不再需要 `tailwind.config.js`。

### React 19（`react` / `react-dom` 19.3.1，v1.3）
- **URL**: https://github.com/facebook/react
- **许可证**: MIT
- **使用方式**: UI 框架。由 18.3.1 升级而来（HeroUI v3 要求 ≥19）。

### Vite 8（`vite` 8.3.4 + `@vitejs/plugin-react` 6.1.2，v1.3）
- **URL**: https://github.com/vitejs/vite
- **许可证**: MIT
- **使用方式**: 前端构建工具。v8 使用 Rolldown 内核，构建更快；对缺失导出等错误更严格（不再静默放过）。
- **说明**: 由 5.4.10 升级而来。

### 未采用的依赖：tauri-plugin-updater（v1.3 评估后放弃）
- **URL**: https://github.com/tauri-apps/plugins-workspace
- **评估结论**: **当前不可用**
- **原因**: 该插件依赖 `zip` crate（`2.3–2.7` 要求 `zip ^2`，`2.8–3.0.0-alpha.3` 要求 `zip ^4`），
  而 `zip` 在 crates.io 上**除 9.0.3 外全部被 yank**（9.0.3 发布于 2026-10-13），
  导致 `^2` 与 `^4` 均无法解析。Cargo `[patch]` 无法把 `^4` 替换为 `9.x`（版本须满足原要求）。
- **替代方案**: 自研更新器（见 `docs/tdd-auto-update.md`），复用现有产物管线
  （`.zip` 安装包 + `SHA256`）与既有依赖（`reqwest` / `sha2` / `flate2` / `tar`），**不新增依赖**。
- **后续**: 上游修复后可迁移回官方插件以获得独立密钥对签名；清单与产物 URL 无需变动。
