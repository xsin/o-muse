# handsoff.md — Agent 执行索引与进度（人类无需访问）

> **用途**：本文件是 Agent 的自用执行索引。当切换 LLM、压缩上下文、或被新 Agent 接手时，从本文件即可恢复全局认知与待办，无需重新阅读 `plan.md` / `brd.md` / `prd.md` / `tdd.md`。
> **权威来源**：`plan.md`（人类撰写原案）、`docs/adr/*`（已固化决策）、`GLOSSARY.md`（术语）。本文件是对它们的**执行视角压缩**，不替代原案。
> **进度约定**：每完成一个 Task，将下方"进度表"对应行 `status` 改为 `done` 并提交 git（commit message `[Task N] <描述>`）。

---

## 0. 进度表（压缩恢复锚点）

| Task | 范围 | status | commit |
|---|---|---|---|
| Task 0 | 仓库初始化 + 许可证合规 | pending | — |
| Task 1 | Rust 后端骨架 | pending | — |
| Task 2 | Python Sidecar（分轨引擎） | pending | — |
| Task 3 | Kontakt 库管理（只读盘点） | pending | — |
| Task 4 | 前端骨架 + 路由 + 工具箱壳 | pending | — |
| Task 5 | Free/Pro 判定 + **Dodo 校验** ⚠️ | pending | — |
| Task 6 | v1 集成测试 + 打包 | pending | — |
| Task 7 | 插件目录自动发现 (v1.1) | pending | — |
| Task 8 | 插件元数据 + 缓存 (v1.1) | pending | — |
| Task 9 | 自定义目录 + 前端集成 (v1.1) | pending | — |
| Task 10 | v1.1 集成测试 + 性能 (v1.1) | pending | — |

**当前应执行**：Task 0（仓库尚未 `git init`，工作区 = `/Volumes/Pluto/dev/proj/MuseRig`）。

---

## 1. 已锁定决策（grilling 八问 + 第二轮五问）

| # | 决策 | 落地文档 |
|---|---|---|
| Q1 | 技术栈升级至最新稳定版（Tauri 保持 v2 线） | ADR-0001 / `tdd.md` §1 |
| Q2 | 平台：v1 macOS only，v1.5 Windows | `brd.md` §4 / ADR-0002 |
| Q3 | `plan.md` 转为本 `handsoff.md`（Agent 自用，人不用看） | — |
| Q4 | 双层：首页"工具箱"启动器 + 各 App 独立页 | `prd.md` §6 |
| Q5 | 首页可见标签"工具箱"（战略仍叫 App Marketplace） | `GLOSSARY.md` |
| Q6 | 卡片用功能名；Plugin Manager v1 标"即将推出" | `prd.md` FR-1 |
| Q7 | 本轮先出文档 + 首页原型，不执行 Task 0 | 本回合交付物 |
| Q8 | 仓库落地 `/Volumes/Pluto/dev/proj/MuseRig`（非 `~/projects/music-rig`） | — |
| Q9 | 轻量 App Registry（前端常量动态渲染） | `tdd.md` §3.1 / `prd.md` FR-1 |
| Q10 | **v1 即接 Dodo 真实校验**（Test Key 已备）—覆盖 plan Task 5 | ADR-0003 / `tdd.md` §3.4 |
| Q11 | 现在做跨平台抽象（`platform_paths` 薄层，v1 仅 macOS 分支） | ADR-0002 / `tdd.md` §2,§3.3 |
| Q12 | 左栏常驻；开 App 追加标签；**支持多 App 同时打开** | `prd.md` §4 FR-2 / §6.3 |
| Q13 | 术语：Music Rig 工具单元 = **App**；音频格式 = **Audio Plugin** | `GLOSSARY.md` |

---

## 2. 技术栈（升级后，见 ADR-0001）

- 桌面壳：**Tauri 2.12.4**（crate）/ 2.12.3（js/cli）；插件 shell/store/sql = 2.5.0/2.5.0/2.6.0
- 后端：**Rust**（新增逻辑全 Rust）+ **Python 3.14** Sidecar（fastapi 0.143.1 / uvicorn 0.54.0 / demucs 4.1.0 / librosa 1.0.0 / pyloudnorm 0.2.0）
- 前端：**React 19.3.1** + **TS 7.0.2** + **Vite 8.3.4** + **TailwindCSS 4** + **react-router-dom 7.18.4** + **Zustand 5.0.15** + **i18next 26.4.5 / react-i18next 17.0.16 / detector 8.2.1**（ADR-0004，中英双语、默认随系统）
- Rust 辅助（升级）：serde 1.0.229 / quick-xml 0.42.0 / plist 1.10.1 / chrono 0.4.45 / walkdir 2.5.0 / rayon 1.12.0 / **sha2 0.11.1（⚠️ breaking）** / thiserror 2.0.21
- 计费：**Dodo Payments**（`.env`: `DODO_PRODUCT_ID`, `DODO_API_KEY` Test Mode，已备）

