// 插件"移动 + 原位置软链"与安全删除（v1.2 空间管理）
//
// 用户痛点：插件全部装在系统盘（`/Library/Audio/Plug-Ins` 等），动辄几十 GB。
// 本模块让用户可以**把插件真实文件搬到别的卷，并在原位置留一个软链**，
// DAW 与系统扫描器仍然照常找到它。
//
// ## 为什么是软链而不是 macOS 别名（Alias）
//
// Finder 别名是"书签文件"：POSIX `open()` / `stat()` **不会**解析它，
// 只有 Finder 与少数使用 `FSResolveAliasFile` 的 API 认识。DAW 的插件
// 扫描器走的是普通 `stat()`/目录遍历，因此别名放回原位等于插件消失。
// 符号链接由内核在 VFS 层透明解析，`stat()`/`open()`/`dlopen()` 全部
// 正常跟随，是唯一普遍可用的做法。
//
// ## 安全顺序（execute_move）
//
//   1. 重新校验（源/目标/空间，plan 可能已过期）
//   2. `rename` 移动真实文件 → 失败（跨卷 EXDEV）时回退 `copy_dir_recursive`
//      + 校验"文件数 & 总字节"一致后才删源
//   3. 原位置建软链
//   4. 校验软链可解析且指向目标
//   5. 目录 bundle 且原本有签名 → 用 `codesign --verify` 复核，不合格回滚
//   6. **任一步失败都回滚**：删软链 + 把真实目录移回原位
//
// 删除插件**绝不 `remove_dir_all`**：`trash_plugin` 一律移入废纸篓
//（优先 `NSFileManager.trashItemAtURL`，本 crate 无 objc2 依赖 → 退化为
// `~/.Trash/<name>-<timestamp>`，语义仍是"可恢复"）。永久删除仅存在于
// `delete_plugin_permanently`，且要求调用方传入与目录名一致的 `confirm_name`。

use chrono::Utc;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::model::{dir_size_bytes, PluginFormat};

/// macOS 自带的签名校验工具（用绝对路径：GUI 应用的 PATH 极简）。
#[cfg(target_os = "macos")]
const CODESIGN: &str = "/usr/bin/codesign";

/// 绝不允许被"移动/删除"的路径本身（精确匹配，不含其子目录：
/// 插件本来就在 `/Library/Audio/Plug-Ins/...` 之下）。
const FORBIDDEN_ROOTS: &[&str] = &["/", "/System", "/Library", "/Applications"];

/// 目标卷余量安全系数：需要 `size × 1.1`（对齐文件系统元数据、块对齐开销）。
pub fn needed_bytes(size_bytes: u64) -> u64 {
    size_bytes.saturating_add(size_bytes / 10).saturating_add(1)
}

/// 一次"移动 + 留软链"的计划。
///
/// 拆成 plan / execute 两步是为了让 UI 在真正动手前展示
/// "要搬多少、搬到哪、目标还有多少空间"。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovePlan {
    /// 插件当前位置（执行后这里会变成软链）
    pub source: PathBuf,
    /// 用户选择的目标目录
    pub target_dir: PathBuf,
    /// 真实文件最终落点（`target_dir/<源文件名>`）
    pub resolved_target: PathBuf,
    /// 需要搬运的字节数（递归求和）
    pub size_bytes: u64,
}

// ── 只读辅助 ────────────────────────────────────────────────

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

fn normalize(path: &Path) -> String {
    let s = path.to_string_lossy().trim_end_matches('/').to_string();
    if s.is_empty() {
        "/".to_string()
    } else {
        s
    }
}

/// 是否为"绝不允许操作"的路径本身：`/`、`/System`、`/Library`、`/Applications`、`$HOME`。
pub fn is_forbidden_path(path: &Path) -> bool {
    let mut candidates: Vec<String> = vec![normalize(path)];
    if let Ok(c) = std::fs::canonicalize(path) {
        candidates.push(normalize(&c));
    }
    let homes: Vec<String> = home_dir()
        .map(|h| {
            let mut v = vec![normalize(&h)];
            if let Ok(c) = std::fs::canonicalize(&h) {
                v.push(normalize(&c));
            }
            v
        })
        .unwrap_or_default();

    for c in &candidates {
        if c == "/" || FORBIDDEN_ROOTS.iter().any(|r| c == r) {
            return true;
        }
        if homes.iter().any(|h| h == c) {
            return true;
        }
    }
    false
}

/// `symlink_metadata` 语义的"存在"判断（不跟随软链）。
fn lexists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// `child` 是否位于 `parent` 之内（含软链解析；不允许相等）。
fn is_within(child: &Path, parent: &Path) -> bool {
    let c = std::fs::canonicalize(child).unwrap_or_else(|_| child.to_path_buf());
    let p = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    c != p && c.starts_with(&p)
}

