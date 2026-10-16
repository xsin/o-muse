# BRD — Business Requirements Document: O Muse

> 读者：产品/商业决策者、Agent 实现者（配合 `prd.md` / `tdd.md`）。
> 本文件聚焦**为什么做**与**商业边界**；功能细节见 `prd.md`，技术约束见 `tdd.md`。

## 1. 产品愿景

**O Muse** 是面向音乐人的**本地桌面助手**，长期定位为"**面向音乐人的应用市场（App Marketplace）**"——每个独立功能是一个 **App**，可由壳动态承载、常驻、多开并存。v1 以本地打包的两个核心工具起步，未来可持续扩展音乐创作小工具。

- 战略愿景标签：**应用市场（App Marketplace）**
- v1 实际首页 UI 标签：**工具箱**（避免"市场=可下载商店"的误解，见 `GLOSSARY.md`）

## 2. 目标用户

- 音乐制作人 / 编曲人 / 混音师（使用 Kontakt 音源、AU/VST 插件、需音频分轨）。
- 需要"一站式、本地优先、隐私安全"的工具集合，而非零散的独立软件。

## 3. 商业模式

- **Free + Pro 订阅制**。
- Free：开源核心功能（分轨、Kontakt 库只读盘点、插件标准目录扫描）。
- Pro：通过**独立闭源扩展模块**解锁写入/修复、自定义插件目录、预设管理等。
- 许可证合规基础：Apache-2.0（StemDeck）与 MIT（KLM）均允许闭源商业衍生与订阅收费。
- **付费校验**：v1 起接入 **Dodo Payments** 真实订阅校验（Test Mode Key 已备，见 `tdd.md` ADR-0003）。

## 4. 产品范围与路线图

| 阶段 | 范围 | 平台 |
|---|---|---|
| **v1 (MVP)** | App 1 音频分轨（StemDeck）、App 2 Kontakt 库管理（只读）、首页工具箱启动器、Dodo 订阅校验 | **macOS only** |
| **v1.1** | App 3 音频插件管理（AU/VST/VST3/AAX/CLAP 扫描/元数据/增量缓存/自定义目录） | macOS |
| **v1.2+** | 插件预设管理（Pro）等 | macOS |
| **v1.5** | 新增 **Windows** 支持（跨平台抽象已在 v1 铺设，见 ADR-0002） | macOS + Windows |

## 5. 品牌与命名

- 产品名：**O Muse**；Pro 版本：**O Muse Pro**。
- 子模块品牌结构：`O Muse / <App 名>`，如 `O Muse / Separate`、`O Muse / Libraries`、`O Muse / Plugins`。
- 商标与域名核查为**人工任务**（非 Agent 范畴）：USPTO TESS、中国商标网、EUIPO、域名 `musicrig.app/.com/.audio`、各商店/平台同名检索。
- 对外文案统一 "O Muse"；`tauri.conf.json` `productName: "O Muse"`、`identifier: "app.musicrig.desktop"`。

## 6. 商业约束与合规

- 必须保留 StemDeck（Apache-2.0）与 KLM（MIT）的原始许可证文本与版权头（见 `tdd.md` §许可证合规）。
- **Pro 模块不得包含 Apache-2.0 原始代码**，仅基于上游的只读基础模块之上自研扩展。
- 不引入 GPL/AGPL 依赖（避免传染闭源 Pro 模块）。
- `README.md` / `NOTICE` 须包含资金声明与归属（StemDeck 明确不接受资助，本产品与其无官方关联）。

## 7. 成功度量（建议）

- v1：可启动的 macOS 应用 + 两个 App 可用 + 订阅校验可接通（Test 环境）。
- v1.1：插件扫描性能达标（50 插件首次 < 2s，缓存 < 200ms）。
- v1.5：Windows 构建产物可启动，功能对齐 macOS。

---

*本 BRD 与 `prd.md`、`tdd.md`、`handsoff.md` 互为补充；决策依据见 `docs/adr/`。*
