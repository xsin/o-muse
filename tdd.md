# TDD — Technical Design Document: O Muse

> 读者：实现者 / Agent（配合 `brd.md` / `prd.md`）。
> 本文件聚焦**怎么做**：技术栈版本、目录结构、关键模块契约、命令签名、许可证合规、跨平台与 Dodo 集成。

## 1. 技术栈（已升级，见 ADR-0001）

| 层 | 技术 | 版本 | 备注 |
|---|---|---|---|
| 桌面壳 | Tauri 2 | crate 2.12.4 / js 2.12.3 / cli 2.12.3 | v3 仍 alpha，保持 v2 线 |
| 插件 | plugin-shell / store / sql | 2.5.0 / 2.5.0 / 2.6.0 | sql 用 `features=["sqlite"]` |
| 后端主逻辑 | Rust | 1.99 | 新增模块全用 Rust |
| 分轨引擎 | Python 3.14 + FastAPI | fastapi 0.143.1 / uvicorn 0.54.0 / demucs 4.1.0 / librosa 1.0.0 / pyloudnorm 0.2.0 | Sidecar 进程 |
| 前端 | React 19 + TS 7 + Vite 8 + TailwindCSS 4 | react 19.3.1 / react-dom 19.3.1 / react-router-dom 7.18.4 / vite 8.3.4 / @vitejs/plugin-react 6.1.2 / typescript 7.0.2 / zustand 5.0.15 | |
| 本地存储 | SQLite via tauri-plugin-sql | 2.6.0 | |
| 计费 | Dodo Payments | — | Test Mode Key 已备（ADR-0003） |
| 国际化 | i18next + react-i18next + browser-languagedetector | i18next 26.4.5 / react-i18next 17.0.16 / detector 8.2.1 | zh-CN + en-US，默认随系统（ADR-0004） |
| 主题 | CSS 变量 + `data-theme`，系统跟随 | — | 亮/暗双主题（PRD FR-9） |

Rust 辅助依赖（按 ADR-0001 升级）：`serde 1.0.229`、`serde_json 1`、`quick-xml 0.42.0`、`plist 1.10.1`、`chrono 0.4.45`、`walkdir 2.5.0`、`rayon 1.12.0`、`sha2 0.11.1`（⚠️ breaking）、`thiserror 2.0.21`。

## 2. 目录结构（最终形态，沿用 plan §1.5）

```
music-rig/
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs                 # Tauri 入口 + 插件注册
│   │   ├── commands/              # Tauri Command 暴露层
│   │   │   ├── mod.rs
│   │   │   ├── stem_separation.rs  # 分轨命令（调 Python sidecar）
│   │   │   ├── library.rs         # Kontakt 库管理命令
│   │   │   └── plugins.rs         # 插件扫描命令（v1.1）
│   │   ├── core/                  # 纯 Rust 业务逻辑（无 Tauri 依赖）
│   │   │   ├── mod.rs
│   │   │   ├── platform_paths.rs  # 【新增, ADR-0002】跨平台路径抽象
│   │   │   ├── library_manager/
│   │   │   │   ├── mod.rs model.rs parser.rs diagnose.rs mutate.rs(Pro)
│   │   │   ├── plugin_scanner/    # v1.1
│   │   │   │   ├── mod.rs model.rs discover.rs extract.rs cache.rs
│   │   │   └── license/mod.rs     # Free/Pro 判定 + Dodo 校验（ADR-0003）
│   │   ├── Cargo.toml
│   │   └── tauri.conf.json
│   ├── src/                       # React 前端
│   │   ├── pages/ (StemDeck/, LibraryManager/, PluginManager/)
│   │   ├── components/  stores/   appRegistry.ts  # 【新增, Q9 轻量注册表】
│   │   ├── i18n/ (index.ts, locales/en.json, locales/zh.json)  # 【新增, ADR-0004】
│   │   └── styles/ (tokens.css)   # CSS 变量令牌（亮/暗主题）
│   ├── python-backend/ (main.py, demucs_runner.py, requirements.txt)
│   ├── licenses/ (APACHE-2.0.txt, MIT.txt, NOTICE)
│   ├── .env.example               # DODO_* 模板（.env 本身 gitignore）
│   ├── .gitignore                 # 含 .env, node_modules, target, dist
│   └── README.md
```

## 3. 关键模块契约

### 3.1 App Registry（前端，Q9）
`src/appRegistry.ts`：
```ts
export interface AppDef {
  id: 'stem_separation' | 'libraries' | 'plugins';
  name: string;          // "StemDeck" / "Kontakt Library Manager" / "Audio Plugin Manager"
  brandPath: string;     // "O Muse / Separate" 等
  route: string;         // "/apps/stem_separation" ...
  icon: string;          // svg path / asset
  enabled: boolean;      // plugins=false in v1
  minVersion: string;    // "1.0" / "1.1"
  badge?: 'soon';
}
```
首页与左栏均从此渲染；新增 App 只改此常量。

