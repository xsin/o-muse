# Glossary — O Muse

> 本文件是纯术语表，不包含实现细节、架构或待办。术语随设计讨论固化时在此就地更新。

## 产品与品牌

- **O Muse**：面向音乐人的本地桌面助手（产品名）。品牌层不绑定任何单一功能。
- **O Muse Pro**：O Muse 的付费订阅版本，通过独立闭源扩展模块解锁写入操作与高级功能。
- **App Marketplace（应用市场）**：O Muse 的**战略愿景**——产品定位为"面向音乐人的应用市场"，每个功能单元（见 *App*）是一个可独立呈现的工具。注意：v1 为**本地打包、随应用分发**，无远程下载/安装能力；"市场"在此指可扩展的工具生态心智模型，而非 App Store 式远程商店。
- **工具箱**：v1 首页左栏的**实际 UI 标签**（用户可见文案）。语义等同 App Marketplace 的当前落地形态，但避免"市场=可下载商店"的误解。

## 领域单元

- **App（应用）**：O Muse 自身承载的一个**独立功能单元**（如 StemDeck 分轨、Kontakt Library Manager、Audio Plugin Manager）。App 是壳（Tauri + React）中可被启动、在左栏常驻为标签页、且可**多开并存**的单元。App 注册于轻量 *App Registry*，由前端动态渲染。
  - **不要**将 App 与 *Audio Plugin* 混淆：App 是 O Muse 的功能模块；Audio Plugin 是被扫描管理的第三方音频格式文件。
- **Audio Plugin（音频插件）**：被 Audio Plugin Manager 扫描与管理的第三方音频格式单元，包括 AU、VST、VST3、AAX、CLAP。它们**不是** O Muse 的 App，也不通过 App Marketplace 分发。
- **App Registry（应用注册表）**：前端一处轻量清单（常量/薄模块），声明每个 App 的 id、名称、路由、图标、启用态、最低支持版本。首页据此动态渲染卡片与左栏项；v1 不实现远程拉取。

## 平台

- **v1 / MVP**：仅 macOS（Apple Silicon / Intel）。所有原生路径按 macOS 约定。
- **v1.5**：新增 Windows 支持。引入薄 *Platform Abstraction*（见 ADR-0002），v1 仅实现 macOS 分支，Windows 留 TODO。

## 授权与计费

- **Free / Pro 边界**：Free 与 Pro 共享基于 Apache-2.0（StemDeck）与 MIT（KLM）代码的**只读基础模块**（`core/`）。Pro 通过独立、完全自研的**扩展模块**（`mutate.rs`、自定义目录功能）解锁额外能力。
- **License Tier（许可证层级）**：运行期判定结果，取值 `Free` 或 `Pro`。v1 接入 **Dodo Payments** 真实订阅校验（Test Mode Key 已备），由 `tauri-plugin-store` 中的 `license_key` + Dodo 验证结果推导（见 ADR-0003）。

## 上游来源（合规相关，非领域行为）

- **StemDeck**：音频分轨上游，Apache-2.0，本产品基于其源码定制改造但无官方关联（见 `licenses/NOTICE`）。
- **KLM（Kontakt Library Manager）**：Kontakt 库管理逻辑参考来源，MIT；本产品用 Rust 重新实现其解析逻辑，未复制 Dart 源码。