> ⚠️ 升级风险：sha2 0.11 改 hash 调用；React19/Vite8/TS7 若与 Tauri 模板摩擦，兜底回退 React18+Vite5 并在该 Task 记录。

---

## 3. 全局禁止（plan §二，执行任何 Task 前复读）

- 不运行 `sudo`/`su`/提权；不调用 `powershell`、`bash -c` 执行外部命令。
- Rust 中不得用 `std::process::Command` 启动**非 Sidecar** 进程（Python sidecar 除外）。
- 不修改 `licenses/`（`APACHE-2.0.txt`/`MIT.txt`/`NOTICE`）；不删除复制文件的 `Copyright` 头。
- Pro 模块（`mutate.rs`、自定义目录）**不得含 Apache-2.0 原始代码**；`Cargo.toml` 不引入 GPL/AGPL。
- 不恢复 StemDeck Git 历史；不 `git clone --mirror` / `fetch --unshallow`；不留 submodule 引用。
- 不要提交 `.env`（含 DODO_* 密钥）→ 加入 `.gitignore`，提供 `.env.example` 模板。

---

## 4. Task 索引（压缩版，细节回 `plan.md`）

### Task 0 — 仓库初始化 + 许可证合规（前置：无）
- `git clone --depth 1 https://github.com/stemdeckapp/stemdeck.git /tmp/stemdeck-src`，记 `git rev-parse HEAD`。
- 工作区 `git init -b main`；`rsync -av --exclude='.git'` 导入；清残留 `.git*`。
- `licenses/APACHE-2.0.txt`（原 LICENSE）、`MIT.txt`、`NOTICE`（记 commit hash）。
- `README.md` 含资金声明（逐字）；`.gitignore`（含 `.env`）；首次 commit `[Task 0]`。
- 验收：无 stemdeckapp 原始 commit；三许可证文件在；NOTICE 含 StemDeck+KLM；README 含"不接受任何资金"；无残留 `.git`；版权头保留。

### Task 1 — Rust 后端骨架（前置：Task 0）
- 建 `src-tauri/src/{commands,core/library_manager,core/plugin_scanner,core/license}` 与各 `mod.rs`（含 `placeholder()`）。
- `Cargo.toml` 依赖见 §2（版本按 ADR-0001 升级；**quick-xml 0.42 / plist 1.10 / sha2 0.11 / rayon 1.12**）。
- `lib.rs` 注册 shell/store/sql 插件。
- 验收：`cargo check` 零错误。禁止写业务逻辑。

### Task 2 — Python Sidecar（分轨引擎）（前置：Task 1）
- `python-backend/main.py`：FastAPI `POST /separate`、`GET /status/{id}`(SSE)、`GET /stems/{id}`。
- `requirements.txt`（fastapi/uvicorn/demucs/librosa/pyloudnorm）。
- `commands/stemdeck.rs`：`start_separation(file_path) -> Result<String>` via **sidecar API**（不在 Rust 直调 python）。
- `tauri.conf.json` `bundle.externalBin` 配置；壳退出时终止 Python 进程。
- 验收：py import OK + `cargo check`。

### Task 3 — Kontakt 库管理（只读盘点）（前置：Task 1）
- `core/library_manager/model.rs`：`LibraryEntry` / `Diagnostic`（见 `tdd.md` §3.2）。
- `parser.rs`：`parse_service_center_xml` / `parse_plist` / `parse_installed_products_json`，默认路径经 `platform_paths`（ADR-0002）提供，macOS：`~/Library/Application Support/Native Instruments/Service Center/`、`~/Library/Preferences/com.native-instruments.Kontakt*.plist`。
- `diagnose.rs`：路径存在性、重复 SNPID、断卷（`/Volumes/` 不可访问）。
- `commands/library.rs`：`scan_libraries() -> Result<Vec<LibraryEntry>>`。
- **仅读取**。验收：`cargo test --lib library_manager` 通过。禁止写入/LaunchDaemon/拉 KLM 源码。

### Task 4 — 前端骨架 + 工具箱壳（前置：Task 1）
- 用 React+TS+Vite 替换 StemDeck 原 vanilla JS 前端；装 `@tauri-apps/api`、`plugin-shell`、`plugin-store`、`react-router-dom`、i18next 生态（ADR-0004）。
- **壳改为 PRD §6 模型**：左栏常驻「工具箱」默认项 + 已开 App 标签（多开，✕ 关闭）；右栏卡片网格（App Registry 渲染）。
- **新增 i18n 初始化 + 双语资源骨架（zh/en）**、CSS 变量主题（亮/暗，系统跟随）；App Registry 文案走 i18n，不写死。
- `appRegistry.ts`（见 `tdd.md` §3.1）：`stemdeck`(启用)、`libraries`(启用)、`plugins`(v1 disabled "即将推出")。
- `zustand` store：`openApps`、`activeView`。
- 验收：`npm run build` 通过。禁止写 Tauri Command 调用。

