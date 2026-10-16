# 技术方案：macOS 公测期一键安装分发（不使用 Apple Developer ID）

> 状态：已实现（公测阶段）。
> 适用范围：O Muse v0.1.0+，macOS 10.13+。
> 目标读者：维护者、贡献者，以及需要审计安装脚本的用户。

---

## 1. 背景与目标

当前处于**公测反馈收集阶段**：需要让 macOS 用户用一条 `curl` 命令装上能跑的 O Muse，
同时**不购买 Apple Developer Program、不经 App Store、不做公证**。

Apple Developer Program 年费对尚未验证产品市场匹配度的阶段是沉没成本；而不做公证的
`.app` 在手动下载场景会被 Gatekeeper 拦（"已损坏"提示）。本方案用两个手段规避：

1. **分发载体用 `.zip` 而非 `.dmg`**：经浏览器/curl 下载的 zip 内的 `.app` **不会**
   被打上 `com.apple.quarantine` 隔离属性（Gatekeeper 只对经"下载"路径的
   bundle 类文件生效），因此首次启动通常不受阻。
2. **安装脚本主动清理隔离属性**：即使用户从别处拿到被标记的文件，脚本也
   `xattr -dr com.apple.quarantine` 兜底。

将来拿到 Developer ID 后，**只需改环境变量和追加公证步骤，安装脚本结构不变**
（见 §7）。

### 1.1 为什么用 ad-hoc 签名

| 选项 | 结果 |
|---|---|
| 完全不签名 | Apple Silicon 上**无法运行**（内核要求 Mach-O 至少要有签名） |
| ad-hoc（`codesign --sign -`） | 可运行，本地 `codesign --verify` 通过；无身份、无公证 |
| Developer ID 签名 + 公证 | 完整 Gatekeeper 放行（**当前不采用**） |

所以 ad-hoc 签名是当前阶段**唯一可行**选项，且它保留了将来切换到真实签名的能力
——签名身份是可配置的（`SIGN_IDENTITY`，见 §6.3）。

---

## 2. 前置修复：缺失的二进制入口（打包阻断）

实现本方案时发现：仓库此前**没有 `src-tauri/src/main.rs`**，而 `Cargo.toml` 的
`[lib]` 只输出 `staticlib`/`cdylib`/`rlib`。后果是 `cargo build --bins` 匹配不到
任何目标、不产出可执行文件，Tauri CLI 因而报
`failed to bundle project: can't open main binary .../o-muse`——
**`.app` / `.dmg` 从 Task 6 起一直无法产出**。

已按 Tauri 2 标准模式补齐 `main.rs`：

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() { music_rig_lib::run(); }
```

补齐后 `tauri build` 正常产出 `O Muse.app`（含 sidecar）。

## 3. 当前阶段硬性约束（已落实到实现）

1. 不使用 App Store；`tauri.conf.json` 无任何 App Store 配置。
2. 不使用 Developer ID 证书，不做公证。
3. 对 `.app` 使用 ad-hoc 签名：
   - `codesign --force --deep --sign - "<AppName>.app"`
   - 签名后执行 `codesign --verify --deep --strict --verbose=2`
   - **不依赖 `--deep` 单独覆盖嵌套 bundle**：O Muse 有一个 sidecar 可执行文件
     （`src-tauri/bin/python-stem-backend`，由 `bundle.externalBin` 注入
     `Contents/MacOS/`），脚本按"从内到外"的顺序先签 sidecar 再签外层 app，
     详见 §5.2。
4. 主分发格式：`.zip`，用 `ditto -c -k --keepParent` 打包（保留 macOS 元数据/扩展属性）。
5. 可选同时产出 `.dmg` 供手动下载，但**文档主推 curl 安装**。
6. 托管在 GitHub Releases（见 §5.4）。

---

## 4. 目录与产物结构

```text
releases/
  latest/
    install.sh            # 稳定 URL，指向最新版逻辑（纳入 git，可审计）
    VERSION.txt           # 当前最新版本号（install.sh 用它把 latest 解析成真实版本）
    checksums.txt         # 最新版校验和（安装脚本优先读这里）
  <version>/
    O Muse-<version>-universal.zip
    O Muse-<version>-universal.zip.sha256
    O Muse-<version>-universal.dmg      # 可选，手动下载用
    O Muse-<version>-universal.dmg.sha256
    checksums.txt
