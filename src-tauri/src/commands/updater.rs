// 自动更新命令（v1.3，自研更新器）
//
// 模块职责：把 `core::updater` 的阻塞式实现桥接为 Tauri 命令。
// 约定：
//   - 全部 `async` + `spawn_blocking`（网络/文件 IO 不得占用主线程）；
//   - 进度经 `omuse://update-progress` 事件推送（与分轨运行时一致）；
//   - **静默语义**（需求 FR-U6）：`update_check(manual = false)` 遇到
//     网络/解析失败返回 `Ok(None)` 并记日志，不把错误抛给前端；
//     `manual = true` 时才返回 `Err`，让设置页展示失败原因。
//
// 注意：本模块的命令尚未在 `lib.rs` 注册（由后续改动负责），
// 因此当前会有 dead_code 警告，属预期。

use crate::core::updater::{self as upd, PlatformAsset, UpdateInfo};

/// 当前应用版本（单一来源：`tauri.conf.json` 的 `package_info`）。
#[tauri::command]
pub async fn current_version(app: tauri::AppHandle) -> String {
    tauri::async_runtime::spawn_blocking(move || {
        app.package_info().version.to_string()
    })
    .await
    .unwrap_or_else(|_| "0.0.0".to_string())
}

/// 检查更新。
///
/// - `manual = false`（启动后台检查）：失败静默为 `Ok(None)`；
/// - `manual = true`（设置页按钮）：失败返回 `Err`，由前端展示原因。
#[tauri::command]
pub async fn update_check(
    app: tauri::AppHandle,
    manual: bool,
) -> Result<Option<UpdateInfo>, String> {
    let current = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || match upd::check(&current) {
        Ok(hit) => Ok(hit),
        Err(e) => {
            // 静默失败只记日志：用户离线时不该看到任何错误
            eprintln!("[updater] 检查更新失败（manual={manual}）: {e}");
            if manual {
                Err(e)
            } else {
                Ok(None)
            }
        }
    })
    .await
    .map_err(|e| format!("检查更新任务异常: {e}"))?
}

/// 下载 + 校验 + 解压（**不替换**），返回本次可安装的版本信息。
///
/// 进度经 `PROGRESS_EVENT` 推送。校验失败会删除下载物并报错。
#[tauri::command]
pub async fn update_install(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let handle = app.clone();
    let emit_handle = app.clone();

    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;

        // 重新拉清单：确保下载的 sha256 与此刻的清单一致（不在前端信任旧数据）
        let manifest = upd::fetch_manifest()?;
        let asset: PlatformAsset = upd::asset_for_current_platform(&manifest)?.clone();
        let info = UpdateInfo {
            current_version: current.clone(),
            version: manifest.version.clone(),
            notes: manifest.notes.clone(),
            pub_date: manifest.pub_date.clone(),
            download_bytes: None,
        };

        let app_data = handle
            .path()
            .app_data_dir()
            .map_err(|e| format!("获取应用数据目录失败: {e}"))?;

        let staged = upd::download_and_stage(&app_data, &info, &asset, move |p| {
            use tauri::Emitter;
            let _ = emit_handle.emit(upd::PROGRESS_EVENT, p);
        })?;

        // 解压后**重新做 ad-hoc 签名**并校验。
        // 原因：侧车 python-stem-backend 是脚本，其 codesign 签名存放在扩展属性中；
        // tar 往返（bsdtar 写 LIBARCHIVE.xattr.*，Rust tar crate 只认 SCHILY.xattr.*）
        // 会导致该签名丢失，进而整个 bundle 签名校验失败。
        // 重新签名是幂等且确定性的，可保证安装后的 bundle 有效。
        resign_bundle(&handle, &staged)?;

        // 体积：下载完成后取实际大小，供前端展示
        let staged_dir = upd::version_dir(&app_data, &info.version);
        let bytes = std::fs::metadata(staged_dir.join("pkg.tar.gz"))
            .ok()
            .map(|m| m.len());
        Ok(UpdateInfo {
            download_bytes: bytes,
            ..info
        })
    })
    .await
    .map_err(|e| format!("下载更新任务异常: {e}"))?
}

