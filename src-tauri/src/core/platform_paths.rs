// 跨平台路径抽象（ADR-0002）
//
// 仅封装路径来源，不封装行为。v1 实现 macOS 分支；
// Windows 分支以占位 TODO 标注，供 v1.5 接入，不影响 v1 macOS 编译。

use std::path::PathBuf;

#[cfg(target_os = "macos")]
pub fn native_instruments_service_center_dirs() -> Vec<PathBuf> {
    match std::env::var("HOME") {
        Ok(home) => vec![PathBuf::from(home)
            .join("Library/Application Support/Native Instruments/Service Center")],
        Err(_) => Vec::new(),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn native_instruments_service_center_dirs() -> Vec<PathBuf> {
    // TODO(v1.5): Windows — %APPDATA%\Native Instruments\Service Center
    Vec::new()
}

#[cfg(target_os = "macos")]
pub fn kontakt_preferences_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join("Library/Preferences"))
}

#[cfg(not(target_os = "macos"))]
pub fn kontakt_preferences_dir() -> Option<PathBuf> {
    // TODO(v1.5): Windows — 对应注册表/目录
    None
}

/// 列出 Kontakt 偏好 plist 文件（macOS：`com.native-instruments.Kontakt*.plist`）。
pub fn kontakt_plist_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            let name = match p.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if name.starts_with("com.native-instruments.Kontakt") && name.ends_with(".plist") {
                out.push(p);
            }
        }
    }
    out
}
