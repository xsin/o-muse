// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// Kontakt 库管理核心：合并四个注册来源盘点、磁盘扫描、注册/注销与 `.nicnt` 生成。
//
// 权限边界：
//   - 免提权路径（installed_products JSON + 用户级 plist）依旧是默认行为，
//     `add_library` / `remove_library` **不提权**、不用 `std::process::Command`；
//   - `/Library` 下的两个来源默认只读，读取不需要提权；
//   - 「补全系统级注册」需要写 `/Library`，**只**经 `privileged` 模块的一次性助手完成：
//     由 `osascript … with administrator privileges` 取得用户显式授权，
//     且必须先用 `kontakt_privilege_plan` 把「具体文件 + 原因」告知用户。
//     授权前不会有任何写入；`osascript` 之外不启动其他外部进程
//     （外部进程一律经 `tauri_plugin_shell`）。

pub mod diagnose;
pub mod model;
pub mod nicnt;
pub mod privileged;
pub mod registry;
pub mod scan;

pub use model::{
    Issue, IssueCode, KontaktLibrary, LibrarySources, NicntFormSpec, ProductMetadata,
    ServiceCenterRegistration, Severity, SystemRegistration, UserRegistration,
};
pub use nicnt::{
    build_container, find_metadata_file, parse_metadata_file, parse_nicnt_bytes, write_nicnt,
};
pub use privileged::{
    helper_entry, run_privileged, PrivilegedOp, PrivilegedRequest, CANCELLED_MARKER,
};
pub use scan::{directory_size, scan_libraries};

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// 合并四个注册来源并逐条诊断。
///
/// 合并键为 `RegKey`（回退 Name / SNPID）的小写形式。被 XML 判定为
/// 「非 Content 类型」的键（插件、工具）会被剔除，避免把 NKS 插件预设目录
/// 误当成库；只有磁盘上确实存在 `.nicnt` / `*_info.nkx` 的 JSON 条目例外。
pub fn inventory() -> Result<Vec<KontaktLibrary>, String> {
    let mut merged: HashMap<String, KontaktLibrary> = HashMap::new();
    let mut non_content: HashSet<String> = HashSet::new();
    let mut readable_sources = 0usize;

    // 1) 系统 Service Center XML（只读，root:admin）
    if let Ok(entries) = std::fs::read_dir(registry::service_center_dir()) {
        readable_sources += 1;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("xml") {
                continue;
            }
            let Some(registration) = registry::read_service_center_xml_at(&path) else {
                continue;
            };
            let metadata = registration.metadata;
            let key = merge_key(&metadata.reg_key, &metadata.name, &metadata.snpid);
            if key.is_empty() {
                continue;
            }
            if !metadata.is_content_library() {
                // XML 的类型字段是权威信息：Plugin / Utility 不是库。
                non_content.insert(key);
                continue;
            }
            let library = merged.entry(key).or_insert_with(|| {
                KontaktLibrary::new(metadata.name.clone(), metadata.reg_key.clone())
            });
            fill_from_metadata(library, &metadata);
            library.sources.has_service_center = true;
        }
    }

    // 2) 系统 plist（只读，root:wheel）
    if let Ok(entries) = std::fs::read_dir(registry::system_preferences_dir()) {
        readable_sources += 1;
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if !is_native_instruments_plist(&file_name) {
                continue;
            }
            let Some(registration) = registry::read_system_plist_at(&entry.path()) else {
                // 形状不同（例如只写 ContentDir/Visibility 而不带 RegKey/SNPID）：
                // 文件存在本身就代表系统级注册存在，按文件名补齐来源标记即可。
                mark_source_by_file_name(&mut merged, &non_content, &file_name, |library| {
                    library.sources.has_system_plist = true;
                });
                continue;
            };
            let key = merge_key(
                &registration.reg_key,
                &registration.name,
                &registration.snpid,
            );
            if key.is_empty() || non_content.contains(&key) {
                continue;
            }
            let library = merged.entry(key).or_insert_with(|| {
                KontaktLibrary::new(registration.name.clone(), registration.reg_key.clone())
            });
            if library.name.is_empty() {
                library.name = registration.name.clone();
            }
            if library.reg_key.is_empty() {
                library.reg_key = registration.reg_key.clone();
            }
            if library.snpid.is_empty() {
                library.snpid = registration.snpid.clone();
            }
            if library.content_path.is_none() {
                library.content_path = registration.content_dir.clone();
            }
            if library.visibility.is_none() {
                library.visibility = registration.visibility;
            }
            library.sources.has_system_plist = true;
        }
    }

    // 3) 用户级 plist（可写，用户所有）
    if let Some(dir) = registry::user_preferences_dir() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            readable_sources += 1;
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if !is_native_instruments_plist(&file_name) {
                    continue;
                }
                let Some(registration) = registry::read_user_plist_at(&entry.path()) else {
                    mark_source_by_file_name(&mut merged, &non_content, &file_name, |library| {
                        library.sources.has_user_plist = true;
                    });
                    continue;
                };
                let key = merge_key(
                    &registration.reg_key,
                    &registration.name,
                    &registration.snpid,
                );
                if key.is_empty() || non_content.contains(&key) {
                    continue;
                }
                let library = merged.entry(key).or_insert_with(|| {
                    KontaktLibrary::new(registration.name.clone(), registration.reg_key.clone())
                });
                if library.name.is_empty() {
                    library.name = registration.name.clone();
                }
                if library.reg_key.is_empty() {
                    library.reg_key = registration.reg_key.clone();
                }
                if library.snpid.is_empty() {
                    library.snpid = registration.snpid.clone();
                }
                if library.user_list_index.is_none() {
                    library.user_list_index = registration.user_list_index;
                }
                library.sources.has_user_plist = true;
            }
        }
    }

    // 4) installed_products JSON（可写，777）
    if let Ok(entries) = std::fs::read_dir(registry::installed_products_dir()) {
        readable_sources += 1;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(content_dir) = registry::read_installed_product_json(&path) else {
                continue;
            };
            let name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if name.is_empty() {
                continue;
            }
            let key = merge_key(&name, &name, "");
            if non_content.contains(&key) {
                continue;
            }
            match merged.get_mut(&key) {
                Some(library) => {
                    // installed_products 的记录由安装器写入，通常是可验证的 POSIX 路径；
                    // 仅当现有路径不存在时才用它替换（例如系统 plist 里记的是已失效的旧位置）。
                    let current_missing = library
                        .content_path
                        .as_deref()
                        .map(|path| !path.exists())
                        .unwrap_or(true);
                    if current_missing {
                        library.content_path = Some(content_dir);
                    }
                    library.sources.has_installed_product_json = true;
                }
                None => {
                    // 没有其他来源佐证时，只有内容目录确实含元数据文件才认定为库。
                    if nicnt::find_metadata_file(&content_dir).is_some() {
                        let mut library = KontaktLibrary::new(name.clone(), name);
                        library.content_path = Some(content_dir);
                        library.sources.has_installed_product_json = true;
                        merged.insert(key, library);
                    }
                }
            }
        }
    }

    if readable_sources == 0 {
        return Err(
            "未能读取任何 Kontakt 注册来源（检查 /Library 与用户偏好目录的访问权限）".to_string(),
        );
    }

    let mut libraries: Vec<KontaktLibrary> = merged.into_values().collect();
    diagnose::diagnose(&mut libraries);
    libraries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(libraries)
}

