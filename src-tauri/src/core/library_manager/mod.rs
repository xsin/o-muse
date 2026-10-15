// Kontakt 库盘点核心（Task 3）
//
// 只读盘点：扫描 Native Instruments Service Center XML、Kontakt 偏好 plist、
// 以及已安装产品 JSON，合并为库条目并产出诊断。不修改任何宿主文件。

pub mod model;
pub mod parser;
pub mod diagnose;

use crate::core::platform_paths::{
    kontakt_preferences_dir, kontakt_plist_files, native_instruments_service_center_dirs,
};
use model::LibraryEntry;
use std::collections::HashSet;
use std::path::PathBuf;

/// 默认扫描源（ADR-0002 的 platform_paths 提供路径来源）。
/// Task 7 的插件扫描器将复用此函数，故先保留为公共 API。
#[allow(dead_code)]
pub fn default_scan_sources() -> Vec<PathBuf> {
    let mut dirs = native_instruments_service_center_dirs();
    if let Some(p) = kontakt_preferences_dir() {
        dirs.push(p);
    }
    dirs
}

/// 执行一次只读盘点。
///
/// 过程（稳健、不抛错）：
///  1. 读取所有 Service Center XML 的 <Application> 条目
///  2. 读取所有 Kontakt 偏好 plist 的已注册库，标记 registered=true 并补全路径
///  3. 合并（按 snpID 去重，路径互相补全）
///  4. 运行诊断（缺失路径、断卷、重复 SNPID）
pub fn scan() -> Vec<LibraryEntry> {
    let mut by_snpid: std::collections::HashMap<String, LibraryEntry> = std::collections::HashMap::new();
    let mut seen = HashSet::new();

    // 1) Service Center XML
    for dir in native_instruments_service_center_dirs() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.extension().and_then(|e| e.to_str()) == Some("xml") {
                    for mut entry in parser::parse_service_center_xml(&p) {
                        let key = entry.snpid.clone();
                        if let Some(existing) = by_snpid.get_mut(&key) {
                            if existing.path.is_none() && entry.path.is_some() {
                                existing.path = entry.path.take();
                            }
                            existing.diagnostics.append(&mut entry.diagnostics);
                        } else {
                            by_snpid.insert(key.clone(), entry);
                        }
                        seen.insert(key);
                    }
                }
            }
        }
    }

    // 2) Kontakt 偏好 plist（已注册库）
    if let Some(pref_dir) = kontakt_preferences_dir() {
        for plist_path in kontakt_plist_files(&pref_dir) {
            for (snp, path) in parser::parse_kontakt_plist(&plist_path) {
                let entry = by_snpid.entry(snp.clone()).or_insert_with(|| {
                    LibraryEntry::new(snp.clone(), snp.clone())
                });
                entry.registered = true;
                if entry.path.is_none() {
                    entry.path = Some(path);
                }
            }
        }
    }

    // 3) 诊断
    let mut entries: Vec<LibraryEntry> = by_snpid.into_values().collect();
    diagnose::diagnose(&mut entries);
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}
