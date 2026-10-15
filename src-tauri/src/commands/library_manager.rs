// Kontakt Library Manager 命令（Task 3）

use crate::core::library_manager;
use crate::core::library_manager::model::LibraryEntry;
use crate::core::license::require_pro;
use tauri::AppHandle;

/// 执行一次 Kontakt 库只读盘点，返回所有库条目及诊断信息（Free 可用）。
#[tauri::command]
pub async fn scan_libraries() -> Result<Vec<LibraryEntry>, String> {
    let entries = library_manager::scan();
    Ok(entries)
}

/// 修复/写入库元数据（Pro 专属）。经 `require_pro` 统一拦截。
#[tauri::command]
pub async fn repair_library(app: AppHandle, snpid: String) -> Result<String, String> {
    // 统一 Pro 拦截：非 Pro 直接返回错误。
    require_pro(&app).await?;
    // v1.1 才实现实际写入逻辑；此处仅占位返回成功回执。
    Ok(format!("repair queued for {snpid} (Pro)"))
}
