// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// Kontakt 系统级注册的**提权写入**（一次性助手）。
//
// 背景：Service Center XML 与系统 plist 位于 `/Library`（root:admin / root:wheel），
// 免提权路径（`/Users/Shared` JSON + 用户级 plist）无法覆盖，导致 Kontakt 6
// 与经典浏览器场景注册不完整。
//
// 为什么不用 SMJobBless：本应用是 ad-hoc 签名（没有 Developer ID），
// 系统不允许安装特权 Helper Tool。因此只使用 macOS 原生授权：
// `/usr/bin/osascript -e 'do shell script … with administrator privileges'`，
// 由系统弹出管理员密码框。
//
// 架构（主二进制复用，不额外打包）：
//   1. 普通用户进程把「写什么」序列化成请求 JSON（0700 目录 / 0600 文件），
//      算出 SHA-256，经 osascript 请求授权；
//   2. 同一个主二进制以 `--privileged-helper <request.json> <sha256>` **再次启动**
//      （此时 euid == 0）；`main.rs` 在启动 Tauri 之前拦截该参数并直接退出；
//   3. 助手逐项校验请求，再按路径白名单执行写入/删除；任一步失败即回滚。
//
// 安全铁律（任何一条不满足即拒绝并以非 0 退出）：
//   1. `geteuid() == 0`，否则拒绝（防止被非特权进程滥用）；
//   2. 请求文件 canonicalize 后必须落在允许的临时目录内（/private/tmp、/var/folders）；
//   3. 请求文件必须是常规文件（拒绝符号链接）；
//   4. 请求文件权限必须恰为 0600；
//   5. 请求文件 ≤ 4 MiB；
//   6. 请求内容 SHA-256 与调用方给出的期望值常数时间比较；
//   7. 请求 JSON 的 `version` 必须为 1。
//   目标路径另有白名单：只允许两个系统目录下的**直接子文件**，
//   父目录 canonicalize 后必须精确等于白名单根；逐级拒绝符号链接、拒绝控制字符文件名。
//
// **绝不接受**：任意 shell 命令、请求文件之外的任何输入。`reason` 字段只进日志。

use crate::core::kontakt::registry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// 请求格式版本；不匹配一律拒绝。
pub const REQUEST_VERSION: u32 = 1;
/// 助手参数：`<exe> --privileged-helper <request.json> <sha256>`。
pub const HELPER_FLAG: &str = "--privileged-helper";
/// 请求文件字节上限（4 MiB）。
pub const MAX_REQUEST_BYTES: u64 = 4 * 1024 * 1024;
/// 单个写入内容解码后的上限（2 MiB）。
pub const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;
/// 单次请求的操作条数上限（防线：请求再大也不能变成脚本）。
pub const MAX_OPERATIONS: usize = 64;
/// 文件名长度上限（字节）。
const MAX_FILE_NAME_BYTES: usize = 255;
/// 请求文件名。
const REQUEST_FILE_NAME: &str = "request.json";
/// 用户取消授权时 osascript 的固定错误码。
const CANCELLED_CODE: &str = "(-128)";
/// 用户取消授权的对外错误前缀（前端据此判断「不是失败」）。
pub const CANCELLED_MARKER: &str = "已取消授权";

// ── 请求模型 ──────────────────────────────────────────────────

/// 提权请求：一次授权内要执行的全部操作。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedRequest {
    /// 必须等于 [`REQUEST_VERSION`]。
    pub version: u32,
    pub operations: Vec<PrivilegedOp>,
    /// 供日志/审计；**不参与任何执行**。
    pub reason: String,
}

/// 单个操作。只有「写固定内容」与「删文件」两种，没有任意命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum PrivilegedOp {
    WriteFile {
        path: String,
        contents_base64: String,
    },
    RemoveFile {
        path: String,
    },
}

// ── 允许根目录（可注入，便于测试不触碰真实 /Library）─────────

/// 提权写入允许的两个系统目录。
///
/// 生产环境用 [`AllowedRoots::production`]（真实的 `/Library/...` 常量）；
/// **单元测试必须用 [`AllowedRoots::new`] 传入 temp 目录**，
/// 这样全部测试都在临时目录里跑，绝不会写真实的 `/Library`。
#[derive(Debug, Clone)]
pub struct AllowedRoots {
    /// `/Library/Application Support/Native Instruments/Service Center`
    pub service_center: PathBuf,
    /// `/Library/Preferences`
    pub preferences: PathBuf,
}

impl AllowedRoots {
    /// 生产常量（唯二允许提权写入的系统目录）。
    pub fn production() -> Self {
        Self {
            service_center: registry::service_center_dir(),
            preferences: registry::system_preferences_dir(),
        }
    }

    /// 注入自定义根（仅供单元测试，避免触碰真实 `/Library`）。
    pub fn new(service_center: impl Into<PathBuf>, preferences: impl Into<PathBuf>) -> Self {
        Self {
            service_center: service_center.into(),
            preferences: preferences.into(),
        }
    }

    /// 父目录（已 canonicalize）恰好等于某个允许根时，返回该根的规范形式。
    fn match_root(&self, parent_canonical: &Path) -> Option<PathBuf> {
        for root in [&self.service_center, &self.preferences] {
            let canonical = canonicalize_lenient(root);
            if canonical == parent_canonical {
                return Some(canonical);
            }
        }
        None
    }
}