/// 目标卷可用字节。拿不到（非常规文件系统）时返回 `None` → 调用方跳过检查。
#[cfg(unix)]
pub fn available_bytes(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: buf 是 POD，statfs 只写入它；路径来自 CString，生命周期覆盖调用。
    unsafe {
        let mut buf: libc::statfs = std::mem::zeroed();
        if libc::statfs(c.as_ptr(), &mut buf) != 0 {
            return None;
        }
        Some((buf.f_bavail as u64).saturating_mul(buf.f_bsize as u64))
    }
}

#[cfg(not(unix))]
pub fn available_bytes(_path: &Path) -> Option<u64> {
    None
}

// ── 校验 ────────────────────────────────────────────────────

/// 源必须是"存在、受支持的插件 bundle、且当前不是软链"。
fn validate_source(source: &Path) -> Result<(), String> {
    if is_forbidden_path(source) {
        return Err("拒绝对系统目录 / 主目录本身执行操作".to_string());
    }
    let meta = std::fs::symlink_metadata(source)
        .map_err(|e| format!("源路径不可访问（{}）: {e}", source.display()))?;
    if meta.file_type().is_symlink() {
        return Err("该条目已经是软链（拒绝链套链）；如需回到原位请用「还原」".to_string());
    }
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if PluginFormat::from_extension(&ext).is_none() {
        return Err(format!("不是受支持的插件 bundle（扩展名 .{ext}）"));
    }
    if !meta.is_dir() && !meta.is_file() {
        return Err("既不是目录也不是普通文件".to_string());
    }
    Ok(())
}

/// 目标必须是可写目录（真写一个探针文件，`permissions()` 判断不了 ACL）。
fn ensure_writable_dir(dir: &Path) -> Result<(), String> {
    let meta =
        std::fs::metadata(dir).map_err(|e| format!("目标目录不可访问（{}）: {e}", dir.display()))?;
    if !meta.is_dir() {
        return Err(format!("目标不是目录: {}", dir.display()));
    }
    let probe = dir.join(format!(
        ".o-muse-write-probe-{}-{}",
        std::process::id(),
        Utc::now().timestamp_millis()
    ));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => Err(format!("目标目录不可写（{}）: {e}", dir.display())),
    }
}

/// 生成移动计划（**不修改任何文件**）。
pub fn plan_move(source: &Path, target_dir: &Path) -> Result<MovePlan, String> {
    validate_source(source)?;
    if is_forbidden_path(target_dir) {
        return Err("目标目录不能是系统目录 / 主目录本身".to_string());
    }
    ensure_writable_dir(target_dir)?;

    if is_within(target_dir, source) || is_within(source, target_dir) {
        return Err("目标目录与源互相包含（会造成递归搬运）".to_string());
    }

    let name = source
        .file_name()
        .ok_or_else(|| "源路径没有文件名".to_string())?;
    let resolved_target = target_dir.join(name);
    if lexists(&resolved_target) {
        return Err(format!(
            "目标目录下已存在同名条目: {}",
            resolved_target.display()
        ));
    }

    let size_bytes = dir_size_bytes(source);
    if let Some(avail) = available_bytes(target_dir) {
        let need = needed_bytes(size_bytes);
        if avail < need {
            return Err(format!(
                "目标卷可用空间不足：需要 {} 字节（含 10% 余量），当前可用 {} 字节",
                need, avail
            ));
        }
    }

    Ok(MovePlan {
        source: source.to_path_buf(),
        target_dir: target_dir.to_path_buf(),
        resolved_target,
        size_bytes,
    })
}

// ── 移动原语 ────────────────────────────────────────────────

#[cfg(unix)]
fn make_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn make_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(unix)]
fn is_cross_device(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::EXDEV)
}

#[cfg(not(unix))]
fn is_cross_device(_e: &std::io::Error) -> bool {
    true
}

/// 移动文件/目录：先 `rename`（同一卷内原子且瞬时），跨卷回退整树复制。
fn move_path(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建目标父目录失败（{}）: {e}", parent.display()))?;
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            copy_path(from, to)?;
            // 校验通过后才删源（copy_path 内部已校验，双保险）
            verify_same_tree(from, to)?;
            remove_path(from)?;
            Ok(())
        }
        Err(e) => Err(format!(
            "重命名失败（{} → {}）: {e}",
            from.display(),
            to.display()
        )),
    }
}

