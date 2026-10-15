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

// ── 插件扫描目录（v1.1 Task 7）───────────────────────────────
//
// 返回 (目录, 该目录主要承载的格式)。发现阶段不做元数据读取。

/// macOS 标准插件目录。
///
/// 依 plan.md §四 Task 7 列出 10 个标准源（AAX 仅系统级 Avid 目录）：
///   AU   .component : /Library + ~/Library .../Components
///   VST  .vst       : /Library + ~/Library .../VST
///   VST3 .vst3      : /Library + ~/Library .../VST3, 以及 ~/.vst3
///   AAX  .aaxplugin : /Library/Application Support/Avid/Audio/Plug-Ins
///   CLAP .clap      : /Library + ~/Library .../CLAP
#[cfg(target_os = "macos")]
pub fn default_plugin_scan_sources() -> Vec<(PathBuf, super::plugin_scanner::model::PluginFormat)> {
    use super::plugin_scanner::model::PluginFormat as F;

    let home = std::env::var("HOME").ok().map(PathBuf::from);
    let sys = PathBuf::from("/Library/Audio/Plug-Ins");
    let user = home.as_ref().map(|h| h.join("Library/Audio/Plug-Ins"));

    let mut out: Vec<(PathBuf, F)> = Vec::new();

    // AU
    out.push((sys.join("Components"), F::AU));
    if let Some(u) = &user {
        out.push((u.join("Components"), F::AU));
    }
    // VST 2
    out.push((sys.join("VST"), F::VST));
    if let Some(u) = &user {
        out.push((u.join("VST"), F::VST));
    }
    // VST 3（含部分 DAW 也搜索的 ~/.vst3）
    out.push((sys.join("VST3"), F::VST3));
    if let Some(u) = &user {
        out.push((u.join("VST3"), F::VST3));
    }
    if let Some(h) = &home {
        out.push((h.join(".vst3"), F::VST3));
    }
    // AAX（仅系统级 Avid 目录）
    out.push((
        PathBuf::from("/Library/Application Support/Avid/Audio/Plug-Ins"),
        F::AAX,
    ));
    // CLAP
    out.push((sys.join("CLAP"), F::CLAP));
    if let Some(u) = &user {
        out.push((u.join("CLAP"), F::CLAP));
    }

    out
}

#[cfg(not(target_os = "macos"))]
pub fn default_plugin_scan_sources() -> Vec<(PathBuf, super::plugin_scanner::model::PluginFormat)> {
    // TODO(v1.5): Windows — C:\Program Files\Common Files\{VST3,AAX,CLAP} 等
    Vec::new()
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
