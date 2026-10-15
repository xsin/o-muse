// Audio Plugin Manager 相关命令（v1.1 Task 7/8/9）

use crate::core::license::require_pro;
use crate::core::plugin_scanner::cache;
use crate::core::plugin_scanner::custom;
use crate::core::plugin_scanner::model::{PluginFormat, PluginInfo, ScanSource};
use crate::core::plugin_scanner::{default_sources, discover_plugins};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "music-rig.dat";

/// Task 1 占位命令：验证 plugin_scanner 命令模块已注册。
#[allow(dead_code)]
#[tauri::command]
pub fn plugin_scanner_placeholder() -> String {
    "plugin_scanner module placeholder (Task 1)".to_string()
}

/// 读取持久化的自定义源（Pro 写入的）。
fn read_custom(app: &AppHandle) -> Vec<ScanSource> {
    let raw = app
        .store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(custom::STORE_KEY))
        .and_then(|v| v.as_str().map(|x| x.to_string()));
    custom::deserialize(raw.as_deref())
}

fn write_custom(app: &AppHandle, sources: &[ScanSource]) -> Result<(), String> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| format!("打开 store 失败: {e}"))?;
    let json = custom::serialize(sources)?;
    let value = serde_json::to_value(json).map_err(|e| format!("写入失败: {e}"))?;
    store.set(custom::STORE_KEY, value);
    store.save().map_err(|e| format!("保存失败: {e}"))
}

/// 插件缓存库路径：`<app data dir>/plugin_cache.db`。
fn cache_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    Ok(dir.join("plugin_cache.db"))
}

/// 列出扫描源（标准 + 自定义）。自定义源仅 Pro 可写入，但列表对 Free 可见。
#[tauri::command]
pub fn list_scan_sources(app: AppHandle) -> Vec<ScanSource> {
    let mut srcs = default_sources();
    srcs.extend(read_custom(&app));
    srcs
}

/// 扫描全部插件（标准源 + 自定义源），带 SQLite 增量缓存。
#[tauri::command]
pub fn scan_all_plugins(app: AppHandle) -> Result<Vec<PluginInfo>, String> {
    let mut srcs = default_sources();
    srcs.extend(read_custom(&app));
    let mut found = discover_plugins(&srcs);

    // 增量缓存：命中直接返回，未命中提取后回写
    if let Ok(path) = cache_path(&app) {
        if let Ok(conn) = cache::open(&path) {
            let _stats = cache::extract_with_cache(&conn, &mut found);
        }
    }
    Ok(found)
}

/// 按格式过滤扫描（UI 分栏）。
#[tauri::command]
pub fn scan_plugins_by_format(app: AppHandle, format: String) -> Result<Vec<PluginInfo>, String> {
    let want = match format.to_uppercase().as_str() {
        "AU" => PluginFormat::AU,
        "VST" => PluginFormat::VST,
        "VST3" => PluginFormat::VST3,
        "AAX" => PluginFormat::AAX,
        "CLAP" => PluginFormat::CLAP,
        _ => PluginFormat::Auto,
    };
    Ok(scan_all_plugins(app)?
        .into_iter()
        .filter(|p| want == PluginFormat::Auto || p.format == want)
        .collect())
}

/// 添加自定义扫描目录（**Pro 专属**）。
///
/// 按 plan 要求：此处**不递归扫描**，仅持久化源。
#[tauri::command]
pub async fn add_custom_plugin_dir(app: AppHandle, path: String) -> Result<ScanSource, String> {
    require_pro(&app).await?;

    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err("所选路径不是有效目录".into());
    }
    let source = custom::build_custom(p);
    let mut sources = read_custom(&app);
    sources = custom::upsert(sources, source.clone());
    write_custom(&app, &sources)?;
    Ok(source)
}

/// 移除自定义扫描目录（**Pro 专属**）。
#[tauri::command]
pub async fn remove_custom_plugin_dir(app: AppHandle, path: String) -> Result<(), String> {
    require_pro(&app).await?;
    let sources = read_custom(&app);
    let left = custom::remove(sources, std::path::Path::new(&path));
    write_custom(&app, &left)
}

/// 清空插件元数据缓存并重新扫描（UI "重新扫描"）。
#[tauri::command]
pub fn rescan_plugins(app: AppHandle) -> Result<Vec<PluginInfo>, String> {
    if let Ok(path) = cache_path(&app) {
        if let Ok(conn) = cache::open(&path) {
            let _ = cache::clear(&conn);
        }
    }
    scan_all_plugins(app)
}