```

约定：

- **支持 universal 包**；若某次构建没有 universal（例如只装了单一 target），
  则退化为 `arm64` / `x86_64` 分别打包，命名 `O Muse-<version>-<arch>.zip`。
- 安装脚本从 `/releases/latest/install.sh` 取脚本，**运行时**再按架构选包。
- **版本号单一来源**：从 `src-tauri/tauri.conf.json` 的 `version` 读取
  （`scripts/release/lib/version.sh` 封装），不硬编码到多个位置。
- SHA256 校验文件：每个产物一份 `<file>.sha256`，外加一份汇总 `checksums.txt`。

### 4.1 关于 `releases/` 是否入库

二进制产物（zip/dmg/checksums）**不入库**（`.gitignore` 排除），真实产物由 CI
上传 GitHub Releases。但 **`releases/latest/install.sh` 纳入版本控制**：
它需要被用户 `less` 审计，且要能从一个稳定路径取到，因此单独开白名单：

```
releases/*
!releases/latest/
releases/latest/*
!releases/latest/install.sh
```

`VERSION.txt` 不入库——它在每次发布时由 `build.sh` 生成，来源仍是
`tauri.conf.json` 的 `version`。

发布时同一批产物会**同时**进入两个通道（见 §6.4）：`gh-pages` 分支的
`releases/` 目录（安装通道），以及同名 tag 的 GitHub Release（归档通道）。
两者的 `install.sh` / `VERSION.txt` / `checksums.txt` / zip 内容完全一致。

### 4.2 latest 如何解析到真实版本

产物按**真实版本号**命名（`O Muse-0.1.0-arm64.zip`），`latest` 只是标签。
安装脚本默认行为：

1. 读 `<base>/latest/VERSION.txt` 得到真实版本号；
2. 按 `<base>/<version>/<file>` 与 `<base>/<version>/download/<file>`
   **两种布局依次尝试**（前者适配自建/Pages，后者适配 GitHub Releases）；
3. 取不到 `VERSION.txt` 时回退为 `latest-<arch>.zip` 命名。

---

## 5. 安装脚本 `install.sh`

交付物：`releases/latest/install.sh`（由 `scripts/release/install.sh` 同步生成）。
要求逐条对应实现：

### 5.1 两种用法

```bash
# 快速安装
curl -fsSL https://xsin.github.io/o-muse/releases/latest/install.sh | bash

# 可审计安装（先看再跑）
curl -fsSL https://xsin.github.io/o-muse/releases/latest/install.sh -o install.sh
less install.sh
bash install.sh
```

脚本被设计为**可直接通过管道执行**（不依赖调用方传入 `$0` 或参数文件）。

### 5.2 安全与健壮性

- `set -euo pipefail`
- 临时目录 `mktemp -d`，退出时 `trap` 清理
- 架构检测：`uname -m` → `arm64` / `x86_64`，**优先 universal 包**
- 下载：`curl -fL --retry 3 --retry-delay 2`
- **SHA256 校验**：优先读 `checksums.txt`，回退 `<file>.sha256`；失败立即退出
- 解压：`ditto -x -k`（保留元数据与扩展属性）
- 退出旧版本：`pkill -x "O Muse" || true`
- 安装位置：
  - 默认 `~/Applications/O Muse.app`（**无需 sudo**）
  - `--system` → `/Applications/O Muse.app`（需要 sudo）
- 防御性移除隔离属性：`xattr -dr com.apple.quarantine … || true`
- `chmod +x` 主可执行；默认 `open` 启动，`--no-launch` 跳过
- 参数：`--version <v>` / `--install-dir <dir>` / `--no-launch` / `--system`
- 清晰日志：版本、架构、安装路径、校验结果
- **覆盖更新不删用户数据**：安装只替换 `.app` bundle；O Muse 的
  用户数据（license key、UI 偏好、插件缓存）位于
  `~/Library/Application Support/work.xsin.omuse/`，**不在** bundle 内，
  因此覆盖安装天然不影响数据。

### 5.3 签名顺序（不只用 `--deep`）

`--deep` 对含嵌套可执行代码的 bundle 在新版 macOS 上已不可靠。构建脚本按
**由内到外**顺序单独签名：

1. `Contents/MacOS/python-stem-backend`（sidecar 可执行文件）
2. 任何 `Contents/Frameworks/*.framework`（当前无）
3. 外层 `O Muse.app`

然后整体 `codesign --verify --deep --strict --verbose=2` 复核。

> 注意：ad-hoc 签名（`-`）**不含 entitlements**。sidecar 由 Tauri 通过
> `tauri_plugin_shell` 的 sidecar API 拉起（非 `std::process::Command`），
> 在 ad-hoc 签名下可正常工作；若将来启用 hardened runtime，需为 sidecar
> 显式授予 `com.apple.security.cs.allow-unsigned-executable-memory` 等权利
> （见 §7）。

---

## 6. 构建与 CI

### 6.1 本地/CI 构建脚本

- `scripts/release/lib/version.sh`：从 `tauri.conf.json` 读版本号
- `scripts/release/build.sh`：构建 → ad-hoc 签名 → 验证 → zip → sha256 → 归档
- `scripts/release/install.sh`：安装脚本本体（被拷贝到 `releases/latest/`）
- `.github/workflows/release.yml`：push tag 触发，产物上传 GitHub Releases

### 6.2 版本号来源

`version.sh` 从 `src-tauri/tauri.conf.json` 的 `version` 字段读取，构建与
`install.sh` 的 `--version` 参数都以它为唯一来源，避免多处硬编码。

### 6.3 未来签名/公证预留环境变量

| 变量 | 当前值 | 将来 |
|---|---|---|
| `SIGN_IDENTITY` | `-`（ad-hoc） | `Developer ID Application: <Name> (<TEAM_ID>)` |
| `NOTARIZE` | `0` | `1` |
| `APPLE_ID` | 空 | 公证用 Apple ID |
| `TEAM_ID` | 空 | 团队 ID |
| `APP_SPECIFIC_PASSWORD` | 空 | 公证用 App 专用密码 |

`build.sh` 已读取这些变量：`SIGN_IDENTITY` 决定 `codesign --sign` 的签名身份；
`NOTARIZE=1` 时才执行 `notarytool submit` / `stapler staple`（当前为 no-op 分支）。

### 6.4 双通道发布：gh-pages + GitHub Release

每次发布**同时**产出两个通道，二者由同一次构建产生、内容一致，不会分叉：

| 通道 | 定位 | 内容 | 特点 |
|---|---|---|---|
| `gh-pages` 分支 | **安装通道** | `install.sh`、`VERSION.txt`、`checksums.txt`、zip | 可变 `latest` 指针，`curl` 的默认来源 |
| GitHub Release | **版本归档** | 同一批产物 + `install.sh` + `VERSION.txt` | 不可变 tag 快照、变更说明、人工下载入口、自动源码包、关注者通知 |

为什么要两个都发（惯例做法）：

- Release 面向**人和流程**：测试用户能看到"这一版改了什么"，可手动下载，
  watch 仓库的人会收到通知，`/releases/latest` 提供稳定的跳转页。
- gh-pages 面向**安装脚本**：需要一个始终指向最新版的**可变**入口，
  而 Release 按定义是不可变的，不能承担这个职责。

### 6.4.1 托管与 URL

**安装通道**：GitHub Pages（`gh-pages` 分支）。CI 把产物写到该分支的
`releases/` 下，Pages 直接静态服务：

```
https://xsin.github.io/o-muse/releases/latest/install.sh
https://xsin.github.io/o-muse/releases/<version>/O Muse-<version>-<arch>.zip
```

安装脚本默认基址即该 URL，无需任何配置即可 `curl | bash`。

基址可用 `OMUSE_BASE_URL` 覆盖，便于迁到自建 CDN 或 GitHub Releases。
脚本会先把基址**归一化为 releases 根**——下列三种写法等价，
都归一到 `https://host/releases`，不会拼出 `.../latest/latest/...`：

```
https://host/releases
https://host/releases/latest
https://host/releases/latest/download
```

因此迁移时安装脚本**无需改动**：脚本对 `<version>/` 与
`<version>/download/` 两种产物布局都会依次尝试，命中即用。

> **仓库需启用 Pages**：Settings → Pages → Source = `Deploy from a branch`
> → 分支 `gh-pages`、目录 `/ (root)`。未启用时 `curl` 会 404，
> 脚本会明确报错而不会静默失败。

**归档通道**：`gh release create`，tag 形如 `v<version>`。
用户可直接从 Release 页面手动下载 zip。

### 6.4.2 把基址指向 Releases 也能用

脚本同时兼容两种托管的路径布局与命名差异，故 `OMUSE_BASE_URL` 指向
`https://github.com/<owner>/<repo>/releases` 亦可正常工作：

| 差异点 | gh-pages | GitHub Releases |
|---|---|---|
| 产物路径 | `<root>/<version>/<file>` | `<root>/download/<tag>/<file>` |
| tag vs 版本号 | 相同（`0.1.0`） | tag 常带 `v`（`v0.1.0`） |
| asset 文件名 | 保留空格 | **空格被替换为点** |
| `latest` 入口 | `<root>/latest/VERSION.txt` | `<root>/latest/download/VERSION.txt` |

对应处理：候选基址会同时列出 `<root>/<v>`、`<root>/download/<v>`、
`<root>/download/v<v>`；文件名与 checksums 匹配时把**空格与点视为等价**；
`latest` 解析会依次尝试两种入口。

### 6.5 静态托管的两个实战坑（已在脚本中处理）

静态托管（GitHub Pages / CDN）的 404 行为与普通服务器不同，
安装脚本必须**不依赖状态码**判定"文件是否存在"：

1. **Pages 对缺失文件返回 `HTTP 404` 但 body 是 HTML**；经代理链路时
   还可能出现 `HTTP/1.1 200` 的中间响应，令 `curl` 以退出码 56
   （连接重置）结束，而非 `-f` 期望的 22。
   → 脚本改为**落盘后校验 zip 魔术字节**（`PK\x03\x04`），以内容为准。
2. **`set -e` + `pipefail` 下命令替换失败会终止整个脚本**。
   所有"允许失败"的网络取数（如 `VERSION.txt` 探测）显式 `|| true` 兜底，
   避免一次网络抖动就让安装中断。
3. **GitHub 会把 Release asset 名里的空格换成点**
   （`O Muse-0.1.0-arm64.zip` → `O.Muse-0.1.0-arm64.zip`），
   而 `checksums.txt` 里写的仍是原始名。
   → 下载时对同一基址尝试"原名"和"点号变体"两种文件名；
   校验和匹配时把空格与点归一化后再比较。

---

## 7. Dodo Payment 注意事项

1. **分发方式不影响支付/授权逻辑**：Dodo 校验仍走
   `core/license/dodo.rs`（HTTP 校验 + TTL 缓存），本次改动未触碰该逻辑。
2. **客户端只含公开 key，绝不嵌入 secret**：`DODO_API_KEY` /
   `DODO_PRODUCT_ID` 通过**环境变量**注入（`.env`，gitignore），
   客户端二进制不含密钥；`install.sh` / CI 亦不携带任何 Dodo 凭据。
3. **license 存储位置**：O Muse 的 license key 存于
   `tauri-plugin-store`（`o-muse.dat`，位于 Application Support 目录的
   普通 JSON 文件），**不使用 Keychain**。
   因此 ad-hoc 签名与将来 Developer ID 签名切换时**不会**发生
   Keychain 访问权限重置，**无需迁移逻辑**（§5.4 路径不变）。
4. 公测阶段如需测试 license，通过 Dodo Test Mode 的 key 走**完整校验路径**，
   不提供绕过校验的后门。

---

## 8. 未来切换到 Developer ID

拿到 Developer ID 后仅需：

1. 设 `SIGN_IDENTITY="Developer ID Application: …"`（替换 `-`）
2. 启用 hardened runtime（`codesign --options runtime`），并为 sidecar 补齐
   entitlements（若 hardened runtime 下 sidecar 拉起被拒）
3. `NOTARIZE=1`，配置 `APPLE_ID` / `TEAM_ID` / `APP_SPECIFIC_PASSWORD`
4. `xcrun notarytool submit … --wait` + `xcrun stapler staple …`
5. 安装脚本结构**不变**，仅追加 `spctl -a -vv` 评估验证
6. **zip/dmg 命名、checksums 格式、安装路径、`releases/latest/install.sh`
   稳定 URL 全部保持不变**——用户侧零感知

---

## 9. 用户安装说明（README 片段）

见 `README.md` 的「安装（公测版）」一节，要点：

```bash
curl -fsSL https://xsin.github.io/o-muse/releases/latest/install.sh | bash
```

- 当前为公测版，未使用 Apple 开发者签名与公证。
- 通过 curl 安装的 `.zip` 通常不会触发 Gatekeeper 隔离。
- 若首次打开仍被阻止：右键 → 打开，或「系统设置 > 隐私与安全性 > 仍要打开」。
- 若提示"已损坏"：
  `xattr -dr com.apple.quarantine ~/Applications/Music\ Rig.app`
- 更新：重新运行同一条命令即可，用户数据保留。

---

## 10. 验收标准与验证方式

| 标准 | 验证方式 |
|---|---|
| `curl … install.sh \| bash` 后应用装到 `~/Applications/O Muse.app` | `bash install.sh --install-dir <tmp>` 干跑 + 真实安装验证 |
| 签名校验通过 | `codesign --verify --deep --strict --verbose=2` |
| SHA256 校验通过 | 篡改 zip 后安装脚本必须报错退出 |
| 应用自动启动 | 默认 `open`；`--no-launch` 跳过 |
| 无"已损坏"提示 | zip 分发 + `xattr -dr com.apple.quarantine` |
| 重复执行可覆盖更新、数据保留 | 二次安装，检查 Application Support 数据未变 |
| Dodo 授权流程正常 | 复用现有 `license` 模块，未改动 |
| 将来只改 `SIGN_IDENTITY` + 公证，不重写脚本 | 环境变量已参数化，公证分支已预留 |
| gh-pages 安装通道可用 | `curl -fsSL https://xsin.github.io/o-muse/releases/latest/install.sh \| bash` |
| Release 归档通道可用 | Release 页面可手动下载 zip；`OMUSE_BASE_URL` 指向 Releases 亦能安装 |

本地可复现的验证命令见 §10.1。

### 10.1 本地验证（不购买 Developer ID）

```bash
# 1) 打 universal 或单架构包
scripts/release/build.sh --arch arm64

# 2) 校验安装脚本语法与静态检查
shellcheck releases/latest/install.sh

# 3) 干跑安装到临时目录（不启动、不写系统目录）
bash releases/latest/install.sh --install-dir /tmp/omuse-install --no-launch

# 4) 校验失败路径：故意损坏 zip，脚本应报错退出

# 5) 两个通道都要能装（gh-pages 与 Releases）
bash releases/latest/install.sh --install-dir /tmp/omuse-a --no-launch
OMUSE_BASE_URL="https://github.com/xsin/o-muse/releases" \
  bash releases/latest/install.sh --install-dir /tmp/omuse-b --no-launch
```

---

## 11. 已知限制

1. **ad-hoc 签名无身份**：`spctl -a -vv` 对 ad-hoc 应用会返回
   "rejected (the code is not signed at all)" 之类的评估失败——这是**预期**的，
   Gatekeeper 不认 ad-hoc；规避手段是 zip 分发 + 清隔离属性（§1）。
2. **universal 包需两个 target**：`aarch64-apple-darwin` 已安装；
   `x86_64-apple-darwin` 需 `rustup target add x86_64-apple-darwin`。
   未安装时构建脚本自动退化为单架构包并如实命名。
3. **Pages 需手动开启**：仓库 Settings → Pages 必须指向 `gh-pages` 分支；
   未开启时 `curl` 会 404，脚本会明确报错（不会静默失败）。
4. **Rosetta**：x86_64 包在 Apple Silicon 上依赖 Rosetta 2；universal 包无此问题，
   故仍以 universal 为优先。
5. **分轨功能依赖本地 Python**：`python-backend/` 源码随包分发到
   `Contents/Resources/python-backend`（`bundle.resources`），
   但**不内置 Python 运行时**——sidecar 启动器会依次寻找项目内 venv、
   再回退系统 `python3`。
   首次使用前需安装依赖：

   ```bash
   python3 -m pip install -r python-backend/requirements.txt
   ```

   （含 `demucs`，会拉取 PyTorch，体积较大。）
   缺依赖时应用会直接显示**可操作**的错误提示（含安装命令），
   而不是干等 30s 超时。
6. **其余功能不受影响**：插件扫描、Kontakt 库盘点、i18n、主题、
   许可激活均不依赖 Python，安装后即可用。