/// 注册一个库：写 installed_products JSON 与用户级 plist。
///
/// 系统级来源（Service Center XML 与系统 plist）位于 `/Library`，
/// 本应用不提权，因此不会被写入——缺少的部分会体现在返回条目的 `issues` 中。
/// 写前若 JSON 非法则整体拒绝；任何一步失败都会回滚已写入的文件。
pub fn add_library(nicnt_path: &Path) -> Result<KontaktLibrary, String> {
    let payload = parse_metadata_file(nicnt_path)?;
    let metadata = payload.metadata;

    let dir = nicnt_path
        .parent()
        .ok_or_else(|| format!("无法确定库目录: {}", nicnt_path.display()))?;
    if !dir.is_dir() {
        return Err(format!("库目录不存在: {}", dir.display()));
    }
    registry::ensure_no_symlink(nicnt_path)?;

    let existing_index =
        registry::read_user_plist(&metadata.reg_key).and_then(|r| r.user_list_index);
    let user_list_index = existing_index.unwrap_or_else(registry::next_user_list_index);

    registry::write_registrations(
        &metadata.name,
        &metadata.reg_key,
        &metadata.snpid,
        dir,
        user_list_index,
    )?;

    let mut library = KontaktLibrary::new(metadata.name.clone(), metadata.reg_key.clone());
    library.snpid = metadata.snpid.clone();
    library.visibility = metadata.visibility;
    library.content_path = Some(dir.to_path_buf());
    library.user_list_index = Some(user_list_index);
    library.content_size_bytes = directory_size(dir);
    library.sources.has_installed_product_json = true;
    library.sources.has_user_plist = true;
    library.sources.has_service_center =
        registry::read_service_center_xml(&metadata.reg_key).is_some();
    library.sources.has_system_plist = registry::system_plist_path(&metadata.reg_key).exists()
        || registry::read_system_plist(&metadata.reg_key).is_some();

    let derived = diagnose::issues_for(&library);
    for issue in derived {
        library.push_issue(issue);
    }
    Ok(library)
}

