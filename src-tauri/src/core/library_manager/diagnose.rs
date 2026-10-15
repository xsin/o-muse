// Kontakt 库盘点诊断（Task 3）

use crate::core::library_manager::model::{Diagnostic, LibraryEntry};
use std::path::Path;

/// 对扫描得到的库条目集合追加诊断：
///  - 路径缺失 / 断卷
///  - SNPID 重复
pub fn diagnose(entries: &mut [LibraryEntry]) {
    // 1) 路径存在性与断卷判断
    for entry in entries.iter_mut() {
        if let Some(path) = &entry.path {
            if !path.exists() {
                if is_disconnected_volume(path) {
                    entry
                        .diagnostics
                        .push(Diagnostic::DisconnectedVolume(path.clone()));
                } else {
                    entry
                        .diagnostics
                        .push(Diagnostic::MissingPath(path.clone()));
                }
            }
        } else {
            entry
                .diagnostics
                .push(Diagnostic::MissingMetadata("path".to_string()));
        }
    }

    // 2) SNPID 重复检测
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for entry in entries.iter() {
        *counts.entry(entry.snpid.clone()).or_insert(0) += 1;
    }
    for entry in entries.iter_mut() {
        if counts.get(&entry.snpid).copied().unwrap_or(0) > 1 {
            entry
                .diagnostics
                .push(Diagnostic::DuplicateSnpid(entry.snpid.clone()));
        }
    }
}

/// 判断路径是否位于 /Volumes 且其所在卷已不可访问（断卷）。
/// 仅做保守判断：路径以 /Volumes 开头且第一段组件不存在即视为断卷。
fn is_disconnected_volume(path: &Path) -> bool {
    let comps: Vec<_> = path.components().collect();
    // /Volumes/<volume>/...
    if comps.len() >= 3 {
        if let Some(std::path::Component::RootDir) = comps.first().copied().into_iter().next() {
            // 简化处理：检查 /Volumes/<volume> 是否可访问
            let vol = path.components().take(3).collect::<std::path::PathBuf>();
            return !vol.exists();
        }
    }
    // 更通用的判断：路径前缀形如 /Volumes/xxx 且前缀不存在
    if let Some(p) = path.to_str() {
        if p.starts_with("/Volumes/") {
            let mut prefix = std::path::PathBuf::new();
            let mut count = 0;
            for c in path.components() {
                prefix.push(c);
                count += 1;
                if count >= 3 {
                    break;
                }
            }
            return !prefix.exists();
        }
    }
    false
}
