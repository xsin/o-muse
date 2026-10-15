// Kontakt Library Manager 命令（Task 3）

use crate::core::library_manager;
use crate::core::library_manager::model::LibraryEntry;

/// 执行一次 Kontakt 库只读盘点，返回所有库条目及诊断信息。
#[tauri::command]
pub async fn scan_libraries() -> Result<Vec<LibraryEntry>, String> {
    let entries = library_manager::scan();
    Ok(entries)
}
