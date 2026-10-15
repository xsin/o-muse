// 分轨（Stem Separation）命令（Task 2）
//
// 调用 Python sidecar（FastAPI + Demucs）完成音频分轨。引擎完全由
// 独立实现的 Python 服务承载，本模块仅做进程看护与 HTTP 编排。

use crate::core::stem_engine::task::{self, Error, StatusResponse, StemsResponse};

fn map_err(e: Error) -> String {
    e.to_string()
}

/// 提交分轨任务。
///
/// 首次调用会启动 Python sidecar，随后通过 HTTP 将文件路径交给
/// FastAPI 服务。返回 job_id，供 `stem_status` / `stem_results` 轮询。
#[tauri::command]
pub fn separate(app: tauri::AppHandle, file_path: String) -> Result<String, String> {
    let base = crate::core::stem_engine::sidecar::ensure_sidecar(&app).map_err(map_err)?;
    task::start(&base, &file_path).map_err(map_err)
}

/// 查询分轨任务状态（含进度 0~100 与已就绪的分轨名）。
#[tauri::command]
pub fn stem_status(app: tauri::AppHandle, job_id: String) -> Result<StatusResponse, String> {
    let base = crate::core::stem_engine::sidecar::ensure_sidecar(&app).map_err(map_err)?;
    task::status(&base, &job_id).map_err(map_err)
}

/// 获取已完成任务的分轨文件清单（路径、大小）。
#[tauri::command]
pub fn stem_results(app: tauri::AppHandle, job_id: String) -> Result<StemsResponse, String> {
    let base = crate::core::stem_engine::sidecar::ensure_sidecar(&app).map_err(map_err)?;
    task::stems(&base, &job_id).map_err(map_err)
}
