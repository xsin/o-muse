// Audio Plugin Manager 相关命令（v1.1 Task 7/8/9；v1.2 空间管理）
//
// 性能约定：扫描涉及目录遍历、plist 解析与 SQLite 写入，均为同步阻塞工作。
// 所有命令一律 `async` + `spawn_blocking`，**绝不阻塞 Tauri 主线程**（否则
// 在插件数量较多时会导致界面假死）。v1.2 的移动/删除同样如此：跨卷复制
// 可能是几十秒的 IO，更不能压在主线程上。

use crate::core::license::require_pro;
use crate::core::plugin_scanner::cache;
use crate::core::plugin_scanner::custom;
use crate::core::plugin_scanner::model::{PluginFormat, PluginInfo, ScanSource};
use crate::core::plugin_scanner::move_plugin as mover;
use crate::core::plugin_scanner::{default_sources, default_sources_vst3, discover_plugins};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "o-muse.dat";

/// 默认扫描格式（v1.2）：只扫 VST3，避免"打开页面即遍历 10+ 目录"。
const DEFAULT_FORMAT: &str = "VST3";

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
    /// 本次实际扫描的格式（`ALL` 或具体格式），供前端回显。
    pub format: String,
}

/// 移动计划 + 目标卷空间信息（前端要展示"要搬多少 / 目标还剩多少"）。
#[derive(Serialize)]
pub struct MovePlanView {
    #[serde(flatten)]
    pub plan: mover::MovePlan,
    /// 目标卷可用字节；拿不到时为 null（前端显示 "—"）
    pub target_free_bytes: Option<u64>,
    /// 需要的字节数（含 10% 余量）
    pub needed_bytes: u64,
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

/// 解析前端传来的格式参数。
///
/// - `None` / `"VST3"` / 空串 → VST3（v1.2 默认）
/// - `"ALL"` / `"*"` → 全部格式
/// - 具体格式 → 该格式
fn parse_wanted(format: Option<&str>) -> Result<Option<PluginFormat>, String> {
    let raw = format.unwrap_or(DEFAULT_FORMAT).trim();
    if raw.is_empty() {
        return Ok(Some(PluginFormat::VST3));
    }
    match raw.to_ascii_uppercase().as_str() {
        "ALL" | "*" | "AUTO" => Ok(None),
        "AU" => Ok(Some(PluginFormat::AU)),
        "VST" => Ok(Some(PluginFormat::VST)),
        "VST3" => Ok(Some(PluginFormat::VST3)),
        "AAX" => Ok(Some(PluginFormat::AAX)),
        "CLAP" => Ok(Some(PluginFormat::CLAP)),
        other => Err(format!("未知的插件格式: {other}")),
    }
}

/// 执行一次扫描（同步体），由 async 命令经 spawn_blocking 调用。
///
/// `format = None` 时使用 v1.2 默认：**只扫 VST3 标准目录**。
fn do_scan(app: &AppHandle, clear_cache: bool, format: Option<&str>) -> Result<ScanResult, String> {
    let want = parse_wanted(format)?;

    // 只扫某个格式时，直接不遍历无关目录（省掉 10+ 次目录树遍历）
    let mut srcs = match want {
        Some(PluginFormat::VST3) => default_sources_vst3(),
        Some(f) => default_sources()
            .into_iter()
            .filter(|s| s.plugin_format == f)
            .collect(),
        None => default_sources(),
    };
    srcs.extend(read_custom(app));
    let mut found = discover_plugins(&srcs);

    // 自定义源可能是 `Auto`（一个目录里混装多种格式）：格式筛选在这里兜底，
    // 保证 "AU" 页签不会冒出 VST3。
    if let Some(f) = want {
        found.retain(|p| p.format == f);
    }

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
            // 清理已删除插件的残留行，避免库无限增长。
            // 仅在全格式扫描时做：格式筛选后的结果集不完整，prune 会误删其它格式的行。
            if want.is_none() {
                if let Err(e) = cache::prune_missing(&conn, &found) {
                    warning.get_or_insert_with(|| format!("清理失效记录失败: {e}"));
                }
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
        format: want.map(|f| f.as_str().to_string()).unwrap_or_else(|| "ALL".to_string()),
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
///
/// **v1.2 默认只扫 VST3**（见 [`default_sources_vst3`]）。需要其它格式或
/// 全格式，请走 [`scan_plugins_by_format`]。
#[tauri::command]
pub async fn scan_all_plugins(app: AppHandle) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || do_scan(&app, false, Some(DEFAULT_FORMAT)))
        .await
        .map_err(|e| format!("扫描任务异常: {e}"))?
}

/// 按格式扫描：`VST3`（默认）/ `ALL` / `AU` / `VST` / `AAX` / `CLAP`。
///
/// 返回完整 [`ScanResult`]（含缓存命中与降级说明），便于前端统一处理。
#[tauri::command]
pub async fn scan_plugins_by_format(
    app: AppHandle,
    format: String,
) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || do_scan(&app, false, Some(&format)))
        .await
        .map_err(|e| format!("扫描任务异常: {e}"))?
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
pub async fn rescan_plugins(app: AppHandle, format: String) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || do_scan(&app, true, Some(&format)))
        .await
        .map_err(|e| format!("扫描任务异常: {e}"))?
}

// ── v1.2 空间管理：移动 / 还原 / 删除 ────────────────────────
//
// 全部 async + spawn_blocking：跨卷复制可能是几十秒 IO。
// 每个"写"命令都先做**服务端校验**（不信任前端传来的路径）。

/// 生成移动计划（只读）：要搬多少字节、真实落点、目标卷余量。
#[tauri::command]
pub async fn plan_move_plugin(source: String, target_dir: String) -> Result<MovePlanView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let plan = mover::plan_move(std::path::Path::new(&source), std::path::Path::new(&target_dir))?;
        Ok(MovePlanView {
            target_free_bytes: mover::available_bytes(&plan.target_dir),
            needed_bytes: mover::needed_bytes(plan.size_bytes),
            plan,
        })
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
}

/// 移动插件到目标目录，并在原位置创建软链。返回软链路径。
///
/// 软链（而非 Finder 别名）：别名只是书签，POSIX `open()`/`stat()` 不解析，
/// DAW 扫描器看不到；软链由内核透明解析，DAW 照常加载。
#[tauri::command]
pub async fn move_plugin(
    app: AppHandle,
    source: String,
    target_dir: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 注册 AppHandle：execute_move 内部用 shell 插件调 codesign 复核签名
        mover::register_app_handle(&app);
        let plan =
            mover::plan_move(std::path::Path::new(&source), std::path::Path::new(&target_dir))?;
        let link = mover::execute_move(&plan)?;
        Ok(link.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
}

/// 把软链还原为真实文件（撤销 [`move_plugin`]）。
#[tauri::command]
pub async fn undo_move_plugin(link_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        mover::undo_move(std::path::Path::new(&link_path))
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
}

/// 删除插件 → **移到废纸篓**（绝不永久删除）。返回废纸篓中的落点。
#[tauri::command]
pub async fn trash_plugin(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mover::trash_plugin(std::path::Path::new(&path))
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
}

/// 永久删除（不可恢复）。要求 `confirm_name` 与插件目录名一致——
/// **服务端二次校验**，不只依赖前端禁用按钮。
#[tauri::command]
pub async fn delete_plugin_permanently(
    path: String,
    confirm_name: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        mover::delete_plugin_permanently(std::path::Path::new(&path), &confirm_name)
    })
    .await
    .map_err(|e| format!("任务异常: {e}"))?
}
