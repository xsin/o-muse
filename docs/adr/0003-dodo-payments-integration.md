# ADR-0003: v1 接入 Dodo Payments 真实订阅校验

- **状态**：已采纳（2026-10-15）
- **决策人**：用户（指令，覆盖 plan Task 5）
- **相关**：plan Task 5（"不要实现实际的订阅验证网络请求"被本 ADR 取代）

## 背景

plan Task 5 原写：`current_tier()` 在 v1 始终返回 `Free`，且**"不要在此 Task 中实现实际的订阅验证网络请求"**。

工作区 `.env` 已包含真实凭据：
- `DODO_PRODUCT_ID`（产品 id）
- `DODO_API_KEY`（Test Mode Key，已备好）

用户于 2026-10-15 明确：**"v1 就要接 Dodo 真实校验。产品 id 和测试 key 已备好。"**

## 决策

- v1 即接入 **Dodo Payments** 订阅校验（Test Mode）。
- `src-tauri/src/license/mod.rs` 的 `current_tier()` 改为：读取 `tauri-plugin-store` 中的 `license_key` → 调用 Dodo API（`DODO_API_KEY` + `DODO_PRODUCT_ID`）验证订阅有效性 → 推导 `Free` / `Pro`。
- 网络调用仅在存在 `license_key` 时触发；无 key 时回退 `Free`。
- `.env` 中的 `DODO_*` 凭据**不提交进 git**（加入 `.gitignore`），由本地/构建环境注入（Tauri 通过 `tauri-plugin-shell` 的环境或 `tauri.conf.json` 的 `env` 读取；具体见 TDD）。
- 前端新增"激活 Pro / 输入 license_key"入口（在 Pro 边界被拦截时引导）。

## 理由

- 用户已具备测试凭据，要求 v1 即验证，避免 v1.1 才补齐导致付费链路滞后。
- 与 plan 的 Free/Pro 边界表（§五）一致，仅把"校验时机"从 v1.1 提前到 v1。

## 权衡 / 风险

- **密钥泄露**：`.env` 含 API key，必须 `.gitignore`；若误提交，立即轮换 Test Key。
- **网络依赖**：校验需外网；设计回退（无网时维持上一次缓存的 tier，或 `Free`）。
- **合规**：Dodo 调用于自研 `license` 模块，不触碰 Apache-2.0 / MIT 上游代码。

## 后果

- plan Task 5 的 `禁止事项: 不要实现订阅验证网络请求` **作废**（被本 ADR 覆盖）。
- handsoff.md 中 Task 5 需标注"含 Dodo 集成"，验证命令增加"激活后 Pro 命令成功"。
- `tauri-plugin-store` 用于持久化 `license_key` 与上次校验结果（带 TTL 缓存）。
