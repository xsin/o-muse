# handsoff.md — Agent 执行索引与进度（人类无需访问）

> **用途**：本文件是 Agent 的自用执行索引。当切换 LLM、压缩上下文、或被新 Agent 接手时，从本文件即可恢复全局认知与待办，无需重新阅读 `plan.md` / `brd.md` / `prd.md` / `tdd.md`。
> **权威来源**：`plan.md`（人类撰写原案）、`docs/adr/*`（已固化决策）、`GLOSSARY.md`（术语）。本文件是对它们的**执行视角压缩**，不替代原案。
> **进度约定**：每完成一个 Task，将下方"进度表"对应行 `status` 改为 `done` 并提交 git（commit message `[Task N] <描述>`）。

---

## 0. 进度表（压缩恢复锚点）

| Task | 范围 | status | commit |
|---|---|---|---|
| Task 0 | 仓库初始化 + 许可证合规 | done | `83eb419` |
| Task 0.x | 方向纠正：独立实现，移除 StemDeck 源码依赖 | done | `00fc7cc` |
| Task 0.y | 项目自身许可证 Apache-2.0：LICENSE + NOTICE 归属 + README | done | — |
| Task 1 | Rust 后端骨架 | done | `18f58b7` |
| Task 2 | Python Sidecar（分轨引擎） | done | `f472cc5` |
| Task 3 | Kontakt 库管理（只读盘点） | done | `57bea16` |
| Task 4 | 前端骨架 + 路由 + 工具箱壳 | done | `[Task 4]` |
| Task 5 | Free/Pro 判定 + **Dodo 校验** ⚠️ | done | `[Task 5]` |
| Task 6 | v1 集成测试 + 打包 | pending | — |
| Task 7 | 插件目录自动发现 (v1.1) | pending | — |
| Task 8 | 插件元数据 + 缓存 (v1.1) | pending | — |
| Task 9 | 自定义目录 + 前端集成 (v1.1) | pending | — |
| Task 10 | v1.1 集成测试 + 性能 (v1.1) | pending | — |

**当前应执行**：Task 6（v1 集成测试 + 打包；前置 Task 0–5 完成）。

> **Task 0 执行备注（偏差记录）**：plan §三 的 `Task 0` 原意是把 StemDeck 直接 `rsync` 进仓库根目录；
> 但本工作区已含我们自己产出的 `README.md`/`plan.md`/`brd/prd/tdd.md`/`docs/`/`prototype/` 等交付物，
> 根级 rsync 会覆盖 `README.md`（含逐字资金声明）并污染根目录。鉴于锁定架构为 **Rust 重新实现**（非 StemDeck fork），
> 改为将上游源码导入隔离目录 `vendor/stemdeck/`（保留全部原始文件与版权头、零 `.git` 残留），以供归属/可追溯性核查。
> 其余 Task 0 步骤严格按 plan 执行：`licenses/{APACHE-2.0.txt,MIT.txt,NOTICE}`、`README.md` 含逐字资金声明、
> `.gitignore`（含 `.env`）、NOTICE 内 commit hash = `5f9405ca68ac31b31ae07ac416141d4d1f7b5f3a`、单次 `[Task 0]` commit（无原始 history）。
> 工作区 = `/Volumes/Pluto/dev/proj/MuseRig`（plan 中的 `~/projects/music-rig` 为旧路径，已弃用）。

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

### Task 1 — Rust 后端骨架（前置：Task 0）✅ done
- 建 `src-tauri/src/{commands,core/library_manager,core/plugin_scanner,core/license}` 与各 `mod.rs`（含 `placeholder()`）。
- 按方向纠正，分轨命令模块命名为 `commands/stem_separation.rs`（非 plan 原写的 `commands/stemdeck.rs`），并新增 `core/stem_engine/{sidecar,task}.rs` 承载 sidecar 生命周期与任务模型（参考 StemDeck 设计思路，未复制源码）。
- `Cargo.toml` 依赖见 §2（版本按 ADR-0001 升级；**quick-xml 0.42 / plist 1.10 / sha2 0.11 / rayon 1.12**）。
- `lib.rs` 注册 shell/store/sql 插件。
- 验收：`cargo check` 零错误（实际 0 warning / 0 error）。禁止写业务逻辑。

