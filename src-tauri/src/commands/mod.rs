// 顶层命令模块

pub mod stem_separation;
pub mod library_manager;
pub mod plugin_scanner;
pub mod license;
pub mod kontakt;

/// 骨架探针命令：证明命令模块已正确注册到 Tauri invoke handler。
#[tauri::command]
pub fn placeholder() -> String {
    "O Muse backend skeleton".to_string()
}

/// 在访达中显示指定路径。
///
/// 用 tauri-plugin-shell 调用系统 `open -R`（不使用 std::process::Command）。
/// `-R` 表示在访达中定位该文件/目录而非打开它。
#[tauri::command]
pub async fn reveal_in_finder(
    app: tauri::AppHandle,
    path: String,
) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;

    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("路径不存在：{path}"));
    }

    let out = app
        .shell()
        .command("/usr/bin/open")
        .args(["-R", &path])
        .output()
        .await
        .map_err(|e| format!("调用 open 失败: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "在访达中显示失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}
