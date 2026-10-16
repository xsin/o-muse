# O Muse — AI Agent 完整技术执行方案 v1.0

**目标读者**：AI Agent（Claude Code / Cursor / Windsurf / Codex）
**执行模式**：逐任务执行，每完成一个 Task 后运行指定验证命令，验证通过再进入下一 Task。
**禁止事项**：不要在任何 Task 中自行改变架构决策、技术选型或许可证策略。遇到歧义时，在代码中写入 `// AGENT_QUESTION: <问题>` 并停止，等待人工确认。


## 一、项目总览

### 1.1 产品定位

**O Muse** 是一款面向音乐人的本地桌面助手，定位为**音乐人必备的瑞士军刀 / 全能工具箱**。v1 首发两个核心工具：**音频分轨**（本地 Demucs 引擎）与 **Kontakt 音色库管理**（独立 Rust 实现），v1.1 扩展**音频插件（AU/VST/VST3/AAX/CLAP）扫描与管理**。品牌层不绑定任何单一功能，未来可持续扩展音乐创作小工具。

### 1.2 商业模式

Free + Pro 订阅制。Free 版保留开源核心功能，Pro 版通过独立闭源模块解锁写入操作和高级功能。本项目自身以 Apache-2.0 发布；所依赖的第三方组件均为 MIT / Apache-2.0 等宽松许可，允许闭源商业衍生与订阅制收费。

### 1.3 品牌与模块结构

品牌名 **O Muse**，子模块采用 `O Muse / <工具名>` 结构：

```
O Muse / Separate      ← 分轨（v1）
O Muse / Libraries     ← Kontakt 库管理（v1）
O Muse / Plugins       ← 插件管理（v1.1）
O Muse / <未来工具>     ← 持续扩展
```

### 1.4 技术栈决策（可升级为最新版）

| 层 | 技术 | 理由 |
|---|---|---|
| 桌面壳 | Tauri 2 | MIT/Apache 2.0 双许可；原生 macOS 打包与 sidecar 机制 |
| 后端主逻辑 | Rust | 业务模块（library_manager、plugin_scanner、license）全部 Rust 实现 |
| 分轨引擎 | Python + FastAPI（Sidecar 进程） | Demucs 为 Python 生态；经 Tauri Sidecar 集成。运行态自举 CPython 3.12.15，开发态用本机 Python |
| 前端 | React + TypeScript + Vite | 便于长期维护；实际锁定为 React 18 / TS 5 / Vite 5，样式用 CSS 变量（未引入 Tailwind） |
| 本地存储 | SQLite（`rusqlite` 用于 `core/`，`tauri-plugin-sql` 供前端） | 插件元数据增量缓存、扫描结果、用户配置 |
| 许可证 | 本项目自身 Apache-2.0；第三方归属见 `licenses/NOTICE` | 见第六章 |

### 1.5 目录结构（最终形态）

