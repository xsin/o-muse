# O Muse

**音乐人的全能工具箱**

O Muse 是一个跨平台桌面应用（Tauri 2 + React 18），把音乐制作相关的
工具以「App」为单位组织起来。左侧常驻工具箱，可同时打开多个 App 并在
它们之间切换。所有处理均在本地完成，隐私优先，离线运行。

## 内置 App

首批内置 App：

- **Stem Separation** — 本地音频分轨，支持人声 / 鼓 / 贝斯 / 其他四轨拆分。
  基于 Meta 的 [Demucs](https://github.com/facebookresearch/demucs) 模型实现，
  交互设计独立完成。
- **Kontakt Library Manager** — 只读盘点 Kontakt 音源库，诊断缺失路径、
  重复 SNPID、断卷。
- **Audio Plugin Manager**（v1.1）— 扫描与管理 AU / VST / VST3 / AAX / CLAP 插件。

## 开源归属

O Muse 为独立实现，**未复制任何第三方项目的源代码**。分轨能力基于
Meta 的 [Demucs](https://github.com/facebookresearch/demucs)（MIT），
实现过程中参考了以下开源项目的设计思路：

- [StemDeck](https://github.com/stemdeckapp/stemdeck)（Apache-2.0）—
  多轨 UI 交互思路、Python sidecar 生命周期管理模式
- [KLM](https://github.com/cloinse/klm)（MIT）—
  Kontakt 库元数据解析逻辑、安全文件操作模型

完整的参考记录见 [`docs/references.md`](docs/references.md)。第三方许可证
文本与 NOTICE 归属声明见 [`licenses/`](licenses/)。

> **资金声明**：StemDeck 项目不接受任何资金、赞助或资助。O Muse 与
> StemDeck 项目及其作者无官方关联或背书关系。

## 仓库结构

```
├── docs/          BRD / PRD / TDD 与 ADR（架构决策记录）
├── prototype/     首页交互原型（home.html，纯静态，无需构建）
├── assets/        品牌资源（logo / banner）
├── src-tauri/     Rust 后端（Task 1 起）
└── licenses/      第三方许可证与 NOTICE 归属声明
```

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2 |
| 前端 | React 18 + TypeScript + Vite 5 |
| 后端 | Rust |
| 分轨引擎 | Python + FastAPI（Sidecar）+ Demucs |
| 本地存储 | SQLite |

## 快速开始

### 安装（公测版，macOS）

当前为**公测阶段**：未使用 Apple 开发者签名、未经 App Store 分发、未做公证。
推荐一条命令安装：

```bash
curl -fsSL https://xsin.github.io/o-muse/releases/latest/install.sh | bash
```

默认安装到 `~/Applications/O Muse.app`（**不需要 sudo**），安装完成后自动启动。

想先审计脚本再执行（推荐给注重安全的用户）：

```bash
curl -fsSL https://xsin.github.io/o-muse/releases/latest/install.sh -o install.sh
less install.sh
bash install.sh
```

**说明：**

- 当前是公测版，未使用 Apple 开发者签名和公证。
- 通过 `curl` 安装（zip 分发）通常不会触发 Gatekeeper 隔离。
- 如果首次打开仍被阻止，**右键点击应用选择"打开"**，或到
  **系统设置 > 隐私与安全性**点击"仍要打开"。
- 如果看到"已损坏"提示，执行：

  ```bash
  xattr -dr com.apple.quarantine ~/Applications/O\ Muse.app
  ```

**其他参数：**

| 参数 | 作用 |
|---|---|
| `--version <v>` | 安装指定版本（默认最新版） |
| `--install-dir <dir>` | 自定义安装目录 |
| `--no-launch` | 安装后不启动 |
| `--system` | 安装到 `/Applications`（需要 sudo） |

**更新：** 重新运行同一条安装命令即可，会覆盖旧版本并**保留你的数据**
（license、界面偏好、插件缓存位于
`~/Library/Application Support/work.xsin.omuse/`，不在应用包内）。

> 分发与签名的完整技术方案见
> [`docs/tech-release-without-developer-id.md`](docs/tech-release-without-developer-id.md)。

### 系统要求

- macOS 10.13 或更高
- Apple Silicon 或 Intel（universal 包优先；无 universal 时按架构选择）
- 分轨功能依赖本地 Python 环境（首次使用会自动拉起内置 sidecar）

### 首次启动流程

1. 应用启动后默认进入「工具箱」。
2. 打开「分轨分离」，选择音频文件，点击"开始分轨"。
3. 首次运行会启动本地分轨服务（可能需要数十秒）。
4. 若 Demucs 不可用，会以**演示模式**返回模拟结果，界面会明确标注。

## 许可证

O Muse 自身以 **Apache License 2.0** 发布，完整文本见根目录
[`LICENSE`](LICENSE)。

本项目未直接复制任何第三方源代码，仅在设计层面参考了上述开源项目。
第三方依赖的许可证文本存放于 [`licenses/`](licenses/)。