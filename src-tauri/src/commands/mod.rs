// 顶层命令模块（Task 1 骨架，无业务逻辑）

pub mod stem_separation;
pub mod library_manager;
pub mod plugin_scanner;

/// Task 1 占位命令：证明命令模块已正确注册到 Tauri invoke handler。
#[tauri::command]
pub fn placeholder() -> String {
    "Music Rig backend skeleton (Task 1)".to_string()
}
