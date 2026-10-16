// 自研自动更新器（v1.3）
//
// 为什么自研：官方 `tauri-plugin-updater` 依赖 `zip` crate，而 `zip` 在
// crates.io 上除 9.0.3 外**全部被 yank**，`^2` 与 `^4` 都无法解析，插件
// 当前无法构建（详见 docs/tdd-auto-update.md §1）。
//
// 因此复用我们已经具备并已验证的能力，**不新增任何依赖**：
//   - 清单：gh-pages 上的 `releases/latest/latest.json`（本模块解析）
//   - 传输：`reqwest::blocking`（已有 blocking 特性）+ 全链路 HTTPS
//   - 校验：`sha2`（边下边算，大小写不敏感比较，失败即删包）
//   - 解压：`flate2` + `tar`（`zip` 不可用，故产物改用 `.app.tar.gz`）
//   - 替换运行中的 `.app`：macOS 原生 `launchctl submit` 接管固定模板脚本
//
// 安全边界（诚实说明，见 TDD §3）：清单与产物同源（都来自 gh-pages），
// 无法抵御「同时篡改清单与产物」的攻击者。当前取舍：公测期 + 账号 2FA。
// 待上游 `zip` 恢复后迁移官方插件即可获得独立密钥对签名。

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// 更新清单地址（代码内常量，便于审计与将来切换）。
pub const UPDATE_MANIFEST_URL: &str =
    "https://xsin.github.io/o-muse/releases/latest/latest.json";

/// 更新进度事件名（与分轨运行时一致的前缀约定）。
pub const PROGRESS_EVENT: &str = "omuse://update-progress";

/// 检查更新的网络超时（需求：静默失败优先，不能让用户等）。
const CHECK_TIMEOUT_SECS: u64 = 10;

/// 下载超时（`.app` 约 21MB，弱网也够用）。
const DOWNLOAD_TIMEOUT_SECS: u64 = 1800;

/// 分块读取大小：64KB。
const CHUNK: usize = 64 * 1024;

/// 更新会话目录名（位于 `<app data>/update/`）。
pub const UPDATE_DIR_NAME: &str = "update";

/// 暂存的 bundle 目录名。
pub const STAGED_APP_NAME: &str = "O Muse.app";

/// 清单中的平台资产。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformAsset {
    pub url: String,
    pub sha256: String,
}

/// 更新清单（由 CI 生成，托管在 gh-pages）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub pub_date: String,
    pub platforms: HashMap<String, PlatformAsset>,
}

/// 返回给前端的「有新版本」信息。
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub version: String,
    pub notes: String,
    pub pub_date: String,
    /// 更新包体积（来自清单 GET 的 Content-Length）；未知为 None。
    pub download_bytes: Option<u64>,
}

/// 进度事件载荷：`stage` 取值见模块内 `STAGE_*` 常量。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProgress {
    pub stage: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub message: String,
}

pub const STAGE_DOWNLOADING: &str = "downloading";
pub const STAGE_VERIFYING: &str = "verifying";
pub const STAGE_EXTRACTING: &str = "extracting";
pub const STAGE_INSTALLING: &str = "installing";

// ── 版本比较 ────────────────────────────────────────────────

/// 解析版本号为可比较元组；忽略预发布/构建后缀。
///
/// 规则（宽松解析，缺失或非数字段按 0 计）：
///   - 允许前导 `v`；
///   - 取 `-` 或 `+` 之前的主体；
///   - 点分段最多取前 3 段，每段只读前导数字。
///
/// 例：`0.10.0` → (0,10,0)；`0.2.0-alpha.1` → (0,2,0)；`1` → (1,0,0)。
pub fn parse_version(v: &str) -> (u64, u64, u64) {
    let body = v.trim().trim_start_matches(['v', 'V']);
    let core = body
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .trim();
    let mut out = [0u64; 3];
    for (i, seg) in core.split('.').take(3).enumerate() {
        let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
        out[i] = digits.parse::<u64>().unwrap_or(0);
    }
    (out[0], out[1], out[2])
}

/// 数值比较（**不可用字符串比较**：`"0.10.0" > "0.9.0"`）。
///
/// 预发布后缀被忽略，因此 `0.2.0-alpha.1` 与 `0.2.0` 视为同一版本（不 newer）。
pub fn is_newer(candidate: &str, current: &str) -> bool {
    parse_version(candidate) > parse_version(current)
}

// ── 平台映射 ────────────────────────────────────────────────

/// 当前进程架构 → 清单平台键。
///
/// `std::env::consts::ARCH` 与编译期 `cfg!` 一致；用常量便于单测。
pub fn platform_key_for(arch: &str) -> Result<&'static str, String> {
    match arch {
        "aarch64" => Ok("darwin-aarch64"),
        "x86_64" => Ok("darwin-x86_64"),
        other => Err(format!("当前平台（{other}）暂不支持自动更新")),
    }
}

/// 当前设备的平台键。
pub fn current_platform_key() -> Result<&'static str, String> {
    platform_key_for(std::env::consts::ARCH)
}

// ── 清单解析 ────────────────────────────────────────────────

fn is_valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// 校验清单完整性：版本号非空、平台表存在、所有资产 sha256 合法。
///
/// 缺 `platforms` 字段在 serde 层即失败（该字段无默认值），此处再做
/// 逐资产的 sha256 合法性检查——清单是外部输入，宁可早失败。
pub fn validate_manifest(m: &UpdateManifest) -> Result<(), String> {
    if m.version.trim().is_empty() {
        return Err("更新清单无效：缺少 version".into());
    }
    if m.platforms.is_empty() {
        return Err("更新清单无效：platforms 为空".into());
    }
    for (key, asset) in &m.platforms {
        if asset.url.trim().is_empty() {
            return Err(format!("更新清单无效：平台 {key} 缺少下载地址"));
        }
        if !asset.url.starts_with("https://") {
            return Err(format!("更新清单无效：平台 {key} 的下载地址不是 HTTPS"));
        }
        if !is_valid_sha256(&asset.sha256) {
            return Err(format!(
                "更新清单无效：平台 {key} 的 sha256 必须是 64 位十六进制"
            ));
        }
    }
    Ok(())
}

/// 解析并校验清单 JSON。
pub fn parse_manifest(raw: &str) -> Result<UpdateManifest, String> {
    let m: UpdateManifest = serde_json::from_str(raw)
        .map_err(|e| format!("更新清单解析失败：{e}"))?;
    validate_manifest(&m)?;
    Ok(m)
}