### 3.2 library_manager（Task 3）
```rust
pub struct LibraryEntry { pub snpid, name, path: Option<PathBuf>, registered: bool, diagnostics: Vec<Diagnostic> }
pub enum Diagnostic { MissingPath(PathBuf), DuplicateSnpid(String), DisconnectedVolume(PathBuf), MissingMetadata(String) }
// parser: parse_service_center_xml / parse_plist / parse_installed_products_json
// 默认路径来源改调 platform_paths（ADR-0002），而非写死
#[tauri::command] pub async fn scan_libraries() -> Result<Vec<LibraryEntry>, String>
```

### 3.3 plugin_scanner（v1.1, Task 7/8/9）
```rust
pub enum PluginFormat { AU, VST, VST3, AAX, CLAP, Auto }
pub struct ScanSource { path, plugin_format, recursive, enabled, is_custom }
pub struct PluginInfo { name, vendor, version, format, path, bundle_id, last_modified }
// default_sources() 委托 platform_paths::default_plugin_scan_sources()
// cache: SQLite plugin_cache 表（path PK + file_size+last_modified 哈希命中）
// 禁止 dlopen/LoadLibrary；VST3 提取包 std::thread::spawn
```

### 3.4 license（Task 5，ADR-0003 覆盖）
```rust
pub enum LicenseTier { Free, Pro }
pub async fn current_tier() -> LicenseTier {
  // 读 store.license_key → Dodo API(DODO_API_KEY, DODO_PRODUCT_ID) 校验 → Pro/Free
  // 无 key / 网络失败 → Free（带 TTL 缓存上次结果）
}
// Pro 专属 Command 入口统一拦截
```

### 3.5 stem_engine::runtime（v1.2，分轨运行时按需自举）

分轨依赖 PyTorch。体积对比：应用本体 ~21MB，而运行时合计
**~259MB 下载 / ~900MB 落盘**（自包含 CPython 24MB + 依赖 ~151MB + htdemucs 权重 ~84MB）。
故**不随包分发**，首次使用时自举到用户目录。

```rust
// core/stem_engine/runtime.rs
pub enum RuntimeStage { Missing, Checking, DownloadingPython, Verifying, Extracting,
                        CreatingVenv, InstallingDeps, DownloadingModel, Ready, Failed }
pub struct RuntimeStatus { stage, progress, message, python_path, size_bytes, estimated_download_bytes }
pub fn detect(app) -> RuntimeStatus            // 不发网络请求
pub fn install(app, emit) -> Result<RuntimeStatus>   // 阻塞式，调用方放 spawn_blocking
pub fn remove(app) -> Result<()>
pub const PROGRESS_EVENT: &str = "omuse://runtime-progress";   // 进度经事件推送
```

**目录布局**（`<app data>` 即 `~/Library/Application Support/work.xsin.omuse/`）：

```text
runtime/
  .ready                       # 就绪标记：全部步骤成功后才写入
  python/bin/python3           # 自包含 CPython 3.12.15
  venv/bin/python3             # 隔离环境（分轨实际使用）
  torch-home/hub/checkpoints/955717e8-8726e21a.th   # htdemucs 权重
```

**7 步流程**：下载 CPython → SHA256 校验 → 解压（纯 Rust tar/flate2）→ 建 venv →
`pip install --only-binary=:all: -r requirements-runtime.txt` →
下载 htdemucs 权重并校验 → 自检（导入 + 加载模型并断言 4 条分轨）。

**关键设计点**：

- **就绪判定用 `.ready` 标记**，而非"目录存在"。未就绪时重新安装会先清理半成品，
  避免下载中断被误判为已完成。
- **numpy 必须显式声明**：demucs 4.1.0 仅在 `darwin + x86_64` 时才声明 numpy，
  arm64 上不安装，而 `demucs/transformer.py` 直接 `import numpy as np`，
  会导致 Apple Silicon 分轨必然失败。`requirements-runtime.txt` 显式加
  `numpy>=1.26,<2`（`<2` 同时满足 demucs 在 x86_64 上的约束，两架构通用）。
- **模型权重由 Rust 端主动下载并校验**，而非交给 demucs 首次分轨时用 Python
  urllib 拉取：可控制、可校验、可显示进度，且**装完即可完全离线**分轨。
- **`TORCH_HOME` 注入**：启动 sidecar 时指向 `<runtime>/torch-home`，
  避免 torch 写入 `~/.cache/torch`（否则卸载无法完整回收，且污染用户主目录）。
- **安全**：CPython 与权重均按**固定 SHA256** 校验；依赖强制 `--only-binary=:all:`
  只走 wheel（避免执行源码包的构建脚本）；解压用 Rust 库；外部进程统一经
  `tauri-plugin-shell`（不使用 `std::process::Command`）。
