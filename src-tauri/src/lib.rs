// O Muse — Rust 后端入口（Tauri 2）
//
// Task 2：引入 Python sidecar（FastAPI 分轨服务）的生命周期管理。
// 守护进程句柄存于 `SidecarState`，由命令层共享。

mod commands;
pub mod core;

use std::sync::Mutex;

/// 守护已启动的 Python sidecar 子进程，供命令层共享。
///
/// 仅持有 `CommandChild` 句柄；不依赖任何第三方项目的私有接口。
pub struct SidecarState(pub Mutex<Option<tauri_plugin_shell::process::CommandChild>>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(SidecarState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            commands::placeholder,
            commands::stem_separation::separate,
            commands::stem_separation::stem_status,
            commands::stem_separation::stem_results,
            commands::stem_separation::runtime_status,
            commands::stem_separation::install_runtime,
            commands::stem_separation::remove_runtime,
            commands::library_manager::scan_libraries,
            commands::library_manager::repair_library,
            commands::plugin_scanner::plugin_scanner_placeholder,
            commands::plugin_scanner::list_scan_sources,
            commands::plugin_scanner::scan_all_plugins,
            commands::plugin_scanner::scan_plugins_by_format,
            commands::plugin_scanner::add_custom_plugin_dir,
            commands::plugin_scanner::remove_custom_plugin_dir,
            commands::plugin_scanner::rescan_plugins,
            commands::license::get_tier,
            commands::license::activate_license_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running O Muse");
}
