# Music Rig

**音乐人的全能工具箱**

Music Rig 是一个跨平台桌面应用（Tauri 2 + React 19），把音乐制作相关的
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

Music Rig 为独立实现，**未复制任何第三方项目的源代码**。分轨能力基于
Meta 的 [Demucs](https://github.com/facebookresearch/demucs)（MIT），
实现过程中参考了以下开源项目的设计思路：

- [StemDeck](https://github.com/stemdeckapp/stemdeck)（Apache-2.0）—
  多轨 UI 交互思路、Python sidecar 生命周期管理模式
- [KLM](https://github.com/cloinse/klm)（MIT）—
  Kontakt 库元数据解析逻辑、安全文件操作模型

完整的参考记录见 [`docs/references.md`](docs/references.md)。第三方许可证
文本与 NOTICE 归属声明见 [`licenses/`](licenses/)。

> **资金声明**：StemDeck 项目不接受任何资金、赞助或资助。Music Rig 与
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
| 前端 | React 19 + TypeScript |
| 后端 | Rust |
| 分轨引擎 | Python + FastAPI（Sidecar）+ Demucs |
| 本地存储 | SQLite |

## 快速开始

（待补充：安装方式、系统要求、首次启动流程）

## 许可证

Music Rig 自身以 **Apache License 2.0** 发布，完整文本见根目录
[`LICENSE`](LICENSE)。

本项目未直接复制任何第三方源代码，仅在设计层面参考了上述开源项目。
第三方依赖的许可证文本存放于 [`licenses/`](licenses/)。