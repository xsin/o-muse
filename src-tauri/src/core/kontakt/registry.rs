// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// 四个注册来源的读写。
//
// | 来源 | 路径 | 可写性 |
// |---|---|---|
// | 系统 Service Center XML | `/Library/Application Support/Native Instruments/Service Center/<Name>.xml` | 只读（root:admin） |
// | 系统 plist | `/Library/Preferences/com.native-instruments.<RegKey>.plist` | 只读（root:wheel） |
// | installed_products JSON | `/Users/Shared/Native Instruments/installed_products/<Name>.json` | 可写（777） |
// | 用户级 plist | `~/Library/Preferences/com.native-instruments.<RegKey>.plist` | 可写（用户所有） |
//
// 三条铁律：
//  1. 本模块自身只以当前用户权限读写：`/Library` 下的来源在此**只读**。
//     需要写入 `/Library` 时由 `privileged` 模块在用户明确授权后、以一次性助手完成
//     （见 docs/adr/0005-privileged-operations.md）。
//  2. 只动注册文件：绝不删除或移动磁盘上的库内容。
//  3. 写前备份、失败回滚、原子写、逐级拒绝符号链接。

use crate::core::kontakt::model::{
    ServiceCenterRegistration, SystemRegistration, UserRegistration,
};
use plist::{Dictionary, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

// ── 路径 ──────────────────────────────────────────────────────

/// 系统 Service Center XML 目录（只读）。
pub fn service_center_dir() -> PathBuf {
    PathBuf::from("/Library/Application Support/Native Instruments/Service Center")
}

/// 系统偏好目录（只读）。
pub fn system_preferences_dir() -> PathBuf {
    PathBuf::from("/Library/Preferences")
}

/// installed_products JSON 目录（可写）。
pub fn installed_products_dir() -> PathBuf {
    PathBuf::from("/Users/Shared/Native Instruments/installed_products")
}

/// 用户偏好目录（可写）。
pub fn user_preferences_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join("Library/Preferences"))
}

/// 注册用 plist 文件名。
pub fn plist_file_name(reg_key: &str) -> String {
    format!("com.native-instruments.{reg_key}.plist")
}

pub fn user_plist_path(reg_key: &str) -> Option<PathBuf> {
    user_preferences_dir().map(|dir| dir.join(plist_file_name(reg_key)))
}

pub fn system_plist_path(reg_key: &str) -> PathBuf {
    system_preferences_dir().join(plist_file_name(reg_key))
}

pub fn service_center_xml_path(reg_key: &str) -> PathBuf {
    service_center_dir().join(format!("{reg_key}.xml"))
}

pub fn installed_product_json_path(name: &str) -> PathBuf {
    installed_products_dir().join(format!("{name}.json"))
}

// ── 读 ────────────────────────────────────────────────────────

/// 读取 plist 字典；文件不存在、非字典或不可读时返回 `None`。
fn read_plist_dict(path: &Path) -> Option<Dictionary> {
    let data = fs::read(path).ok()?;
    match Value::from_reader(std::io::Cursor::new(data)).ok()? {
        Value::Dictionary(dict) => Some(dict),
        _ => None,
    }
}

fn dict_string(dict: &Dictionary, key: &str) -> Option<String> {
    dict.get(key).and_then(Value::as_string).map(str::to_string)
}

fn dict_int(dict: &Dictionary, key: &str) -> Option<i64> {
    dict.get(key).and_then(|v| {
        v.as_signed_integer()
            .or_else(|| v.as_unsigned_integer().map(|u| u as i64))
            .or_else(|| v.as_boolean().map(i64::from))
    })
}

/// 把 NI 记录的 HFS 风格路径转换为 POSIX 路径。
///
/// 实测：系统 plist 中位于外接卷上的库会写成
/// `Uranus:XKontakt:Standard Libs:Alex Pfeffer - SOL:`。
/// 这种字符串直接当 POSIX 路径用无法判断存在性，故按 macOS 的 HFS↔POSIX 规则转换：
/// 去掉结尾冒号、冒号换成斜杠、补上 `/Volumes/` 前缀。
/// 已经是 POSIX（以 `/` 开头）或不含冒号的字符串原样返回。
fn normalize_content_dir(raw: &str) -> PathBuf {
    let text = raw.trim();
    if text.starts_with('/') || !text.contains(':') {
        return PathBuf::from(text);
    }
    let trimmed = text.trim_end_matches(':');
    PathBuf::from(format!("/Volumes/{}", trimmed.replace(':', "/")))
}

