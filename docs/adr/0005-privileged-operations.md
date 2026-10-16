# ADR-0005：提权操作策略

- 状态：已采纳（v1.3）
- 影响范围：Kontakt 库注册（系统级文件读写）

## 背景

Kontakt 的库注册分布在多个位置，其中两个位于 `/Library` 下，属 `root` 所有：

| 路径 | 属主/权限 | 用途 |
|---|---|---|
| `/Library/Application Support/Native Instruments/Service Center/<Name>.xml` | `root:admin 755` | Kontakt 经典浏览器据此识别音色库 |
| `/Library/Preferences/com.native-instruments.<RegKey>.plist` | `root:wheel 755` | 记录库的内容路径与可见性 |

另外两个位置无需提权即可写：

| 路径 | 权限 | 用途 |
|---|---|---|
| `/Users/Shared/Native Instruments/installed_products/<Name>.json` | `rwxrwxrwx` | Kontakt 7/8 的内容路径记录 |
| `~/Library/Preferences/com.native-instruments.<RegKey>.plist` | 用户所有 | 用户级注册与列表顺序 |

项目早期把"不运行 `sudo`/`su`/任何提权命令"列为全局禁止，因此首版只做免提权路径，
系统级缺口以 `UnprivilegedWriteRequired` 提示用户自行处理。

对 Kontakt 6 及更早版本、以及依赖经典浏览器的场景，缺少系统级注册会导致库**不出现**，
仅靠提示无法真正解决问题。故解禁提权。

## 决策

**允许提权，但必须同时满足以下三条：**

1. **明确告知**——在发起授权前，向用户逐条列出**具体的操作对象（完整文件路径）**
   与**原因**，并说明是否会影响磁盘上的音色文件。
2. **最小权限**——提权仅用于写入上表中两个 `/Library` 路径下的**直接子文件**，
   不接受任意路径、任意命令。
3. **一次性**——不安装 LaunchDaemon / 后台常驻服务 / 登录项；每次操作独立授权、执行完即退出。

## 实现方式

因本项目为 **ad-hoc 签名**（无 Developer ID），`SMJobBless` / 特权 Helper Tool
不可用。因此采用 macOS 原生授权：

```bash
osascript -e 'do shell script (…) with administrator privileges'
```

由系统弹出标准管理员密码框，用户可拒绝。授权后执行**一次性助手**——
复用主二进制的隐藏入口：

```
o-muse --privileged-helper <request.json> <sha256>
```

### 请求文件与校验

以普通用户身份把请求写到 `0700` 目录 / `0600` 文件，并把其 SHA-256 传给助手。
助手（以 root 运行）在**做任何事之前**逐项校验：

- `geteuid() == 0`
- 请求文件 canonicalize 后位于 `/private/tmp`、`/var/folders`、`/private/var/folders` 之下
- 必须是常规文件（拒绝符号链接），权限位恰为 `0600`，大小 ≤ 4 MiB
- SHA-256 与入参**常数时间**比对
- `version == 1`

### 路径白名单

仅允许操作以下两个目录的**直接子文件**（无更深层级）：

- `/Library/Application Support/Native Instruments/Service Center/`
- `/Library/Preferences/`

并校验：绝对路径、父目录 canonicalize 后在前缀内、文件名非空且 ≤255 字节、
不含 `/`、非 `.`/`..`、无控制字符。XML 内容必须含 `<ProductHints>`；
plist 必须可解析。写入 ≤ 2 MiB。

### 原子性与回滚

写入使用"临时文件 + rename"的原子替换；执行前对每个将被覆盖/删除的目标做内存备份，
**任一操作失败则整体回滚**。目标路径逐级检查符号链接。

`reason` 字段仅供审计记录，**不参与任何执行逻辑**。

## 被否决的方案

| 方案 | 否决原因 |
|---|---|
| `SMJobBless` 特权 Helper Tool | 需要 Developer ID 签名，当前为 ad-hoc |
| 直接把 `sudo` 交给用户 | 无法限定操作范围，且把安全责任转嫁给用户 |
| 始终免提权、只做提示 | Kontakt 6 / 经典浏览器场景功能不完整 |
| 让用户手动复制文件 | 体验差且易错 |

## 后果

**正面**：Kontakt 库注册在 6/7/8 全版本可用；提权范围被白名单严格限制；
用户每次都被明确告知操作对象与原因，可随时拒绝。

**代价**：多一次系统授权弹窗；助手校验逻辑本身是 root 权限代码，
必须保持严格（已用单元测试覆盖路径白名单、权限位、SHA-256、回滚等防线）。

**未来**：取得 Developer ID 后可迁移到 `SMJobBless`，届时授权体验更平滑，
但**白名单与告知要求不变**。