/// 取当前平台的资产；缺该平台键时给出可读错误。
pub fn asset_for_current_platform(
    m: &UpdateManifest,
) -> Result<&PlatformAsset, String> {
    let key = current_platform_key()?;
    m.platforms
        .get(key)
        .ok_or_else(|| format!("更新清单中没有当前平台（{key}）的产物"))
}

// ── 检查更新 ────────────────────────────────────────────────

fn http_client(timeout_secs: u64) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .connect_timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))
}

/// 拉取并解析清单（不比对版本）。
pub fn fetch_manifest() -> Result<UpdateManifest, String> {
    let client = http_client(CHECK_TIMEOUT_SECS)?;
    let resp = client
        .get(UPDATE_MANIFEST_URL)
        .send()
        .map_err(|e| format!("检查更新失败（网络不可达）: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("检查更新失败：HTTP {}", resp.status()));
    }
    let body = resp
        .text()
        .map_err(|e| format!("读取更新清单失败: {e}"))?;
    parse_manifest(&body)
}

/// 检查是否有新版本。
///
/// 网络/解析失败一律返回 `Err`，由命令层决定是否静默（需求 FR-U6）：
/// `manual = false` 时命令层吞掉错误返回 `Ok(None)`。
pub fn check(current: &str) -> Result<Option<UpdateInfo>, String> {
    let manifest = fetch_manifest()?;
    if !is_newer(&manifest.version, current) {
        return Ok(None);
    }
    let asset = asset_for_current_platform(&manifest)?;

    // 体积：仅一次 HEAD 探测，**不为了拿体积而多下一次包**。
    let download_bytes = probe_content_length(&asset.url);

    Ok(Some(UpdateInfo {
        current_version: current.to_string(),
        version: manifest.version,
        notes: manifest.notes,
        pub_date: manifest.pub_date,
        download_bytes,
    }))
}

/// HEAD 探测 Content-Length；失败返回 None（体积只是提示信息）。
fn probe_content_length(url: &str) -> Option<u64> {
    let client = http_client(CHECK_TIMEOUT_SECS).ok()?;
    let resp = client.head(url).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.content_length().or_else(|| {
        resp.headers()
            .get("content-length")?
            .to_str()
            .ok()?
            .parse()
            .ok()
    })
}

// ── 下载 / 校验 / 解压 ──────────────────────────────────────

/// 更新会话目录：`<app data>/update/<version>/`。
pub fn version_dir(app_data: &Path, version: &str) -> PathBuf {
    app_data.join(UPDATE_DIR_NAME).join(sanitize_component(version))
}

/// 把版本号里可能存在的路径分隔符挡在外面（清单是外部输入）。
///
/// 只保留 `[A-Za-z0-9._-]`，并额外拒绝 `.`、`..` 与任何以 `.` 开头的段，
/// 避免 `../../etc` 这类值被拼进路径。
fn sanitize_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut out: Vec<&str> = Vec::new();
    for seg in cleaned.split('.') {
        // 只保留以字母数字开头的段：`..`、`.hidden` 一律丢弃
        if seg.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) {
            out.push(seg);
        }
    }
    let joined = out.join(".");
    if joined.is_empty() {
        "_".to_string()
    } else {
        joined
    }
}

/// 实际改名的目标和临时下载路径。
fn pkg_paths(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join("pkg.tar.gz"), dir.join("pkg.tar.gz.part"))
}

/// 带进度地下载更新包并做 SHA256 校验，落盘到 `<dir>/pkg.tar.gz`。
///
/// 先写 `.part` 再改名：中断不会留下被误认为完整的包。
/// 校验失败 → **删除临时文件并返回明确错误**（需求 FR-U3）。
pub fn download_verified(
    url: &str,
    expected_sha256: &str,
    dir: &Path,
    mut emit: impl FnMut(UpdateProgress),
) -> Result<PathBuf, String> {
    if !is_valid_sha256(expected_sha256) {
        return Err("更新清单中的 sha256 非法（必须是 64 位十六进制）".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("创建更新目录失败: {e}"))?;
    let (dest, part) = pkg_paths(dir);
    let _ = std::fs::remove_file(&part);
    let _ = std::fs::remove_file(&dest);

    let client = http_client(DOWNLOAD_TIMEOUT_SECS)?;
    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| format!("下载更新包失败（请检查网络）: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载更新包失败：HTTP {}", resp.status()));
    }
    let total = resp.content_length();

    emit(UpdateProgress {
        stage: STAGE_DOWNLOADING.into(),
        downloaded: 0,
        total,
        message: "正在下载更新包…".into(),
    });

    let mut file =
        std::fs::File::create(&part).map_err(|e| format!("创建更新包文件失败: {e}"))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut done: u64 = 0;
    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("读取更新包数据失败: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入更新包失败: {e}"))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        emit(UpdateProgress {
            stage: STAGE_DOWNLOADING.into(),
            downloaded: done,
            total,
            message: format!("正在下载更新包… {}", human_bytes(done)),
        });
    }
    file.flush().map_err(|e| format!("写入更新包失败: {e}"))?;
    drop(file);

    emit(UpdateProgress {
        stage: STAGE_VERIFYING.into(),
        downloaded: done,
        total,
        message: "正在校验更新包…".into(),
    });

    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        // 校验失败必须删除，不留半成品
        let _ = std::fs::remove_file(&part);
        let _ = std::fs::remove_file(&dest);
        return Err(format!(
            "更新包校验失败，已取消安装（期望 {expected_sha256}，实际 {actual}）"
        ));
    }

    std::fs::rename(&part, &dest).map_err(|e| format!("重命名更新包失败: {e}"))?;
    Ok(dest)
}

/// 人类可读体积（仅用于进度文案）。
fn human_bytes(n: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if n as f64 >= MB {
        format!("{:.1}MB", n as f64 / MB)
    } else {
        format!("{:.0}KB", n as f64 / 1024.0)
    }
}