- **磁盘余量前置检查**：不足 900MB 直接报错并给出实际可用值。
- **下载原子性**：先写 `.part` 再改名，校验失败即清理，不留半个文件。
- **`OMUSE_RUNTIME_DIR` 环境变量**可覆盖运行时根目录（供测试与高级用户自定义）。

**解释器选择顺序**（`src-tauri/bin/python-stem-backend-<triple>` 启动器）：
托管运行时（`OMUSE_RUNTIME_DIR` 注入）→ 项目内 venv（开发态）→ 系统 `python3`。
注意 macOS 自带仅 Python 3.9.6（且为 Xcode CLT 转发桩），而当前 PyTorch 需 3.10+，
故系统 Python **不满足**依赖，仅为兜底。

**sidecar 失败直报**：捕获子进程 stderr 尾部；进程秒退时立即失败并回传真实原因
（缺运行时/缺依赖/缺源码），不再干等 30s 健康检查超时。

**依赖双文件**：
- `python-backend/requirements.txt` —— 开发态松散约束（含可选/预留依赖）
- `python-backend/requirements-runtime.txt` —— 运行时精简锁定集

精简理由：`librosa` / `pyloudnorm` / `soundfile` 当前代码未使用（原为响度对齐预留），
其中 librosa 会引入 numba/llvmlite 等大体积包，故运行时不再安装。

**测试覆盖**（`cargo test --lib runtime`，9 例）：URL 与 SHA 自洽性、路径布局、
`.ready` 语义、解压往返，以及**hermetic 下载校验**（本地一次性 HTTP 服务器：
正常下载 + 哈希不匹配时拒绝并清理临时文件）。

## 4. 首页/壳层实现（PRD §6）
- 左栏常驻（232px）；`market` 默认项 + 动态 `openApps` 标签（多开，✕ 关闭）。
- `zustand` store：`openApps: AppId[]`、`activeView: 'market' | AppId`。
- 路由 `react-router-dom` v7；原 plan Task 4 顶栏三页签改为本多开左栏模型。

## 4.1 国际化实现（ADR-0004 / PRD FR-8）
- `src/i18n/index.ts`：`i18next.init`，`react-i18next` 注入，`browser-languagedetector` 检测顺序 `['localStorage','navigator']`（持久化键 `ui_lang`）。
- 文案资源：`src/i18n/locales/{en,zh}.json`，命名空间 `translation`；壳层、卡片、App 名均走 `t('apps.stem_separation.name')` 等。
- App Registry 的 `appRegistry.ts` 仅存 `id / route / icon / enabled / minVersion`，**不存中文/英文字符串**，字符串全部从 i18n 取。
- 默认语言逻辑：store 有 `ui_lang` → 用之；否则系统 `zh-CN`→`zh`，其余→`en`（回退 `en-US`）。
- 左栏「设置」提供语言切换；切换写入 store 并 `i18n.changeLanguage()`，即时生效。

## 4.2 主题实现（PRD FR-9）
- `src/styles/tokens.css` 定义 `:root`（亮）与 `[data-theme="dark"]`（暗）的全套 `--n-*`/`--brand` 令牌，沿用参考 dashboard 配色。
- `document.documentElement.dataset.theme` 初始化：store `ui_theme` → 系统 `prefers-color-scheme` → 默认亮。
- 主题仅 CSS 变量作用域，无 JS 重渲染；切换写 store 并持久化，原型 `prototype/home.html` 已验证交互。

## 5. 许可证合规（plan §六，强制）
- `licenses/APACHE-2.0.txt`、`MIT.txt`、`NOTICE` 不可修改；NOTICE 记 StemDeck commit hash。
- 复制自 StemDeck 的文件保留原始 `Copyright` 头（`grep -r "Copyright" src-tauri/`）。
- KLM 移植逻辑顶部注释 `// Ported from KLM (https://github.com/cloinse/klm) under MIT License`。
- Pro 模块不含 Apache-2.0 原始代码；`Cargo.toml` 不引入 GPL/AGPL。
- `README.md` 含"原项目不接受资金支持"声明；`git log` 无 stemdeckapp 原始 commit。

## 6. 全局禁止（plan §二）
- 不运行 `sudo`/`su`/提权；不调用 `powershell`、`bash -c` 执行外部命令。
- Rust 中不得用 `std::process::Command` 启动非 Sidecar 进程（Python sidecar 除外）。
- 不修改 `licenses/`；Pro 模块不引用 Apache-2.0 原始文件；不恢复 StemDeck Git 历史。

## 7. 执行顺序与升级风险
- 逐 Task 执行（见 `handsoff.md`）：Task 0→1→2→3→4→5→6（v1），Task 7→8→9→10（v1.1）。
- 升级风险（ADR-0001）：`sha2 0.11` 改 hash 调用；React19/Vite8/TS7 若与 Tauri 模板摩擦，兜底回退 React18+Vite5，并在该 Task 记录。
- Dodo 集成（ADR-0003）替代 plan Task 5 的"不做网络校验"。