/// 读取用户级 plist。
///
/// 判别特征：同时存在 `RegKey` 与 `SNPID`。NI 生态里插件类 plist 只有
/// `Name`/`RegKey`/`UserListIndex`，据此把库注册与插件注册区分开。
pub fn read_user_plist(reg_key: &str) -> Option<UserRegistration> {
    read_user_plist_at(&user_plist_path(reg_key)?)
}

/// 读取指定路径的用户级 plist（供盘点阶段批量遍历）。
pub(super) fn read_user_plist_at(path: &Path) -> Option<UserRegistration> {
    let dict = read_plist_dict(path)?;
    let reg_key = dict_string(&dict, "RegKey")?;
    let snpid = dict_string(&dict, "SNPID")?;
    Some(UserRegistration {
        snpid,
        name: dict_string(&dict, "Name").unwrap_or_else(|| reg_key.clone()),
        reg_key,
        user_list_index: dict_int(&dict, "UserListIndex").map(|v| v as i32),
        user_removed: dict_int(&dict, "UserRemoved").unwrap_or(0) != 0,
    })
}

/// 读取系统 plist（只读来源）。
pub fn read_system_plist(reg_key: &str) -> Option<SystemRegistration> {
    read_system_plist_at(&system_plist_path(reg_key))
}

/// 读取指定路径的系统 plist（供盘点阶段批量遍历）。
pub(super) fn read_system_plist_at(path: &Path) -> Option<SystemRegistration> {
    let dict = read_plist_dict(path)?;
    let reg_key = dict_string(&dict, "RegKey")?;
    let snpid = dict_string(&dict, "SNPID")?;
    Some(SystemRegistration {
        name: dict_string(&dict, "Name").unwrap_or_else(|| reg_key.clone()),
        reg_key,
        snpid,
        content_dir: dict_string(&dict, "ContentDir").map(|s| normalize_content_dir(&s)),
        visibility: dict_int(&dict, "Visibility").map(|v| v as i32),
        hu: dict_string(&dict, "HU"),
        jdx: dict_string(&dict, "JDX"),
        upid: dict_string(&dict, "UPID"),
        auth_system: dict_string(&dict, "AuthSystem"),
    })
}

/// 读取 Service Center XML（只读来源）。
///
/// 文件名通常等于 `Name`；先按 `<RegKey>.xml` 直读，未命中再扫描目录按 `RegKey` 匹配。
pub fn read_service_center_xml(reg_key: &str) -> Option<ServiceCenterRegistration> {
    let direct = service_center_xml_path(reg_key);
    if let Some(reg) = read_service_center_xml_at(&direct) {
        return Some(reg);
    }
    for entry in fs::read_dir(service_center_dir()).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("xml") {
            continue;
        }
        if let Some(reg) = read_service_center_xml_at(&path) {
            if reg.metadata.reg_key.eq_ignore_ascii_case(reg_key) {
                return Some(reg);
            }
        }
    }
    None
}

pub(super) fn read_service_center_xml_at(path: &Path) -> Option<ServiceCenterRegistration> {
    let raw_xml = String::from_utf8(fs::read(path).ok()?).ok()?;
    let metadata = super::nicnt::parse_product_hints(&raw_xml).ok()?;
    if metadata.name.is_empty() || metadata.reg_key.is_empty() {
        return None;
    }
    Some(ServiceCenterRegistration { metadata, raw_xml })
}

