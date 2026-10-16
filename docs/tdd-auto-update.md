# TDD — 自动更新（Auto Update）

> 独立技术设计。需求见 [`prd-auto-update.md`](prd-auto-update.md)。
> 总纲见 [`tdd.md`](tdd.md)（本文件由其中 §3.6 引入）。
> 分发前提见 [`tech-release-without-developer-id.md`](tech-release-without-developer-id.md)。

## 1. 方案选型

| 方案 | 结论 |
|---|---|
| Tauri 官方 `tauri-plugin-updater` | **不可用（上游依赖断裂）** |
| **自研更新器（复用现有产物管线 + `flate2`/`tar`）** | **采用** |
| Sparkle | 需额外框架与签名配置，与 Tauri 集成度低 |
| 只提示、让用户手动重跑安装命令 | 不满足「点更新后自动完成并重启」 |

### 为什么放弃官方插件

`tauri-plugin-updater` 依赖 `zip` crate：
`2.3–2.7` 要求 `zip ^2`，`2.8–3.0.0-alpha.3` 要求 `zip ^4`。
而 `zip` 在 crates.io 上**除 9.0.3 外全部被 yank**（9.0.3 发布于 2026-10-13），
导致 `^2` 与 `^4` **都无法解析**，插件当前无法构建。

Cargo 的 `[patch]` 无法把 `^4` 换成 `9.x`（版本必须满足原要求），
强行 fork 一个被 yank 的版本又会引入未知风险（大规模 yank 通常伴随问题）。
因此不阻塞在上游，改为自研。

### 自研方案为什么是低风险的

更新所需的一切**我们已经具备并且已验证过**（`install.sh` 一直在用同一套）：

| 能力 | 现有实现 |
|---|---|
| 版本清单 | gh-pages 上的 `releases/latest/VERSION.txt`、`checksums.txt` |
| 产物下载 | `.zip`（`ditto -c -k` 打包） |
| 完整性校验 | **SHA256**（已有 `sha2`） |
| 解压 | **`flate2` + `tar`**（v1.2 为自举 Python 运行时引入，已在用） |
| 替换运行中的 `.app` | macOS 原生 `launchctl submit`（脱离父进程执行替换脚本） |
| 重启 | 替换脚本末尾 `open` 新 bundle |

不需要新增任何依赖，也不依赖被 yank 的 crate。

## 2. 更新产物与清单

### 更新清单 `latest.json`

CI 每次发布生成，落在 gh-pages：

```
https://xsin.github.io/o-muse/releases/latest/latest.json
```

结构：

```json
{
  "version": "0.2.0",
  "notes": "本次更新要点…",
  "pub_date": "2026-10-16T09:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "url": "https://xsin.github.io/o-muse/releases/0.2.0/O Muse-0.2.0-arm64.zip",
      "sha256": "<zip 的 SHA256>"
    },
    "darwin-x86_64": { "url": "…", "sha256": "…" }
  }
}
```

> 复用现成的 `.zip` 产物；`sha256` 取自同一份 `checksums.txt`，**不额外产出签名文件**。

### 产物

除现有 `.zip`（curl 安装用）外，额外产出：

| 文件 | 用途 |
|---|---|
| `O Muse-<version>-<arch>.zip` | **同时**用于 curl 安装与自动更新 |
| `O Muse-<version>-<arch>.zip.sha256` | 完整性校验（两处共用） |
| `latest.json` | 更新清单（新增） |

> 安装与更新**共用同一份产物**，不存在两套包分叉的可能。

## 3. 完整性与传输安全

自研方案不使用独立签名密钥对，依赖**两层**保护：

1. **传输层**：全链路 HTTPS（GitHub Pages / `githubusercontent`）。
2. **内容层**：**SHA256 校验**。清单 `latest.json` 里携带 zip 的 SHA256，
   客户端下载后必须校验通过才允许安装；不匹配则**中止并删除下载物**。

> **安全边界的诚实说明**：`latest.json` 与 zip 同源（都来自 gh-pages），
> 因此无法抵御「同时篡改清单与产物」的攻击者。官方插件的独立密钥对能抵御这一点。
> 当前接受的取舍：公测期、无商业价值、且 gh-pages 由 GitHub 账号保护
> （账号启用 2FA）。
>
> **升级路径**：待 `zip` 依赖问题在上游解决后，迁移到 `tauri-plugin-updater`，
> 即可获得独立密钥对签名。届时 `latest.json` 结构增加 `signature` 字段，
> 客户端改用插件校验；**清单与产物 URL 保持不变**。

## 4. 配置

`src-tauri/tauri.conf.json` 无需新增 updater 配置（自研，不经插件）。

更新源在代码中以常量定义，便于审计与将来切换：

```rust
pub const UPDATE_MANIFEST_URL: &str =
    "https://xsin.github.io/o-muse/releases/latest/latest.json";
```

CI 生成 `latest.json` 时必须与 `tauri.conf.json` 的 `version` 一致（单一来源）。

## 5. 模块职责

### Rust

```
src-tauri/src/core/updater/mod.rs      # 清单解析、版本比较、下载校验、替换编排
src-tauri/src/commands/updater.rs      # Tauri 命令
src-tauri/src/lib.rs                   # 注册命令
```

命令（全部 `async` + `spawn_blocking`，不阻塞主线程）：