### Task 5 — Free/Pro 判定 + **Dodo 校验**（前置：Task 1）⚠️ ADR-0003 覆盖原"不做网络校验"
- `license/mod.rs`：`LicenseTier{Free,Pro}`；`current_tier() async` 读 `store.license_key` → **Dodo API** 校验（`DODO_API_KEY`+`DODO_PRODUCT_ID`）→ 推导；无 key/失败→`Free`（带 TTL 缓存）。
- Pro 专属 Command 入口统一拦截：`if current_tier()!=Pro { return Err("此功能需要 Music Rig Pro 订阅") }`。
- Pro 范围：`mutate.rs`（库写入/修复）、`add_custom_source`（v1.1）。
- 验收：`cargo test --lib license`；Free 下 Pro 命令报错。**新增**：激活后 Pro 命令成功。
- ⚠️ 密钥经 `.env`（gitignore），本地/构建注入。

### Task 6 — v1 集成测试 + 打包（前置：Task 0–5）
- `tests/integration.rs`：扫描模拟 XML、调用 `scan_libraries` 不 panic、Free 下 Pro 拒绝。
- `tauri.conf.json` `productName:"Music Rig"`；`npm run tauri build` 产 `.dmg/.app`。
- 验收：`cargo test && npm run tauri build`；产物名 Music Rig。禁止 CI 引入无缓存网络。

### Task 7 — 插件目录自动发现 (v1.1)（前置：Task 6）
- `plugin_scanner/model.rs`：`ScanSource`/`PluginFormat{AU,VST,VST3,AAX,CLAP,Auto}`/`PluginInfo`（见 `tdd.md` §3.3）。
- `discover.rs`：`default_sources()` 委托 `platform_paths::default_plugin_scan_sources()`（macOS 标准目录见 `tdd.md`）；`discover_plugins()` 用 walkdir 按扩展名过滤，不读元数据。
- 验收：`cargo test --lib plugin_scanner::discover`。

### Task 8 — 插件元数据 + 增量缓存 (v1.1)（前置：Task 7）
- `extract.rs`：AU/VST3 读 `Contents/Info.plist`；VST2 二进制头；AAX plist；CLAP plist/二进制。VST3 用 `std::thread::spawn` 包裹避免阻塞主线程。
- `cache.rs`：SQLite `plugin_cache`（见 `tdd.md`）；`file_size+last_modified` 哈希命中直接返回。
- 验收：`cargo test --lib plugin_scanner::extract && plugin_scanner::cache`。**禁止 dlopen/LoadLibrary**。

### Task 9 — 自定义目录 + 前端集成 (v1.1)（前置：Task 8）
- `discover.rs`：`add_custom_source`/`remove_custom_source`（Pro 专属，持久化 store）。
- `commands/plugins.rs`：`scan_all_plugins`、`add_custom_plugin_dir`、`list_scan_sources`。
- `PluginManagerPage`：源列表/添加目录（Tauri dialog）/主表格/搜索/扫描进度。
- 验收：`npm run build && cargo check`。

### Task 10 — v1.1 集成测试 + 性能 (v1.1)（前置：Task 9）
- 模拟目录；`default_sources()` ≥8 目录；扫描数量正确；二次缓存命中 100%。
- 性能：50 插件首扫 <2s、缓存 <200ms。
- 验收：`cargo test --test plugin_integration` 通过且基准达标。禁止依赖真实插件。

---

## 5. Free/Pro 边界（plan §五，不变）

分轨✅✅｜库只读盘点✅✅｜库写入/修复❌✅｜插件标准目录✅✅｜插件自定义目录❌✅｜插件元数据完整视图✅✅｜插件预设管理(v1.2+)❌✅。
原则：Free/Pro 共享 `core/` 只读基础（Apache/MIT）；Pro 走独立自研扩展模块。

## 6. 许可证合规清单（每 Task 后勾选）

- [ ] 三许可证文件未改；[ ] 复制文件保留 `Copyright` 头；[ ] KLM 移植逻辑顶部注明 MIT 来源；[ ] Pro 模块不含 Apache 原始代码；[ ] README 含资金声明；[ ] NOTICE 含 StemDeck commit hash；[ ] `git log` 无 stemdeckapp 原始 commit；[ ] 无残留 submodule；[ ] `Cargo.toml` 无 GPL/AGPL。