/// 读取 installed_products JSON 的 `ContentDir`（键名大小写不敏感，容忍 BOM）。
///
/// 仅返回内容目录；文件缺失、非法 JSON 或不含 `ContentDir` 时返回 `None`。
pub fn read_installed_product_json(path: &Path) -> Option<PathBuf> {
    let bytes = fs::read(path).ok()?;
    let body = bytes
        .strip_prefix(&[0xEF, 0xBB, 0xBF][..])
        .unwrap_or(&bytes);
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let object = value.as_object()?;
    for (key, value) in object {
        if key.eq_ignore_ascii_case("ContentDir") {
            if let Some(text) = value.as_str() {
                return Some(normalize_content_dir(text));
            }
        }
    }
    None
}

/// 扫描用户偏好目录，返回下一个可用的 `UserListIndex`。
pub fn next_user_list_index() -> i32 {
    let mut max = -1i32;
    let Some(dir) = user_preferences_dir() else {
        return 0;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("com.native-instruments.") || !name.ends_with(".plist") {
            continue;
        }
        if let Some(reg) = read_user_plist_at(&entry.path()) {
            if let Some(index) = reg.user_list_index {
                max = max.max(index);
            }
        }
    }
    max.saturating_add(1)
}

// ── 写 ────────────────────────────────────────────────────────

/// 写入 installed_products JSON。
///
/// 文件已存在时做**字节级补丁**：只替换 `ContentDir` 的值（并把键名规范化为
/// `ContentDir`），BOM、键序、其余字段与尾随换行全部保留；文件内容不是合法 JSON
/// 时拒绝操作且不触碰原文件。
pub fn write_installed_product_json(name: &str, content_dir: &Path) -> Result<(), String> {
    super::nicnt::validate_component(name, "Name")?;
    write_installed_product_json_at(&installed_product_json_path(name), content_dir)
}

fn write_installed_product_json_at(path: &Path, content_dir: &Path) -> Result<(), String> {
    ensure_parent_writable(path)?;
    ensure_no_symlink(path)?;

    let dir_text = content_dir.to_string_lossy().to_string();
    let original = snapshot(path);
    let bytes = match original.as_deref() {
        Some(existing) => patch_content_dir(existing, &dir_text)?,
        None => new_content_dir_json(&dir_text)?,
    };

    if let Err(e) = atomic_write(path, &bytes) {
        restore(path, original.as_deref());
        return Err(e);
    }
    Ok(())
}

/// 新建 installed_products JSON 的内容（与真实样本一致：无 BOM、无尾随换行）。
fn new_content_dir_json(content_dir: &str) -> Result<Vec<u8>, String> {
    let quoted = serde_json::to_string(content_dir).map_err(|e| format!("序列化路径失败: {e}"))?;
    Ok(format!("{{\"ContentDir\":{quoted}}}").into_bytes())
}

/// 写入用户级 plist（XML plist 格式）。
pub fn write_user_plist(
    reg_key: &str,
    snpid: &str,
    name: &str,
    user_list_index: i32,
) -> Result<(), String> {
    super::nicnt::validate_component(reg_key, "RegKey")?;
    super::nicnt::validate_component(name, "Name")?;
    let path = user_plist_path(reg_key)
        .ok_or_else(|| "无法确定用户偏好目录（HOME 未设置）".to_string())?;
    write_user_plist_at(&path, reg_key, snpid, name, user_list_index)
}

fn write_user_plist_at(
    path: &Path,
    reg_key: &str,
    snpid: &str,
    name: &str,
    user_list_index: i32,
) -> Result<(), String> {
    ensure_parent_writable(path)?;
    ensure_no_symlink(path)?;

    let mut dict = Dictionary::new();
    dict.insert("SNPID".to_string(), Value::String(snpid.to_string()));
    dict.insert("Name".to_string(), Value::String(name.to_string()));
    dict.insert(
        "UserListIndex".to_string(),
        Value::Integer(user_list_index.into()),
    );
    dict.insert("RegKey".to_string(), Value::String(reg_key.to_string()));
    dict.insert("UserRemoved".to_string(), Value::Integer(0.into()));

    let mut buf = Vec::new();
    plist::to_writer_xml(&mut buf, &Value::Dictionary(dict))
        .map_err(|e| format!("序列化用户 plist 失败: {e}"))?;

    let original = snapshot(path);
    if let Err(e) = atomic_write(path, &buf) {
        restore(path, original.as_deref());
        return Err(e);
    }
    Ok(())
}

