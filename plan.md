# Music Rig — AI Agent 完整技术执行方案 v1.0

**目标读者**：AI Agent（Claude Code / Cursor / Windsurf / Codex）
**执行模式**：逐任务执行，每完成一个 Task 后运行指定验证命令，验证通过再进入下一 Task。
**禁止事项**：不要在任何 Task 中自行改变架构决策、技术选型或许可证策略。遇到歧义时，在代码中写入 `// AGENT_QUESTION: <问题>` 并停止，等待人工确认。


## 一、项目总览

### 1.1 产品定位

**Music Rig** 是一款面向音乐人的本地桌面助手，定位为**音乐人必备的瑞士军刀 / 全能工具箱**。v1 首发两个核心工具：**音频分轨**（基于 StemDeck）与 **Kontakt 音色库管理**（参考 KLM 逻辑），v1.1 扩展**音频插件（AU/VST/VST3/AAX/CLAP）扫描与管理**。品牌层不绑定任何单一功能，未来可持续扩展音乐创作小工具。

### 1.2 商业模式

Free + Pro 订阅制。Free 版保留开源核心功能，Pro 版通过独立闭源模块解锁写入操作和高级功能。Apache 2.0（StemDeck）和 MIT（KLM）均允许闭源商业衍生和订阅制收费。

### 1.3 品牌与模块结构

品牌名 **Music Rig**，子模块采用 `Music Rig / <工具名>` 结构：

```
Music Rig / Separate      ← 分轨（v1）
Music Rig / Libraries     ← Kontakt 库管理（v1）
Music Rig / Plugins       ← 插件管理（v1.1）
Music Rig / <未来工具>     ← 持续扩展
```

### 1.4 技术栈决策（可升级为最新版）

| 层 | 技术 | 理由 |
|---|---|---|
| 桌面壳 | Tauri 2 | 复用 StemDeck 的 Tauri 2 架构，MIT/Apache 2.0 双许可 |
| 后端主逻辑 | Rust | 所有新增模块（library_manager、plugin_scanner）用 Rust 编写 |
| 分轨引擎 | Python 3.12 + FastAPI（Sidecar 进程） | StemDeck 原生架构，通过 Tauri Sidecar 模式集成 |
| 前端 | React + TypeScript（Tauri 2 推荐） | StemDeck 使用 vanilla JS，但为便于 AI Agent 维护，升级为 React19+TS7+TailwindCSS4 |
| 本地存储 | SQLite（通过 `tauri-plugin-sql`） | 插件库存缓存、扫描结果、用户配置 |
| 许可证合规 | Apache 2.0 + MIT 双许可保留 | 见第六章 |

### 1.5 目录结构（最终形态）

```
music-rig/
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── lib.rs               # Tauri 入口，插件注册
│   │   ├── commands/            # Tauri Command 暴露层
│   │   │   ├── mod.rs
│   │   │   ├── stemdeck.rs      # 分轨相关命令（调用 Python sidecar）
│   │   │   ├── library.rs       # Kontakt 库管理命令
│   │   │   └── plugins.rs       # 插件扫描命令
│   │   ├── core/                # 纯 Rust 业务逻辑（无 Tauri 依赖）
│   │   │   ├── mod.rs
│   │   │   ├── library_manager/ # KLM 逻辑移植
│   │   │   │   ├── mod.rs
│   │   │   │   ├── model.rs     # LibraryEntry, Diagnostic
│   │   │   │   ├── parser.rs    # XML / plist / JSON 解析
│   │   │   │   ├── diagnose.rs  # 诊断引擎
│   │   │   │   └── mutate.rs    # 写入操作（Pro 专属）
│   │   │   └── plugin_scanner/  # 插件扫描器
│   │   │       ├── mod.rs
│   │   │       ├── model.rs     # PluginInfo, ScanSource
│   │   │       ├── discover.rs  # 自动发现标准目录
│   │   │       ├── extract.rs   # 元数据提取
│   │   │       └── cache.rs     # 增量扫描缓存
│   │   └── license/             # Free/Pro 判定
│   │       └── mod.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                          # React 前端
│   ├── pages/
│   │   ├── StemDeck/            # 分轨页面
│   │   ├── LibraryManager/      # Kontakt 库管理页面
│   │   └── PluginManager/       # 插件管理页面（v1.1）
│   ├── components/
│   └── stores/                   # 状态管理
├── python-backend/               # Python 分轨引擎（Sidecar）
│   ├── main.py                  # FastAPI 入口
│   ├── demucs_runner.py
│   └── requirements.txt
├── licenses/
│   ├── APACHE-2.0.txt
│   ├── MIT.txt
│   └── NOTICE                   # StemDeck / KLM 归属声明
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
- 不要运行 `sudo`、`su` 或任何提权命令
- 不要调用 `powershell`、`bash -c "..."` 执行外部命令
- 不要在 Rust 代码中使用 `std::process::Command` 启动非 Sidecar 进程（Python sidecar 除外）
- 不要修改 `licenses/` 目录下的任何文件
- 不要在 Pro 模块中引用 Apache 2.0 原始代码文件
- 不要在任何 Task 中恢复或重建 StemDeck 的原始 Git 历史


## 三、v1 执行计划

### Task 0: 仓库初始化与许可证合规

**前置条件**: 无

**执行动作**:

**Step 1 — 拉取 StemDeck 源码（不 fork，不保留 Git 历史）**

```bash
# 1. 在临时目录拉取 StemDeck（浅克隆，仅取最新快照）
git clone --depth 1 https://github.com/stemdeckapp/stemdeck.git /tmp/stemdeck-src

