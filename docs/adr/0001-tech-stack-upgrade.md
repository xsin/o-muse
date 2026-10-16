# ADR-0001: 技术栈升级至当前最新稳定版

- **状态**：已采纳（2026-10-15）
- **决策人**：用户（指令）+ Agent（事实核查）
- **相关**：plan.md §1.4

## 背景

plan.md §1.4 原标"技术栈决策（不可更改）"。用户于 2026-10-15 将 §1.4 改为"（可升级为最新版）"，并指示"如有新版本就升级"。Agent 核查了 crates.io / npm / PyPI 当前稳定版本。

## 决策

在保留 **Tauri 2 线**（v3 仍是 alpha，未稳定）的前提下，升级到当前最新稳定版。锁定版本：

| 层 | 采用版本 | 备注 |
|---|---|---|
| Tauri (crate + js api/cli) | 2.12.4 / 2.12.3 | 保持 v2 线 |
| plugin-shell / store / sql | 2.5.0 / 2.5.0 / 2.6.0 | |
| Rust: quick-xml | 0.42.0 | 由 0.36 升 |
| Rust: plist | 1.10.1 | 由 1.7 升 |
| Rust: sha2 | 0.11.1 | ⚠️ 破坏性 API（digest 调用方式变化） |
| Rust: chrono/walkdir/rayon/thiserror/serde | 0.4.45 / 2.5.0 / 1.12.0 / 2.0.21 / 1.0.229 | |
| React / react-dom | 19.3.1 | 大版本 |
| react-router-dom | 7.18.4 | data router |
| Vite | 8.3.4 | 大版本 |
| @vitejs/plugin-react | 6.1.2 | |
| TypeScript | 7.0.2 | 大版本 |
| Zustand | 5.0.15 | 大版本 |
| TailwindCSS | 4.x | plan §1.4 指定 |
| Python 运行时 | 3.14 | 本机；requirements 跟随 |
| fastapi / uvicorn / demucs / librosa / pyloudnorm | 0.143.1 / 0.54.0 / 4.1.0 / 1.0.0 / 0.2.0 | |

## 理由

用户明确要求升级；上述版本为核查时各注册表的最新稳定版，可最大化安全性与维护寿命。Tauri 2 线成熟稳定且与 v1 范围匹配，无需追 v3 alpha。

## 权衡 / 风险

- **sha2 0.11** 相对 0.10 有 breaking API，移植时需改写 hash 调用。
- **React 19 / Vite 8 / TS 7** 较新，与 Tauri 官方模板存在摩擦风险。若在某 Task 卡死，回退到 React 18 LTS + Vite 5 作为兜底（记录于对应 Task 的"禁止事项/回退"）。
- 升级不改动 **架构决策、许可证策略、目录结构形态**（仍受 §1.5 约束）。

## 后果

- 所有 `[dependencies]` 版本号需按上表在 Task 1 写入 `Cargo.toml`。
- `npm install` 拉取的 React 生态为 19/7/8 线。
- 该 ADR 不取代 plan 的"禁止更改架构/许可证"约束，仅覆盖版本号。
