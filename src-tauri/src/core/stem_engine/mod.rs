// 分轨引擎核心（Task 1 骨架，无业务逻辑）
//
// 职责边界（后续 Task 落地）：
// - sidecar.rs：管理与 Python(Demucs) 子进程的启动/健康检查/退出清理，
//   其设计参考上游参考项目的 backend watchdog 机制（仅架构参考，未复制源码）。
// - task.rs：分轨任务的标识、状态与结果路径管理。

pub mod sidecar;
pub mod task;
