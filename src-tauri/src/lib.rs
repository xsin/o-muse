// Music Rig — Rust 后端入口（Tauri 2）
//
// Task 2：引入 Python sidecar（FastAPI 分轨服务）的生命周期管理。
// 守护进程句柄存于 `SidecarState`，由命令层共享。

mod commands;
mod core;

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
        .manage(SidecarState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            commands::placeholder,
            commands::stem_separation::separate,
            commands::stem_separation::stem_status,
            commands::stem_separation::stem_results,
            commands::library_manager::library_manager_placeholder,
            commands::plugin_scanner::plugin_scanner_placeholder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Music Rig");
}
