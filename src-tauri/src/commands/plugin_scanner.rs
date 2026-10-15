// Audio Plugin Manager 相关命令（v1.1 Task 7/8/9）

use crate::core::plugin_scanner::model::{PluginFormat, PluginInfo, ScanSource};
use crate::core::plugin_scanner::{default_sources, discover_plugins};

/// Task 1 占位命令：验证 plugin_scanner 命令模块已注册。
#[allow(dead_code)]
#[tauri::command]
pub fn plugin_scanner_placeholder() -> String {
    "plugin_scanner module placeholder (Task 1)".to_string()
}

/// 列出默认（标准）扫描源。Free 可用。
#[tauri::command]
pub fn list_scan_sources() -> Vec<ScanSource> {
    default_sources()
}

/// 扫描全部插件（默认标准源，不含自定义目录）。Free 可用。
#[tauri::command]
pub fn scan_all_plugins() -> Vec<PluginInfo> {
    let srcs = default_sources();
    discover_plugins(&srcs)
}

/// 按指定格式过滤扫描（便于 UI 分栏）。
#[tauri::command]
pub fn scan_plugins_by_format(format: String) -> Vec<PluginInfo> {
    let want = match format.to_uppercase().as_str() {
        "AU" => PluginFormat::AU,
        "VST" => PluginFormat::VST,
        "VST3" => PluginFormat::VST3,
        "AAX" => PluginFormat::AAX,
        "CLAP" => PluginFormat::CLAP,
        _ => PluginFormat::Auto,
    };
    scan_all_plugins()
        .into_iter()
        .filter(|p| want == PluginFormat::Auto || p.format == want)
        .collect()
}