> **Task 1 执行备注（偏差记录）**：
> 1. **tauri 默认 features**：plan §三 Task 1 写 `tauri = { version = "2", features = [] }`。
>    但 Tauri 2 的 lib 在 `tauri::generate_context!()`/Builder 构造上依赖默认 `wry` 特性，
>    关闭 `features=[]` 会导致 `cargo check` 链接/构造失败。已改为保留默认 features 并锁定 `2.12.4`
>    （ADR-0001 指定的 v2 线）。属构建必要，未改动任何业务逻辑或插件语义。
> 2. **命令宏唯一性**：Tauri 2 的 `#[tauri::command]` 在 crate 根生成以函数名派生的宏
>    （`__cmd__placeholder` 等），四个 `placeholder` 命令会 `E0428` 冲突。已改各命令模块占位命令为
>    唯一命名（`stem_separation_placeholder` / `library_manager_placeholder` / `plugin_scanner_placeholder`
>    及顶层 `placeholder`），均在 `generate_handler!` 中注册。
> 3. **沙箱 CARGO_HOME**：DSH 工作区写沙箱禁止写入全局 `~/.cargo/registry`，`cargo` 报
>    `Operation not permitted`。已设 `CARGO_HOME=/Volumes/Pluto/dev/proj/MuseRig/.cargo-home`（在树内）
>    并加入 `.gitignore`。后续 cargo 命令需沿用该环境变量。
> 4. **cargo 退出码陷阱**：`cargo check 2>&1 | tail` 的管道会掩盖真实错误码；改用 `PIPESTATUS[0]` 取 cargo 真实退出码。

### Task 2 — Python Sidecar（分轨引擎）（前置：Task 1）✅ done
- 架构：Rust 通过 **tauri-plugin-shell 的 sidecar API** 启动 Python 服务（**不在 Rust 用 `std::process::Command`**），Rust↔Python 走 **localhost HTTP（reqwest blocking）**。
- `src-tauri/src/core/stem_engine/sidecar.rs`：真实 `ensure_sidecar()`（首次拉起 sidecar、后台消费 stdout/stderr、轮询 `/health` 就绪）、`kill_sidecar()`（退出清理，Task 7 用）。
- `src-tauri/src/core/stem_engine/task.rs`：`Error`/`StartResponse`/`StatusResponse`/`StemsResponse` 数据模型 + `start()`/`status()`/`stems()` HTTP 客户端。
- `src-tauri/src/commands/stem_separation.rs`：真实命令 `separate` / `stem_status` / `stem_results`（均经 sidecar HTTP）。模块名 `stem_separation`，代码内**零 `stemdeck` 字样**。
- `src-tauri/src/lib.rs`：`SidecarState(Mutex<Option<CommandChild>>)` 经 `manage()` 共享；`generate_handler!` 注册三个分轨命令。
- `python-backend/`：**独立实现** FastAPI 服务（`server.py` + `separator.py` + `demo.py` + `constants.py` + `main.py` + `requirements.txt`）。引擎默认 Demucs（MIT），**缺失时自动回退 demo 静音 WAV**，保证整条链在沙箱内可编译/联调，不复制任何第三方源码。
- `tauri.conf.json`：`bundle.externalBin: ["bin/python-stem-backend"]`；`src-tauri/bin/python-stem-backend-aarch64-apple-darwin` 为 dev 桩启动器（调用系统 `python3 main.py`，生产由打包流程替换为内嵌运行时）。
- 验收：Python 五文件 `py_compile` 全 OK；`cargo check` **0 error / 0 warning**（实际 EXIT=0）。
- 新增依赖记录：`docs/references.md`（Demucs/fastapi/uvicorn 等）。

> **Task 2 执行备注（偏差记录）**：
> 1. **externalBin 必须存在**：`tauri.conf.json` 声明 `externalBin` 后，Tauri build-script 强制要求对应平台二进制（如 `python-stem-backend-aarch64-apple-darwin`）存在，否则 `cargo check` 失败。已新增 `src-tauri/bin/` 下的 dev 桩启动器解决，`Cargo.lock` 一并生成。
> 2. **sidecar 名对齐**：`sidecar.rs` 中 `SIDECAR_NAME = "python-stem-backend"` 与 `externalBin` 注册名一致。
> 3. **HTTP 通信**：Rust 端 `reqwest` blocking；Python 端 `/health` 轮询上限 30s；`/status` 用 SSE 推流进度。

### Task 3 — Kontakt 库管理（只读盘点）（前置：Task 1）✅ done
- `core/library_manager/model.rs`：`LibraryEntry` / `Diagnostic`（见 `tdd.md` §3.2）。
- `parser.rs`：`parse_service_center_xml`（quick-xml 0.42）/ `parse_kontakt_plist`（plist 1.10）/ `parse_installed_products_json`（serde_json），默认路径经 `platform_paths`（ADR-0002）提供，macOS：`~/Library/Application Support/Native Instruments/Service Center/`、`~/Library/Preferences/com.native-instruments.Kontakt*.plist`。
- `diagnose.rs`：路径存在性、重复 SNPID、断卷（`/Volumes/` 不可访问）。
- `commands/library_manager.rs`：`scan_libraries() -> Result<Vec<LibraryEntry>, String>`（async）。
- `core/platform_paths.rs`：`native_instruments_service_center_dirs()` / `kontakt_preferences_dir()` / `kontakt_plist_files()`（已在 `core/mod.rs` 注册）。
- **仅读取**。验收：`cargo check` 0 error / 0 warning；`grep -ri stemdeck src-tauri python-backend` 为空。禁止写入/LaunchDaemon/拉 KLM 源码。
- 新增依赖记录：`docs/references.md`（quick-xml / plist）。