```rust
pub const UPDATE_MANIFEST_URL: &str =
    "https://xsin.github.io/o-muse/releases/latest/latest.json";
pub const PROGRESS_EVENT: &str = "omuse://update-progress";

#[derive(Serialize, Deserialize)]
pub struct UpdateManifest { version, notes, pub_date, platforms: HashMap<String, PlatformAsset> }
#[derive(Serialize, Deserialize)]
pub struct PlatformAsset { url: String, sha256: String }

#[derive(Serialize)]
pub struct UpdateInfo { current_version, version, notes, pub_date, download_bytes: Option<u64> }

#[tauri::command] pub async fn update_check(app) -> Result<Option<UpdateInfo>, String>;
#[tauri::command] pub async fn update_download(app) -> Result<UpdateInfo, String>;  // 下载+校验+解压
#[tauri::command] pub async fn update_install_and_restart(app) -> Result<(), String>;
#[tauri::command] pub async fn current_version() -> String;
```

进度经事件推送（与分轨运行时一致）：

```
omuse://update-progress → { stage: "downloading"|"verifying"|"extracting"|"installing", downloaded, total }
```

### 版本比较

**不能用字符串比较**（`"0.10.0" < "0.9.0"` 是错的）。实现 `parse_version -> (u64,u64,u64)`
并逐段数值比较；忽略预发布后缀（公测期不做 prerelease 排序）。

### 替换与重启（关键时序）

运行中的 `.app` 不能被自己覆盖，因此：

1. 下载 zip → 校验 SHA256 → 解压到 `<app data>/update/<version>/O Muse.app`
2. 生成替换脚本，内容固定为模板（**不含任何用户输入拼接**）：
   等待本进程 PID 退出 → 备份旧 bundle 到 `<app data>/update/backup` →
   用 `ditto` 替换 → 失败则回滚 → `open` 新 bundle → 删除自身
3. 经 `launchctl submit -l com.omuse.update-<uuid> -- /bin/sh <script>` **脱离父进程**执行
4. 应用主动退出

用 `launchctl submit` 而非普通子进程，是因为它由 launchd 接管，
**不受我们退出影响**。

### 前端

```
src/components/UpdateBanner.tsx     # 顶部提示条（非模态）
src/pages/SettingsPage.tsx          # 「更新」区块：版本、自动检查开关、手动检查
src/store/useUI.ts                  # 自动检查开关 + 「稍后」状态
```

状态机：

```
idle ──检查──▶ checking ──有新版──▶ available ──更新──▶ downloading ──▶ installing ──▶ restart
                  │                     │
                  └──无/失败──▶ idle     └──稍后──▶ dismissed(本次会话)
```

## 6. 关键行为

### 启动检查的时机

启动后**延迟 5 秒**再检查，避免与首屏渲染、插件扫描争抢资源。
随后每 6 小时一次（用 `tauri::async_runtime` 定时，不引入额外调度依赖）。

### 静默失败

`update_check` 在以下情况返回 `Ok(None)` 而非 `Err`，前端因此**不弹错误**：
网络不可达、DNS 失败、超时、服务端非 2xx。

只有「用户主动点击检查」时，才把失败原因展示出来（`Ok(Err)` 语义由前端区分）。

### 验签失败必须中止

`update_install` 依赖插件的签名校验；校验不通过时**不执行替换**，
并把失败原因回传。前端以**危险色**提示，且不提供「仍然安装」。

### 重启前的任务检查

若分轨任务正在进行（`stem_status` 为 processing），重启前提示
「分轨任务将中断，确认重启？」，避免用户丢失长任务。

### 用户数据保留

更新只替换 `/Applications`（或 `~/Applications`）下的 `.app`；
应用数据在 `~/Library/Application Support/work.xsin.omuse/`，**不在 bundle 内**，
因此 license、偏好、插件缓存、分轨运行时（约 900MB）都不会重装。

## 7. CI 变更

在既有 `release.yml` 中追加一步：生成 `latest.json` 并推到 gh-pages。

- 版本号读取 `src-tauri/tauri.conf.json`（单一来源）。
- `sha256` 从同目录的 `checksums.txt` 中提取，**不重新计算**，避免两份不一致。
- `url` 指向版本化目录的 zip。
- `notes` 取 Release notes 的首段（或写入固定的简短说明）。

`latest.json` 落在 `releases/latest/`，与 `install.sh`、`VERSION.txt`、`checksums.txt` 同级。

## 8. 测试

### 可在 CI/本地自动验证

- Rust 单测：`UpdateInfo` 的 serde 往返；版本比较（`0.10.0 > 0.9.0`，不能按字符串比）；
  `latest.json` 解析（含缺字段、空 platforms 的容错）。
- 前端：typecheck + build。
- 清单契约：用一个固定的 `latest.json` 样本断言解析结果。

### 必须真机验证（无法自动化）

- macOS 上「下载 → 验签 → 替换运行中的 `.app` → 重启」的完整时序。
- 篡改签名后的拒绝行为。
- 从 `~/Applications` 与 `/Applications` 两种安装位置更新。

这些留给人工验证，并在 `handsoff.md` 记录。

## 9. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 替换运行中的 `.app` 时序问题 | 交给官方插件；失败时保留旧 bundle 并提示 |
| 密钥轮换导致老客户端无法更新 | 公告 + 手动重装指引 |
| 更新源域名变更 | 保持 `xsin.github.io/o-muse` 稳定；变更前先发一版带新 endpoint 的过渡版本 |
| 用户处于离线/弱网 | 静默失败，不打扰 |
| 更新包体积偏大 | `.app` 约 21MB，可接受；后续可评估差量更新 |

## 10. 与既有约束的关系

- **不新增依赖**：自研更新器复用已有 `reqwest` / `sha2` / `flate2` / `tar`。
- **不调用 `std::process::Command`**：外部进程一律经 `tauri_plugin_shell`；\n  替换脚本的脱离式执行用 macOS 原生 `launchctl submit`。
- **依赖全部最新**：安装时锁定当时的 latest，并记入 `docs/references.md`。
- **许可合规**：新增依赖须补记到 `docs/references.md`。