/// 解压 `.app.tar.gz`（`flate2` + `tar`，不经外部进程）。
///
/// 用 `zip` crate 是不可行的（上游被 yank），因此 CI 额外产出
/// `.app.tar.gz`，客户端只保留 tar.gz 解压能力。
pub fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let f = std::fs::File::open(archive).map_err(|e| format!("打开更新包失败: {e}"))?;
    let gz = flate2::read::GzDecoder::new(f);
    let mut ar = tar::Archive::new(gz);
    ar.set_preserve_permissions(true);
    // 尝试恢复扩展属性。注意：Rust 的 tar crate 只识别 `SCHILY.xattr.*`，
    // 而 macOS bsdtar 写的是 `LIBARCHIVE.xattr.*`，故这里**通常不会生效**；
    // 侧车脚本的 codesign 签名因此可能丢失，由调用方在解压后重新签名兜底
    // （见 commands/updater.rs 的 resign_bundle）。
    ar.set_unpack_xattrs(true);
    std::fs::create_dir_all(dest).map_err(|e| format!("创建解压目录失败: {e}"))?;
    ar.unpack(dest).map_err(|e| format!("解压更新包失败: {e}"))?;
    Ok(())
}

/// 在解压结果中定位顶层 `O Muse.app`，并校验它是合法 bundle。
///
/// 接受两种布局：归档顶层即 `.app`（`--keepParent` 的产物），
/// 或顶层含若干条目、其中一个是 `.app`。
pub fn locate_app_bundle(root: &Path) -> Result<PathBuf, String> {
    let candidates = ["O Muse.app", "O Muse.app/"];
    for name in candidates {
        let p = root.join(name.trim_end_matches('/'));
        if is_app_bundle(&p) {
            return Ok(p);
        }
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.extension().and_then(|e| e.to_str()) == Some("app") && is_app_bundle(&p) {
                return Ok(p);
            }
        }
    }
    Err("更新包内容异常：未找到包含 Contents/Info.plist 的 .app".into())
}

/// 判定目录是否为可用的 app bundle。
pub fn is_app_bundle(p: &Path) -> bool {
    p.is_dir() && p.join("Contents").join("Info.plist").is_file()
}

/// 下载 → 校验 → 解压到 `<app data>/update/<version>/`（**不替换**）。
///
/// 返回解压出的 `.app` 路径，供 `install_and_restart` 使用。
/// 绝不直接覆盖当前运行的 bundle。
pub fn download_and_stage(
    app_data: &Path,
    info: &UpdateInfo,
    asset: &PlatformAsset,
    mut emit: impl FnMut(UpdateProgress),
) -> Result<PathBuf, String> {
    let dir = version_dir(app_data, &info.version);
    // 上次失败留下的解压残留先清掉，避免新旧文件混淆
    let staged_root = dir.join("staged");
    if staged_root.exists() {
        std::fs::remove_dir_all(&staged_root)
            .map_err(|e| format!("清理上次解压目录失败: {e}"))?;
    }

    let archive = download_verified(&asset.url, &asset.sha256, &dir, &mut emit)?;

    emit(UpdateProgress {
        stage: STAGE_EXTRACTING.into(),
        downloaded: std::fs::metadata(&archive).map(|m| m.len()).unwrap_or(0),
        total: None,
        message: "正在解压更新包…".into(),
    });
    extract_tar_gz(&archive, &staged_root)?;
    let staged = locate_app_bundle(&staged_root)?;
    Ok(staged)
}

// ── 替换与重启 ──────────────────────────────────────────────

/// 当前运行的 `.app` 路径（`.../O Muse.app/Contents/MacOS/o-muse` → 上溯三级）。
pub fn current_app_bundle() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("无法定位应用可执行文件: {e}"))?;
    app_bundle_from_exe(&exe)
}

/// 从可执行文件路径反推 `.app`（纯函数，便于单测）。
pub fn app_bundle_from_exe(exe: &Path) -> Result<PathBuf, String> {
    let macos = exe
        .parent()
        .ok_or_else(|| "无法从可执行文件路径反推 bundle：路径过短".to_string())?;
    let contents = macos
        .parent()
        .ok_or_else(|| "无法从可执行文件路径反推 bundle：路径过短".to_string())?;
    let bundle = contents
        .parent()
        .ok_or_else(|| "无法从可执行文件路径反推 bundle：路径过短".to_string())?;
    if macos.file_name().and_then(|n| n.to_str()) != Some("MacOS")
        || contents.file_name().and_then(|n| n.to_str()) != Some("Contents")
        || bundle.extension().and_then(|e| e.to_str()) != Some("app")
    {
        return Err(format!(
            "不可用于更新的应用位置：{}（应为 <名称>.app/Contents/MacOS/<可执行文件>）",
            exe.display()
        ));
    }
    Ok(bundle.to_path_buf())
}

/// 路径安全检查：必须是绝对路径，且不含单引号/换行/控制字符。
///
/// 这些字符会破坏单引号包裹的 shell 字面量，进而造成命令注入。
pub fn validate_shell_path(p: &Path) -> Result<(), String> {
    let s = p
        .to_str()
        .ok_or_else(|| "路径不是合法 UTF-8".to_string())?;
    if !s.starts_with('/') {
        return Err(format!("路径必须是绝对路径：{s}"));
    }
    if s.contains('\'') {
        return Err(format!("路径含非法字符（单引号）：{s}"));
    }
    if s.contains('\n') || s.contains('\r') {
        return Err(format!("路径含非法字符（换行）：{s}"));
    }
    if s.chars().any(|c| c.is_control()) {
        return Err(format!("路径含非法控制字符：{s}"));
    }
    Ok(())
}

/// 单引号包裹的 shell 字面量（内部单引号已在调用前被拒绝）。
fn shq(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy())
}