/// 助手运行时配置（请求文件允许的临时目录同样可注入，便于测试）。
#[derive(Debug, Clone)]
pub struct HelperConfig {
    pub roots: AllowedRoots,
    /// 请求文件必须位于这些目录之下（canonicalize 后比较）。
    pub request_roots: Vec<PathBuf>,
}

impl HelperConfig {
    pub fn production() -> Self {
        Self {
            roots: AllowedRoots::production(),
            // `/var` 与 `/tmp` 在 macOS 上是符号链接，canonicalize 后分别是
            // `/private/var/folders` 与 `/private/tmp`；这里把两种写法都列出，
            // 比较时统一用 canonicalize_lenient 归一。
            request_roots: vec![
                PathBuf::from("/private/tmp"),
                PathBuf::from("/var/folders"),
                PathBuf::from("/private/var/folders"),
            ],
        }
    }

    fn is_allowed_request_path(&self, canonical: &Path) -> bool {
        self.request_roots.iter().any(|root| {
            let root_canonical = canonicalize_lenient(root);
            canonical.starts_with(&root_canonical)
        })
    }
}

/// 归一化路径：对**存在的最深祖先**做 canonicalize，再把尚不存在的尾部拼回去。
///
/// 用途有两个：一是让 `/var` → `/private/var` 这类符号链接在比较时一致；
/// 二是允许「目标目录尚不存在」的情况（此时无法直接 canonicalize）。
fn canonicalize_lenient(path: &Path) -> PathBuf {
    if !path.is_absolute() {
        return path.to_path_buf();
    }
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        if let Ok(resolved) = fs::canonicalize(&current) {
            let mut out = resolved;
            for part in missing.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (current.parent(), current.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_os_string());
                current = parent.to_path_buf();
            }
            // 走到根仍失败（例如路径不存在且无父目录）：原样返回。
            _ => return path.to_path_buf(),
        }
    }
}

// ── 目标路径白名单 ────────────────────────────────────────────

/// 校验一个待写入/删除的目标路径，返回**由规范根重新拼装**的绝对路径。
///
/// 规则：
///   - 必须是绝对路径；
///   - 文件名非空、≤ 255 字节、不含 `/`、不是 `.` / `..`、不含控制字符（< 0x20 与 0x7F）；
///   - 父目录 canonicalize 后必须**精确等于**某个允许根（因此不允许再嵌套子目录）；
///   - 返回值永远由「规范根 + 已校验文件名」拼成，原始字符串中的 `..` / `.` 不参与拼路径。
pub fn validate_target_path(raw: &str, roots: &AllowedRoots) -> Result<PathBuf, String> {
    if raw.is_empty() {
        return Err("目标路径不能为空".to_string());
    }
    if !raw.starts_with('/') {
        return Err(format!("目标路径必须是绝对路径，拒绝: {raw}"));
    }
    let path = Path::new(raw);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("目标路径缺少合法文件名，拒绝: {raw}"))?;
    if name.is_empty() {
        return Err(format!("目标路径文件名为空，拒绝: {raw}"));
    }
    if name == "." || name == ".." {
        return Err(format!("目标路径文件名不能是 `.` 或 `..`，拒绝: {raw}"));
    }
    if name.len() > MAX_FILE_NAME_BYTES {
        return Err(format!(
            "目标文件名超过 {MAX_FILE_NAME_BYTES} 字节，拒绝: {raw}"
        ));
    }
    if name.contains('/') {
        return Err(format!("目标文件名不能包含 `/`，拒绝: {raw}"));
    }
    if name
        .chars()
        .any(|c| (c as u32) < 0x20 || c == '\u{7f}')
    {
        return Err(format!("目标文件名含控制字符，拒绝: {raw}"));
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("目标路径没有父目录，拒绝: {raw}"))?;
    let parent_canonical = canonicalize_lenient(parent);
    let root = roots
        .match_root(&parent_canonical)
        .ok_or_else(|| format!("目标路径不在允许的系统目录内，拒绝: {raw}"))?;

    let target = root.join(name);
    if target.parent() != Some(root.as_path()) {
        return Err(format!("目标路径层级非法，拒绝: {raw}"));
    }
    Ok(target)
}

// ── 请求文件校验（root 侧入口）────────────────────────────────

/// 助手入口：以 root 运行时处理一次请求，返回进程退出码（成功 0，失败 1）。
///
/// 结果 JSON 打到 stdout：`{"ok":true,"applied":[…]}` 或 `{"ok":false,"error":"…"}`；
/// 失败原因同时写 stderr（osascript 在命令非 0 退出时只会转发 stderr）。
pub fn helper_entry(request_path: &str, expected_sha256: &str) -> i32 {
    match run_helper(request_path, expected_sha256, &HelperConfig::production()) {
        Ok(applied) => {
            print_result(true, &applied, None);
            0
        }
        Err(error) => {
            eprintln!("omuse {HELPER_FLAG}: {error}");
            print_result(false, &[], Some(&error));
            1
        }
    }
}

fn run_helper(
    request_path: &str,
    expected_sha256: &str,
    config: &HelperConfig,
) -> Result<Vec<String>, String> {
    // 防线 1：必须真的是 root，否则拒绝（防止被非特权进程当工具滥用）。
    if unsafe { libc::geteuid() } != 0 {
        return Err("提权助手必须以 root 身份运行，已拒绝".to_string());
    }
    let request = validate_request_file(request_path, expected_sha256, config)?;
    // `reason` 只用于审计日志，绝不参与执行。
    eprintln!("omuse {HELPER_FLAG}: reason={}", request.reason);
    apply_operations(&request.operations, &config.roots)
}