/// 一次性写入一套注册（installed_products JSON + 用户级 plist）。
///
/// 两个文件都先备份到内存，任一步失败即整体回滚。
/// 写入前先试算 JSON 补丁，非法 JSON 会在动任何文件之前被拒绝。
pub fn write_registrations(
    name: &str,
    reg_key: &str,
    snpid: &str,
    content_dir: &Path,
    user_list_index: i32,
) -> Result<(), String> {
    super::nicnt::validate_component(name, "Name")?;
    super::nicnt::validate_component(reg_key, "RegKey")?;

    let json_path = installed_product_json_path(name);
    let plist_path = user_plist_path(reg_key)
        .ok_or_else(|| "无法确定用户偏好目录（HOME 未设置）".to_string())?;

    let dir_text = content_dir.to_string_lossy().to_string();
    let json_backup = snapshot(&json_path);
    let plist_backup = snapshot(&plist_path);

    // 预检：确认既有 JSON 合法（补丁试算），避免写完 plist 才发现问题。
    if let Some(existing) = json_backup.as_deref() {
        patch_content_dir(existing, &dir_text)?;
    }

    if let Err(err) = write_installed_product_json_at(&json_path, content_dir) {
        return Err(err);
    }
    if let Err(err) = write_user_plist_at(&plist_path, reg_key, snpid, name, user_list_index) {
        restore(&json_path, json_backup.as_deref());
        restore(&plist_path, plist_backup.as_deref());
        return Err(err);
    }
    Ok(())
}

/// 移除注册：删除 installed_products JSON 与用户级 plist。
///
/// **只删注册文件，绝不删除磁盘内容。** 任一步失败则回滚已删除的文件。
pub fn remove_registration(reg_key: &str, name: &str) -> Result<(), String> {
    super::nicnt::validate_component(name, "Name")?;
    super::nicnt::validate_component(reg_key, "RegKey")?;

    let json_path = installed_product_json_path(name);
    let plist_path = user_plist_path(reg_key);
    remove_registration_at(&json_path, plist_path.as_deref())
}

/// 按具体路径执行删除（便于单测覆盖回滚与「不动内容」语义）。
fn remove_registration_at(json_path: &Path, plist_path: Option<&Path>) -> Result<(), String> {
    let json_backup = snapshot(json_path);
    let plist_backup = plist_path.and_then(snapshot);

    if json_backup.is_some() {
        ensure_no_symlink(json_path)?;
        if let Err(e) = fs::remove_file(json_path) {
            return Err(format!("删除注册文件 {} 失败: {e}", json_path.display()));
        }
    }

    if let (Some(path), Some(_)) = (plist_path, plist_backup.as_deref()) {
        let removed = ensure_no_symlink(path).and_then(|()| {
            fs::remove_file(path).map_err(|e| format!("删除注册文件 {} 失败: {e}", path.display()))
        });
        if let Err(e) = removed {
            // 回滚已删除的 JSON
            restore(json_path, json_backup.as_deref());
            return Err(e);
        }
    }

    Ok(())
}

// ── 通用工具 ──────────────────────────────────────────────────

/// 逐级检查符号链接：路径中任一已存在的组件是符号链接即拒绝操作。
pub fn ensure_no_symlink(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if let Ok(meta) = fs::symlink_metadata(&current) {
            if meta.file_type().is_symlink() {
                return Err(format!("路径含符号链接，拒绝操作: {}", current.display()));
            }
        }
    }
    Ok(())
}

/// 必要时创建父目录（创建前先做符号链接检查）。
fn ensure_parent_writable(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    ensure_no_symlink(parent)?;
    if !parent.exists() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录 {} 失败: {e}", parent.display()))?;
    }
    Ok(())
}

