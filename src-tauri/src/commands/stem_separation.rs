// 分轨（Stem Separation）相关命令（Task 1 骨架，无业务逻辑）
//
// 设计参考：StemDeck 的 Python sidecar 生命周期管理模式与 backend watchdog
// 机制（仅作架构参考，未复制其源代码）。实际分轨由 Demucs 通过 Python
// sidecar 完成，具体实现在 Task 2 落地。

/// Task 1 占位命令：验证 stem_separation 命令模块已注册。
///
/// 注：Tauri 2 的 `#[tauri::command]` 会在 crate 根生成以函数名派生的宏，
/// 因此各命令模块的占位命令必须唯一命名，避免 `__cmd__placeholder` 冲突。
#[tauri::command]
pub fn stem_separation_placeholder() -> String {
    "stem_separation module placeholder (Task 1)".to_string()
}