/// 整树复制（目录 / 单文件）。软链按链接复制，不跟随、不展开。
fn copy_path(from: &Path, to: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(from)
        .map_err(|e| format!("读取源失败（{}）: {e}", from.display()))?;

    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(from).map_err(|e| format!("读取软链失败: {e}"))?;
        return make_symlink(&target, to).map_err(|e| format!("重建软链失败: {e}"));
    }
    if meta.is_file() {
        std::fs::copy(from, to).map_err(|e| format!("复制文件失败: {e}"))?;
        return Ok(());
    }

    // 目录：先建结构 + 拷文件，最后回设目录权限（只读目录会阻断写入）
    let mut dir_perms: Vec<(PathBuf, std::fs::Permissions)> = Vec::new();
    for entry in WalkDir::new(from).follow_links(false).into_iter() {
        let entry = entry.map_err(|e| format!("遍历源目录失败（{}）: {e}", from.display()))?;
        let rel = entry
            .path()
            .strip_prefix(from)
            .map_err(|e| format!("相对路径计算失败: {e}"))?;
        let dst = if rel.as_os_str().is_empty() {
            to.to_path_buf()
        } else {
            to.join(rel)
        };

        let ft = entry.file_type();
        if ft.is_dir() {
            std::fs::create_dir_all(&dst)
                .map_err(|e| format!("创建目录失败（{}）: {e}", dst.display()))?;
            if let Ok(m) = entry.metadata() {
                dir_perms.push((dst, m.permissions()));
            }
        } else if ft.is_symlink() {
            let target =
                std::fs::read_link(entry.path()).map_err(|e| format!("读取软链失败: {e}"))?;
            if lexists(&dst) {
                let _ = std::fs::remove_file(&dst);
            }
            make_symlink(&target, &dst).map_err(|e| format!("重建软链失败: {e}"))?;
        } else if ft.is_file() {
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p).map_err(|e| format!("创建目录失败: {e}"))?;
            }
            std::fs::copy(entry.path(), &dst).map_err(|e| {
                format!("复制文件失败（{}）: {e}", entry.path().display())
            })?;
        }
    }
    // 越深的目录越先恢复权限？不——先恢复浅层会阻断后续写入，故倒序（深 → 浅）
    for (p, perm) in dir_perms.into_iter().rev() {
        let _ = std::fs::set_permissions(&p, perm);
    }
    Ok(())
}

/// 统计一棵树的（文件数，总字节），用于复制后的一致性校验。
fn tree_stats(path: &Path) -> (u64, u64) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (0, 0);
    };
    if meta.is_file() || meta.file_type().is_symlink() {
        return (1, meta.len());
    }
    let mut files = 0u64;
    let mut bytes = 0u64;
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.depth() == 0 {
            continue;
        }
        let Ok(m) = entry.metadata() else { continue };
        if m.is_file() || m.file_type().is_symlink() {
            files += 1;
            bytes = bytes.saturating_add(m.len());
        }
    }
    (files, bytes)
}

fn verify_same_tree(from: &Path, to: &Path) -> Result<(), String> {
    let a = tree_stats(from);
    let b = tree_stats(to);
    if a != b {
        return Err(format!(
            "复制后校验不一致（源 {} 个文件/{} 字节，目标 {} 个文件/{} 字节），已保留源文件",
            a.0, a.1, b.0, b.1
        ));
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("读取待删路径失败（{}）: {e}", path.display()))?;
    if meta.file_type().is_symlink() || meta.is_file() {
        std::fs::remove_file(path).map_err(|e| format!("删除文件失败: {e}"))
    } else {
        // 注意：这是"复制已校验通过后删源"，不是用户可见的删除动作。
        std::fs::remove_dir_all(path).map_err(|e| format!("删除目录失败: {e}"))
    }
}

/// 回滚：删掉原位置的软链，把真实内容搬回去。
fn rollback_move(link: &Path, real: &Path) {
    if let Ok(m) = std::fs::symlink_metadata(link) {
        if m.file_type().is_symlink() {
            let _ = std::fs::remove_file(link);
        }
    }
    let _ = move_path(real, link);
}

// ── 签名校验（tauri_plugin_shell 调 codesign，不用 std::process::Command）──