/// 备份文件内容；文件不存在返回 `None`。
fn snapshot(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

/// 回滚（尽力而为）：原本存在则写回内容，原本不存在则删除。
fn restore(path: &Path, original: Option<&[u8]>) {
    match original {
        Some(bytes) => {
            let _ = fs::write(path, bytes);
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

/// 原子写：先写同目录临时文件，再 rename 覆盖目标。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = temp_sibling(path);
    if let Err(e) = write_file(&tmp, bytes) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("原子替换 {} 失败: {e}", path.display()));
    }
    Ok(())
}

/// 原子创建：目标已存在则报错，绝不覆盖。
pub fn atomic_write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    // 先以 create_new 占位，防止并发覆盖；随后 rename 原子替换占位文件。
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!("目标已存在，拒绝覆盖: {}", path.display()));
        }
        Err(e) => return Err(format!("创建 {} 失败: {e}", path.display())),
    }

    let tmp = temp_sibling(path);
    if let Err(e) = write_file(&tmp, bytes) {
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(path);
        return Err(e);
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(path);
        return Err(format!("原子替换 {} 失败: {e}", path.display()));
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file =
        fs::File::create(path).map_err(|e| format!("创建临时文件 {} 失败: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("写入 {} 失败: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("落盘 {} 失败: {e}", path.display()))?;
    Ok(())
}

/// 同目录临时文件名：必须与目标同一文件系统，rename 才是原子操作。
fn temp_sibling(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    parent.join(format!(".{name}.omuse-{}-{nanos}.tmp", std::process::id()))
}

// ── 系统级写入（提权助手专用）────────────────────────────────
//
// 以下函数会写入 `/Library`（root:admin / root:wheel），**只允许在已经提权的
// 助手进程中调用**（见 `core::kontakt::privileged`）。路径白名单校验由
// privileged 模块负责；这里只做防御性的符号链接检查、原子写入与权限设置。
//
// 既有函数的行为不因本节改动：`write_registrations` / `remove_registration`
// 依旧只碰用户可写来源。

/// 以 root 身份原子写入系统级注册文件，落盘权限 0644；父目录不存在时创建（0755）。
pub fn write_system_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法确定父目录: {}", path.display()))?;
    // 逐级检查符号链接：父目录与目标本身都不允许是链接。
    ensure_no_symlink(parent)?;
    ensure_no_symlink(path)?;

    if !parent.exists() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建系统目录 {} 失败: {e}", parent.display()))?;
        set_permissions(parent, 0o755)?;
    }

    atomic_write(path, bytes)?;
    // `atomic_write` 经 `File::create` 建临时文件，权限受 umask 影响；显式设为 0644。
    set_permissions(path, 0o644)
}

/// 以 root 身份删除系统级注册文件（文件本就不存在时视为成功）。
pub fn remove_system_file(path: &Path) -> Result<(), String> {
    ensure_no_symlink(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("删除 {} 失败: {e}", path.display())),
    }
}

/// 设置权限位（Unix）；同时用于创建目录/文件后的 umask 兜底。
pub fn set_permissions(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(mode);
    fs::set_permissions(path, permissions)
        .map_err(|e| format!("设置 {} 权限为 0o{mode:o} 失败: {e}", path.display()))
}

/// 生成系统级 plist（`/Library/Preferences/com.native-instruments.<RegKey>.plist`）的字节内容。
///
/// 字段名与 `read_system_plist_at` 的读取约定一一对应，保证写出去的文件能被自己读回。
/// `ContentDir` 采用可逆的 NI 路径写法：外接卷写成 `<卷名>:目录:…`（与
/// `normalize_content_dir` 的 HFS→POSIX 规则互逆），其余卷写 POSIX 绝对路径。
pub fn system_plist_bytes(registration: &SystemRegistration) -> Result<Vec<u8>, String> {
    super::nicnt::validate_component(&registration.reg_key, "RegKey")?;
    super::nicnt::validate_component(&registration.name, "Name")?;
    if registration.snpid.trim().is_empty() {
        return Err("SNPID 为空，拒绝生成系统 plist".to_string());
    }

    let mut dict = Dictionary::new();
    dict.insert(
        "RegKey".to_string(),
        Value::String(registration.reg_key.clone()),
    );
    dict.insert("SNPID".to_string(), Value::String(registration.snpid.clone()));
    dict.insert("Name".to_string(), Value::String(registration.name.clone()));
    if let Some(dir) = registration.content_dir.as_deref() {
        dict.insert(
            "ContentDir".to_string(),
            Value::String(ni_content_dir_string(dir)),
        );
    }
    dict.insert(
        "Visibility".to_string(),
        Value::Integer(registration.visibility.unwrap_or(3).into()),
    );
    for (key, value) in [
        ("HU", registration.hu.as_deref()),
        ("JDX", registration.jdx.as_deref()),
        ("UPID", registration.upid.as_deref()),
        ("AuthSystem", registration.auth_system.as_deref()),
    ] {
        if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
            dict.insert(key.to_string(), Value::String(value.trim().to_string()));
        }
    }

    let mut buf = Vec::new();
    plist::to_writer_xml(&mut buf, &Value::Dictionary(dict))
        .map_err(|e| format!("序列化系统 plist 失败: {e}"))?;
    Ok(buf)
}

