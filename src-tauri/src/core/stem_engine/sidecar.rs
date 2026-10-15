// Python sidecar 生命周期管理（Task 1 骨架，无业务逻辑）
//
// 注意：Rust 侧禁止直接使用 std::process::Command 直接派生子进程，
// 必须通过 tauri_plugin_shell 的 sidecar API 启动 Python 进程（见 plan 全局约束）。
// 具体实现在 Task 2 落地；此处仅为模块占位。

/// Task 1 占位：验证 sidecar 模块已注册。
#[allow(dead_code)]
pub fn placeholder() -> String {
    "stem_engine::sidecar placeholder (Task 1)".to_string()
}