/// 生成替换脚本内容（**固定模板，不含任何用户输入拼接**）。
///
/// 所有路径都在写入前经 `validate_shell_path` 严格校验。逻辑：
///   1. 等本进程退出（最多 60s，`kill -0` 探测）
///   2. 备份旧 bundle 到 `<app data>/update/backup/O Muse.app`
///   3. `ditto` 替换
///   4. 校验新 bundle 的 `Contents/Info.plist`；失败则用备份回滚
///   5. `open` 新 bundle
///   6. 删除脚本自身与暂存目录
///
/// 为什么交给 `launchctl submit`：由 launchd 接管，**不随应用退出而终止**；
/// 普通子进程会随父进程一起被清理。
pub fn build_swap_script(
    pid: u32,
    staged_app: &Path,
    target_app: &Path,
    backup_app: &Path,
    script_path: &Path,
) -> Result<String, String> {
    validate_shell_path(staged_app)?;
    validate_shell_path(target_app)?;
    validate_shell_path(backup_app)?;
    validate_shell_path(script_path)?;

    let s = shq(staged_app);
    let t = shq(target_app);
    let b = shq(backup_app);
    let me = shq(script_path);
    let target_plist = shq(&target_app.join("Contents").join("Info.plist"));
    let staged_parent = staged_app
        .parent()
        .ok_or_else(|| "暂存路径没有父目录".to_string())?;
    let sp = shq(staged_parent);

    Ok(format!(
        r#"#!/bin/sh
# O Muse 自动更新替换脚本（固定模板，由 Rust 端生成）
# 参数均由 Rust 端做绝对路径 + 字符白名单校验后单引号包裹。
set -u
PID={pid}
i=0
while kill -0 "$PID" 2>/dev/null; do
  sleep 0.3
  i=$((i + 1))
  if [ "$i" -gt 200 ]; then
    break
  fi
done
rm -rf {b}
mkdir -p "$(dirname {b})"
if [ -d {t} ]; then
  /usr/bin/ditto {t} {b} || exit 1
fi
rm -rf {t}
if ! /usr/bin/ditto {s} {t}; then
  rm -rf {t}
  if [ -d {b} ]; then
    /usr/bin/ditto {b} {t}
  fi
  exit 1
fi
if [ ! -f {target_plist} ]; then
  rm -rf {t}
  if [ -d {b} ]; then
    /usr/bin/ditto {b} {t}
  fi
  exit 1
fi
/usr/bin/open {t}
rm -rf {sp}
rm -f {me}
exit 0
"#,
        pid = pid,
        b = b,
        t = t,
        s = s,
        sp = sp,
        me = me,
        target_plist = target_plist,
    ))
}

/// 写脚本文件并 `chmod 0700`。
fn write_swap_script(path: &Path, body: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建脚本目录失败: {e}"))?;
    }
    std::fs::write(path, body).map_err(|e| format!("写入替换脚本失败: {e}"))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("设置脚本权限失败: {e}"))?;
    Ok(())
}

