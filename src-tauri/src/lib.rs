// Music Rig — Rust 后端入口（Tauri 2）
//
// 本文件为 Task 1 骨架：仅搭建模块与插件注册，不含任何业务逻辑。
// 业务逻辑将在后续 Task 中按模块逐步填充。

mod commands;
mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::placeholder,
            commands::stem_separation::stem_separation_placeholder,
            commands::library_manager::library_manager_placeholder,
            commands::plugin_scanner::plugin_scanner_placeholder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Music Rig");
}