# 2. 记录拉取到的 commit hash，用于 NOTICE 归属声明
cd /tmp/stemdeck-src && git rev-parse HEAD > /tmp/stemdeck-commit.txt
```

**Step 2 — 初始化本地仓库并导入源码**

```bash
# 1. 创建项目根目录
mkdir -p ~/projects/music-rig && cd ~/projects/music-rig

# 2. 初始化全新 Git 仓库（无原始历史）
git init -b main

# 3. 复制 StemDeck 源码（排除 .git 目录）
rsync -av --exclude='.git' /tmp/stemdeck-src/ ./

# 4. 确认无残留 Git 元数据
rm -rf .git/modules .gitattributes 2>/dev/null || true
find . -name ".git" -type d -not -path "./.git" -exec rm -rf {} + 2>/dev/null || true

# 5. 确认无原始历史
git log --oneline | wc -l   # 应为 0（尚无 commit）
```

**Step 3 — 清理 StemDeck 原始许可证与归属文件**

不要删除 StemDeck 原有的 `LICENSE` 文件（Apache 2.0 要求保留），但将其移动到 `licenses/APACHE-2.0.txt`。**不要修改任何原始版权头。**

```bash
mkdir -p licenses
cp /tmp/stemdeck-src/LICENSE licenses/APACHE-2.0.txt 2>/dev/null || \
  cp /tmp/stemdeck-src/LICENSE.md licenses/APACHE-2.0.txt 2>/dev/null || \
  echo "AGENT_QUESTION: 未找到 StemDeck 的 LICENSE 文件，请人工确认许可证文件名"
```

**Step 4 — 创建 MIT 许可证文本**

将 MIT 完整文本写入 `licenses/MIT.txt`，版权行使用 KLM 上游声明的版权持有人。

**Step 5 — 创建 NOTICE 归属声明**

创建 `licenses/NOTICE`，内容如下：

```
本产品（Music Rig）包含来自以下开源项目的代码或逻辑：

─────────────────────────────────────────────────────
项目一：StemDeck
来源：https://github.com/stemdeckapp/stemdeck
拉取版本：commit <填入 /tmp/stemdeck-commit.txt 内容>
版权：Copyright (c) Thales Claro
许可证：Apache License, Version 2.0（见 licenses/APACHE-2.0.txt）

本产品基于 StemDeck 源码进行定制化改造。StemDeck 项目明确声明
不接受任何资金、赞助或资助。本产品与 StemDeck 项目及其作者无
任何官方关联、合作或背书关系。本产品的商业化行为不代表 StemDeck
项目立场。

─────────────────────────────────────────────────────
项目二：KLM - Kontakt Library Manager
来源：https://github.com/cloinse/klm
版权：Copyright (c) cloinse
许可证：MIT License（见 licenses/MIT.txt）

