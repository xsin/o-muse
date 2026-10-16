# ADR-0004: 国际化（i18n）技术选型

- **状态**：已采纳（2026-10-15）
- **决策人**：用户（指令）+ Agent（选型）
- **相关**：PRD FR-8 / TDD §1,§4.1

## 背景

用户要求：支持**英文、中文**两套文案，默认**随系统**区域；同时要求暗夜模式（见 PRD FR-9）。

需求是壳层（React 前端）级的多语言，不涉及 Rust 核心业务逻辑（其错误/日志以英文为主，且多为用户不可见）。

## 决策

采用 **i18next 生态**（React 事实标准、与 Tauri 无冲突）：
- `i18next` **26.4.5**
- `react-i18next` **17.0.16**
- `i18next-browser-languagedetector` **8.2.1**

- 资源文件：`src/i18n/locales/{en,zh}.json`（命名空间 `translation`）。
- 检测顺序：`['localStorage','navigator']`；持久化键 `ui_lang`（存于 `tauri-plugin-store`）。
- 默认语言：store 有值→用之；否则系统 `zh-CN`→`zh`，其余→`en`（回退 `en-US`）。
- App Registry **只存 id/route/icon/enabled**，字符串全走 i18n（如 `apps.stem_separation.name`）。

## 理由

- i18next 生态成熟、树摇友好、与 react-router/vite8 兼容良好；browser-languagedetector 直接满足"默认随系统"。
- 不在 Rust 侧做 i18n（避免过度设计）；仅前端壳层文案需翻译。
- 与暗夜模式正交：主题走 CSS 变量，语言走 i18n，互不耦合。

## 权衡 / 风险

- 新增大约 3 个依赖；需在 Task 4 一并引入初始化，否则后续文案会散落硬编码。
- 字符串抽取需纪律：所有用户可见文案经 `t()`；review 时 grep 硬编码中文/英文。

## 后果

- Task 4（前端骨架）需新增 `src/i18n/` 与 `locales`，并在入口初始化 i18next。
- `handsoff.md` Task 4 备注追加"含 i18n 初始化 + 双语资源骨架"。
- Free/Pro 提示、设置面板、卡片徽标等均在双语资源内维护。