/// 替换运行中的 `.app` 并交由 launchd 重启（脚本会等本进程退出）。
///
/// 成功后前端**必须**立即调用 `app_exit`。
#[tauri::command]
pub async fn update_apply(app: tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let app_data = handle
            .path()
            .app_data_dir()
            .map_err(|e| format!("获取应用数据目录失败: {e}"))?;

        // 与 `update_install` 同样的推导：<app data>/update/<version>/staged/O Muse.app
        let staged = find_staged_bundle(&app_data)?;
        upd::install_and_restart(&handle, &app_data, &staged)
    })
    .await
    .map_err(|e| format!("安装更新任务异常: {e}"))?
}

/// 在 `<app data>/update/` 下寻找已解压就绪的 bundle（取版本号最大者）。
fn find_staged_bundle(app_data: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let base = app_data.join(upd::UPDATE_DIR_NAME);
    let rd = std::fs::read_dir(&base)
        .map_err(|_| "尚未下载任何更新包，请先点击「更新」".to_string())?;

    let mut best: Option<((u64, u64, u64), std::path::PathBuf)> = None;
    for ent in rd.flatten() {
        let candidate = ent.path().join("staged").join(upd::STAGED_APP_NAME);
        if !upd::is_app_bundle(&candidate) {
            continue;
        }
        let ver = ent.file_name().to_string_lossy().to_string();
        let key = upd::parse_version(&ver);
        if best.as_ref().map(|(k, _)| key > *k).unwrap_or(true) {
            best = Some((key, candidate));
        }
    }
    best.map(|(_, p)| p)
        .ok_or_else(|| "尚未下载任何更新包，请先点击「更新」".to_string())
}

/// 退出应用（替换脚本正在等我们退出）。
///
/// 原计划用 `@tauri-apps/plugin-process`，但该插件已卸载，故改用
/// Tauri 原生 `app.exit(0)`。
#[tauri::command]
pub async fn app_exit(app: tauri::AppHandle) -> Result<(), String> {
    // 稍等一拍再退出，确保本命令的响应能先回到前端（前端拿到结果后再调用，
    // 但即便响应丢失，退出行为本身也不受影响）。
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        app.exit(0);
    })
    .await
    .map_err(|e| format!("退出应用失败: {e}"))?;
    Ok(())
}

/// 对暂存的 `.app` 重新做 ad-hoc 签名，并校验签名有效。
///
/// 必须成功：签名无效的 bundle 在 macOS 上可能无法启动，
/// 因此这里失败即中止更新，而不是冒险替换。
fn resign_bundle(app: &tauri::AppHandle, bundle: &std::path::Path) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;

    let run = |args: Vec<String>| -> Result<(bool, String), String> {
        let out = tauri::async_runtime::block_on(
            app.shell()
                .command("/usr/bin/codesign")
                .args(args)
                .output(),
        )
        .map_err(|e| format!("调用 codesign 失败: {e}"))?;
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Ok((out.status.success(), err))
    };

    let bundle_s = bundle.to_string_lossy().to_string();

    // 由内到外：先签可执行文件，再签 bundle 本体。
    // --deep 在新版 macOS 上不可靠，故显式逐个签。
    let macos_dir = bundle.join("Contents").join("MacOS");
    if let Ok(rd) = std::fs::read_dir(&macos_dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if !p.is_file() {
                continue;
            }
            let (ok, err) = run(vec![
                "--force".into(),
                "--sign".into(),
                "-".into(),
                p.to_string_lossy().to_string(),
            ])?;
            if !ok {
                return Err(format!("签名 {} 失败：{err}", p.display()));
            }
        }
    }

    let (ok, err) = run(vec![
        "--force".into(),
        "--sign".into(),
        "-".into(),
        bundle_s.clone(),
    ])?;
    if !ok {
        return Err(format!("签名更新包失败：{err}"));
    }

    let (ok, err) = run(vec![
        "--verify".into(),
        "--deep".into(),
        "--strict".into(),
        bundle_s,
    ])?;
    if !ok {
        return Err(format!("更新包签名校验未通过，已中止安装：{err}"));
    }
    Ok(())
}
