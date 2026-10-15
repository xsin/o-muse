// Python sidecar 生命周期管理（Task 2）
//
// 通过 tauri-plugin-shell 的 `sidecar` API 启动 Python 分轨服务
// （FastAPI + Demucs），并在退出时 kill 子进程。设计为独立实现，
// 仅借鉴"上游参考项目"的 backend watchdog 架构模式，未使用其任何私有接口。

use tauri::Manager;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

/// sidecar 可执行文件在 tauri.conf.json `bundle.externalBin` 中的注册名。
pub const SIDECAR_NAME: &str = "python-stem-backend";

/// 分轨服务监听端口（与 python-backend 的 constants.py 保持一致）。
pub const SIDECAR_PORT: u16 = 8787;

/// 启动 Python sidecar；若已运行则直接复用。返回健康检查后的 base URL。
pub fn ensure_sidecar(app: &tauri::AppHandle) -> crate::core::stem_engine::task::Result<String> {
    {
        let state = app.state::<crate::SidecarState>();
        let guard = state.0.lock().unwrap();
        if guard.is_some() {
            return Ok(format!("http://127.0.0.1:{}", SIDECAR_PORT));
        }
    }

    let (mut rx, child) = app
        .shell()
        .sidecar(SIDECAR_NAME)
        .map_err(|e| crate::core::stem_engine::task::Error::Sidecar(format!("{e:?}")))?
        .spawn()
        .map_err(|e| crate::core::stem_engine::task::Error::Sidecar(format!("{e:?}")))?;

    // 在后台消费 sidecar 的 stdout/stderr，避免管道缓冲导致进程挂起。
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let CommandEvent::Terminated(_) = event {
                break;
            }
        }
    });

    {
        let state = app.state::<crate::SidecarState>();
        let mut guard = state.0.lock().unwrap();
        *guard = Some(child);
    }

    wait_for_health(app)
}

/// 阻塞等待 sidecar 的 `/health` 端点就绪（最多约 30s）。
fn wait_for_health(app: &tauri::AppHandle) -> crate::core::stem_engine::task::Result<String> {
    let base = format!("http://127.0.0.1:{}", SIDECAR_PORT);
    for _ in 0..150 {
        if let Ok(200) = reqwest::blocking::get(format!("{base}/health")).map(|r| r.status().as_u16()) {
            return Ok(base);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let _ = app; // 健康检查失败也保留子进程，交由调用方决定。
    Ok(base)
}

/// 终止 sidecar。供命令层显式调用（如应用退出前清理）。
#[allow(dead_code)]
pub fn kill_sidecar(app: &tauri::AppHandle) -> crate::core::stem_engine::task::Result<()> {
    let child_opt: Option<CommandChild> = {
        let state = app.state::<crate::SidecarState>();
        let mut guard = state.0.lock().unwrap();
        guard.take()
    };
    if let Some(child) = child_opt {
        child.kill().map_err(|e| {
            crate::core::stem_engine::task::Error::Sidecar(format!("kill failed: {e:?}"))
        })?;
    }
    Ok(())
}

/// 便捷锁：避免在多个地方重复写 Mutex guard 取用。
#[allow(dead_code)]
pub(crate) fn with_child<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&mut Option<CommandChild>) -> T,
) -> T {
    let state = app.state::<crate::SidecarState>();
    let mut guard = state.0.lock().unwrap();
    f(&mut guard)
}