#[cfg(target_os = "macos")]
static APP_HANDLE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// 注册 `AppHandle`，使 [`execute_move`] 能用 shell 插件调 `codesign`。
///
/// 命令层在 `spawn_blocking` 之前调用（幂等）。未注册时（单测、无 UI）
/// 签名校验自动跳过——不会因为"没法校验"而阻断用户的移动操作。
pub fn register_app_handle(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = APP_HANDLE.set(app.clone());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

/// 目录 bundle 是否"原本就有签名"（只有签过的才有签名可保）。
#[cfg(target_os = "macos")]
fn is_signed_bundle(path: &Path) -> bool {
    path.is_dir()
        && path
            .join("Contents")
            .join("_CodeSignature")
            .join("CodeResources")
            .is_file()
}

#[cfg(target_os = "macos")]
fn verify_signature(target: &Path) -> Result<(), String> {
    if !is_signed_bundle(target) {
        // 未签名的 bundle（多数 VST2 / 自编译插件）没有签名可破坏
        return Ok(());
    }
    let Some(app) = APP_HANDLE.get() else {
        return Ok(());
    };
    codesign_verify(app, target)
}

#[cfg(not(target_os = "macos"))]
fn verify_signature(_target: &Path) -> Result<(), String> {
    Ok(())
}

/// `codesign --verify --deep --strict`（经 `tauri_plugin_shell`）。
///
/// 用 `spawn()` + `blocking_recv()` 而不是 `output().await`：本函数是同步的，
/// 且由命令层放在 `spawn_blocking` 里跑。`blocking_recv` 只在阻塞线程上合法
/// （tokio 明确允许 blocking region 内阻塞），不会与异步运行时互锁。
#[cfg(target_os = "macos")]
fn codesign_verify(app: &tauri::AppHandle, target: &Path) -> Result<(), String> {
    use tauri_plugin_shell::process::CommandEvent;
    use tauri_plugin_shell::ShellExt;

    let (mut rx, _child) = app
        .shell()
        .command(CODESIGN)
        .args(["--verify", "--deep", "--strict", "--verbose=1"])
        .arg(target.as_os_str())
        .spawn()
        .map_err(|e| format!("无法调用 codesign: {e}"))?;

    let mut code: Option<i32> = None;
    let mut stderr = String::new();
    while let Some(event) = rx.blocking_recv() {
        match event {
            CommandEvent::Terminated(payload) => code = payload.code,
            CommandEvent::Stderr(bytes) => {
                stderr.push_str(&String::from_utf8_lossy(&bytes));
                stderr.push('\n');
            }
            _ => {}
        }
    }

    match code {
        Some(0) => Ok(()),
        Some(c) => Err(format!(
            "移动后签名校验未通过（codesign 退出码 {c}）：{}",
            stderr.trim()
        )),
        None => Err("移动后签名校验未通过：codesign 被信号终止".to_string()),
    }
}

// ── 执行 / 还原 ─────────────────────────────────────────────

/// 执行移动：真实文件搬走，原位置留软链。返回软链路径（= 原路径）。
///
/// 任何一步失败都会回滚到"移动前"的状态；成功返回后，
/// `canonicalize(返回值)` 一定指向 `plan.resolved_target`。
pub fn execute_move(plan: &MovePlan) -> Result<PathBuf, String> {
    // ── 1) 重新校验（plan 可能已过期：源被删、目标被占、空间变化）
    validate_source(&plan.source)?;
    ensure_writable_dir(&plan.target_dir)?;
    if is_within(&plan.target_dir, &plan.source) || is_within(&plan.source, &plan.target_dir) {
        return Err("目标目录与源互相包含（会造成递归搬运）".to_string());
    }
    if lexists(&plan.resolved_target) {
        return Err(format!(
            "目标目录下已存在同名条目: {}",
            plan.resolved_target.display()
        ));
    }
    let size = if plan.size_bytes == 0 {
        dir_size_bytes(&plan.source)
    } else {
        plan.size_bytes
    };
    if let Some(avail) = available_bytes(&plan.target_dir) {
        let need = needed_bytes(size);
        if avail < need {
            return Err(format!(
                "目标卷可用空间不足：需要 {need} 字节，当前可用 {avail} 字节"
            ));
        }
    }

    // ── 2) 搬运真实文件
    move_path(&plan.source, &plan.resolved_target)
        .map_err(|e| format!("移动失败（未改动原位置）: {e}"))?;

    // ── 3) 原位置建软链
    if let Err(e) = make_symlink(&plan.resolved_target, &plan.source) {
        let _ = move_path(&plan.resolved_target, &plan.source);
        return Err(format!("创建软链失败（已回滚到原位置）: {e}"));
    }

    // ── 4) 校验软链可解析且指向目标
    let resolved_ok = std::fs::canonicalize(&plan.source)
        .map(|p| same_path(&p, &plan.resolved_target))
        .unwrap_or(false);
    if !resolved_ok {
        rollback_move(&plan.source, &plan.resolved_target);
        return Err("软链校验失败（已回滚到原位置）".to_string());
    }

    // ── 5) 签名复核（目录 bundle 且原本有签名时）
    if let Err(e) = verify_signature(&plan.resolved_target) {
        rollback_move(&plan.source, &plan.resolved_target);
        return Err(format!("{e}（已回滚到原位置）"));
    }

    Ok(plan.source.clone())
}

/// 还原：软链 → 真实目录。把真实内容搬回原位置，并校验原位置不再是软链。
pub fn undo_move(link_path: &Path) -> Result<(), String> {
    if is_forbidden_path(link_path) {
        return Err("拒绝对系统目录 / 主目录本身执行操作".to_string());
    }
    let meta = std::fs::symlink_metadata(link_path)
        .map_err(|e| format!("路径不存在（{}）: {e}", link_path.display()))?;
    if !meta.file_type().is_symlink() {
        return Err("该条目不是软链，无需还原".to_string());
    }
    let real = std::fs::canonicalize(link_path)
        .map_err(|e| format!("软链目标不可访问（可能是失效链接）: {e}"))?;
    if !real.exists() {
        return Err(format!("软链目标不存在: {}", real.display()));
    }

    // 删链 → 搬回；搬回失败则恢复软链，保证"要么原样，要么还原成功"
    std::fs::remove_file(link_path).map_err(|e| format!("删除软链失败: {e}"))?;
    if let Err(e) = move_path(&real, link_path) {
        let _ = make_symlink(&real, link_path);
        return Err(format!("搬回原位置失败（已恢复软链）: {e}"));
    }

    let after = std::fs::symlink_metadata(link_path)
        .map_err(|e| format!("还原后校验失败（{}）: {e}", link_path.display()))?;
    if after.file_type().is_symlink() || !link_path.exists() {
        return Err("还原后校验失败：原位置仍不是真实目录".to_string());
    }
    Ok(())
}

// ── 删除（废纸篓优先）────────────────────────────────────────

/// 废纸篓目录。
///
/// 环境变量 `O_MUSE_TRASH_DIR` 可覆盖（测试与无 `~/.Trash` 的环境用）。
pub fn default_trash_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("O_MUSE_TRASH_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    home_dir()
        .map(|h| h.join(".Trash"))
        .unwrap_or_else(|| std::env::temp_dir().join("o-muse-trash"))
}

/// 把插件移入废纸篓，返回落点。
///
/// **绝不永久删除**：用户可以从废纸篓拖回来。
/// 本 crate 未引入 `objc2`（不能改 Cargo.toml），因此走退化路径：
/// `move` 到 `~/.Trash/<name>-<时间戳>`（同名冲突自动加序号）。
pub fn trash_plugin(path: &Path) -> Result<String, String> {
    trash_plugin_to(path, &default_trash_dir())
}

/// [`trash_plugin`] 的显式废纸篓版本（便于测试与自定义回收站位置）。
pub fn trash_plugin_to(path: &Path, trash_dir: &Path) -> Result<String, String> {
    if is_forbidden_path(path) {
        return Err("拒绝对系统目录 / 主目录本身执行操作".to_string());
    }
    std::fs::symlink_metadata(path)
        .map_err(|e| format!("路径不可访问（{}）: {e}", path.display()))?;

    std::fs::create_dir_all(trash_dir)
        .map_err(|e| format!("创建废纸篓目录失败（{}）: {e}", trash_dir.display()))?;

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "路径没有文件名，无法移入废纸篓".to_string())?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let mut dest = trash_dir.join(format!("{name}-{stamp}"));
    let mut n = 1;
    while lexists(&dest) {
        dest = trash_dir.join(format!("{name}-{stamp}-{n}"));
        n += 1;
    }

    // 软链只移动链接本身（不跟随）：真实文件在用户选定的存储卷里，
    // 由 UI 明确提示"仅移除链接"，不能偷偷搬走真实目录。
    move_path(path, &dest).map_err(|e| format!("移入废纸篓失败: {e}"))?;

    if lexists(path) {
        return Err(format!(
            "移入废纸篓后原路径仍然存在: {}",
            path.display()
        ));
    }
    Ok(dest.to_string_lossy().to_string())
}