```
o-muse/
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── main.rs              # 桌面端二进制入口（调用 lib 的 run()）
│   │   ├── lib.rs               # Tauri 入口，插件注册，命令注册
│   │   ├── commands/            # Tauri Command 暴露层
│   │   │   ├── mod.rs
│   │   │   ├── stem_separation.rs  # 分轨命令（含运行时自举）
│   │   │   ├── library_manager.rs  # Kontakt 库管理命令
│   │   │   ├── plugin_scanner.rs   # 插件扫描命令
│   │   │   └── license.rs          # 许可命令
│   │   ├── core/                # 纯 Rust 业务逻辑（无 Tauri 依赖）
│   │   │   ├── mod.rs
│   │   │   ├── platform_paths.rs   # 跨平台路径抽象（ADR-0002）
│   │   │   ├── library_manager/    # Kontakt 库解析（逻辑参考 KLM，独立重写）
│   │   │   │   ├── mod.rs  model.rs  parser.rs  diagnose.rs
│   │   │   ├── plugin_scanner/     # 插件扫描器
│   │   │   │   ├── mod.rs  model.rs  discover.rs
│   │   │   │   ├── extract.rs  cache.rs  custom.rs
│   │   │   ├── stem_engine/        # 分轨引擎
│   │   │   │   ├── mod.rs  sidecar.rs  task.rs
│   │   │   │   └── runtime.rs      # 运行时按需自举
│   │   │   └── license/            # Free/Pro 判定（Dodo 校验）
│   │   │       ├── mod.rs  dodo.rs
│   ├── bin/                     # externalBin：python-stem-backend 启动器
│   ├── tests/                   # 集成测试
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                          # React 前端
│   ├── pages/                   # StemSeparationPage / LibraryManagerPage
│   │                            # PluginManagerPage / MarketPage / SettingsPage
│   ├── components/              # Sidebar / ui.tsx / RuntimeSection.tsx
│   ├── i18n/                    # i18next 资源（en / zh）
│   ├── store/                   # zustand 状态管理
│   ├── styles/                  # 设计令牌（亮/暗）
│   └── appRegistry.ts           # 轻量 App Registry（Q9）
├── python-backend/               # Python 分轨引擎（Sidecar）
│   ├── main.py  server.py  separator.py  demo.py  constants.py
│   ├── requirements.txt         # 开发态松散约束
│   └── requirements-runtime.txt # 运行时精简锁定集
├── docs/                         # ADR、技术方案
├── scripts/release/              # 打包与安装脚本
├── releases/latest/install.sh    # 一键安装脚本（稳定 URL）
├── licenses/
│   ├── APACHE-2.0.txt
│   ├── MIT.txt
│   └── NOTICE                   # 设计参考与依赖的归属声明
├── LICENSE                       # 本项目自身许可（Apache-2.0）
└── README.md
```


## 二、Agent 执行协议

每个 Task 格式如下：

```
### Task N: <标题>
前置条件: <依赖的 Task 编号>
执行动作: <具体指令>
验证命令: <Agent 必须运行的命令>
通过标准: <预期输出>
禁止事项: <不得做的事>
```

**全局禁止**：
- 默认不要运行 `sudo`、`su` 或任何提权命令；**例外**：功能确实需要提权时（见 `docs/adr/0005-privileged-operations.md`），必须向用户逐条列出具体操作对象（完整路径）与原因，限定在白名单路径内，并以一次性助手执行、不安装常驻服务
- 不要调用 `powershell`、`bash -c "..."` 执行外部命令
- 不要在 Rust 代码中使用 `std::process::Command` 启动非 Sidecar 进程（Python sidecar 除外）
- 不要修改 `licenses/` 目录下的任何文件
- 不要在 Pro 模块中引用 Apache 2.0 原始代码文件
- 不要在任何 Task 中引入或重建任何上游项目的 Git 历史


## 三、v1 执行计划

### Task 0: 仓库初始化与许可证合规

**前置条件**: 无

**总原则 — 独立实现，不复制源码**

本项目为**独立实现**（Rust 重写）。所有上游项目**仅作设计参考**，
不向其仓库复制任何源代码。由此确定：

- 不 `clone` / `rsync` 任何上游仓库内容进本仓库
- 无代码引入，故不做源码级归属；但仍须保留**设计参考与第三方依赖**的归属声明
- 本仓库 Git 历史从零开始，与任何上游项目无关

**执行动作**:

**Step 1 — 初始化全新仓库**

```bash
mkdir -p <工作区> && cd <工作区>
git init -b main
```

**Step 2 — 本项目自身许可证：Apache License 2.0**

将 Apache-2.0 完整文本写入仓库根目录 `LICENSE`。

**Step 3 — 第三方许可证文本**

- `licenses/APACHE-2.0.txt`：Apache-2.0 完整文本
- `licenses/MIT.txt`：MIT 完整文本

正文**不得修改**。

**Step 4 — 创建 `licenses/NOTICE` 归属声明**

`licenses/NOTICE` 是归属信息的**唯一权威**，须包含：

1. 本项目自身的许可与版权行
2. 明确声明"**未直接复制任何第三方项目的源代码**"
3. 各**设计参考项目**：名称、来源 URL、许可证、仅作设计参考的说明、
   以及"无官方关联/合作/背书"声明
4. 各**直接依赖**（如 Demucs、Tauri）的来源、版权与许可证

**Step 5 — 创建 `README.md`**

README 须包含「**开源归属**」小节，要求：