/// POSIX 内容路径 → NI 记录的路径写法（`normalize_content_dir` 的逆运算）。
///
/// `/Volumes/<卷名>/a/b` → `<卷名>:a:b`（读回时再补回 `/Volumes/`）。
/// 非外接卷路径原样返回 POSIX 形式——本应用无法可靠得知启动卷的卷名，
/// 因此不臆造 `<启动卷>:…` 形式的 HFS 路径。
fn ni_content_dir_string(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("/Volumes/") {
        if let Some((volume, tail)) = rest.split_once('/') {
            if !volume.is_empty() && !tail.is_empty() {
                return format!("{volume}:{}", tail.replace('/', ":"));
            }
        }
    }
    text.to_string()
}

// ── JSON 字节级补丁 ───────────────────────────────────────────

/// JSON 对象成员的位置信息（偏移相对传入的字节切片）。
struct JsonMember<'a> {
    raw_key: &'a [u8],
    key_start: usize,
    key_end: usize,
    value_start: usize,
    value_end: usize,
}

/// 返回字符串字面量结束后的下标（已跳过闭合引号），处理反斜杠转义。
fn json_string_end(bytes: &[u8], start: usize) -> Result<usize, String> {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err("JSON 字符串未闭合".to_string())
}

/// 扫描所有 `"键": "字符串值"` 成员（浅层扫描，足以覆盖 installed_products 结构）。
fn scan_json_members(bytes: &[u8]) -> Result<Vec<JsonMember<'_>>, String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let key_end = json_string_end(bytes, i)?;
        let mut after = key_end;
        while after < bytes.len() && bytes[after].is_ascii_whitespace() {
            after += 1;
        }
        if after < bytes.len() && bytes[after] == b':' {
            let mut value_start = after + 1;
            while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
                value_start += 1;
            }
            if value_start < bytes.len() && bytes[value_start] == b'"' {
                let value_end = json_string_end(bytes, value_start)?;
                out.push(JsonMember {
                    raw_key: &bytes[i..key_end],
                    key_start: i,
                    key_end,
                    value_start,
                    value_end,
                });
                i = value_end;
                continue;
            }
        }
        i = key_end;
    }
    Ok(out)
}