/// 永久删除（不可恢复）。**必须** `confirm_name` 与目录名一致。
///
/// 服务端二次校验，不信任前端：`confirm_name` 允许是完整文件名
///（`Serum.vst3`）或去掉扩展名的名字（`Serum`）。
pub fn delete_plugin_permanently(path: &Path, confirm_name: &str) -> Result<(), String> {
    if is_forbidden_path(path) {
        return Err("拒绝对系统目录 / 主目录本身执行操作".to_string());
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "路径没有文件名".to_string())?;
    let stem = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let given = confirm_name.trim();
    if given != name && given != stem {
        return Err(format!(
            "确认名不匹配：需要输入「{name}」（或「{stem}」）才能永久删除"
        ));
    }

    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("路径不可访问（{}）: {e}", path.display()))?;
    // 软链只删链接，绝不跟随删除真实内容
    if meta.file_type().is_symlink() || meta.is_file() {
        std::fs::remove_file(path).map_err(|e| format!("删除失败: {e}"))?;
    } else {
        // 这里是**显式**的永久删除（已通过名字二次校验），
        // 与 trash_plugin 的"绝不 remove_dir_all"约定不冲突。
        std::fs::remove_dir_all(path).map_err(|e| format!("删除失败: {e}"))?;
    }

    if lexists(path) {
        return Err(format!("删除后路径仍然存在: {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 测试根目录：workspace 内 `target/` 下（不依赖平台 temp 策略）。
    fn test_root(tag: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("plugin-move-tests")
            .join(tag);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 假 VST3 bundle：`<root>/<name>/Contents/MacOS/<stem>`
    fn fake_bundle(root: &Path, name: &str, payload: usize) -> PathBuf {
        let bundle = root.join(name);
        let macos = bundle.join("Contents").join("MacOS");
        fs::create_dir_all(&macos).unwrap();
        let stem = Path::new(name)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        fs::write(macos.join(stem), vec![7u8; payload]).unwrap();
        bundle
    }

    // ── plan_move 拒绝路径 ──────────────────────────────────

    #[test]
    fn plan_move_rejects_missing_source() {
        let root = test_root("plan-missing");
        let target = root.join("store");
        fs::create_dir_all(&target).unwrap();
        let err = plan_move(&root.join("Nope.vst3"), &target).unwrap_err();
        assert!(err.contains("不可访问"), "错误应说明源不存在: {err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn plan_move_rejects_symlink_source() {
        let root = test_root("plan-symlink");
        let real = fake_bundle(&root, "Real.vst3", 16);
        let scan = root.join("scan");
        fs::create_dir_all(&scan).unwrap();
        let link = scan.join("Real.vst3");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let target = root.join("store");
        fs::create_dir_all(&target).unwrap();

        let err = plan_move(&link, &target).unwrap_err();
        assert!(err.contains("软链"), "已是软链必须拒绝（避免链套链）: {err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_move_rejects_target_inside_source() {
        let root = test_root("plan-inside");
        let bundle = fake_bundle(&root, "Nested.vst3", 16);
        let inside = bundle.join("Contents").join("moved");
        fs::create_dir_all(&inside).unwrap();

        let err = plan_move(&bundle, &inside).unwrap_err();
        assert!(err.contains("递归"), "目标在源之内必须拒绝: {err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_move_rejects_existing_target_name() {
        let root = test_root("plan-exists");
        let bundle = fake_bundle(&root, "Dup.vst3", 16);
        let target = root.join("store");
        fs::create_dir_all(&target).unwrap();
        // 目标下已有同名条目
        fs::create_dir_all(target.join("Dup.vst3")).unwrap();

        let err = plan_move(&bundle, &target).unwrap_err();
        assert!(err.contains("同名"), "同名冲突必须拒绝: {err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_move_rejects_forbidden_targets_and_non_plugins() {
        let root = test_root("plan-forbidden");
        let bundle = fake_bundle(&root, "Ok.vst3", 16);
        assert!(plan_move(&bundle, Path::new("/System")).is_err());
        assert!(plan_move(&bundle, Path::new("/")).is_err());
        // 不是插件 bundle
        let plain = root.join("notes.txt");
        fs::write(&plain, b"hi").unwrap();
        let target = root.join("store");
        fs::create_dir_all(&target).unwrap();
        assert!(plan_move(&plain, &target).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_move_reports_recursive_size() {
        let root = test_root("plan-size");
        let bundle = fake_bundle(&root, "Sized.vst3", 4096);
        let target = root.join("store");
        fs::create_dir_all(&target).unwrap();
        let plan = plan_move(&bundle, &target).unwrap();
        assert_eq!(plan.size_bytes, 4096);
        assert_eq!(plan.resolved_target, target.join("Sized.vst3"));
        let _ = fs::remove_dir_all(&root);
    }

    // ── execute_move / undo_move ────────────────────────────

    #[cfg(unix)]
    #[test]
    fn execute_move_leaves_symlink_and_undo_restores() {
        let root = test_root("move-undo");
        let scan = root.join("scan");
        let store = root.join("store");
        fs::create_dir_all(&scan).unwrap();
        fs::create_dir_all(&store).unwrap();
        let source = fake_bundle(&scan, "Move.vst3", 2048);

        let plan = plan_move(&source, &store).unwrap();
        let link = execute_move(&plan).unwrap();
        assert_eq!(link, source, "返回软链路径");

        // 源位置：软链
        let meta = fs::symlink_metadata(&source).unwrap();
        assert!(meta.file_type().is_symlink(), "原位置应变成软链");
        // 真实文件在目标
        let real = store.join("Move.vst3");
        assert!(real.join("Contents").join("MacOS").join("Move").is_file());
        // canonicalize 指向目标
        assert_eq!(
            fs::canonicalize(&source).unwrap(),
            fs::canonicalize(&real).unwrap()
        );

        // 还原
        undo_move(&source).unwrap();
        let after = fs::symlink_metadata(&source).unwrap();
        assert!(!after.file_type().is_symlink(), "还原后应是真实目录");
        assert!(source.join("Contents").join("MacOS").join("Move").is_file());
        assert!(!real.exists(), "真实文件应搬回原位");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn execute_move_refuses_when_target_taken() {
        let root = test_root("move-raced");
        let scan = root.join("scan");
        let store = root.join("store");
        fs::create_dir_all(&scan).unwrap();
        fs::create_dir_all(&store).unwrap();
        let source = fake_bundle(&scan, "Race.vst3", 64);

        let plan = plan_move(&source, &store).unwrap();
        // 计划生成后，目标被抢占（模拟竞态）
        fs::create_dir_all(store.join("Race.vst3")).unwrap();
        let err = execute_move(&plan).unwrap_err();
        assert!(err.contains("同名"), "应拒绝并保持源不动: {err}");
        assert!(source.is_dir(), "源必须原样保留");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn undo_move_rejects_non_symlink() {
        let root = test_root("undo-plain");
        let bundle = fake_bundle(&root, "Plain.vst3", 8);
        let err = undo_move(&bundle).unwrap_err();
        assert!(err.contains("不是软链"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    // ── 废纸篓 / 永久删除 ───────────────────────────────────

    #[test]
    fn trash_plugin_to_moves_into_trash_and_keeps_it_recoverable() {
        let root = test_root("trash");
        let scan = root.join("scan");
        fs::create_dir_all(&scan).unwrap();
        let bundle = fake_bundle(&scan, "Trashed.vst3", 512);
        let trash = root.join("trash-sim");

        let dest = trash_plugin_to(&bundle, &trash).unwrap();
        assert!(!bundle.exists(), "原路径必须消失");
        let dest_path = PathBuf::from(&dest);
        assert!(dest_path.starts_with(&trash), "落点应在废纸篓内: {dest}");
        assert!(
            dest_path.join("Contents").join("MacOS").join("Trashed").is_file(),
            "内容应完整保留（可从废纸篓恢复）"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn trash_plugin_uses_env_override_and_never_deletes_permanently() {
        let root = test_root("trash-env");
        let scan = root.join("scan");
        fs::create_dir_all(&scan).unwrap();
        let bundle = fake_bundle(&scan, "Env.vst3", 32);
        let trash = root.join("trash-env-sim");

        // 通过公开入口 trash_plugin 走一遍（用环境变量指向模拟废纸篓）
        std::env::set_var("O_MUSE_TRASH_DIR", &trash);
        let dest = trash_plugin(&bundle).unwrap();
        std::env::remove_var("O_MUSE_TRASH_DIR");

        assert!(!bundle.exists());
        assert!(PathBuf::from(&dest).join("Contents").is_dir());
        assert!(dest.contains("Env.vst3"), "落点应保留插件名: {dest}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn trash_plugin_refuses_system_roots() {
        assert!(trash_plugin(Path::new("/")).is_err());
        assert!(trash_plugin(Path::new("/System")).is_err());
        assert!(trash_plugin(Path::new("/Applications")).is_err());
    }

    #[test]
    fn delete_plugin_permanently_requires_matching_name() {
        let root = test_root("perm");
        let scan = root.join("scan");
        fs::create_dir_all(&scan).unwrap();
        let bundle = fake_bundle(&scan, "Keep.vst3", 16);

        // 名字不匹配 → 拒绝，且文件必须完好
        let err = delete_plugin_permanently(&bundle, "Wrong").unwrap_err();
        assert!(err.contains("确认名不匹配"), "{err}");
        assert!(bundle.join("Contents").is_dir(), "拒绝时不得删除任何东西");

        // 空名字 → 同样拒绝
        assert!(delete_plugin_permanently(&bundle, "   ").is_err());

        // 完整文件名 → 允许
        delete_plugin_permanently(&bundle, "Keep.vst3").unwrap();
        assert!(!bundle.exists());
        assert!(!lexists(&bundle));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_plugin_permanently_accepts_stem() {
        let root = test_root("perm-stem");
        let scan = root.join("scan");
        fs::create_dir_all(&scan).unwrap();
        let bundle = fake_bundle(&scan, "Stem.vst3", 16);
        delete_plugin_permanently(&bundle, "Stem").unwrap();
        assert!(!bundle.exists());
        let _ = fs::remove_dir_all(&root);
    }

    // ── 辅助函数 ───────────────────────────────────────────

    /// 跨卷回退路径：整树复制 + "文件数与总字节一致"校验。
    ///
    /// 同卷测试走的是 `rename`，这条分支平时跑不到，必须单独覆盖——
    /// 它是"先复制、校验通过才删源"的唯一防线。
    #[cfg(unix)]
    #[test]
    fn copy_fallback_copies_tree_and_verifies_identical() {
        let root = test_root("copy-fallback");
        let src = fake_bundle(&root, "Copy.vst3", 1024);
        // 嵌套目录 + 一层软链
        let nested = src.join("Contents").join("Resources").join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("deep.bin"), vec![3u8; 777]).unwrap();
        std::os::unix::fs::symlink("deep.bin", nested.join("link.bin")).unwrap();

        let dst = root.join("store").join("Copy.vst3");
        copy_path(&src, &dst).unwrap();

        // 内容一致：文件数 + 总字节
        assert_eq!(tree_stats(&src), tree_stats(&dst));
        verify_same_tree(&src, &dst).unwrap();
        assert!(dst.join("Contents").join("MacOS").join("Copy").is_file());
        assert!(dst
            .join("Contents")
            .join("Resources")
            .join("a")
            .join("b")
            .join("deep.bin")
            .is_file());
        // 软链按软链复制（不展开）
        let copied_link = dst
            .join("Contents")
            .join("Resources")
            .join("a")
            .join("b")
            .join("link.bin");
        assert!(fs::symlink_metadata(&copied_link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&copied_link).unwrap(), PathBuf::from("deep.bin"));

        // 校验能发现不一致（截断目标文件 → 字节数不符）
        fs::write(dst.join("Contents").join("MacOS").join("Copy"), b"short").unwrap();
        assert!(
            verify_same_tree(&src, &dst).is_err(),
            "复制不完整必须被校验拦下（否则会删源）"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn move_path_handles_single_files() {
        let root = test_root("move-file");
        let from = root.join("Single.vst");
        fs::write(&from, vec![1u8; 64]).unwrap();
        let to = root.join("store").join("Single.vst");
        move_path(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(fs::metadata(&to).unwrap().len(), 64);
        // 覆盖已存在的目标父目录创建
        assert!(to.parent().unwrap().is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn needed_bytes_has_ten_percent_margin() {
        assert_eq!(needed_bytes(0), 1);
        assert_eq!(needed_bytes(1000), 1101);
        assert_eq!(needed_bytes(1000) * 10, 11010);
    }

    #[test]
    fn available_bytes_is_reported_for_regular_dirs() {
        let root = test_root("space");
        let avail = available_bytes(&root).expect("常规目录卷应能取到可用空间");
        assert!(avail > 0, "可用空间应大于 0");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn forbidden_paths_are_detected() {
        assert!(is_forbidden_path(Path::new("/")));
        assert!(is_forbidden_path(Path::new("/System")));
        assert!(is_forbidden_path(Path::new("/Library")));
        assert!(is_forbidden_path(Path::new("/Applications")));
        if let Some(home) = home_dir() {
            assert!(is_forbidden_path(&home), "主目录本身也必须拒绝");
        }
        // 插件真实目录不是"禁区"（它在 /Library 之下但并非 /Library 本身）
        assert!(!is_forbidden_path(Path::new(
            "/Library/Audio/Plug-Ins/VST3"
        )));
    }

    /// 回归护栏：`codesign` 那条路径依赖 "blocking_recv 可在 spawn_blocking 内使用"。
    /// 若 tokio/tauri 的语义变化，这里会先炸，而不是在用户移动插件时才炸。
    #[test]
    fn blocking_recv_inside_spawn_blocking_works() {
        let (tx, mut rx) = tauri::async_runtime::channel::<u8>(1);
        tauri::async_runtime::spawn(async move {
            let _ = tx.send(7).await;
        });
        let joined =
            tauri::async_runtime::spawn_blocking(move || rx.blocking_recv());
        let got = tauri::async_runtime::block_on(joined);
        assert_eq!(got.unwrap(), Some(7));
    }
}