- 声明本项目为独立实现、**未复制任何第三方项目的源代码**
- **逐项点名**设计参考项目：名称、仓库链接、许可证、参考的具体方面
- 列出直接依赖（如 Demucs）及其许可证
- 含**资金声明**：原项目不接受任何资金、赞助或资助，且本项目与其无关联/无背书
- 指向 `docs/references.md`（完整参考记录）与 `licenses/`（许可证文本与 NOTICE）

**Step 6 — 创建 `.gitignore`**

至少包含：`.env`、`node_modules`、`target`、`dist`、`*.pyc`、`__pycache__/`。
构建缓存与密钥**一律不入库**。

**Step 7 — 首次提交**

```bash
git add -A
git commit -m "[Task 0] 初始化 O Muse 仓库：独立实现架构 + 许可证合规文件"
```

**验证命令**:

```bash
# 1. 无任何上游 Git 历史（提交数应等于本项目自己的提交数）
git log --oneline | wc -l

# 2. 许可证文件齐全
ls LICENSE licenses/APACHE-2.0.txt licenses/MIT.txt licenses/NOTICE

# 3. NOTICE 声明未复制代码，且含设计参考与依赖归属
grep -q "未直接复制" licenses/NOTICE && echo "NOTICE OK"

# 4. README 含资金/归属声明
grep -q "不接受任何资金" README.md && echo "README OK"

# 5. 无残留 submodule / 嵌套 .git
find . -name ".git" -type d -not -path "./.git" | wc -l   # 应为 0
cat .gitmodules 2>/dev/null || echo "no submodules  # 正确"

# 6. 无 GPL/AGPL 依赖
grep -iE "gpl" src-tauri/Cargo.toml package.json || echo "no GPL  # 正确"
```

**通过标准**:

1. `git log` 中无任何上游项目的原始 commit
2. `LICENSE` 与 `licenses/{APACHE-2.0.txt,MIT.txt,NOTICE}` 齐全
3. `NOTICE` 含设计参考与依赖归属，并声明未复制源代码
4. `README` 含资金/归属声明
5. 无残留 `.git` 子目录与 submodule
6. `Cargo.toml` / `package.json` 无 GPL/AGPL

**禁止事项**:

- 不要 `clone`、`rsync` 或以任何方式把上游仓库源码复制进本仓库
- 不要使用 `git clone --mirror`、`git fetch --unshallow` 或任何方式引入上游历史
- 不要保留 `.git/modules` 或 submodule 引用
- 不要修改 `licenses/APACHE-2.0.txt` 与 `licenses/MIT.txt` 正文
- 不要在 `NOTICE` 中遗漏任何设计参考项目或直接依赖的归属


### Task 1: Rust 后端骨架搭建

**前置条件**: Task 0

**执行动作**:

1. 在 `src-tauri/src/` 下创建前述目录结构。
2. 在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-shell = "2"
tauri-plugin-store = "2"
tauri-plugin-sql = { version = "2", features = ["sqlite"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
quick-xml = "0.36"
plist = "1.7"
chrono = { version = "0.4", features = ["serde"] }
walkdir = "2"
rayon = "1.10"
sha2 = "0.10"
thiserror = "2"
```

3. 在 `lib.rs` 中注册 `tauri_plugin_shell`、`tauri_plugin_store`、`tauri_plugin_sql`。
4. 创建空的 `commands/`、`core/library_manager/`、`core/plugin_scanner/` 模块，每个 `mod.rs` 至少包含一个 `pub fn placeholder()`。

**验证命令**: `cd src-tauri && cargo check`

**通过标准**: 编译通过，零错误。

**禁止事项**: 不要在此 Task 中实现任何业务逻辑。


### Task 2: Python Sidecar 集成（分轨引擎）

**前置条件**: Task 1

**执行动作**:

1. 在 `python-backend/` 中创建 `main.py`，实现 FastAPI 服务，暴露以下端点：
   - `POST /separate`：接收音频文件路径，返回任务 ID
   - `GET /status/{task_id}`：返回分轨进度（SSE 流）
   - `GET /stems/{task_id}`：返回分轨结果文件路径列表
2. 使用 `uv` 管理依赖，生成 `requirements.txt`（包含 `fastapi`、`uvicorn`、`demucs`、`librosa`、`pyloudnorm`）。
3. 在 `src-tauri/src/commands/stem_separation.rs` 中实现 Tauri Command：
   - `start_separation(file_path: String) -> Result<String, String>`
   - 使用 `tauri_plugin_shell` 的 sidecar API 启动 Python 进程
4. 在 `tauri.conf.json` 的 `bundle.externalBin` 中配置 Python sidecar 二进制路径。
5. 实现 Sidecar 生命周期管理：桌面壳退出时终止 Python 进程（采用 backend watchdog 机制，独立实现）。

**验证命令**: `cd python-backend && python -c "import fastapi, demucs; print('OK')"` 然后 `cd src-tauri && cargo check`

**通过标准**: Python 依赖可导入，Rust 编译通过。

**禁止事项**: 不要在 Rust 中直接调用 Python 解释器路径；必须通过 Tauri Sidecar API。


### Task 3: Kontakt 库管理模块（只读盘点）

**前置条件**: Task 1

**执行动作**:

将 KLM 的核心解析逻辑用 Rust 重写。**本 Task 不拉取 KLM 源码，仅通过阅读 GitHub 上的公开源码理解其解析逻辑，然后用 Rust 独立实现。** 参考 KLM 的架构：`platform/macos/` 下的 XML、PLIST、JSON 集成层。

1. 在 `core/library_manager/model.rs` 中定义：

```rust
pub struct LibraryEntry {
    pub snpid: String,
    pub name: String,
    pub path: Option<PathBuf>,
    pub registered: bool,
    pub diagnostics: Vec<Diagnostic>,
}

pub enum Diagnostic {
    MissingPath(PathBuf),
    DuplicateSnpid(String),
    DisconnectedVolume(PathBuf),
    MissingMetadata(String),
}
```

2. 在 `parser.rs` 中实现三个解析函数：

- `parse_service_center_xml(path: &Path) -> Result<Vec<LibraryEntry>>`
- `parse_plist(path: &Path) -> Result<Vec<LibraryEntry>>`
- `parse_installed_products_json(path: &Path) -> Result<Vec<LibraryEntry>>`

macOS 上的默认路径：
- Service Center XML: `~/Library/Application Support/Native Instruments/Service Center/`
- Kontakt 偏好设置 plist: `~/Library/Preferences/com.native-instruments.Kontakt*.plist`

3. 在 `diagnose.rs` 中实现诊断规则：
   - 路径存在性检查（`Path::exists()`）
   - 重复 SNPID 检测（HashMap 计数）
   - 断开的卷检测（检查路径是否以 `/Volumes/` 开头且该卷不可访问）
4. 在 `commands/library.rs` 中暴露：

```rust
#[tauri::command]
pub async fn scan_libraries() -> Result<Vec<LibraryEntry>, String>
```

**只做读取，不做任何写入。**

**验证命令**: `cd src-tauri && cargo test --lib library_manager`

**通过标准**: 所有单元测试通过；扫描函数在测试环境中返回 `Ok`。

**禁止事项**:
- 不要实现任何写入逻辑
- 不要安装后台服务或 LaunchDaemon（参考 KLM 安全模型）
- 不要在此 Task 中拉取 KLM 的 Flutter/Dart 源码到项目目录


### Task 4: 前端骨架与页面路由

**前置条件**: Task 1

**执行动作**:

1. 在 `src/` 中创建 React + TypeScript 项目结构（使用 Vite），配置 `index.html` 入口与 `tsconfig.json`。
2. 安装 `@tauri-apps/api`、`@tauri-apps/plugin-shell`、`@tauri-apps/plugin-store`、`react-router-dom`。
3. 创建三个页面组件：
   - `StemSeparationPage`：文件选择 + 分轨进度条 + 分轨结果列表
   - `LibraryManagerPage`：库存表格 + 搜索框 + 诊断标记列
   - `PluginManagerPage`：v1 中仅显示"即将推出"占位
4. 创建顶部导航栏，三个页面之间可切换。品牌名统一显示为 **O Muse**，导航栏使用 `O Muse / Separate`、`O Muse / Libraries`、`O Muse / Plugins` 结构。
5. 在 `src/stores/` 中创建 Zustand store（或 React Context），管理全局状态。

**验证命令**: `npm run build`

**通过标准**: TypeScript 编译通过，无错误。

**禁止事项**: 不要在此 Task 中实现任何 Tauri Command 调用逻辑。


### Task 5: Free/Pro 许可证判定模块

**前置条件**: Task 1

**执行动作**:

1. 在 `src-tauri/src/license/mod.rs` 中定义：

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum LicenseTier {
    Free,
    Pro,
}

pub fn current_tier() -> LicenseTier {
    // v1 阶段：始终返回 Free
    // v1.1 阶段：读取 tauri-plugin-store 中的 license_key 并验证
    LicenseTier::Free
}
```

2. 在所有 Pro 专属 Tauri Command 的入口处添加：

```rust
if license::current_tier() != LicenseTier::Pro {
    return Err("此功能需要 O Muse Pro 订阅".into());
}
```

3. Pro 专属命令标记为：
   - `core/library_manager/mutate.rs` 中的所有函数（库注册写入、批量修复）
   - v1.1 的插件目录自定义添加

**验证命令**: `cd src-tauri && cargo test --lib license`

**通过标准**: 单元测试通过，Free tier 下 Pro 命令返回错误。

**禁止事项**: 不要在此 Task 中实现实际的订阅验证网络请求。


### Task 6: v1 集成测试与打包

**前置条件**: Task 0–5

**执行动作**:

1. 编写集成测试 `tests/integration.rs`，覆盖：
   - 扫描一个包含模拟 XML 的临时目录，验证返回的 `LibraryEntry` 数量和诊断结果
   - 调用 `scan_libraries()` 命令，验证不 panic
   - 验证 Free tier 下 Pro 命令被拒绝
2. 运行 `cargo test`。
3. 在 `tauri.conf.json` 中设置 `productName: "O Muse"`。
4. 构建 Tauri 应用：`npm run tauri build`。
5. 验证生成的 `.dmg` 或 `.app` 可启动，应用名为 O Muse。

**验证命令**: `cargo test && npm run tauri build`

**通过标准**: 所有测试通过；构建产物生成且名称为 O Muse。

**禁止事项**: 不要在 CI 配置中引入需要网络访问的步骤（除非使用 GitHub Actions 的缓存）。


## 四、v1.1 执行计划（插件管理）

### Task 7: 插件目录自动发现

**前置条件**: Task 6

**执行动作**:

1. 在 `core/plugin_scanner/model.rs` 中定义：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSource {
    pub path: PathBuf,
    pub plugin_format: PluginFormat,
    pub recursive: bool,
    pub enabled: bool,
    pub is_custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginFormat {
    AU, VST, VST3, AAX, CLAP, Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub format: PluginFormat,
    pub path: PathBuf,
    pub bundle_id: Option<String>,
    pub last_modified: chrono::DateTime<chrono::Utc>,
}
```

2. 在 `discover.rs` 中实现 `default_sources() -> Vec<ScanSource>`，返回以下标准目录：

**AU**：`/Library/Audio/Plug-Ins/Components/`（系统级，`.component`），`~/Library/Audio/Plug-Ins/Components/`（用户级）
**VST 2**：`/Library/Audio/Plug-Ins/VST/`（系统级，`.vst`），`~/Library/Audio/Plug-Ins/VST/`（用户级）
**VST 3**：`/Library/Audio/Plug-Ins/VST3/`（系统级，`.vst3`），`~/Library/Audio/Plug-Ins/VST3/`（用户级），`~/.vst3/`（部分 DAW 搜索路径）
**AAX**：`/Library/Application Support/Avid/Audio/Plug-Ins/`（仅系统级，`.aaxplugin`）
**CLAP**：`/Library/Audio/Plug-Ins/CLAP/`（系统级，`.clap`），`~/Library/Audio/Plug-Ins/CLAP/`（用户级）

3. 实现 `discover_plugins(source: &ScanSource) -> Result<Vec<PluginInfo>>`：
   - 使用 `walkdir` 遍历目录（如果 `recursive` 为 true）
   - 按文件扩展名过滤（`.component` → AU，`.vst` → VST，`.vst3` → VST3，`.aaxplugin` → AAX，`.clap` → CLAP）
   - 仅返回匹配扩展名的条目，不读取元数据（元数据提取在 Task 8）

**验证命令**: `cd src-tauri && cargo test --lib plugin_scanner::discover`

**通过标准**: 单元测试在模拟目录结构上返回正确数量的插件。

**禁止事项**: 不要在此 Task 中实现元数据提取或缓存。


### Task 8: 插件元数据提取与增量缓存

**前置条件**: Task 7

**执行动作**:

1. 在 `extract.rs` 中实现元数据提取。对每种格式：

- **AU（`.component`）** ：读取 bundle 内的 `Contents/Info.plist`，提取 `CFBundleName`、`CFBundleIdentifier`、`CFBundleShortVersionString`。
- **VST3（`.vst3`）** ：读取 `Contents/Info.plist`（如果存在），提取 `CFBundleName`、`CFBundleIdentifier`。如果 plist 不存在，回退到解析二进制中的版本字符串（使用简单的字节模式匹配）。
- **VST 2（`.vst`）** ：从二进制文件头部提取插件名称（VST2 规范中名称位于固定偏移处）。
- **AAX（`.aaxplugin`）** ：读取 `Contents/Info.plist`。
- **CLAP（`.clap`）** ：读取 `Contents/Info.plist`（如果存在），或从二进制中提取。

2. 在 `cache.rs` 中实现基于 SQLite 的增量缓存：

```sql
CREATE TABLE IF NOT EXISTS plugin_cache (
    path TEXT PRIMARY KEY,
    file_size INTEGER,
    last_modified TEXT,
    name TEXT,
    vendor TEXT,
    version TEXT,
    format TEXT,
    scanned_at TEXT
);
```

扫描逻辑：对每个发现的插件文件，计算 `file_size` + `last_modified` 的组合哈希。如果与缓存记录匹配，直接返回缓存数据；否则重新提取并更新缓存。

3. 使用 `rayon` 并行提取元数据，但注意：**VST3 插件在 macOS 上可能不支持从后台线程调用 `bundleEntry`**。对于 VST3，使用 `rayon` 的 `par_iter` 但将每个提取任务包装在 `std::thread::spawn` 中，避免阻塞 Tauri 主线程。

**验证命令**: `cd src-tauri && cargo test --lib plugin_scanner::extract && cargo test --lib plugin_scanner::cache`

**通过标准**: 元数据提取测试通过；缓存命中率测试通过（第二次扫描不重新提取）。

**禁止事项**: 不要在提取过程中加载插件的可执行代码（不要 dlopen / LoadLibrary）。


### Task 9: 自定义目录支持与前端集成

**前置条件**: Task 8

**执行动作**:

1. 在 `discover.rs` 中添加：

```rust
pub fn add_custom_source(path: PathBuf) -> Result<ScanSource> {
    // 自动检测格式：遍历 path，统计各扩展名出现次数，取最多者作为 format
    // 如果无法检测，format 设为 Auto（扫描时同时匹配所有扩展名）
    // 将 ScanSource 持久化到 tauri-plugin-store
    // 此函数为 Pro 专属，入口处检查 license::current_tier()
}

pub fn remove_custom_source(path: &Path) -> Result<()> {
    // 从 store 中移除
    // 此函数为 Pro 专属
}
```

2. 在 `commands/plugins.rs` 中暴露：

```rust
#[tauri::command]
pub async fn scan_all_plugins() -> Result<Vec<PluginInfo>, String>

#[tauri::command]
pub async fn add_custom_plugin_dir(path: String) -> Result<ScanSource, String>

#[tauri::command]
pub async fn list_scan_sources() -> Result<Vec<ScanSource>, String>
```

3. 前端 `PluginManagerPage` 实现：

- 顶部：扫描源列表，每个源显示路径、格式、启用开关、删除按钮
- "添加自定义目录"按钮：调用 Tauri dialog 选择目录，然后调用 `add_custom_plugin_dir`
- 主表格：插件列表，列包括名称、厂商、版本、格式、路径
- 搜索框：按名称/厂商过滤
- "扫描"按钮：调用 `scan_all_plugins`，显示进度条

**验证命令**: `npm run build && cd src-tauri && cargo check`

**通过标准**: 前端编译通过，Rust 编译通过。

**禁止事项**: 不要在 `add_custom_plugin_dir` 中递归扫描（递归扫描在 `scan_all_plugins` 中统一处理）。


### Task 10: v1.1 集成测试与性能验证

**前置条件**: Task 9

**执行动作**:

1. 在测试环境中创建模拟插件目录结构：

```
/tmp/test_plugins/
├── AU/MyPlugin.component/Contents/Info.plist
├── VST3/OtherPlugin.vst3/Contents/Info.plist
└── CLAP/ClapPlugin.clap
```

2. 编写集成测试，验证：
   - `default_sources()` 返回至少 8 个标准目录
   - 对模拟目录的扫描返回正确数量的 `PluginInfo`
   - 第二次扫描的缓存命中率达到 100%（通过日志或计时验证）
3. 性能基准：在包含 50 个模拟插件的目录上，首次扫描耗时 < 2 秒，缓存扫描耗时 < 200ms。

**验证命令**: `cargo test --test plugin_integration`

**通过标准**: 所有集成测试通过；性能基准达标。

**禁止事项**: 不要在测试中依赖真实插件文件。


## 五、Free/Pro 功能边界

| 功能 | Free | Pro |
|---|---|---|
| 分轨 | ✅ | ✅ |
| Kontakt 库只读盘点 | ✅ | ✅ |
| Kontakt 库写入/修复/重排 | ❌ | ✅ |
| 插件扫描（标准目录） | ✅ | ✅ |
| 插件扫描（自定义目录） | ❌ | ✅ |
| 插件元数据完整视图 | ✅ | ✅ |
| 插件预设管理（v1.2+） | ❌ | ✅ |

**边界原则**：Free 和 Pro 共享同一套基于 Apache 2.0/MIT 代码的**基础模块**（`core/` 下的只读逻辑）。Pro 通过独立的、完全由你编写的**扩展模块**（`mutate.rs`、自定义目录功能）解锁额外能力。不要把 Apache 2.0 原始代码文件标记为 Pro 专属。


## 六、许可证合规检查清单

Agent 在每个 Task 完成后必须确认：

- [ ] `LICENSE`、`licenses/APACHE-2.0.txt`、`licenses/MIT.txt`、`licenses/NOTICE` 未被修改
- [ ] 未从任何上游项目复制源代码（本仓库代码均为独立实现）
- [ ] 参考了外部逻辑的模块在文件顶部注明来源与许可证（例如
      `// 逻辑参考 …… 在 MIT 许可下实现，本模块为独立重写，不复制其源码`）
- [ ] Pro 模块不包含任何上游项目的原始代码
- [ ] `README.md` 中包含"原项目不接受资金支持"声明
- [ ] `NOTICE` 中完整列出设计参考项目与直接依赖的归属
- [ ] `git log` 中无任何来自上游项目的原始 commit
- [ ] 项目中无残留 `.git/modules` 或 submodule 引用
- [ ] `Cargo.toml` 中没有引入 GPL/AGPL 依赖


## 七、品牌层补充说明

**产品名**：O Muse
**子模块命名**：`O Muse / Separate`、`O Muse / Libraries`、`O Muse / Plugins`
**Pro 版本命名**：O Muse Pro

**商标与域名核查（人工执行，非 Agent 任务）**：

1. 到 USPTO TESS（类别 9 / 42）、中国商标网、EUIPO 查询 "O Muse"
2. 核查 `omuse` 相关域名（如 `omuse.app`、`omuse.audio`）可用性
3. 在 App Store / Google Play / GitHub / npm 搜索 "O Muse"，确认无同名活跃产品
4. 确认无在先注册后，再对外统一品牌

**品牌传播要求**：

- 所有对外文案统一使用 "O Muse"
- `tauri.conf.json` 中 `productName: "O Muse"`，`identifier: "work.xsin.omuse"`（bundle id 变动会改变应用数据目录，发布后不宜再改）
- 应用图标、启动画面、关于页面统一品牌


## 八、给 Agent 的最终指令

**开始执行 Task 0。** 关键要求：**新建一个全新的 Git 仓库，以独立实现为前提，不导入任何上游项目源码**。许可证合规通过 `LICENSE` + `licenses/{APACHE-2.0.txt,MIT.txt,NOTICE}` 完整覆盖；`NOTICE` 负责设计参考与依赖的归属声明，并明确声明未复制源代码。

完成后运行验证命令，将输出粘贴到执行日志中，然后继续 Task 1。每个 Task 完成后提交一次 Git commit，commit message 格式：`[Task N] <描述>`。

如果任何验证命令失败，停止执行，不要跳过。在日志中记录失败原因，然后等待人工指令。