/// 移除注册（installed_products JSON + 用户级 plist）。
///
/// **只删注册文件，绝不删除磁盘内容。**
pub fn remove_library(reg_key: &str, name: &str) -> Result<(), String> {
    registry::remove_registration(reg_key, name)
}

/// 返回 `.nicnt` 创建表单的默认模板。
pub fn create_nicnt_form_spec() -> NicntFormSpec {
    NicntFormSpec::default()
}

/// 合并键：RegKey 优先，回退 Name、SNPID，统一小写。
fn merge_key(reg_key: &str, name: &str, snpid: &str) -> String {
    let raw = if !reg_key.trim().is_empty() {
        reg_key
    } else if !name.trim().is_empty() {
        name
    } else {
        snpid
    };
    raw.trim().to_lowercase()
}

/// 用 XML 元数据补齐空字段（已有值不覆盖）。
fn fill_from_metadata(library: &mut KontaktLibrary, metadata: &ProductMetadata) {
    if library.name.is_empty() {
        library.name = metadata.name.clone();
    }
    if library.reg_key.is_empty() {
        library.reg_key = metadata.reg_key.clone();
    }
    if library.snpid.is_empty() {
        library.snpid = metadata.snpid.clone();
    }
    if library.visibility.is_none() {
        library.visibility = metadata.visibility;
    }
}

/// 是否为 NI 注册 plist 文件名。
fn is_native_instruments_plist(file_name: &str) -> bool {
    file_name.starts_with("com.native-instruments.") && file_name.ends_with(".plist")
}

/// 从 `com.native-instruments.<RegKey>.plist` 的文件名取出合并键。
fn reg_key_from_plist_file(file_name: &str) -> Option<String> {
    let stem = file_name
        .strip_prefix("com.native-instruments.")?
        .strip_suffix(".plist")?;
    if stem.is_empty() {
        return None;
    }
    Some(stem.to_lowercase())
}

/// 按文件名给已存在的条目补来源标记（plist 形状不标准时使用）。
fn mark_source_by_file_name(
    merged: &mut HashMap<String, KontaktLibrary>,
    non_content: &HashSet<String>,
    file_name: &str,
    mark: impl Fn(&mut KontaktLibrary),
) {
    let Some(key) = reg_key_from_plist_file(file_name) else {
        return;
    };
    if non_content.contains(&key) {
        return;
    }
    if let Some(library) = merged.get_mut(&key) {
        mark(library);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_不panic且字段完整() {
        match inventory() {
            Ok(libraries) => {
                for library in libraries {
                    assert!(!library.name.is_empty(), "库名不应为空");
                    assert!(!library.reg_key.is_empty(), "RegKey 不应为空");
                }
            }
            Err(message) => assert!(!message.is_empty(), "错误信息不应为空"),
        }
    }

    #[test]
    fn 表单默认值可直接用于创建() {
        let spec = create_nicnt_form_spec();
        assert_eq!(spec.product_type, "Content");
        assert_eq!(spec.visibility, 3);
        assert!(spec.upid.is_none());
        assert!(spec.hu.is_none() && spec.jdx.is_none());
    }

    #[test]
    fn 合并键回退顺序正确() {
        assert_eq!(merge_key("RegKey", "Name", "SNP"), "regkey");
        assert_eq!(merge_key("  ", "Name", "SNP"), "name");
        assert_eq!(merge_key("", "", "SNP"), "snp");
        assert_eq!(merge_key("", "", ""), "");
    }
}