/// 校验请求文件并解析出请求内容（防线 2–7）。
///
/// 独立成函数是为了让单元测试能直接覆盖全部拒绝分支（不必真的以 root 运行）。
pub fn validate_request_file(
    request_path: &str,
    expected_sha256: &str,
    config: &HelperConfig,
) -> Result<PrivilegedRequest, String> {
    let raw = Path::new(request_path);

    // 防线 3（前半）：先看原始路径——canonicalize 会把符号链接解析掉，必须先拒绝。
    let link_meta = fs::symlink_metadata(raw)
        .map_err(|e| format!("无法访问请求文件 {request_path}: {e}"))?;
    if link_meta.file_type().is_symlink() {
        return Err("请求文件是符号链接，拒绝".to_string());
    }

    // 防线 2：canonicalize 后必须落在允许的临时目录内。
    let canonical = fs::canonicalize(raw).map_err(|e| format!("无法解析请求文件路径: {e}"))?;
    if !config.is_allowed_request_path(&canonical) {
        return Err(format!(
            "请求文件不在允许的临时目录内，拒绝: {}",
            canonical.display()
        ));
    }

    // 防线 3（后半）＋ 4 ＋ 5：用已打开句柄做 fstat，避免「检查后再打开」的 TOCTOU。
    let file = fs::File::open(&canonical).map_err(|e| format!("打开请求文件失败: {e}"))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("读取请求文件元数据失败: {e}"))?;
    if !meta.file_type().is_file() {
        return Err("请求文件必须是常规文件，拒绝".to_string());
    }
    let mode = meta.permissions().mode() & 0o7777;
    if mode != 0o600 {
        return Err(format!("请求文件权限必须是 0600，实际为 0o{mode:o}，拒绝"));
    }
    if meta.len() > MAX_REQUEST_BYTES {
        return Err(format!(
            "请求文件超过 {MAX_REQUEST_BYTES} 字节上限，拒绝"
        ));
    }

    let mut bytes = Vec::new();
    file.take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("读取请求文件失败: {e}"))?;
    if bytes.len() as u64 != meta.len() {
        return Err("请求文件在读取期间被修改，拒绝".to_string());
    }

    // 防线 6：SHA-256 常数时间比较（长度不同直接判否）。
    let actual = sha256_hex(&bytes);
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if !constant_time_eq(actual.as_bytes(), expected.as_bytes()) {
        return Err("请求文件 SHA-256 校验失败，拒绝".to_string());
    }

    // 防线 7：解析 JSON 并校验版本。
    let request: PrivilegedRequest =
        serde_json::from_slice(&bytes).map_err(|e| format!("请求 JSON 解析失败: {e}"))?;
    if request.version != REQUEST_VERSION {
        return Err(format!(
            "请求版本不受支持: {}（需要 {}），拒绝",
            request.version, REQUEST_VERSION
        ));
    }
    if request.operations.len() > MAX_OPERATIONS {
        return Err(format!(
            "请求操作条数超过 {MAX_OPERATIONS} 条上限，拒绝"
        ));
    }
    Ok(request)
}

// ── 执行与回滚 ────────────────────────────────────────────────

/// 一条已执行操作的「撤销记录」。
struct AppliedChange {
    path: PathBuf,
    /// 原内容与原权限（文件原本不存在时为 `None`）。
    original: Option<(Vec<u8>, u32)>,
}

/// 逐个执行操作；任一失败即回滚此前（含本次）的改动。
pub fn apply_operations(
    operations: &[PrivilegedOp],
    roots: &AllowedRoots,
) -> Result<Vec<String>, String> {
    let mut undo: Vec<AppliedChange> = Vec::new();
    let mut applied: Vec<String> = Vec::new();

    for (index, operation) in operations.iter().enumerate() {
        match apply_one(operation, roots, &mut undo) {
            Ok(path) => applied.push(path),
            Err(error) => {
                rollback(&undo);
                return Err(format!("第 {} 项操作失败: {error}", index + 1));
            }
        }
    }
    Ok(applied)
}

fn apply_one(
    operation: &PrivilegedOp,
    roots: &AllowedRoots,
    undo: &mut Vec<AppliedChange>,
) -> Result<String, String> {
    match operation {
        PrivilegedOp::WriteFile {
            path,
            contents_base64,
        } => {
            let target = validate_target_path(path, roots)?;
            let bytes = base64_decode(contents_base64)?;
            if bytes.len() > MAX_CONTENT_BYTES {
                return Err(format!(
                    "写入内容 {} 字节超过 {MAX_CONTENT_BYTES} 字节上限",
                    bytes.len()
                ));
            }
            validate_content(&target, &bytes)?;
            registry::ensure_no_symlink(&target)?;
            // 先备份再写：失败时连同本次一起回滚。
            undo.push(AppliedChange {
                path: target.clone(),
                original: snapshot(&target),
            });
            registry::write_system_file(&target, &bytes)?;
            Ok(target.to_string_lossy().to_string())
        }
        PrivilegedOp::RemoveFile { path } => {
            let target = validate_target_path(path, roots)?;
            registry::ensure_no_symlink(&target)?;
            undo.push(AppliedChange {
                path: target.clone(),
                original: snapshot(&target),
            });
            registry::remove_system_file(&target)?;
            Ok(target.to_string_lossy().to_string())
        }
    }
}