本产品参考 KLM 的 Kontakt 库元数据解析逻辑与安全文件操作模型，
用 Rust 语言重新实现。未直接复制 KLM 的 Flutter/Dart 源代码。

─────────────────────────────────────────────────────
```

**Step 6 — 创建 README.md**

创建 `README.md`，顶部必须包含以下声明（**逐字照抄，不得改写**）：

```markdown
# Music Rig

> **开源归属声明**
>
> 本项目基于 [StemDeck](https://github.com/stemdeckapp/stemdeck) 构建，
> 原项目不接受任何资金、赞助或资助。本项目与 StemDeck 项目及其作者
> 无官方关联或背书关系。
>
> 本项目的 Kontakt 库管理能力参考了
> [KLM](https://github.com/cloinse/klm) 的设计，原项目以 MIT 许可证发布。

（以下为项目自身说明）
```

**Step 7 — 首次提交**

```bash
# 创建 .gitignore（至少包含 .git/modules、node_modules、target、dist、*.pyc）
git add -A
git commit -m "[Task 0] 初始化 Music Rig 仓库：拉取 StemDeck 源码，配置许可证合规文件"
```

**验证命令**:

```bash
# 1. 确认无原始 Git 历史
git log --oneline | grep -c "stemdeckapp" || echo "0  # 应为 0"

# 2. 确认许可证文件存在
ls licenses/APACHE-2.0.txt licenses/MIT.txt licenses/NOTICE

# 3. 确认 NOTICE 包含归属声明
grep -q "StemDeck" licenses/NOTICE && grep -q "KLM" licenses/NOTICE && echo "NOTICE OK"

# 4. 确认 README 包含资金声明
grep -q "不接受任何资金" README.md && echo "README OK"

# 5. 确认无残留的原始 Git 元数据
find . -name ".git" -type d -not -path "./.git" | wc -l   # 应为 0

# 6. 确认原始版权头保留
grep -r "Copyright" src-tauri/src/ 2>/dev/null | head -5 || echo "AGENT_QUESTION: 未在 src-tauri 找到版权头，需人工核查"
```

**通过标准**:

1. `git log` 中无任何来自 stemdeckapp 的原始 commit
2. 三个许可证文件存在，NOTICE 内容完整
3. README 包含资金声明
4. 无残留 `.git` 子目录
5. 原始文件中的版权头未被删除

**禁止事项**:

- 不要使用 `git clone --mirror`、`git fetch --unshallow` 或任何方式重建原始历史
- 不要保留 `.git/modules` 或 submodule 引用
- 不要删除 StemDeck 原始文件中的 `Copyright` 注释头
- 不要修改 `licenses/APACHE-2.0.txt` 和 `licenses/MIT.txt` 的正文
- 不要在 NOTICE 中遗漏拉取 commit hash


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
3. 在 `src-tauri/src/commands/stemdeck.rs` 中实现 Tauri Command：
   - `start_separation(file_path: String) -> Result<String, String>`
   - 使用 `tauri_plugin_shell` 的 sidecar API 启动 Python 进程
4. 在 `tauri.conf.json` 的 `bundle.externalBin` 中配置 Python sidecar 二进制路径。
5. 实现 Sidecar 生命周期管理：桌面壳退出时终止 Python 进程（参考 StemDeck 的 backend watchdog 机制）。

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

1. 在 `src/` 中创建 React + TypeScript 项目结构（使用 Vite）。StemDeck 原始前端为 vanilla JS，本 Task 将其替换为 React+TS，因此需要删除 StemDeck 原有的前端入口文件（`index.html` 中的脚本引用、`src/` 下的 vanilla JS 文件），仅保留 Tauri 配置和 Rust 后端。
2. 安装 `@tauri-apps/api`、`@tauri-apps/plugin-shell`、`@tauri-apps/plugin-store`、`react-router-dom`。
3. 创建三个页面组件：
   - `StemDeckPage`：文件拖放区域 + 分轨进度条 + 多轨波形占位
   - `LibraryManagerPage`：库存表格 + 搜索框 + 诊断标记列
   - `PluginManagerPage`：v1 中仅显示"即将推出"占位
4. 创建顶部导航栏，三个页面之间可切换。品牌名统一显示为 **Music Rig**，导航栏使用 `Music Rig / Separate`、`Music Rig / Libraries`、`Music Rig / Plugins` 结构。
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
    return Err("此功能需要 Music Rig Pro 订阅".into());
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
3. 在 `tauri.conf.json` 中设置 `productName: "Music Rig"`。
4. 构建 Tauri 应用：`npm run tauri build`。
5. 验证生成的 `.dmg` 或 `.app` 可启动，应用名为 Music Rig。

**验证命令**: `cargo test && npm run tauri build`

**通过标准**: 所有测试通过；构建产物生成且名称为 Music Rig。

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
| 分轨（StemDeck 核心） | ✅ | ✅ |
| Kontakt 库只读盘点 | ✅ | ✅ |
| Kontakt 库写入/修复/重排 | ❌ | ✅ |
| 插件扫描（标准目录） | ✅ | ✅ |
| 插件扫描（自定义目录） | ❌ | ✅ |
| 插件元数据完整视图 | ✅ | ✅ |
| 插件预设管理（v1.2+） | ❌ | ✅ |

**边界原则**：Free 和 Pro 共享同一套基于 Apache 2.0/MIT 代码的**基础模块**（`core/` 下的只读逻辑）。Pro 通过独立的、完全由你编写的**扩展模块**（`mutate.rs`、自定义目录功能）解锁额外能力。不要把 Apache 2.0 原始代码文件标记为 Pro 专属。


## 六、许可证合规检查清单

Agent 在每个 Task 完成后必须确认：

- [ ] `licenses/APACHE-2.0.txt`、`licenses/MIT.txt`、`licenses/NOTICE` 未被修改
- [ ] 所有从 StemDeck 复制的文件保留了原始版权头（`grep -r "Copyright" src-tauri/`）
- [ ] 所有从 KLM 移植的逻辑在文件顶部注释中注明来源（例如 `// Ported from KLM (https://github.com/cloinse/klm) under MIT License`）
- [ ] Pro 模块（`mutate.rs`、自定义目录功能）不包含 Apache 2.0 原始代码
- [ ] `README.md` 中包含"原项目不接受资金支持"声明
- [ ] `NOTICE` 中包含拉取 StemDeck 时的 commit hash（可追溯性）
- [ ] `git log` 中无任何来自 stemdeckapp 的原始 commit
- [ ] 项目中无残留 `.git/modules` 或 submodule 引用
- [ ] `Cargo.toml` 中没有引入 GPL/AGPL 依赖


## 七、品牌层补充说明

**产品名**：Music Rig
**子模块命名**：`Music Rig / Separate`、`Music Rig / Libraries`、`Music Rig / Plugins`
**Pro 版本命名**：Music Rig Pro

**商标与域名核查（人工执行，非 Agent 任务）**：

1. 到 USPTO TESS（类别 9 / 42）、中国商标网、EUIPO 查询 "Music Rig"
2. 查询 `musicrig.app`、`musicrig.com`、`musicrig.audio` 域名可用性
3. 在 App Store / Google Play / GitHub / npm 搜索 "Music Rig"、"MusicRig"，确认无同名活跃产品
4. 由于 "Music" 是通用词、组合词注册性较好，但仍需确认无在先注册

**品牌传播要求**：

- 所有对外文案统一使用 "Music Rig"
- `tauri.conf.json` 中 `productName: "Music Rig"`，`identifier: "app.musicrig.desktop"`（或你最终确定的域名反写）
- 应用图标、启动画面、关于页面统一品牌


## 八、给 Agent 的最终指令

**开始执行 Task 0。** 关键要求：**使用 `git clone --depth 1` 拉取 StemDeck 源码，然后复制到新初始化的 `music-rig` 仓库中，彻底清除原始 Git 历史**。在 NOTICE 中记录拉取到的 commit hash 以满足可追溯性要求。

完成后运行验证命令，将输出粘贴到执行日志中，然后继续 Task 1。每个 Task 完成后提交一次 Git commit，commit message 格式：`[Task N] <描述>`。

如果任何验证命令失败，停止执行，不要跳过。在日志中记录失败原因，然后等待人工指令。