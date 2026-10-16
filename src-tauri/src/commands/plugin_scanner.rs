// Audio Plugin Manager 相关命令（v1.1 Task 7/8/9）
//
// 性能约定：扫描涉及目录遍历、plist 解析与 SQLite 写入，均为同步阻塞工作。
// 所有命令一律 `async` + `spawn_blocking`，**绝不阻塞 Tauri 主线程**（否则
// 在插件数量较多时会导致界面假死）。

use crate::core::license::require_pro;
use crate::core::plugin_scanner::cache;
use crate::core::plugin_scanner::custom;
use crate::core::plugin_scanner::model::{PluginFormat, PluginInfo, ScanSource};
use crate::core::plugin_scanner::{default_sources, discover_plugins};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "o-muse.dat";

/// Task 1 占位命令：验证 plugin_scanner 命令模块已注册。
#[allow(dead_code)]
#[tauri::command]
pub fn plugin_scanner_placeholder() -> String {
    "plugin_scanner module placeholder (Task 1)".to_string()
}

/// 扫描结果 + 缓存统计，便于前端展示"已缓存 N 个"。
#[derive(Serialize)]
pub struct ScanResult {
    pub plugins: Vec<PluginInfo>,
    pub hits: usize,
    pub misses: usize,
    /// 缓存不可用时的降级说明（仍返回插件，只是元数据为回退值）。
    pub cache_warning: Option<String>,
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

/// 执行一次扫描（同步体），由 async 命令经 spawn_blocking 调用。
fn do_scan(app: &AppHandle, clear_cache: bool) -> Result<ScanResult, String> {
    let mut srcs = default_sources();
    srcs.extend(read_custom(app));
    let mut found = discover_plugins(&srcs);

    let mut warning: Option<String> = None;
    let mut hits = 0usize;
    let mut misses = found.len();

    match cache_path(app).and_then(|p| cache::open(&p).map_err(|e| e.to_string())) {
        Ok(conn) => {
            if clear_cache {
                if let Err(e) = cache::clear(&conn) {
                    warning = Some(format!("清理缓存失败: {e}"));
                }
            }
            let stats = cache::extract_with_cache(&conn, &mut found);
            hits = stats.hits;
            misses = stats.misses;
            // 清理已删除插件的残留行，避免库无限增长
            if let Err(e) = cache::prune_missing(&conn, &found) {
                warning.get_or_insert_with(|| format!("清理失效记录失败: {e}"));
            }
        }
        Err(e) => {
            // 缓存不可用时仍返回结果，但明确告知前端（元数据为回退值）
            warning = Some(format!("元数据缓存不可用（{}），已降级为无缓存扫描", e));
            crate::core::plugin_scanner::extract::extract_all(&mut found);
        }
    }

    Ok(ScanResult {
        plugins: found,
        hits,
        misses,
        cache_warning: warning,
    })
}

/// 列出扫描源（标准 + 自定义）。列表对 Free 可见；仅写入需 Pro。
#[tauri::command]
pub async fn list_scan_sources(app: AppHandle) -> Vec<ScanSource> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut srcs = default_sources();
        srcs.extend(read_custom(&app));
        srcs
    })
    .await
    .unwrap_or_default()
}

/// 扫描全部插件（标准源 + 自定义源），带 SQLite 增量缓存。
#[tauri::command]
pub async fn scan_all_plugins(app: AppHandle) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || do_scan(&app, false))
        .await
        .map_err(|e| format!("扫描任务异常: {e}"))?
}

/// 按格式过滤扫描。
///
/// 仅在**已扫描结果**上过滤：先取全量（命中缓存时很快），再过滤，
/// 避免每个格式页签都触发一次完整目录遍历。
#[tauri::command]
pub async fn scan_plugins_by_format(
    app: AppHandle,
    format: String,
) -> Result<Vec<PluginInfo>, String> {
    let upper = format.to_uppercase();
    let want = match upper.as_str() {
        "AU" => PluginFormat::AU,
        "VST" => PluginFormat::VST,
        "VST3" => PluginFormat::VST3,
        "AAX" => PluginFormat::AAX,
        "CLAP" => PluginFormat::CLAP,
        _ => return Err(format!("未知的插件格式: {format}")),
    };
    let res = scan_all_plugins(app).await?;
    Ok(res
        .plugins
        .into_iter()
        .filter(|p| p.format == want)
        .collect())
}

/// 添加自定义扫描目录（**Pro 专属**）。
///
/// 按 plan 要求：此处**不递归扫描**，仅持久化源。
#[tauri::command]
pub async fn add_custom_plugin_dir(app: AppHandle, path: String) -> Result<ScanSource, String> {
    require_pro(&app).await?;

    tauri::async_runtime::spawn_blocking(move || {
        let p = PathBuf::from(&path);
        if !p.is_dir() {
            return Err("所选路径不是有效目录".to_string());
        }
        let source = custom::build_custom(p);
        let sources = custom::upsert(read_custom(&app), source.clone());
        write_custom(&app, &sources)?;
        Ok(source)
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
}

/// 移除自定义扫描目录（**Pro 专属**）。
#[tauri::command]
pub async fn remove_custom_plugin_dir(app: AppHandle, path: String) -> Result<(), String> {
    require_pro(&app).await?;
    tauri::async_runtime::spawn_blocking(move || {
        // 用 key() 归一化比较，容忍结尾斜杠差异
        let target = custom::normalize_path_key(&PathBuf::from(&path));
        let sources = read_custom(&app);
        let left: Vec<ScanSource> = sources
            .into_iter()
            .filter(|s| s.key() != target)
            .collect();
        write_custom(&app, &left)
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
}

/// 清空插件元数据缓存并重新扫描（UI "重新扫描"）。
#[tauri::command]
pub async fn rescan_plugins(app: AppHandle) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || do_scan(&app, true))
        .await
        .map_err(|e| format!("扫描任务异常: {e}"))?
}