/// 回滚（尽力而为，逆序）：原本存在则写回内容与权限，原本不存在则删除。
fn rollback(undo: &[AppliedChange]) {
    for change in undo.iter().rev() {
        match &change.original {
            Some((bytes, mode)) => {
                if registry::write_system_file(&change.path, bytes).is_ok() {
                    let _ = registry::set_permissions(&change.path, *mode);
                }
            }
            None => {
                let _ = fs::remove_file(&change.path);
            }
        }
    }
}

/// 内存快照：内容 + 权限位；不是常规文件（含符号链接）时返回 `None`。
fn snapshot(path: &Path) -> Option<(Vec<u8>, u32)> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    Some((bytes, meta.permissions().mode() & 0o7777))
}

/// 内容形态校验：`.xml` 必须是 ProductHints 片段，`.plist` 必须能被 plist 解析。
fn validate_content(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match extension.as_deref() {
        Some("xml") => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| "XML 内容不是合法 UTF-8，拒绝写入".to_string())?;
            if !text.contains("<ProductHints") || !text.contains("</ProductHints>") {
                return Err("XML 内容缺少 <ProductHints> 片段，拒绝写入".to_string());
            }
        }
        Some("plist") => {
            plist::Value::from_reader(std::io::Cursor::new(bytes))
                .map_err(|e| format!("plist 内容无法解析，拒绝写入: {e}"))?;
        }
        _ => {}
    }
    Ok(())
}

// ── 用户侧：构造请求 → 请求授权 → 执行 ────────────────────────

/// 以普通用户身份发起一次提权写入。
///
/// 步骤：构造 0700/0600 请求文件 → 计算 SHA-256 → 经 `tauri_plugin_shell` 调用
/// `osascript … with administrator privileges` → 解析助手 stdout 的结果 JSON。
/// 无论成功、失败还是用户取消，临时目录都会被清理（RAII 守卫）。
pub async fn run_privileged(
    app: &tauri::AppHandle,
    request: &PrivilegedRequest,
) -> Result<Vec<String>, String> {
    use tauri_plugin_shell::ShellExt;

    let directory = create_request_dir()?;
    // 守卫：任何提前 return / `?` 都会触发 Drop，保证失败也清理。
    let _guard = TempDirGuard(directory.clone());

    let request_path = directory.join(REQUEST_FILE_NAME);
    let bytes = serde_json::to_vec(request).map_err(|e| format!("序列化提权请求失败: {e}"))?;
    write_private_file(&request_path, &bytes)?;
    let expected_sha256 = sha256_hex(&bytes);

    let helper = std::env::current_exe()
        .map_err(|e| format!("无法确定助手可执行文件路径: {e}"))?;
    let script = osascript_script(&helper, &request_path, &expected_sha256);

    let output = app
        .shell()
        .command("/usr/bin/osascript")
        .args(["-e", script.as_str()])
        .output()
        .await
        .map_err(|e| format!("启动 osascript 失败: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        if is_cancelled(&stdout, &stderr) {
            return Err(format!(
                "{CANCELLED_MARKER}：未获得管理员授权，未写入任何系统级文件。"
            ));
        }
        let detail = parse_error_json(&stdout)
            .unwrap_or_else(|| tail_text(&stderr, 400));
        return Err(format!("提权写入失败：{detail}"));
    }

    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).map_err(|e| format!("无法解析助手输出: {e}"))?;
    if parsed.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        let applied = parsed
            .get("applied")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return Ok(applied);
    }
    Err(format!(
        "提权写入失败：{}",
        parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("助手未返回错误详情")
    ))
}

/// 构造 osascript 脚本。
///
/// **三个值都作为 AppleScript 变量、用 `quoted form of` 交给 shell 引用**，
/// 不把任何值直接拼进 shell 命令；AppleScript 字符串字面量本身也会转义。
fn osascript_script(helper: &Path, request_path: &Path, expected_sha256: &str) -> String {
    let helper = applescript_literal(&helper.to_string_lossy());
    let request = applescript_literal(&request_path.to_string_lossy());
    let sha = applescript_literal(expected_sha256);
    format!(
        "set helperPath to {helper}\n\
         set requestPath to {request}\n\
         set expectedSha to {sha}\n\
         do shell script (quoted form of helperPath & \" {HELPER_FLAG} \" & quoted form of requestPath & \" \" & quoted form of expectedSha) with administrator privileges"
    )
}

/// AppleScript 字符串字面量转义（反斜杠与双引号）。
fn applescript_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            // 路径里不可能出现换行；防御性替换，避免拼出跨行的脚本。
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// 用户取消授权：osascript 返回 `User canceled. (-128)`。
fn is_cancelled(stdout: &str, stderr: &str) -> bool {
    let hit = |text: &str| {
        text.contains(CANCELLED_CODE)
            || text.contains("User canceled")
            || text.contains("User cancelled")
            || text.contains("已取消")
    };
    hit(stdout) || hit(stderr)
}

/// 从助手 stdout 里取 `{"ok":false,"error":"…"}` 的 error（可能被 osascript 吞掉）。
fn parse_error_json(stdout: &str) -> Option<String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    value
        .get("error")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// 取文本尾部（错误信息可能很长）。
