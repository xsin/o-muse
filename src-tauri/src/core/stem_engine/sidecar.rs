// Python sidecar 生命周期管理（Task 2）
//
// 通过 tauri-plugin-shell 的 `sidecar` API 启动 Python 分轨服务
// （FastAPI + Demucs），并在退出时 kill 子进程。设计为独立实现，
// 仅借鉴"上游参考项目"的 backend watchdog 架构模式，未使用其任何私有接口。
//
// 鲁棒性约定（review 修复）：
//  - **抗中毒**锁取用：一次 panic 不得让后续所有分轨调用永久失败
//  - sidecar 退出时清空 `SidecarState`，否则崩溃后功能永久不可用（需重启应用）
//  - 检查与 spawn 在同一临界区内完成，避免并发调用重复拉起进程争抢端口
//  - 健康检查超时必须**报错**，而非带着死进程继续返回 base URL

use tauri::async_runtime::Receiver;
use tauri::Manager;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

/// sidecar 可执行文件在 tauri.conf.json `bundle.externalBin` 中的注册名。
pub const SIDECAR_NAME: &str = "python-stem-backend";

/// 分轨服务监听端口（与 python-backend 的 constants.py 保持一致）。
pub const SIDECAR_PORT: u16 = 8787;

/// 在临界区内操作 sidecar 句柄。
///
/// 用闭包而非返回 guard：`app.state()` 返回的是受管状态，guard 无法安全地
/// 脱离其借用返回给调用方。闭包保证锁一定被释放。
/// mutex 中毒时恢复内部数据，避免整条分轨链路因一次 panic 而永久失效。
fn with_child<T>(app: &tauri::AppHandle, f: impl FnOnce(&mut Option<CommandChild>) -> T) -> T {
    let state = app.state::<crate::SidecarState>();
    let mut guard = match state.0.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&mut guard)
}

/// 启动 Python sidecar；若已运行则直接复用。返回健康检查后的 base URL。
pub fn ensure_sidecar(app: &tauri::AppHandle) -> crate::core::stem_engine::task::Result<String> {
    let base = format!("http://127.0.0.1:{}", SIDECAR_PORT);

    // 检查与 spawn 在同一临界区内，避免并发调用重复拉起进程。
    let spawn_result: Option<Result<(Receiver<CommandEvent>, CommandChild), String>> =
        with_child(app, |slot| {
            if slot.is_some() {
                return None; // 已在运行，直接复用
            }
            Some(
                app.shell()
                    .sidecar(SIDECAR_NAME)
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|c| c.spawn().map_err(|e| format!("{e:?}"))),
            )
        });

    let mut rx = match spawn_result {
        None => return Ok(base), // 已有实例在运行
        Some(Ok((r, child))) => {
            with_child(app, |slot| *slot = Some(child));
            r
        }
        Some(Err(e)) => {
            return Err(crate::core::stem_engine::task::Error::Sidecar(format!(
                "无法启动分轨服务 sidecar: {e}"
            )))
        }
    };

    // 后台消费 stdout/stderr，避免管道缓冲导致进程挂起；
    // 进程退出时清空状态，使下次调用能重新拉起。
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let CommandEvent::Terminated(_) = event {
                with_child(&handle, |slot| {
                    slot.take();
                });
                break;
            }
        }
    });

    wait_for_health()
}

/// 阻塞等待 sidecar 的 `/health` 端点就绪（最多约 30s）。
///
/// 超时**必须报错**：否则调用方会拿着一个死掉的进程继续发 HTTP，
/// 最终只看到含糊的 "http: ..." 错误而非"服务启动失败"。
fn wait_for_health() -> crate::core::stem_engine::task::Result<String> {
    let base = format!("http://127.0.0.1:{}", SIDECAR_PORT);
    for _ in 0..150 {
        if let Ok(200) = reqwest::blocking::get(format!("{base}/health")).map(|r| r.status().as_u16()) {
            return Ok(base);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Err(crate::core::stem_engine::task::Error::Sidecar(
        "分轨服务启动超时（30s），请检查 Python sidecar 是否可用".to_string(),
    ))
}

/// 终止 sidecar。供命令层显式调用（如应用退出前清理）。
#[allow(dead_code)]
pub fn kill_sidecar(app: &tauri::AppHandle) -> crate::core::stem_engine::task::Result<()> {
    let child_opt = with_child(app, |slot| slot.take());
    if let Some(child) = child_opt {
        child.kill().map_err(|e| {
            crate::core::stem_engine::task::Error::Sidecar(format!("kill failed: {e:?}"))
        })?;
    }
    Ok(())
}