/// 生成一个不依赖外部 crate 的 uuid v4（`/dev/urandom`）。
fn uuid_v4() -> String {
    use std::io::Read as _;
    let mut b = [0u8; 16];
    let ok = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .is_ok();
    if !ok {
        // 兜底：时间 + pid（仅用于生成互不冲突的标签，不用于安全用途）
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        b[..8].copy_from_slice(&nanos.to_le_bytes()[..8]);
        b[8..12].copy_from_slice(&std::process::id().to_le_bytes());
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// 准备替换脚本（不执行），返回 `(脚本路径, launchctl 标签)`。
///
/// 拆出来单独暴露，方便在**不实际替换任何东西**的前提下验证脚本生成。
pub fn prepare_swap_script(
    app_data: &Path,
    staged_app: &Path,
    target_app: &Path,
) -> Result<(PathBuf, String, PathBuf), String> {
    let base = app_data.join(UPDATE_DIR_NAME);
    let backup_app = base.join("backup").join(STAGED_APP_NAME);
    let id = uuid_v4();
    let script = base.join(format!("swap-{id}.sh"));
    let body = build_swap_script(
        std::process::id(),
        staged_app,
        target_app,
        &backup_app,
        &script,
    )?;
    write_swap_script(&script, &body)?;
    Ok((script, format!("com.omuse.update.{id}"), backup_app))
}

/// 替换运行中的 bundle 并重启：经 `launchctl submit` 脱离父进程执行脚本。
///
/// **本函数不自己退出应用**：脚本会先等本进程退出。调用方（前端）
/// 必须在命令成功后立刻调用 `app_exit`。
pub fn install_and_restart(
    app: &tauri::AppHandle,
    app_data: &Path,
    staged_app: &Path,
) -> Result<(), String> {
    if !is_app_bundle(staged_app) {
        return Err("更新包尚未就绪或内容不完整，请先下载更新".into());
    }
    let target = current_app_bundle()?;
    if target == staged_app {
        return Err("目标 bundle 与暂存 bundle 相同，拒绝执行替换".into());
    }
    if target.starts_with(app_data) {
        return Err(format!(
            "当前运行的应用位于更新目录内（{}），拒绝自替换",
            target.display()
        ));
    }
    let (script, label, _backup) = prepare_swap_script(app_data, staged_app, &target)?;
    submit_with_launchctl(app, &label, &script)
}

/// 经 tauri-plugin-shell 执行 `launchctl submit`。
///
/// 用 `submit` 而不是让应用自己 fork：launchd 接管后脚本不会随我们退出而终止。
/// 用插件而非 `std::process::Command`，保持「外部进程统一走 shell 插件」的约定。
pub fn submit_with_launchctl(
    app: &tauri::AppHandle,
    label: &str,
    script: &Path,
) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    let script_str = script.to_string_lossy().to_string();
    let out = tauri::async_runtime::block_on(
        app.shell()
            .command("/bin/launchctl")
            .args([
                "submit",
                "-l",
                label,
                "--",
                "/bin/sh",
                script_str.as_str(),
            ])
            .output(),
    )
    .map_err(|e| format!("调用 launchctl 失败: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "调度更新脚本失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

// ── 应用句柄 ────────────────────────────────────────────────
//
// 替换脚本的调度经 `tauri_plugin_shell` 执行 `launchctl submit`，
// 需要调用方把 `AppHandle` 传进来（core 层不自行保存全局单例）。

#[cfg(test)]
mod tests {
    use super::*;

    // ── 1. 版本比较 ─────────────────────────────────────────

    #[test]
    fn version_parsing_basics() {
        assert_eq!(parse_version("0.10.0"), (0, 10, 0));
        assert_eq!(parse_version("1.2.3"), (1, 2, 3));
        assert_eq!(parse_version("v0.1.2"), (0, 1, 2));
        assert_eq!(parse_version("1"), (1, 0, 0));
        assert_eq!(parse_version("1.2"), (1, 2, 0));
        assert_eq!(parse_version("0.2.0-alpha.1"), (0, 2, 0));
        assert_eq!(parse_version("0.2.0+build.7"), (0, 2, 0));
        assert_eq!(parse_version(""), (0, 0, 0));
    }

    #[test]
    fn numeric_comparison_not_string_comparison() {
        // 字符串比较会得出 "0.10.0" < "0.9.0" 的错误结论
        assert!("0.10.0" < "0.9.0", "字符串比较的错误结论（反证）");
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("0.2.0", "0.1.2"));
    }

    #[test]
    fn same_version_is_not_newer() {
        assert!(!is_newer("0.1.2", "0.1.2"));
        assert!(!is_newer("v0.1.2", "0.1.2"));
        // 预发布后缀被忽略 → 与正式版同号，不算更新
        assert_eq!(parse_version("0.2.0-alpha.1"), parse_version("0.2.0"));
        assert!(!is_newer("0.2.0-alpha.1", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0-alpha.1"));
    }

    #[test]
    fn older_candidate_is_not_newer() {
        assert!(!is_newer("0.1.1", "0.1.2"));
        assert!(!is_newer("0.9.0", "0.10.0"));
        assert!(!is_newer("1.0.0", "1.0.1"));
    }

    // ── 2. 清单解析 ─────────────────────────────────────────

    fn sample_manifest_json(version: &str) -> String {
        format!(
            r#"{{
              "version": "{version}",
              "notes": "修复若干问题",
              "pub_date": "2026-10-16T09:00:00Z",
              "platforms": {{
                "darwin-aarch64": {{
                  "url": "https://xsin.github.io/o-muse/releases/0.2.0/O%20Muse-0.2.0-arm64.app.tar.gz",
                  "sha256": "{sha}"
                }}
              }}
            }}"#,
            version = version,
            sha = "a".repeat(64)
        )
    }

    #[test]
    fn manifest_parses_full_document() {
        let m = parse_manifest(&sample_manifest_json("0.2.0")).unwrap();
        assert_eq!(m.version, "0.2.0");
        assert_eq!(m.notes, "修复若干问题");
        assert_eq!(m.pub_date, "2026-10-16T09:00:00Z");
        assert_eq!(m.platforms.len(), 1);
        let a = m.platforms.get("darwin-aarch64").unwrap();
        assert!(a.url.ends_with(".app.tar.gz"), "url={}", a.url);
        assert_eq!(a.sha256.len(), 64);
    }

    #[test]
    fn manifest_missing_platforms_is_rejected() {
        let raw = r#"{"version":"0.2.0","notes":"n","pub_date":"2026-10-16T09:00:00Z"}"#;
        let err = parse_manifest(raw).unwrap_err();
        assert!(err.contains("更新清单解析失败"), "err={err}");
    }

    #[test]
    fn manifest_empty_platforms_is_rejected() {
        let raw = r#"{"version":"0.2.0","notes":"n","pub_date":"d","platforms":{}}"#;
        let err = parse_manifest(raw).unwrap_err();
        assert!(err.contains("platforms 为空"), "err={err}");
    }

    #[test]
    fn manifest_missing_current_platform_key_is_an_error() {
        let raw = format!(
            r#"{{"version":"0.2.0","notes":"n","pub_date":"d",
                 "platforms":{{"darwin-iOS":{{"url":"https://x/y.tar.gz","sha256":"{}"}}}}}}"#,
            "b".repeat(64)
        );
        let m = parse_manifest(&raw).unwrap();
        // 清单本身合法，但当前平台无产物
        let err = asset_for_current_platform(&m).unwrap_err();
        assert!(err.contains("没有当前平台"), "err={err}");
        assert!(err.contains(current_platform_key().unwrap()), "err={err}");
    }

    #[test]
    fn manifest_bad_sha256_length_is_rejected() {
        let raw = format!(
            r#"{{"version":"0.2.0","notes":"n","pub_date":"d",
                 "platforms":{{"darwin-aarch64":{{"url":"https://x/y.tar.gz","sha256":"{}"}}}}}}"#,
            "c".repeat(63)
        );
        let err = parse_manifest(&raw).unwrap_err();
        assert!(err.contains("64 位十六进制"), "err={err}");
    }

    #[test]
    fn manifest_non_hex_sha256_is_rejected() {
        let raw = format!(
            r#"{{"version":"0.2.0","notes":"n","pub_date":"d",
                 "platforms":{{"darwin-aarch64":{{"url":"https://x/y.tar.gz","sha256":"{}"}}}}}}"#,
            "z".repeat(64)
        );
        let err = parse_manifest(&raw).unwrap_err();
        assert!(err.contains("64 位十六进制"), "err={err}");
    }

    #[test]
    fn manifest_non_https_url_is_rejected() {
        let raw = format!(
            r#"{{"version":"0.2.0","notes":"n","pub_date":"d",
                 "platforms":{{"darwin-aarch64":{{"url":"http://x/y.tar.gz","sha256":"{}"}}}}}}"#,
            "d".repeat(64)
        );
        let err = parse_manifest(&raw).unwrap_err();
        assert!(err.contains("不是 HTTPS"), "err={err}");
    }

    #[test]
    fn manifest_empty_version_is_rejected() {
        let raw = format!(
            r#"{{"version":"  ","notes":"n","pub_date":"d",
                 "platforms":{{"darwin-aarch64":{{"url":"https://x/y.tar.gz","sha256":"{}"}}}}}}"#,
            "e".repeat(64)
        );
        let err = parse_manifest(&raw).unwrap_err();
        assert!(err.contains("缺少 version"), "err={err}");
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(parse_manifest("{not json").is_err());
    }

    #[test]
    fn update_info_and_manifest_serde_roundtrip() {
        let m = parse_manifest(&sample_manifest_json("0.2.0")).unwrap();
        let round: UpdateManifest =
            serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(round.version, m.version);
        assert_eq!(round.platforms.len(), m.platforms.len());

        let info = UpdateInfo {
            current_version: "0.1.2".into(),
            version: "0.2.0".into(),
            notes: "notes".into(),
            pub_date: "2026-10-16T09:00:00Z".into(),
            download_bytes: Some(21 * 1024 * 1024),
        };
        let v: serde_json::Value = serde_json::to_value(&info).unwrap();
        assert_eq!(v["current_version"], "0.1.2");
        assert_eq!(v["download_bytes"], 21 * 1024 * 1024u64);
        assert!(v["version"].is_string());
    }

    #[test]
    fn update_info_serializes_null_for_unknown_size() {
        let info = UpdateInfo {
            current_version: "0.1.2".into(),
            version: "0.2.0".into(),
            notes: String::new(),
            pub_date: String::new(),
            download_bytes: None,
        };
        let v: serde_json::Value = serde_json::to_value(&info).unwrap();
        assert!(v["download_bytes"].is_null());
    }

    // ── 3. SHA256 校验 ─────────────────────────────────────

    /// 单次响应 HTTP 服务器（hermetic，不依赖外网）。
    fn serve_once(
        body: Vec<u8>,
        status_line: &'static str,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nContent-Type: application/gzip\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
                let _ = s.flush();
            }
        });
        (format!("http://{addr}/pkg.tar.gz"), h)
    }

    fn sha256_hex(data: &[u8]) -> String {
        let mut h = sha2::Sha256::new();
        h.update(data);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("omuse_upd_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn download_accepts_correct_sha256_and_reports_progress() {
        let payload = b"fake-app-bundle-tarball".to_vec();
        let sha = sha256_hex(&payload);
        let (url, h) = serve_once(payload.clone(), "200 OK");
        let dir = tmp_dir("ok");

        let mut events = Vec::new();
        let path =
            download_verified(&url, &sha, &dir, |p| events.push(p)).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), payload);
        assert_eq!(path.file_name().unwrap(), "pkg.tar.gz");
        assert!(!dir.join("pkg.tar.gz.part").exists(), "不应残留 .part");
        assert!(!events.is_empty(), "应至少回传一次进度");
        assert_eq!(events[0].stage, STAGE_DOWNLOADING);
        assert!(events.iter().any(|e| e.stage == STAGE_VERIFYING));
        assert_eq!(events.last().unwrap().stage, STAGE_VERIFYING);
        let last = events.last().unwrap();
        assert_eq!(last.downloaded, payload.len() as u64);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = h.join();
    }

    #[test]
    fn download_sha256_is_case_insensitive() {
        let payload = b"case-insensitive".to_vec();
        let sha = sha256_hex(&payload).to_uppercase();
        let (url, h) = serve_once(payload.clone(), "200 OK");
        let dir = tmp_dir("case");

        let path = download_verified(&url, &sha, &dir, |_| {}).unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), payload);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = h.join();
    }

    #[test]
    fn download_rejects_wrong_sha256_and_deletes_temp_files() {
        let (url, h) = serve_once(b"tampered-content".to_vec(), "200 OK");
        let dir = tmp_dir("bad");
        let bogus = "0".repeat(64);

        let err = download_verified(&url, &bogus, &dir, |_| {}).unwrap_err();
        assert!(err.contains("更新包校验失败，已取消安装"), "err={err}");
        assert!(!dir.join("pkg.tar.gz").exists(), "失败不得留下包");
        assert!(!dir.join("pkg.tar.gz.part").exists(), "失败必须删除临时文件");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = h.join();
    }

    #[test]
    fn download_rejects_invalid_expected_hash_format() {
        let dir = tmp_dir("badhash");
        let err =
            download_verified("http://127.0.0.1:1/x", "abc", &dir, |_| {}).unwrap_err();
        assert!(err.contains("sha256 非法"), "err={err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn download_rejects_non_2xx_status() {
        let (url, h) = serve_once(b"nope".to_vec(), "500 Internal Server Error");
        let dir = tmp_dir("http500");
        let err =
            download_verified(&url, &"f".repeat(64), &dir, |_| {}).unwrap_err();
        assert!(err.contains("HTTP 500"), "err={err}");
        assert!(!dir.join("pkg.tar.gz").exists());

        let _ = std::fs::remove_dir_all(&dir);
        let _ = h.join();
    }

    #[test]
    fn check_propagates_network_error_as_err() {
        // 不可达地址 → 必须是 Err（是否静默由命令层决定）
        let err = fetch_manifest_from("http://127.0.0.1:1/latest.json").unwrap_err();
        assert!(err.contains("检查更新失败"), "err={err}");
    }

    /// 测试用：向任意 URL 拉清单（`fetch_manifest` 固定生产 URL）。
    fn fetch_manifest_from(url: &str) -> Result<UpdateManifest, String> {
        let client = http_client(2)?;
        let resp = client
            .get(url)
            .send()
            .map_err(|e| format!("检查更新失败（网络不可达）: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("检查更新失败：HTTP {}", resp.status()));
        }
        let body = resp.text().map_err(|e| format!("读取更新清单失败: {e}"))?;
        parse_manifest(&body)
    }

    #[test]
    fn manifest_over_http_is_parsed_and_compared() {
        let body = sample_manifest_json("0.10.0");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(body.as_bytes());
                let _ = s.flush();
            }
        });
        let m = fetch_manifest_from(&format!("http://{addr}/latest.json")).unwrap();
        assert_eq!(m.version, "0.10.0");
        assert!(is_newer(&m.version, "0.9.0"));
        let _ = h.join();
    }

    // ── 4. 路径安全 ────────────────────────────────────────

    #[test]
    fn path_with_single_quote_is_rejected() {
        let p = Path::new("/tmp/O' Muse.app");
        let err = validate_shell_path(p).unwrap_err();
        assert!(err.contains("单引号"), "err={err}");
    }

    #[test]
    fn path_with_newline_is_rejected() {
        let p = Path::new("/tmp/a\nopen /etc/passwd");
        let err = validate_shell_path(p).unwrap_err();
        assert!(err.contains("换行"), "err={err}");
        let p2 = Path::new("/tmp/a\rb");
        assert!(validate_shell_path(p2).is_err());
    }

    #[test]
    fn path_with_control_char_is_rejected() {
        let p = Path::new("/tmp/a\u{7}b");
        let err = validate_shell_path(p).unwrap_err();
        assert!(err.contains("控制字符"), "err={err}");
        let p2 = Path::new("/tmp/a\u{0}b");
        assert!(validate_shell_path(p2).is_err());
    }

    #[test]
    fn relative_path_is_rejected() {
        let err = validate_shell_path(Path::new("O Muse.app")).unwrap_err();
        assert!(err.contains("绝对路径"), "err={err}");
    }

    #[test]
    fn swap_script_rejects_unsafe_paths() {
        let target = Path::new("/Applications/O Muse.app");
        let backup = Path::new("/tmp/backup/O Muse.app");
        let script = Path::new("/tmp/swap.sh");
        let staged = Path::new("/tmp/O'Muse.app");
        let err =
            build_swap_script(1, staged, target, backup, script).unwrap_err();
        assert!(err.contains("单引号"), "err={err}");

        let err2 = build_swap_script(
            1,
            Path::new("/tmp/a\nb.app"),
            target,
            backup,
            script,
        )
        .unwrap_err();
        assert!(err2.contains("换行"), "err2={err2}");
    }

    #[test]
    fn swap_script_contains_required_steps() {
        let body = build_swap_script(
            4242,
            Path::new("/Users/x/Library/Application Support/work.xsin.omuse/update/0.2.0/staged/O Muse.app"),
            Path::new("/Applications/O Muse.app"),
            Path::new("/Users/x/Library/Application Support/work.xsin.omuse/update/backup/O Muse.app"),
            Path::new("/Users/x/Library/Application Support/work.xsin.omuse/update/swap-abc.sh"),
        )
        .unwrap();

        assert!(body.starts_with("#!/bin/sh\n"), "应为 sh 脚本");
        assert!(body.contains("PID=4242"));
        assert!(body.contains("while kill -0 \"$PID\" 2>/dev/null; do"));
        assert!(body.contains("sleep 0.3"));
        assert!(body.contains(r#"if [ "$i" -gt 200 ]; then"#), "等待上限 60s（200 × 0.3s）");        assert!(body.contains("/usr/bin/ditto"));
        assert!(body.contains("'/Applications/O Muse.app'"), "目标路径应单引号包裹");
        assert!(body.contains("/Contents/Info.plist'"), "应校验 Info.plist");
        assert!(body.contains("/usr/bin/open '/Applications/O Muse.app'"));
        assert!(body.contains("rm -f '/Users/x/Library/Application Support/work.xsin.omuse/update/swap-abc.sh'"));
        assert!(
            body.contains("rm -rf '/Users/x/Library/Application Support/work.xsin.omuse/update/0.2.0/staged'"),
            "应清理暂存目录"
        );
        // 回滚路径存在
        assert!(body.contains("if [ -d '/Users/x/Library/Application Support/work.xsin.omuse/update/backup/O Muse.app' ]; then"));
        assert!(body.contains("exit 0"));
    }

    #[test]
    fn swap_script_has_no_unquoted_user_paths() {
        // 每个路径都必须以单引号包裹出现
        let staged = Path::new("/tmp/staged/O Muse.app");
        let target = Path::new("/tmp/target/O Muse.app");
        let backup = Path::new("/tmp/backup/O Muse.app");
        let script = Path::new("/tmp/swap-1.sh");
        let body = build_swap_script(7, staged, target, backup, script).unwrap();
        for needle in [
            "'/tmp/staged/O Muse.app'",
            "'/tmp/target/O Muse.app'",
            "'/tmp/backup/O Muse.app'",
            "'/tmp/swap-1.sh'",
            "'/tmp/staged'",
        ] {
            assert!(body.contains(needle), "脚本文本缺少 {needle}\n{body}");
        }
    }

    #[test]
    fn prepare_swap_script_writes_executable_file_and_valid_label() {
        let dir = tmp_dir("prep");
        let staged = dir.join("staged").join("O Muse.app");
        std::fs::create_dir_all(staged.join("Contents")).unwrap();
        std::fs::write(staged.join("Contents").join("Info.plist"), b"plist").unwrap();

        // 目标用假路径（不触碰真实安装），只验证生成与落盘
        let target = dir.join("Applications").join("O Muse.app");
        let (script, label, backup) =
            prepare_swap_script(&dir, &staged, &target).unwrap();

        assert!(script.exists(), "脚本应已写入");
        assert_eq!(script.extension().unwrap(), "sh");
        assert!(label.starts_with("com.omuse.update."));
        assert!(label.ends_with(&script.file_stem().unwrap().to_string_lossy().replace("swap-", "")));
        assert!(backup.ends_with("update/backup/O Muse.app"));
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&script).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "脚本权限必须为 0700");
        }
        let body = std::fs::read_to_string(&script).unwrap();
        assert!(body.contains("PID="), "脚本应写入当前 pid");
        assert!(!body.contains("'''"), "不应出现空字面量");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uuid_v4_shape_and_uniqueness() {
        let a = uuid_v4();
        let b = uuid_v4();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), vec![8, 4, 4, 4, 12]);
        assert_eq!(&a[14..15], "4", "版本位应为 4");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    // ── 5. .tar.gz 解压与 bundle 识别 ───────────────────────

    /// 造一个 `.app.tar.gz`（顶层即 `O Muse.app`）。
    fn make_app_targz(dest: &Path, with_info_plist: bool, extra_top_level: bool) {
        // 目录名必须**每次唯一**：同名会让并行执行的测试互相 remove_dir_all，
        // 造成归档内容缺失（表现为 locate_app_bundle 找不到 .app）。
        static SRC_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SRC_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let src = tmp_dir(&format!(
            "srcrf_{}_{}_{}",
            with_info_plist as u8,
            extra_top_level as u8,
            seq
        ));
        let app = src.join("O Muse.app");
        std::fs::create_dir_all(app.join("Contents").join("MacOS")).unwrap();
        std::fs::write(app.join("Contents").join("MacOS").join("o-muse"), b"binary").unwrap();
        if with_info_plist {
            std::fs::write(app.join("Contents").join("Info.plist"), b"<plist/>").unwrap();
        }
        if extra_top_level {
            std::fs::write(src.join("README.txt"), b"readme").unwrap();
        }

        let f = std::fs::File::create(dest).unwrap();
        let gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        if extra_top_level {
            b.append_dir_all("O Muse.app", &app).unwrap();
            b.append_path_with_name(src.join("README.txt"), "README.txt").unwrap();
        } else {
            b.append_dir_all("O Muse.app", &app).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();

        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn tar_gz_extract_and_recognize_app_bundle() {
        let dir = tmp_dir("untar");
        let archive = dir.join("O Muse-0.2.0-arm64.app.tar.gz");
        make_app_targz(&archive, true, false);

        let out = dir.join("staged");
        extract_tar_gz(&archive, &out).unwrap();

        let staged = locate_app_bundle(&out).unwrap();
        assert_eq!(staged.file_name().unwrap(), "O Muse.app");
        assert!(staged.join("Contents").join("Info.plist").is_file());
        assert!(staged.join("Contents").join("MacOS").join("o-muse").is_file());
        assert!(is_app_bundle(&staged));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tar_gz_with_extra_top_level_entries_still_recognized() {
        let dir = tmp_dir("untar2");
        let archive = dir.join("a.app.tar.gz");
        make_app_targz(&archive, true, true);
        let out = dir.join("staged");
        extract_tar_gz(&archive, &out).unwrap();
        let staged = locate_app_bundle(&out).unwrap();
        assert!(is_app_bundle(&staged));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_without_info_plist_is_rejected() {
        let dir = tmp_dir("untar3");
        let archive = dir.join("bad.app.tar.gz");
        make_app_targz(&archive, false, false);
        let out = dir.join("staged");
        extract_tar_gz(&archive, &out).unwrap();
        let err = locate_app_bundle(&out).unwrap_err();
        assert!(err.contains("未找到"), "err={err}");
        assert!(err.contains("Contents/Info.plist"), "err={err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_with_no_app_at_all_is_rejected() {
        let dir = tmp_dir("untar4");
        let out = dir.join("staged");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("random.txt"), b"x").unwrap();
        assert!(locate_app_bundle(&out).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_archive_fails_to_extract() {
        let dir = tmp_dir("untar5");
        let archive = dir.join("corrupt.tar.gz");
        std::fs::write(&archive, b"not a gzip stream").unwrap();
        let out = dir.join("staged");
        let err = extract_tar_gz(&archive, &out).unwrap_err();
        assert!(err.contains("解压更新包失败"), "err={err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn download_and_stage_end_to_end_from_local_http() {
        // 全链路（下载 → 校验 → 解压 → 定位 bundle），仅网络端点换成本地
        let pkg_dir = tmp_dir("e2e_pkg");
        let archive = pkg_dir.join("O Muse-0.2.0-arm64.app.tar.gz");
        make_app_targz(&archive, true, false);
        let payload = std::fs::read(&archive).unwrap();
        let sha = sha256_hex(&payload);

        let (url, h) = serve_once(payload.clone(), "200 OK");
        let app_data = tmp_dir("e2e_data");
        let info = UpdateInfo {
            current_version: "0.1.2".into(),
            version: "0.2.0".into(),
            notes: "n".into(),
            pub_date: "d".into(),
            download_bytes: None,
        };
        let asset = PlatformAsset {
            url,
            sha256: sha,
        };
        let mut stages = Vec::new();
        let staged =
            download_and_stage(&app_data, &info, &asset, |p| stages.push(p.stage)).unwrap();

        assert!(staged.ends_with("staged/O Muse.app"), "staged={staged:?}");
        assert!(staged.join("Contents").join("Info.plist").is_file());
        assert!(app_data.join("update/0.2.0/pkg.tar.gz").is_file());
        assert!(stages.contains(&STAGE_DOWNLOADING.to_string()));
        assert!(stages.contains(&STAGE_VERIFYING.to_string()));
        assert!(stages.contains(&STAGE_EXTRACTING.to_string()));

        let _ = std::fs::remove_dir_all(&app_data);
        let _ = std::fs::remove_dir_all(&pkg_dir);
        let _ = h.join();
    }

    #[test]
    fn download_and_stage_rejects_tampered_package() {
        let pkg_dir = tmp_dir("e2e_bad");
        let archive = pkg_dir.join("a.app.tar.gz");
        make_app_targz(&archive, true, false);
        let payload = std::fs::read(&archive).unwrap();
        let (url, h) = serve_once(payload, "200 OK");

        let app_data = tmp_dir("e2e_bad_data");
        let info = UpdateInfo {
            current_version: "0.1.2".into(),
            version: "0.2.0".into(),
            notes: String::new(),
            pub_date: String::new(),
            download_bytes: None,
        };
        let asset = PlatformAsset {
            url,
            sha256: "1".repeat(64),
        };
        let err = download_and_stage(&app_data, &info, &asset, |_| {}).unwrap_err();
        assert!(err.contains("更新包校验失败，已取消安装"), "err={err}");
        assert!(
            !app_data.join("update/0.2.0/pkg.tar.gz").exists(),
            "校验失败不得留下包"
        );
        assert!(
            !app_data.join("update/0.2.0/staged/O Muse.app").exists(),
            "校验失败不得解压"
        );

        let _ = std::fs::remove_dir_all(&app_data);
        let _ = std::fs::remove_dir_all(&pkg_dir);
        let _ = h.join();
    }

    // ── 6. 平台映射与路径反推 ───────────────────────────────

    #[test]
    fn platform_key_mapping_is_correct() {
        assert_eq!(platform_key_for("aarch64").unwrap(), "darwin-aarch64");
        assert_eq!(platform_key_for("x86_64").unwrap(), "darwin-x86_64");
        assert!(platform_key_for("arm").is_err());
        assert!(platform_key_for("x86").is_err());
        assert!(platform_key_for("aarch64-pc-windows-msvc").is_err());
    }

    #[test]
    fn current_platform_key_matches_host_arch() {
        let key = current_platform_key().unwrap();
        if cfg!(target_arch = "aarch64") {
            assert_eq!(key, "darwin-aarch64");
        } else if cfg!(target_arch = "x86_64") {
            assert_eq!(key, "darwin-x86_64");
        }
    }

    #[test]
    fn app_bundle_is_derived_from_executable_path() {
        let p = Path::new("/Applications/O Muse.app/Contents/MacOS/o-muse");
        assert_eq!(
            app_bundle_from_exe(p).unwrap(),
            PathBuf::from("/Applications/O Muse.app")
        );
        let home = Path::new("/Users/x/Applications/O Muse.app/Contents/MacOS/o-muse");
        assert_eq!(
            app_bundle_from_exe(home).unwrap(),
            PathBuf::from("/Users/x/Applications/O Muse.app")
        );
    }

    #[test]
    fn app_bundle_derivation_rejects_non_bundle_paths() {
        assert!(app_bundle_from_exe(Path::new("/usr/local/bin/o-muse")).is_err());
        assert!(app_bundle_from_exe(Path::new("/tmp/O Muse.app/Contents/o-muse")).is_err());
        assert!(app_bundle_from_exe(Path::new("o-muse")).is_err());
    }

    #[test]
    fn version_dir_sanitizes_manifest_version() {
        let d = version_dir(Path::new("/data"), "../../etc");
        assert!(d.starts_with("/data/update/"));
        assert!(!d.to_string_lossy().contains(".."), "d={d:?}");
        let d2 = version_dir(Path::new("/data"), "0.2.0");
        assert!(d2.ends_with("update/0.2.0"));
    }

    #[test]
    fn is_app_bundle_requires_info_plist() {
        let dir = tmp_dir("bundle");
        let app = dir.join("X.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        assert!(!is_app_bundle(&app), "缺 Info.plist 不算 bundle");
        std::fs::write(app.join("Contents").join("Info.plist"), b"p").unwrap();
        assert!(is_app_bundle(&app));
        assert!(!is_app_bundle(&dir.join("nope.app")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