/// 只改 `ContentDir` 值的字节级补丁。
fn patch_content_dir(existing: &[u8], new_dir: &str) -> Result<Vec<u8>, String> {
    let bom_len = if existing.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    let body = &existing[bom_len..];
    let parsed: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("已存在的 JSON 非法，拒绝改写: {e}"))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| "已存在的 JSON 顶层不是对象，拒绝改写".to_string())?;

    let quoted = serde_json::to_string(new_dir).map_err(|e| format!("序列化路径失败: {e}"))?;
    let members = scan_json_members(body)?;
    let hits: Vec<&JsonMember<'_>> = members
        .iter()
        .filter(|m| m.raw_key.eq_ignore_ascii_case(br#""ContentDir""#))
        .collect();
    if hits.len() > 1 {
        return Err("已存在的 JSON 含多个 ContentDir 字段，拒绝改写".to_string());
    }

    let mut out = Vec::with_capacity(existing.len() + quoted.len());
    out.extend_from_slice(&existing[..bom_len]);
    match hits.first() {
        Some(member) => {
            out.extend_from_slice(&body[..member.key_start]);
            out.extend_from_slice(br#""ContentDir""#);
            out.extend_from_slice(&body[member.key_end..member.value_start]);
            out.extend_from_slice(quoted.as_bytes());
            out.extend_from_slice(&body[member.value_end..]);
        }
        None => {
            // 键存在但值不是字符串（例如 null）：直接拒绝，避免写出重复键。
            if object
                .keys()
                .any(|key| key.eq_ignore_ascii_case("ContentDir"))
            {
                return Err("已存在的 JSON 中 ContentDir 不是字符串，拒绝改写".to_string());
            }
            let brace = body
                .iter()
                .position(|b| *b == b'{')
                .ok_or_else(|| "已存在的 JSON 顶层不是对象，拒绝改写".to_string())?;
            out.extend_from_slice(&body[..=brace]);
            out.extend_from_slice(br#""ContentDir":"#);
            out.extend_from_slice(quoted.as_bytes());
            let rest = &body[brace + 1..];
            if rest.iter().any(|b| !b.is_ascii_whitespace() && *b != b'}') {
                out.push(b',');
            }
            out.extend_from_slice(rest);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as Json;

    /// 夹具根目录：规范化后使用，绕开 macOS `/var` → `/private/var` 系统符号链接。
    fn fixture_root(tag: &str) -> PathBuf {
        let base = std::fs::canonicalize(std::env::temp_dir()).expect("临时目录可用");
        let dir = base.join(format!("omuse_kontakt_reg_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn json_字节补丁保留其他字段与bom和尾随换行() {
        let root = fixture_root("patch");
        let path = root.join("Lib.json");
        let original: Vec<u8> = [
            &[0xEF, 0xBB, 0xBF][..],
            br#"{"ContentDir":"/old","Extra":1}"#,
            b"\n",
        ]
        .concat();
        fs::write(&path, &original).unwrap();

        write_installed_product_json_at(&path, Path::new("/new/path")).unwrap();

        let patched = fs::read(&path).unwrap();
        assert!(patched.starts_with(&[0xEF, 0xBB, 0xBF]), "BOM 必须保留");
        assert!(patched.ends_with(b"\n"), "尾随换行必须保留");
        let text = String::from_utf8(patched[3..].to_vec()).unwrap();
        assert!(text.contains("\"Extra\":1"), "其他字段必须保留: {text}");
        assert!(
            text.contains("\"ContentDir\":\"/new/path\""),
            "值应更新: {text}"
        );
        assert!(!text.contains("/old"));

        let parsed: Json = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["Extra"], Json::from(1));
        assert_eq!(parsed["ContentDir"], Json::from("/new/path"));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn json_小写键名被规范化为content_dir() {
        let patched = patch_content_dir(br#"{"contentDir":"/old"}"#, "/new").unwrap();
        assert_eq!(
            String::from_utf8(patched).unwrap(),
            r#"{"ContentDir":"/new"}"#
        );
    }

    #[test]
    fn json_缺少键时插入且不破坏原有成员() {
        let patched = patch_content_dir(br#"{"InstallDir":"/x"}"#, "/lib").unwrap();
        let text = String::from_utf8(patched).unwrap();
        let parsed: Json = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["InstallDir"], Json::from("/x"));
        assert_eq!(parsed["ContentDir"], Json::from("/lib"));
    }

    #[test]
    fn json_非法内容被拒绝且原文件不变() {
        let root = fixture_root("badjson");
        let path = root.join("Broken.json");
        let original = b"{not json at all";
        fs::write(&path, original).unwrap();

        let err = write_installed_product_json_at(&path, Path::new("/whatever")).unwrap_err();
        assert!(err.contains("拒绝改写"), "错误信息应说明拒绝原因: {err}");
        assert_eq!(
            fs::read(&path).unwrap(),
            original.to_vec(),
            "原文件必须保持不变"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 用户_plist_往返读写() {
        let root = fixture_root("plist");
        let path = root.join("com.native-instruments.Altera Keys.plist");

        write_user_plist_at(&path, "Altera Keys", "1c7", "Altera Keys", 0).unwrap();
        let reg = read_user_plist_at(&path).expect("应能读回");
        assert_eq!(reg.snpid, "1c7");
        assert_eq!(reg.name, "Altera Keys");
        assert_eq!(reg.reg_key, "Altera Keys");
        assert_eq!(reg.user_list_index, Some(0));
        assert!(!reg.user_removed);

        // 覆盖写回到同一路径仍是原子替换，内容更新
        write_user_plist_at(&path, "Altera Keys", "1c7", "Altera Keys", 12).unwrap();
        assert_eq!(read_user_plist_at(&path).unwrap().user_list_index, Some(12));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 原子写失败不留残留文件() {
        let root = fixture_root("atomic");
        let target = root.join("missing").join("x.json");

        let err = atomic_write(&target, b"{}").unwrap_err();
        assert!(!err.is_empty());
        assert!(!root.join("missing").exists(), "父目录不应被创建");
        assert!(!target.exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 原子创建拒绝覆盖既有文件() {
        let root = fixture_root("newonly");
        let target = root.join("keep.txt");
        fs::write(&target, b"original").unwrap();

        assert!(atomic_write_new(&target, b"replaced").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original".to_vec());

        let fresh = root.join("fresh.txt");
        atomic_write_new(&fresh, b"hello").unwrap();
        assert_eq!(fs::read(&fresh).unwrap(), b"hello".to_vec());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 符号链接路径被拒绝() {
        let root = fixture_root("symlink");
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert!(ensure_no_symlink(&real.join("file.json")).is_ok());
        let err = ensure_no_symlink(&link.join("file.json")).unwrap_err();
        assert!(err.contains("符号链接"));
        // 写入也应被拒绝
        assert!(write_installed_product_json_at(&link.join("Lib.json"), Path::new("/x")).is_err());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 删除注册不触及磁盘内容() {
        let root = fixture_root("remove");
        let json = root.join("Lib.json");
        let plist = root.join("com.native-instruments.Lib.plist");
        let content = root.join("Lib");
        fs::create_dir_all(&content).unwrap();
        fs::write(content.join("Lib.nicnt"), b"container").unwrap();
        fs::write(&json, br#"{"ContentDir":"/content"}"#).unwrap();
        write_user_plist_at(&plist, "Lib", "S1", "Lib", 3).unwrap();

        remove_registration_at(&json, Some(&plist)).unwrap();

        assert!(!json.exists(), "JSON 注册文件应被删除");
        assert!(!plist.exists(), "用户 plist 应被删除");
        assert!(content.join("Lib.nicnt").exists(), "磁盘内容绝不能被删除");

        // 幂等：再次调用不报错
        remove_registration_at(&json, Some(&plist)).unwrap();

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn hfs_风格路径被转换为_posix() {
        assert_eq!(
            normalize_content_dir("Uranus:XKontakt:Standard Libs:Alex Pfeffer - SOL:"),
            PathBuf::from("/Volumes/Uranus/XKontakt/Standard Libs/Alex Pfeffer - SOL")
        );
        assert_eq!(
            normalize_content_dir("Macintosh HD:Users:lv:Libs:Foo"),
            PathBuf::from("/Volumes/Macintosh HD/Users/lv/Libs/Foo")
        );
        // POSIX 路径与无冒号字符串原样保留
        assert_eq!(
            normalize_content_dir("/Users/lv/Libs/Foo"),
            PathBuf::from("/Users/lv/Libs/Foo")
        );
        assert_eq!(normalize_content_dir("relative"), PathBuf::from("relative"));
    }

    #[test]
    fn 读取_installed_products_json_的content_dir() {
        let root = fixture_root("readjson");
        let path = root.join("Lib.json");
        fs::write(&path, br#"{"ContentDir":"/libs/Altera Keys"}"#).unwrap();
        assert_eq!(
            read_installed_product_json(&path),
            Some(PathBuf::from("/libs/Altera Keys"))
        );

        fs::write(&path, br#"{"InstallDir":"/only/install"}"#).unwrap();
        assert_eq!(read_installed_product_json(&path), None);

        fs::write(&path, b"{broken").unwrap();
        assert_eq!(read_installed_product_json(&path), None);

        fs::remove_dir_all(&root).ok();
    }
}