fn tail_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "osascript 未返回错误信息".to_string();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= limit {
        return trimmed.to_string();
    }
    chars[chars.len() - limit..].iter().collect()
}

/// 创建 0700 的请求目录：`<TMPDIR>/omuse-priv-<uuid>/`。
fn create_request_dir() -> Result<PathBuf, String> {
    let base = fs::canonicalize(std::env::temp_dir())
        .map_err(|e| format!("无法解析临时目录: {e}"))?;
    for _ in 0..16 {
        let candidate = base.join(format!("omuse-priv-{}", uuid_v4()));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&candidate) {
            Ok(()) => {
                // `mode(0o700)` 会与 umask 相与，这里再显式设置一次兜底。
                registry::set_permissions(&candidate, 0o700)?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("创建提权请求目录失败: {e}")),
        }
    }
    Err("无法创建唯一的提权请求目录".to_string())
}

/// 写 0600 的请求文件（`create_new` + 显式 chmod）。
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options
        .open(path)
        .map_err(|e| format!("创建提权请求文件失败: {e}"))?;
    file.write_all(bytes)
        .map_err(|e| format!("写入提权请求文件失败: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("落盘提权请求文件失败: {e}"))?;
    registry::set_permissions(path, 0o600)
}

/// 临时目录守卫：Drop 时递归删除，成功/失败/取消统一清理。
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ── 基础工具：SHA-256 / 常数时间比较 / base64 / UUID ──────────

/// SHA-256 的十六进制小写表示。
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 常数时间比较：逐字节异或累加，长度不同直接判否（不提前返回中间结果）。
///
/// 请求文件 SHA 校验用它，避免用 `==` 带来的短路比较。
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff: u8 = if a.len() == b.len() { 0 } else { 1 };
    for index in 0..a.len().min(b.len()) {
        diff |= a[index] ^ b[index];
    }
    diff == 0
}

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 标准 base64 编码（不引入新依赖）。
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let packed = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_ALPHABET[(packed >> 18) as usize & 63] as char);
        out.push(B64_ALPHABET[(packed >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[(packed >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[packed as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 标准 base64 解码（严格：非法字符、错误填充、错误长度一律拒绝）。
pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err("base64 长度不是 4 的倍数，拒绝".to_string());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut values = [0u32; 4];
        let mut padding = 0usize;
        for (index, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                if index < 2 {
                    return Err("base64 填充位置非法，拒绝".to_string());
                }
                padding += 1;
                continue;
            }
            if padding > 0 {
                return Err("base64 填充之后仍有数据，拒绝".to_string());
            }
            values[index] = b64_value(*byte)?;
        }
        let packed =
            (values[0] << 18) | (values[1] << 12) | (values[2] << 6) | values[3];
        out.push((packed >> 16) as u8);
        if padding < 2 {
            out.push((packed >> 8) as u8);
        }
        if padding < 1 {
            out.push(packed as u8);
        }
    }
    Ok(out)
}

fn b64_value(byte: u8) -> Result<u32, String> {
    match byte {
        b'A'..=b'Z' => Ok((byte - b'A') as u32),
        b'a'..=b'z' => Ok((byte - b'a') as u32 + 26),
        b'0'..=b'9' => Ok((byte - b'0') as u32 + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err("base64 含非法字符，拒绝".to_string()),
    }
}

/// UUID v4 字符串（仅用于临时目录名；不引入 `uuid` 依赖）。
fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    random_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// 填充随机字节：macOS 用 `arc4random_buf`；其他平台退回时间/进程号哈希。
fn random_bytes(out: &mut [u8]) {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        unsafe {
            libc::arc4random_buf(out.as_mut_ptr().cast(), out.len());
        }
        return;
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut hasher = Sha256::new();
        hasher.update(nanos.to_le_bytes());
        hasher.update(std::process::id().to_le_bytes());
        hasher.update(seq.to_le_bytes());
        let digest = hasher.finalize();
        for (index, slot) in out.iter_mut().enumerate() {
            *slot = digest[index % digest.len()];
        }
    }
}

/// 打印结果 JSON 到 stdout（助手与用户侧解析的唯一契约）。
fn print_result(ok: bool, applied: &[String], error: Option<&str>) {
    let value = if ok {
        serde_json::json!({ "ok": true, "applied": applied })
    } else {
        serde_json::json!({ "ok": false, "error": error.unwrap_or("未知错误") })
    };
    println!("{value}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合法的最小系统 plist 内容。
    const PLIST_BYTES: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<plist version=\"1.0\"><dict><key>RegKey</key><string>Roll</string>\
<key>SNPID</key><string>R1</string></dict></plist>";
    /// 合法的 ProductHints 片段。
    const XML_BYTES: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n\
<ProductHints spec=\"1.0.16\"><Product><Name>T</Name><RegKey>T</RegKey>\
<SNPID>T1</SNPID><ProductSpecific><Visibility type=\"Number\">3</Visibility>\
</ProductSpecific></Product></ProductHints>\n";

    /// 造一套临时夹具：service_center / preferences / 请求目录三者同根。
    /// **测试全部在这个 temp 目录里跑，绝不触碰真实 `/Library`。**
    fn fixture(tag: &str) -> PathBuf {
        let base = fs::canonicalize(std::env::temp_dir()).expect("临时目录可用");
        let dir = base.join(format!("omuse_priv_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("service_center")).unwrap();
        fs::create_dir_all(dir.join("preferences")).unwrap();
        dir
    }

    fn roots(dir: &Path) -> AllowedRoots {
        AllowedRoots::new(dir.join("service_center"), dir.join("preferences"))
    }

    fn helper_config(dir: &Path) -> HelperConfig {
        HelperConfig {
            roots: roots(dir),
            // 请求文件只允许落在夹具目录内（注入后与生产常量无关）。
            request_roots: vec![dir.to_path_buf()],
        }
    }

    fn write_request(dir: &Path, request: &PrivilegedRequest, mode: u32) -> (PathBuf, String) {
        let path = dir.join("request.json");
        let bytes = serde_json::to_vec(request).unwrap();
        fs::write(&path, &bytes).unwrap();
        registry::set_permissions(&path, mode).unwrap();
        (path, sha256_hex(&bytes))
    }

    fn write_op(path: &Path, contents: &[u8]) -> PrivilegedOp {
        PrivilegedOp::WriteFile {
            path: path.to_string_lossy().to_string(),
            contents_base64: base64_encode(contents),
        }
    }

    // 1) 路径白名单
    #[test]
    fn 白名单允许两个目录的直接子文件() {
        let dir = fixture("allow");
        let roots = roots(&dir);
        let xml = dir.join("service_center").join("Altera Keys.xml");
        let plist = dir.join("preferences").join("com.native-instruments.Altera Keys.plist");

        assert_eq!(
            validate_target_path(&xml.to_string_lossy(), &roots).unwrap(),
            xml
        );
        assert_eq!(
            validate_target_path(&plist.to_string_lossy(), &roots).unwrap(),
            plist
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 白名单拒绝一切非法路径() {
        let dir = fixture("reject");
        let roots = roots(&dir);
        let prefs = dir.join("preferences");
        let service = dir.join("service_center");

        let cases = vec![
            // 白名单之外的真实系统文件
            "/etc/passwd".to_string(),
            "/Library/Preferences/com.native-instruments.X.plist".to_string(),
            // 再嵌套子目录
            format!("{}/sub/x.plist", prefs.display()),
            format!("{}/a/b.plist", prefs.display()),
            // `..` 逃逸
            format!("{}/../escape.plist", prefs.display()),
            format!("{}/..", prefs.display()),
            // 空文件名 / 目录本身
            format!("{}/", prefs.display()),
            prefs.to_string_lossy().to_string(),
            // 超长文件名
            format!("{}/{}", prefs.display(), "a".repeat(300)),
            // 控制字符
            format!("{}/bad\u{1}name.plist", prefs.display()),
            // 非绝对路径
            "relative.plist".to_string(),
            "./x.plist".to_string(),
            String::new(),
        ];
        for case in cases {
            assert!(
                validate_target_path(&case, &roots).is_err(),
                "应拒绝: {case}"
            );
        }

        // 对照组：同目录下的直接子文件必须通过（证明拒绝不是因为根不匹配）。
        assert!(validate_target_path(&format!("{}/ok.plist", prefs.display()), &roots).is_ok());
        assert!(validate_target_path(
            &format!("{}/ok.xml", service.display()),
            &roots
        )
        .is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    // 2) 请求文件权限
    #[test]
    fn 请求文件权限非0600时拒绝() {
        let dir = fixture("mode");
        let config = helper_config(&dir);
        let request = PrivilegedRequest {
            version: REQUEST_VERSION,
            operations: vec![write_op(&dir.join("preferences").join("a.plist"), PLIST_BYTES)],
            reason: "单元测试".to_string(),
        };
        let (path, sha) = write_request(&dir, &request, 0o644);
        let error = validate_request_file(&path.to_string_lossy(), &sha, &config).unwrap_err();
        assert!(error.contains("0600"), "错误信息应提到权限: {error}");
        let _ = fs::remove_dir_all(&dir);
    }

    // 3) 请求文件是符号链接
    #[test]
    fn 请求文件是符号链接时拒绝() {
        let dir = fixture("symlink");
        let config = helper_config(&dir);
        let request = PrivilegedRequest {
            version: REQUEST_VERSION,
            operations: Vec::new(),
            reason: "单元测试".to_string(),
        };
        let (real, sha) = write_request(&dir, &request, 0o600);
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let error = validate_request_file(&link.to_string_lossy(), &sha, &config).unwrap_err();
        assert!(error.contains("符号链接"), "错误信息应说明符号链接: {error}");
        // 对照：真实文件本身可通过
        assert!(validate_request_file(&real.to_string_lossy(), &sha, &config).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    // 4) SHA-256 不匹配
    #[test]
    fn sha256不匹配时拒绝() {
        let dir = fixture("sha");
        let config = helper_config(&dir);
        let request = PrivilegedRequest {
            version: REQUEST_VERSION,
            operations: Vec::new(),
            reason: "单元测试".to_string(),
        };
        let (path, sha) = write_request(&dir, &request, 0o600);
        let wrong = format!("{}0", &sha[..63]);
        let error = validate_request_file(&path.to_string_lossy(), &wrong, &config).unwrap_err();
        assert!(error.contains("SHA-256"), "错误信息应提到校验失败: {error}");
        assert!(validate_request_file(&path.to_string_lossy(), "", &config).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    // 5) version != 1
    #[test]
    fn 请求版本不为1时拒绝() {
        let dir = fixture("version");
        let config = helper_config(&dir);
        let request = PrivilegedRequest {
            version: 2,
            operations: Vec::new(),
            reason: "单元测试".to_string(),
        };
        let (path, sha) = write_request(&dir, &request, 0o600);
        let error = validate_request_file(&path.to_string_lossy(), &sha, &config).unwrap_err();
        assert!(error.contains("版本"), "错误信息应提到版本: {error}");
        let _ = fs::remove_dir_all(&dir);
    }

    // 请求文件不在允许的临时目录内
    #[test]
    fn 请求文件越出允许临时目录时拒绝() {
        let dir = fixture("outside");
        let config = helper_config(&dir);
        let other = fixture("outside_other");
        let request = PrivilegedRequest {
            version: REQUEST_VERSION,
            operations: Vec::new(),
            reason: "单元测试".to_string(),
        };
        let (path, sha) = write_request(&other, &request, 0o600);
        let error = validate_request_file(&path.to_string_lossy(), &sha, &config).unwrap_err();
        assert!(error.contains("临时目录"), "错误信息应提到临时目录: {error}");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&other);
    }

    // 非 root 时助手拒绝
    #[test]
    fn 非root时助手拒绝() {
        if unsafe { libc::geteuid() } == 0 {
            return; // 以 root 跑测试时该防线无从验证
        }
        let dir = fixture("nonroot");
        let request = PrivilegedRequest {
            version: REQUEST_VERSION,
            operations: Vec::new(),
            reason: "单元测试".to_string(),
        };
        let (path, sha) = write_request(&dir, &request, 0o600);
        let error =
            run_helper(&path.to_string_lossy(), &sha, &helper_config(&dir)).unwrap_err();
        assert!(error.contains("root"), "错误信息应说明需要 root: {error}");
        let _ = fs::remove_dir_all(&dir);
    }

    // 6) XML 内容校验
    #[test]
    fn xml_缺少_producthints_时拒绝() {
        let dir = fixture("xml");
        let roots = roots(&dir);
        let target = roots.service_center.join("Bad.xml");

        let error = apply_operations(
            &[PrivilegedOp::WriteFile {
                path: target.to_string_lossy().to_string(),
                contents_base64: base64_encode(b"<foo/>"),
            }],
            &roots,
        )
        .unwrap_err();
        assert!(error.contains("ProductHints"), "错误信息应提到 ProductHints: {error}");
        assert!(!target.exists(), "非法内容不得落盘");

        // 对照：合法 ProductHints 可以写入
        let applied =
            apply_operations(&[write_op(&target, XML_BYTES)], &roots).expect("合法 XML 应写入");
        assert_eq!(applied.len(), 1);
        assert_eq!(fs::read(&target).unwrap(), XML_BYTES);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn plist无法解析时拒绝() {
        let dir = fixture("plist");
        let roots = roots(&dir);
        let target = roots.preferences.join("com.native-instruments.Bad.plist");
        let error = apply_operations(
            &[PrivilegedOp::WriteFile {
                path: target.to_string_lossy().to_string(),
                contents_base64: base64_encode(b"not a plist"),
            }],
            &roots,
        )
        .unwrap_err();
        assert!(error.contains("plist"), "错误信息应提到 plist: {error}");
        assert!(!target.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    // 7) 回滚
    #[test]
    fn 后续操作失败时回滚已执行的操作() {
        let dir = fixture("rollback");
        let roots = roots(&dir);
        let target = roots.preferences.join("com.native-instruments.Roll.plist");

        let operations = vec![
            write_op(&target, PLIST_BYTES),
            // 非法路径：/etc/passwd 不在白名单内 → 第二项失败 → 第一项必须回滚
            PrivilegedOp::RemoveFile {
                path: "/etc/passwd".to_string(),
            },
        ];

        // (a) 目标原本不存在 → 回滚后必须消失
        let error = apply_operations(&operations, &roots).unwrap_err();
        assert!(error.contains("第 2 项"), "错误信息应指出第几项: {error}");
        assert!(!target.exists(), "第一项写入必须被回滚");

        // (b) 目标原本存在 → 回滚后必须恢复原内容
        fs::write(&target, b"original-content").unwrap();
        assert!(apply_operations(&operations, &roots).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original-content");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 删除操作失败时回滚已删除的文件() {
        let dir = fixture("rollback_remove");
        let roots = roots(&dir);
        let target = roots.service_center.join("Roll.xml");
        fs::write(&target, XML_BYTES).unwrap();

        let operations = vec![
            PrivilegedOp::RemoveFile {
                path: target.to_string_lossy().to_string(),
            },
            PrivilegedOp::RemoveFile {
                path: "/etc/hosts".to_string(),
            },
        ];
        assert!(apply_operations(&operations, &roots).is_err());
        assert_eq!(fs::read(&target).unwrap(), XML_BYTES, "被删文件必须恢复");
        let _ = fs::remove_dir_all(&dir);
    }

    // 8) 常数时间比较
    #[test]
    fn 常数时间比较函数正确() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"ab", b"abc"));
        assert!(!constant_time_eq(b"", b"a"));
        // SHA-256 十六进制串（64 字符）长度不同也必须判否
        let sha = sha256_hex(b"payload");
        assert_eq!(sha.len(), 64);
        assert!(constant_time_eq(sha.as_bytes(), sha.as_bytes()));
        assert!(!constant_time_eq(sha.as_bytes(), &sha.as_bytes()[..63]));
    }

    #[test]
    fn base64往返与非法输入() {
        for case in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"fooba",
            b"foobar",
            &[0u8, 255, 128, 7][..],
        ] {
            let encoded = base64_encode(case);
            assert_eq!(base64_decode(&encoded).unwrap(), case, "{encoded}");
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert!(base64_decode("Zm9vYmF").is_err(), "长度非 4 的倍数应拒绝");
        assert!(base64_decode("Zm9v!mFy").is_err(), "非法字符应拒绝");
        assert!(base64_decode("=m9vYmFy").is_err(), "填充位置非法应拒绝");
        assert!(base64_decode("Zm9v=bar").is_err(), "填充后仍有数据应拒绝");
    }

    // 端到端（在 temp 夹具内）：合法请求 → 校验通过 → 应用 → 权限 0644
    #[test]
    fn 合法请求可通过校验并应用() {
        let dir = fixture("happy");
        let config = helper_config(&dir);
        let target = config.roots.preferences.join("com.native-instruments.Happy.plist");
        let request = PrivilegedRequest {
            version: REQUEST_VERSION,
            operations: vec![write_op(&target, PLIST_BYTES)],
            reason: "单元测试".to_string(),
        };
        let (path, sha) = write_request(&dir, &request, 0o600);

        let parsed = validate_request_file(&path.to_string_lossy(), &sha, &config).unwrap();
        assert_eq!(parsed.operations, request.operations);
        let applied = apply_operations(&parsed.operations, &config.roots).unwrap();
        assert_eq!(applied, vec![target.to_string_lossy().to_string()]);

        let meta = fs::symlink_metadata(&target).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o644, "系统文件权限应为 0644");
        assert_eq!(fs::read(&target).unwrap(), PLIST_BYTES);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 目标目录不存在时按0755创建() {
        let dir = fixture("mkdir");
        let roots = roots(&dir);
        // 模拟「Service Center 目录尚不存在」：删掉夹具里的目录再写。
        fs::remove_dir_all(&roots.service_center).unwrap();
        let target = roots.service_center.join("New.xml");

        let applied =
            apply_operations(&[write_op(&target, XML_BYTES)], &roots).expect("应能创建目录并写入");
        assert_eq!(applied.len(), 1);

        let dir_mode = fs::symlink_metadata(&roots.service_center)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(dir_mode, 0o755, "新建目录权限应为 0755");
        let file_mode = fs::symlink_metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(file_mode, 0o644, "新建文件权限应为 0644");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 助手对不存在的请求返回非0() {
        // 无论是否 root：文件不存在都必须失败（返回码 1）。
        assert_eq!(helper_entry("/nonexistent-omuse-request.json", "00"), 1);
    }

    #[test]
    fn 请求体积超限时拒绝() {
        let dir = fixture("oversize");
        let config = helper_config(&dir);
        let path = dir.join("big.json");
        let big = vec![b' '; MAX_REQUEST_BYTES as usize + 1];
        fs::write(&path, &big).unwrap();
        registry::set_permissions(&path, 0o600).unwrap();
        let sha = sha256_hex(&big);
        let error = validate_request_file(&path.to_string_lossy(), &sha, &config).unwrap_err();
        assert!(error.contains("上限"), "错误信息应提到体积上限: {error}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 助手参数常量与文档一致() {
        assert_eq!(HELPER_FLAG, "--privileged-helper");
        assert_eq!(REQUEST_VERSION, 1);
    }

    #[test]
    fn 请求_json_字段名稳定() {
        // 请求格式是对外契约（助手与用户侧共享），字段名变更必须有意识地改。
        let request = PrivilegedRequest {
            version: 1,
            operations: vec![PrivilegedOp::RemoveFile {
                path: "/Library/Preferences/x.plist".to_string(),
            }],
            reason: "r".to_string(),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["operations"][0]["kind"], "RemoveFile");
        assert_eq!(json["operations"][0]["path"], "/Library/Preferences/x.plist");
        assert!(json.get("reason").is_some());
    }

    #[test]
    fn osascript脚本不直接拼接取值() {
        let script = osascript_script(
            Path::new("/Applications/O Muse.app/Contents/MacOS/o-muse"),
            Path::new("/private/var/folders/x/omuse-priv-1/request.json"),
            "abc123",
        );
        // 三个值都以 AppleScript 变量 + `quoted form of` 传递
        assert!(script.contains("set helperPath to \"/Applications"));
        assert!(script.contains("quoted form of helperPath"));
        assert!(script.contains("quoted form of requestPath"));
        assert!(script.contains("quoted form of expectedSha"));
        assert!(script.contains("--privileged-helper"));
        assert!(script.contains("with administrator privileges"));
        // 含引号/反斜杠的值必须被转义，不能破坏脚本结构
        let escaped = applescript_literal("a\"b\\c");
        assert_eq!(escaped, "\"a\\\"b\\\\c\"");
        let injected = osascript_script(
            Path::new("/tmp/x\" & (do shell script \"rm -rf /\") & \""),
            Path::new("/tmp/r"),
            "s",
        );
        assert!(!injected.contains("do shell script \"rm -rf /\""), "注入必须被转义: {injected}");
    }
}
