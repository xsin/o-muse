// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// Kontakt 库管理命令（Tauri 命令层）
//
// 性能约定：盘点、扫描、注册/注销涉及目录遍历与文件 IO，全部
// `async` + `spawn_blocking`，绝不阻塞 Tauri 主线程。

use crate::core::kontakt;
use crate::core::kontakt::model::{KontaktLibrary, NicntFormSpec};
use std::path::PathBuf;

/// 合并四个注册来源盘点所有库（含诊断）。
#[tauri::command]
pub async fn kontakt_inventory() -> Result<Vec<KontaktLibrary>, String> {
    tauri::async_runtime::spawn_blocking(kontakt::inventory)
        .await
        .map_err(|e| format!("盘点任务异常: {e}"))?
}

/// 扫描用户给定根目录，返回其中（多半尚未注册的）库。
#[tauri::command]
pub async fn kontakt_scan(root: String) -> Result<Vec<KontaktLibrary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = PathBuf::from(&root);
        if !dir.is_dir() {
            return Err(format!("扫描目录不存在或不是目录: {root}"));
        }
        Ok(kontakt::scan_libraries(&dir))
    })
    .await
    .map_err(|e| format!("扫描任务异常: {e}"))?
}

/// 注册一个库：读取 `.nicnt`，写入 installed_products JSON 与用户级 plist。
#[tauri::command]
pub async fn kontakt_add(nicnt_path: String) -> Result<KontaktLibrary, String> {
    tauri::async_runtime::spawn_blocking(move || kontakt::add_library(&PathBuf::from(&nicnt_path)))
        .await
        .map_err(|e| format!("添加任务异常: {e}"))?
}

/// 注销一个库：删除注册文件，**不动磁盘内容**。
#[tauri::command]
pub async fn kontakt_remove(reg_key: String, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || kontakt::remove_library(&reg_key, &name))
        .await
        .map_err(|e| format!("移除任务异常: {e}"))?
}

/// 返回 `.nicnt` 创建表单的默认模板。
#[tauri::command]
pub async fn kontakt_nicnt_spec() -> Result<NicntFormSpec, String> {
    tauri::async_runtime::spawn_blocking(kontakt::create_nicnt_form_spec)
        .await
        .map_err(|e| format!("任务异常: {e}"))
}

/// 在用户选择的库目录内生成 `.nicnt`，返回文件路径。
///
/// 同名文件已存在时直接报错（绝不覆盖）；绝不会写入 `/Library` 等系统目录。
#[tauri::command]
pub async fn kontakt_create_nicnt(dir: String, spec: NicntFormSpec) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target_dir = PathBuf::from(&dir);
        let path = kontakt::write_nicnt(&target_dir, spec)?;
        Ok(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("生成 nicnt 任务异常: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root(tag: &str) -> PathBuf {
        let base = std::fs::canonicalize(std::env::temp_dir()).expect("临时目录可用");
        let dir = base.join(format!("omuse_kontakt_cmd_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn 命令层默认表单可获取() {
        let spec = tauri::async_runtime::block_on(kontakt_nicnt_spec()).expect("应返回默认表单");
        assert_eq!(spec.product_type, "Content");
        assert_eq!(spec.visibility, 3);
    }

    #[test]
    fn 命令层拒绝不存在的扫描目录() {
        let missing = std::env::temp_dir().join("omuse_definitely_missing_dir_9f8e7d");
        let err =
            tauri::async_runtime::block_on(kontakt_scan(missing.to_string_lossy().to_string()))
                .unwrap_err();
        assert!(err.contains("不存在"), "错误信息应说明目录不存在: {err}");
    }

    #[test]
    fn 命令层生成并扫描_nicnt() {
        let root = fixture_root("create");
        let spec = NicntFormSpec {
            name: "CmdLib".to_string(),
            reg_key: "CmdLib".to_string(),
            snpid: "C1".to_string(),
            company: "O Muse".to_string(),
            ..NicntFormSpec::default()
        };

        let created = tauri::async_runtime::block_on(kontakt_create_nicnt(
            root.to_string_lossy().to_string(),
            spec,
        ))
        .expect("应能生成 nicnt");
        assert!(PathBuf::from(&created).is_file());

        std::fs::create_dir_all(root.join("Samples")).unwrap();
        let found =
            tauri::async_runtime::block_on(kontakt_scan(root.to_string_lossy().to_string()))
                .expect("应能扫描");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "CmdLib");

        std::fs::remove_dir_all(&root).ok();
    }
}
