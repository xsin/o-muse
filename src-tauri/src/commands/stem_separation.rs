// 分轨（Stem Separation）命令（Task 2）
//
// 调用 Python sidecar（FastAPI + Demucs）完成音频分轨。引擎完全由
// 独立实现的 Python 服务承载，本模块仅做进程看护与 HTTP 编排。
//
// 性能约定：sidecar 拉起（含最长 30s 健康检查）与 HTTP 调用均为阻塞操作，
// 一律 `async` + `spawn_blocking`，**不阻塞 Tauri 主线程**。

use crate::core::stem_engine::task::{self, Error, StatusResponse, StemsResponse};

fn map_err(e: Error) -> String {
    e.to_string()
}

/// 提交分轨任务。
///
/// 首次调用会启动 Python sidecar，随后通过 HTTP 将文件路径交给
/// FastAPI 服务。返回 job_id，供 `stem_status` / `stem_results` 轮询。
#[tauri::command]
pub async fn separate(app: tauri::AppHandle, file_path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base =
            crate::core::stem_engine::sidecar::ensure_sidecar(&app).map_err(map_err)?;
        task::start(&base, &file_path).map_err(map_err)
    })
    .await
    .map_err(|e| format!("分轨任务异常: {e}"))?
}

/// 查询分轨任务状态（含进度 0~100 与已就绪的分轨名）。
///
/// 由前端轮询调用；走 spawn_blocking 以免慢请求卡住 UI。
#[tauri::command]
pub async fn stem_status(app: tauri::AppHandle, job_id: String) -> Result<StatusResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base =
            crate::core::stem_engine::sidecar::ensure_sidecar(&app).map_err(map_err)?;
        task::status(&base, &job_id).map_err(map_err)
    })
    .await
    .map_err(|e| format!("查询状态异常: {e}"))?
}

/// 获取已完成任务的分轨文件清单（路径、大小）。
#[tauri::command]
pub async fn stem_results(app: tauri::AppHandle, job_id: String) -> Result<StemsResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base =
            crate::core::stem_engine::sidecar::ensure_sidecar(&app).map_err(map_err)?;
        task::stems(&base, &job_id).map_err(map_err)
    })
    .await
    .map_err(|e| format!("获取分轨结果异常: {e}"))?
}

// ── 分轨运行时自举（v1.2）────────────────────────────────────
//
// 分轨依赖 PyTorch（约 151MB）与模型权重（约 84MB），远大于应用本体，
// 故不在安装包内分发，而是在首次使用时按需自举（详见
// docs/tech-release-without-developer-id.md 与 prd/tdd 的运行时章节）。

use crate::core::stem_engine::runtime as rt;

/// 查询分轨运行时状态（不发起网络请求）。
#[tauri::command]
pub fn runtime_status(app: tauri::AppHandle) -> rt::RuntimeStatus {
    rt::detect(&app)
}

/// 安装/修复分轨运行时。耗时较长，走 spawn_blocking 并推送进度事件。
#[tauri::command]
pub async fn install_runtime(app: tauri::AppHandle) -> Result<rt::RuntimeStatus, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let emit_handle = handle.clone();
        rt::install(&handle, move |p| {
            use tauri::Emitter;
            let _ = emit_handle.emit(rt::PROGRESS_EVENT, p);
        })
    })
    .await
    .map_err(|e| format!("安装任务异常: {e}"))?
}

/// 卸载分轨运行时（回收约 900MB 磁盘）。
#[tauri::command]
pub async fn remove_runtime(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || rt::remove(&app))
        .await
        .map_err(|e| format!("卸载任务异常: {e}"))?
}