> **Task 3 执行备注（偏差记录）**：
> 1. **命令文件名**：原 plan 写 `commands/library.rs`，实际落地 `commands/library_manager.rs` 与模块 `core/library_manager` 对齐；旧占位命令 `library_manager_placeholder` 已删除。
> 2. **quick-xml 0.42 API**：`Reader::trim_text` 已移除（改为 `Config::trim_text`，文本值已手动 `.trim()`）；`BytesText`/`BytesCData` 经 `Deref<Target=str>` 取文本（无 `unescape()`）；`e.name()` 返回 `QName<&str>`（`AsRef<str>`）。已据此修正解析器。
> 3. **plist 1.10 API**：`plist::Value::from_bytes` 不存在，改用 `from_reader`（传入 `Cursor<Vec<u8>>`）。
> 4. **KLM 移植标注**：`parser.rs` 头部注明逻辑参考 KLM（MIT）且为独立重写，未复制源码。
> 5. **`kontakt_preferences_dir()` 命名偏差**：ADR-0002 原签名为 `kontakt_preferences_glob()`，本实现采用更精确的 `kontakt_preferences_dir() -> Option<PathBuf>` + `kontakt_plist_files()`，二者语义等价，待 ADR 同步。

### Task 4 — 前端骨架 + 工具箱壳（前置：Task 1）✅ done
- 用 React+TS+Vite 替换原 vanilla 前端；装 `@tauri-apps/api`、`plugin-shell`、`plugin-store`、`react-router-dom`、i18next 生态（ADR-0004）。
- **壳改为 PRD §6 模型**：左栏常驻「工具箱」默认项 + 已开 App 标签（多开，✕ 关闭）；右栏卡片网格（App Registry 渲染）。`zustand` store：`openApps`、`activeView`、`theme`、`lang`。
- **新增 i18n 初始化 + 双语资源骨架（zh/en）**、CSS 变量主题（`tokens.css` 亮/暗，系统跟随）；App Registry 文案走 i18n，不写死。
- `appRegistry.ts`（见 `tdd.md` §3.1）：`stem_separation`(启用)、`libraries`(启用)、`plugins`(v1 disabled "即将推出")。
- Library 页已接入 `invoke("scan_libraries")` 实际调用（非占位）；Stem/Plugin 页为 UI 占位。
- 验收：`npm run build` 通过 + `npm run typecheck` 0 错误 + `cargo check` 仍 0 错误；`grep -ri stemdeck src-tauri src python-backend` 空。
- **偏差记录（Task 4）**：
  - 前端栈从 plan 的 React19/Vite8/TS7 **回退到 React18.3 / Vite5.4 / TS5.6**（tdd.md §7 允许的兜底），因前沿组合与 `@tauri-apps/cli`/Tauri 模板存在真实摩擦风险；记录于此。
  - **强制命名**：appRegistry 的 id/route 统一为 `stem_separation`（非 `stemdeck`），符合"代码不得出现 stemdeck"硬规范；tdd.md §3.1 示例里的 `id:'stemdeck'` 仅为示例，以本实现为准。
  - `tauri.conf.json`：`beforeDevCommand= npm run dev`、`beforeBuildCommand= npm run build`、`frontendDist="../dist"`（devUrl 仍 5173）。
  - npm 缓存因沙箱拒写 `~/.npm`，改设 `npm_config_cache=.npm-cache/`（已 gitignore）。

### Task 5 — Free/Pro 判定 + **Dodo 校验**（前置：Task 1）✅ done
- `core/license/mod.rs`：`LicenseTier{Free,Pro}`；`current_tier(app) async` 优先读 `tauri-plugin-store` TTL 缓存（24h）→ 否则读 `license_key` → **Dodo API** 校验（`DODO_API_KEY`+`DODO_PRODUCT_ID`，`core/license/dodo.rs` 自研）→ 推导；无 key/失败→`Free`（写回缓存）。`activate_license(app,key) async` 写入 key + 校验 + 缓存。
- Pro 专属 Command 入口统一经 `require_pro(app)` 拦截：`if !Pro { return Err("此功能需要 Music Rig Pro 订阅") }`。已落地示例：`commands/library_manager.rs::repair_library`（Pro 占位）。
- 新增命令：`get_tier`、`activate_license_key`（前端设置页可激活并展示 tier）。
- 验收：`cargo test --lib license` 通过（5 个解析用例）；`cargo check` 0 错误/0 警告；前端 `typecheck`/`build` 通过。⚠️ 密钥经 `.env`（gitignore），本地/构建注入，不进 git。
- **偏差记录（Task 5）**：Dodo 校验采用保守解析（兼容 `items`/`subscriptions` 数组与单对象，active/trialing 视为有效），任何网络/解析异常一律回退 Free，不误判为 Pro。Pro 实际写入逻辑（mutate.rs）推迟至 v1.1，本 Task 仅建立拦截边界与占位命令。

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
