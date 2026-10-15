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

## Music Rig — 工具箱（Toolbox）

Music Rig 是一个跨平台桌面应用（Tauri 2 + React 19），把音乐制作相关的
工具以「App」为单位组织起来，左侧常驻工具箱，可同时打开多个 App 并在
它们之间切换。

首批内置 App：

- **StemDeck** — 本地音频分轨（人声 / 鼓 / 贝斯 / 其他），隐私优先，离线运行。
- **Kontakt Library Manager** — 只读盘点 Kontakt 音源库，诊断缺失路径、重复 SNPID、断卷。
- **Audio Plugin Manager**（v1.1）— 扫描与管理 AU / VST / VST3 / AAX / CLAP 插件。

## 仓库结构

- `docs/` — BRD / PRD / TDD 与 ADR（架构决策记录）。
- `prototype/` — 首页交互原型（`home.html`，纯静态，无需构建）。
- `assets/` — 品牌资源（logo / banner）。
- `src-tauri/` — Rust 后端（Task 1 起）。
- `licenses/` — 第三方许可证与 NOTICE 归属声明。

## 许可证

本项目包含来自 StemDeck（Apache-2.0）与 KLM（MIT）的代码或逻辑，详见
[`licenses/NOTICE`](licenses/NOTICE)。原始许可证文本分别见
`licenses/APACHE-2.0.txt` 与 `licenses/MIT.txt`。

> **资金声明（逐字）**：原项目不接受任何资金、赞助或资助。本项目与
> StemDeck 项目及其作者无官方关联或背书关系。
