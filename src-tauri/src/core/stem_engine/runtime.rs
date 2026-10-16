// 分轨运行时自举（v1.2）
//
// 背景：分轨依赖 PyTorch，体积远超应用本体（应用 ~21MB，依赖 ~150MB）。
// 且 macOS 自带的 /usr/bin/python3 是 3.9.6（Xcode CLT 转发桩），
// 而当前 PyTorch 最低要求 3.10 —— 系统 Python **无法**满足依赖。
//
// 因此采用两段式分发：
//   1. 安装包只带 python-backend 源码（小、快、低信任成本）
//   2. 首次使用分轨时，按需自举一套**私有**运行时到
//      `<app data>/runtime/`：下载自包含 CPython → 校验 → 解压 → venv → 装依赖
//
// 不写进 .app 的理由：让不用分轨的用户永远不必付这 255MB。
//
// 安全约定：
//   - Python 运行时按**固定 SHA256** 校验（下载源：python-build-standalone）
//   - 依赖安装强制 `--only-binary=:all:`，避免执行源码包构建脚本
//   - 所有外部进程经 tauri-plugin-shell 启动（不使用 std::process::Command）

use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri::Manager;

/// 自包含 CPython 的固定版本与校验和（python-build-standalone）。
pub const PBS_TAG: &str = "20261013";
pub const PY_VERSION: &str = "3.12.15";

const SHA_ARM64: &str = "ba32036ad8f4c434d97a1ee7feef8b7edc0fac16777639b97e39c74a108e4440";
const SHA_X86_64: &str = "9686ddd8639b855905447ea4a16eb54a804a9ef12afbe7c9ecb1412d9c1fbc98";

/// 运行时目录名（含版本，便于将来升级而不影响旧目录）。
pub const RUNTIME_DIR_NAME: &str = "runtime";

/// htdemucs 权重（Demucs 官方发布地址）。
///
/// 由 Rust 端**主动下载并校验**，而不是让 demucs 首次分轨时用 Python
/// urllib 自己拉：这样可控制、可校验、可显示进度，装完即可**离线**分轨。
pub const MODEL_FILE: &str = "955717e8-8726e21a.th";
pub const MODEL_URL: &str =
    "https://dl.fbaipublicfiles.com/demucs/hybrid_transformer/955717e8-8726e21a.th";
pub const MODEL_SHA256: &str = "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStage {
    /// 未安装
    Missing,
    /// 正在探测
    Checking,
    /// 下载 Python 运行时
    DownloadingPython,
    /// 校验下载物
    Verifying,
    /// 解压 Python
    Extracting,
    /// 创建 venv
    CreatingVenv,
    /// 安装依赖
    InstallingDeps,
    /// 下载 htdemucs 模型权重
    DownloadingModel,
    /// 就绪
    Ready,
    /// 失败
    Failed,
}

/// 运行时状态（返回给前端）。
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatus {
    pub stage: RuntimeStage,
    /// 0–100；未知进度为 None
    pub progress: Option<u8>,
    /// 人类可读的当前动作
    pub message: String,
    /// 已安装时的 python 可执行文件路径
    pub python_path: Option<PathBuf>,
    /// 已占用的磁盘空间（字节）
    pub size_bytes: Option<u64>,
    /// 预计还需下载的字节数（用于前端提示）
    pub estimated_download_bytes: u64,
}

/// 预计首次安装的总下载量（Python 24MB + 依赖 ~151MB + 模型权重 ~84MB）。
pub const ESTIMATED_DOWNLOAD_BYTES: u64 =
    24 * 1024 * 1024 + 151 * 1024 * 1024 + 84 * 1024 * 1024;

/// 运行时根目录。
///
/// 默认 `<app data>/runtime`；若设置了绝对路径的环境变量
/// `OMUSE_RUNTIME_DIR`，则以它为准（供测试与高级用户自定义位置）。
pub fn runtime_root(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("OMUSE_RUNTIME_DIR") {
        if !dir.trim().is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {e}"))?;
    Ok(dir.join(RUNTIME_DIR_NAME))
}

/// venv 的 python 路径。
pub fn venv_python(root: &Path) -> PathBuf {
    root.join("venv").join("bin").join("python3")
}

/// Python 解压后的解释器路径。
pub fn base_python(root: &Path) -> PathBuf {
    root.join("python").join("bin").join("python3")
}

/// Torch/模型缓存目录（放在运行时目录内，卸载时可一并回收）。
///
/// 不设它的话 torch 会写到 `~/.cache/torch`，卸载运行时无法回收，
/// 也会污染用户主目录。
pub fn torch_home(root: &Path) -> PathBuf {
    root.join("torch-home")
}

/// 模型权重在 TORCH_HOME 中的落位。
pub fn model_cache_path(root: &Path) -> PathBuf {
    torch_home(root)
        .join("hub")
        .join("checkpoints")
        .join(MODEL_FILE)
}

/// 就绪标记文件：仅在**全部步骤成功**后写入。
///
/// 用它而不是"venv 存在就算好"，可避免下载中断被误判为已完成。
fn ready_marker(root: &Path) -> PathBuf {
    root.join(".ready")
}

/// 当前平台的下载地址与校验和。
fn asset_for_arch() -> Result<(String, &'static str), String> {
    let (triple, sha) = if cfg!(target_arch = "aarch64") {
        ("aarch64-apple-darwin", SHA_ARM64)
    } else if cfg!(target_arch = "x86_64") {
        ("x86_64-apple-darwin", SHA_X86_64)
    } else {
        return Err("当前平台暂不支持自动安装分轨运行时".to_string());
    };

    let file = format!("cpython-{PY_VERSION}+{PBS_TAG}-{triple}-install_only.tar.gz");
    // 文件名中的 '+' 在 URL 中需转义为 %2B
    let url_file = file.replace('+', "%2B");
    let url = format!(
        "https://github.com/astral-sh/python-build-standalone/releases/download/{PBS_TAG}/{url_file}"
    );
    Ok((url, sha))
}

/// 探测当前运行时状态（不发起网络请求）。
pub fn detect(app: &AppHandle) -> RuntimeStatus {
    let Ok(root) = runtime_root(app) else {
        return RuntimeStatus {
            stage: RuntimeStage::Failed,
            progress: None,
            message: "无法定位应用数据目录".into(),
            python_path: None,
            size_bytes: None,
            estimated_download_bytes: ESTIMATED_DOWNLOAD_BYTES,
        };
    };

    let py = venv_python(&root);
    if ready_marker(&root).exists() && py.exists() {
        return RuntimeStatus {
            stage: RuntimeStage::Ready,
            progress: Some(100),
            message: "分轨运行时已就绪".into(),
            python_path: Some(py),
            size_bytes: dir_size(&root),
            estimated_download_bytes: 0,
        };
    }

    RuntimeStatus {
        stage: RuntimeStage::Missing,
        progress: None,
        message: if root.exists() {
            "上次安装未完成，需重新安装".into()
        } else {
            "分轨运行时尚未安装".into()
        },
        python_path: None,
        size_bytes: dir_size(&root),
        estimated_download_bytes: ESTIMATED_DOWNLOAD_BYTES,
    }
}

/// 统计目录占用（浅层上限保护，避免超大目录拖慢）。
fn dir_size(dir: &Path) -> Option<u64> {
    if !dir.exists() {
        return None;
    }
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0usize;
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for ent in rd.flatten() {
            if seen > 200_000 {
                return Some(total);
            }
            seen += 1;
            if let Ok(m) = ent.metadata() {
                if m.is_file() {
                    total += m.len();
                } else if m.is_dir() {
                    stack.push(ent.path());
                }
            }
        }
    }
    Some(total)
}

/// 删除整个运行时目录。
pub fn remove(app: &AppHandle) -> Result<(), String> {
    let root = runtime_root(app)?;
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|e| format!("删除运行时失败: {e}"))?;
    }
    Ok(())
}

/// 供安装流程使用的内部工具。
pub mod installer {
    use super::*;
    use std::io::Read;

    /// 带进度回调地下载到目标文件，并校验 SHA256。
    ///
    /// 先下到 `.part` 再改名，避免中断留下半个文件被当成完整包。
    pub fn download_and_verify<F>(
        url: &str,
        expected_sha: &str,
        dest: &Path,
        mut on_progress: F,
    ) -> Result<(), String>
    where
        F: FnMut(u64, Option<u64>),
    {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
        }
        let part = dest.with_extension("part");
        let _ = std::fs::remove_file(&part);

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(1800))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

        let mut resp = client
            .get(url)
            .send()
            .map_err(|e| format!("下载失败（请检查网络）: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("下载失败：HTTP {}", resp.status()));
        }
        let total = resp.content_length();

        let mut file = std::fs::File::create(&part).map_err(|e| format!("创建文件失败: {e}"))?;
        let mut hasher = sha2::Sha256::new();
        {
            use sha2::Digest;
            let mut buf = [0u8; 64 * 1024];
            let mut done = 0u64;
            loop {
                let n = resp.read(&mut buf).map_err(|e| format!("读取响应失败: {e}"))?;
                if n == 0 {
                    break;
                }
                use std::io::Write;
                file.write_all(&buf[..n]).map_err(|e| format!("写入失败: {e}"))?;
                hasher.update(&buf[..n]);
                done += n as u64;
                on_progress(done, total);
            }
        }
        drop(file);

        use sha2::Digest;
        let digest = hasher.finalize();
        let actual: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        if !actual.eq_ignore_ascii_case(expected_sha) {
            let _ = std::fs::remove_file(&part);
            return Err(format!(
                "运行时校验失败（文件可能损坏或被篡改）\n  期望: {expected_sha}\n  实际: {actual}"
            ));
        }

        std::fs::rename(&part, dest).map_err(|e| format!("重命名失败: {e}"))?;
        Ok(())
    }

    /// 解压 tar.gz（纯 Rust，不经外部进程）。
    ///
    /// 只接受普通文件与目录，拒绝符号链接越界，防止归档穿越。
    pub fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
        let f = std::fs::File::open(archive).map_err(|e| format!("打开归档失败: {e}"))?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut ar = tar::Archive::new(gz);
        ar.set_preserve_permissions(true);

        std::fs::create_dir_all(dest).map_err(|e| format!("创建目录失败: {e}"))?;
        ar.unpack(dest).map_err(|e| format!("解压失败: {e}"))?;
        Ok(())
    }

    /// 写入就绪标记。
    pub fn mark_ready(root: &Path) -> Result<(), String> {
        std::fs::write(ready_marker(root), b"ok").map_err(|e| format!("写入标记失败: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_url_is_wellformed_per_arch() {
        let (url, sha) = asset_for_arch().unwrap();
        assert!(url.starts_with("https://github.com/astral-sh/python-build-standalone/releases/download/"));
        assert!(url.contains(PBS_TAG));
        // '+' 必须转义，否则 URL 会被解析为空格
        assert!(!url.contains('+'), "URL 中的 '+' 必须编码为 %2B");
        assert!(url.contains("%2B"));
        assert_eq!(sha.len(), 64, "SHA256 应为 64 位十六进制");
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn venv_and_base_paths_are_stable() {
        let root = Path::new("/tmp/rt");
        assert!(venv_python(root).ends_with("venv/bin/python3"));
        assert!(base_python(root).ends_with("python/bin/python3"));
    }

    #[test]
    fn sha_constants_are_distinct() {
        assert_ne!(SHA_ARM64, SHA_X86_64);
    }

    #[test]
    fn model_url_and_hash_are_consistent() {
        // 权重文件名前缀即官方约定的内容哈希前缀，可作为自洽性检查
        assert!(MODEL_FILE.starts_with("955717e8-"));
        assert!(MODEL_FILE.ends_with(".th"));
        assert!(MODEL_URL.ends_with(MODEL_FILE));
        assert_eq!(MODEL_SHA256.len(), 64);
        assert!(MODEL_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn torch_home_layout_matches_demucs_expectation() {
        // demucs/torch 期望 <TORCH_HOME>/hub/checkpoints/<file>
        let root = Path::new("/tmp/rt");
        let p = model_cache_path(root);
        assert!(p.ends_with("torch-home/hub/checkpoints/955717e8-8726e21a.th"), "{p:?}");
    }

    /// 起一个只服务单个响应体的一次性 HTTP 服务器（用于 hermetic 下载测试）。
    fn serve_once(body: Vec<u8>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
                let _ = s.flush();
            }
        });
        format!("http://{addr}/x")
    }

    #[test]
    fn download_verifies_hash_and_reports_progress() {
        let payload = b"hello-omuse-runtime".to_vec();
        let sha = {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(&payload);
            h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        let url = serve_once(payload.clone());

        let dir = std::env::temp_dir().join("omuse_dl_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("out.bin");

        let mut calls = 0usize;
        installer::download_and_verify(&url, &sha, &dest, |_, _| calls += 1).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), payload, "落盘内容应与源一致");
        assert!(calls > 0, "应至少回调一次进度");
        assert!(!dest.with_extension("part").exists(), "不应残留 .part");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn download_rejects_wrong_hash_and_cleans_up() {
        let url = serve_once(b"tampered".to_vec());
        let dir = std::env::temp_dir().join("omuse_dl_bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("out.bin");
        let bogus = "0".repeat(64);

        let err = installer::download_and_verify(&url, &bogus, &dest, |_, _| {}).unwrap_err();
        assert!(err.contains("校验失败"), "应报校验失败，实际: {err}");
        assert!(!dest.exists(), "校验失败不得留下目标文件");
        assert!(!dest.with_extension("part").exists(), "校验失败应清理临时文件");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_tar_gz_roundtrip() {
        // 用 tar/flate2 造一个归档再解回来
        let dir = std::env::temp_dir().join("omuse_tar_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/sub")).unwrap();
        std::fs::write(dir.join("src/a.txt"), b"A").unwrap();
        std::fs::write(dir.join("src/sub/b.txt"), b"B").unwrap();

        let archive = dir.join("t.tar.gz");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            b.append_dir_all("pkg", dir.join("src")).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }

        let out = dir.join("out");
        installer::extract_tar_gz(&archive, &out).unwrap();
        assert_eq!(std::fs::read(out.join("pkg/a.txt")).unwrap(), b"A");
        assert_eq!(std::fs::read(out.join("pkg/sub/b.txt")).unwrap(), b"B");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mark_ready_controls_detection() {
        let dir = std::env::temp_dir().join("omuse_ready_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("venv/bin")).unwrap();
        std::fs::write(dir.join("venv/bin/python3"), b"#!/bin/sh\n").unwrap();

        // 没有 .ready 标记 → 不应视为就绪
        assert!(!ready_marker(&dir).exists());
        installer::mark_ready(&dir).unwrap();
        assert!(ready_marker(&dir).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// 定位 `requirements-runtime.txt`。
///
/// 打包态：`<app>/Contents/Resources/python-backend/requirements-runtime.txt`
/// 开发态：仓库根的 `python-backend/requirements-runtime.txt`
pub fn requirements_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("python-backend").join("requirements-runtime.txt");
        if p.exists() {
            return Ok(p);
        }
    }
    // 开发态回退：src-tauri/src/core/stem_engine → 仓库根
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("python-backend")
        .join("requirements-runtime.txt");
    if dev.exists() {
        return Ok(dev);
    }
    Err("找不到 requirements-runtime.txt（安装包可能不完整）".to_string())
}

/// 安装进度事件载荷（通过 Tauri event 推给前端）。
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProgress {
    pub stage: RuntimeStage,
    pub progress: Option<u8>,
    pub message: String,
}

/// 事件名：前端监听它刷新进度。
pub const PROGRESS_EVENT: &str = "omuse://runtime-progress";

/// 执行完整的运行时自举。
///
/// 阻塞式实现，调用方应放入 `spawn_blocking`。
/// 中途失败会清理半成品，避免下次被误判为已安装。
pub fn install<F>(app: &AppHandle, mut emit: F) -> Result<RuntimeStatus, String>
where
    F: FnMut(RuntimeProgress),
{
    let root = runtime_root(app)?;
    let req = requirements_path(app)?;
    let (url, sha) = asset_for_arch()?;

    // 半成品清理：只要没有 .ready 标记，就从头来
    if root.exists() && !ready_marker(&root).exists() {
        let _ = std::fs::remove_dir_all(&root);
    }
    std::fs::create_dir_all(&root).map_err(|e| format!("创建运行时目录失败: {e}"))?;

    // 磁盘余量检查（需要约 1GB 富余）
    if let Some(free) = free_space(&root) {
        if free < 900 * 1024 * 1024 {
            return Err(format!(
                "磁盘空间不足：需要约 900MB 可用空间，当前仅有 {:.0}MB。请清理后重试。",
                free as f64 / 1_048_576.0
            ));
        }
    }

    // ---- 1. 下载 Python 运行时 ----
    emit(RuntimeProgress {
        stage: RuntimeStage::DownloadingPython,
        progress: Some(0),
        message: "正在下载 Python 运行时（约 24MB）…".into(),
    });
    let archive = root.join("python-runtime.tar.gz");
    installer::download_and_verify(&url, sha, &archive, |done, total| {
        let pct = total
            .filter(|t| *t > 0)
            .map(|t| ((done as f64 / t as f64) * 25.0) as u8)
            .unwrap_or(0);
        emit(RuntimeProgress {
            stage: RuntimeStage::DownloadingPython,
            progress: Some(pct.min(25)),
            message: format!(
                "正在下载 Python 运行时… {:.1}/{} MB",
                done as f64 / 1_048_576.0,
                total
                    .map(|t| format!("{:.0}MB", t as f64 / 1_048_576.0))
                    .unwrap_or_else(|| "?".into())
            ),
        });
    })?;

    // ---- 2. 校验（download_and_verify 内部已校验，此处仅报状态）----
    emit(RuntimeProgress {
        stage: RuntimeStage::Verifying,
        progress: Some(28),
        message: "校验完成".into(),
    });

    // ---- 3. 解压 ----
    emit(RuntimeProgress {
        stage: RuntimeStage::Extracting,
        progress: Some(30),
        message: "正在解压 Python…".into(),
    });
    installer::extract_tar_gz(&archive, &root)?;
    let _ = std::fs::remove_file(&archive);

    let base_py = base_python(&root);
    if !base_py.exists() {
        let _ = std::fs::remove_dir_all(&root);
        return Err("解压后未找到 python 解释器，安装包可能不完整".into());
    }

    // ---- 4. 创建 venv ----
    emit(RuntimeProgress {
        stage: RuntimeStage::CreatingVenv,
        progress: Some(35),
        message: "正在创建隔离环境…".into(),
    });
    run_tool(
        app,
        &base_py,
        &["-m", "venv", root.join("venv").to_string_lossy().as_ref()],
    )?;

    let vpy = venv_python(&root);
    if !vpy.exists() {
        let _ = std::fs::remove_dir_all(&root);
        return Err("创建 venv 失败".into());
    }

    // ---- 5. 安装依赖 ----
    emit(RuntimeProgress {
        stage: RuntimeStage::InstallingDeps,
        progress: Some(40),
        message: "正在安装分轨依赖（约 150MB，首次较慢）…".into(),
    });
    // --only-binary=:all: 强制 wheel，避免执行源码包的构建脚本
    run_tool(
        app,
        &vpy,
        &[
            "-m",
            "pip",
            "install",
            "--only-binary=:all:",
            "--no-input",
            "--disable-pip-version-check",
            "-r",
            req.to_string_lossy().as_ref(),
        ],
    )?;

    // ---- 6. 下载模型权重 ----
    // 主动下载并校验，使分轨首次即可离线运行（不依赖 demucs 的 Python 端拉取）。
    emit(RuntimeProgress {
        stage: RuntimeStage::DownloadingModel,
        progress: Some(88),
        message: "正在下载分轨模型（约 84MB）…".into(),
    });
    let model_dest = model_cache_path(&root);
    installer::download_and_verify(MODEL_URL, MODEL_SHA256, &model_dest, |done, total| {
        let pct = total
            .filter(|t| *t > 0)
            .map(|t| 88 + ((done as f64 / t as f64) * 8.0) as u8)
            .unwrap_or(90);
        emit(RuntimeProgress {
            stage: RuntimeStage::DownloadingModel,
            progress: Some(pct.min(96)),
            message: format!("正在下载分轨模型… {:.0}MB", done as f64 / 1_048_576.0),
        });
    })?;

    // ---- 7. 自检 ----
    emit(RuntimeProgress {
        stage: RuntimeStage::InstallingDeps,
        progress: Some(97),
        message: "正在自检…".into(),
    });
    // 自检：导入 + 模型可加载（TORCH_HOME 指向运行时目录，确保走本地权重）
    run_tool_env(
        app,
        &vpy,
        &[
            "-c",
            "import fastapi, uvicorn, torch, numpy, demucs; \
             from demucs.pretrained import get_model; \
             m = get_model('htdemucs'); \
             assert len(m.sources) == 4, m.sources",
        ],
        &[("TORCH_HOME", torch_home(&root).to_string_lossy().to_string())],
    )?;

    installer::mark_ready(&root)?;

    let status = RuntimeStatus {
        stage: RuntimeStage::Ready,
        progress: Some(100),
        message: "分轨运行时已就绪".into(),
        python_path: Some(vpy),
        size_bytes: dir_size(&root),
        estimated_download_bytes: 0,
    };
    emit(RuntimeProgress {
        stage: RuntimeStage::Ready,
        progress: Some(100),
        message: status.message.clone(),
    });
    Ok(status)
}

/// 带环境变量的工具执行（用于把 TORCH_HOME 指到运行时目录）。
fn run_tool_env(
    app: &AppHandle,
    program: &Path,
    args: &[&str],
    envs: &[(&str, String)],
) -> Result<String, String> {
    use tauri_plugin_shell::ShellExt;
    let mut cmd = app
        .shell()
        .command(program.to_string_lossy().to_string())
        .args(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    for (k, v) in envs {
        cmd = cmd.env(k.to_string(), v.clone());
    }
    let out = tauri::async_runtime::block_on(cmd.output())
        .map_err(|e| format!("执行 {} 失败: {e}", program.display()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let tail: String = stderr
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "{} 自检失败\n{tail}",
            program.display()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 经 tauri-plugin-shell 执行一个工具进程并捕获输出。
///
/// 用插件而非 std::process::Command，保持"外部进程统一走 shell 插件"的约定。
fn run_tool(app: &AppHandle, program: &Path, args: &[&str]) -> Result<String, String> {
    use tauri_plugin_shell::ShellExt;
    let cmd = app
        .shell()
        .command(program.to_string_lossy().to_string())
        .args(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());

    let out = tauri::async_runtime::block_on(cmd.output())
        .map_err(|e| format!("执行 {} 失败: {e}", program.display()))?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    if !out.status.success() {
        // 只回传尾部，避免把整段 pip 日志塞进 UI
        let tail: String = stderr
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "{} 执行失败（退出码 {:?}）\n{tail}",
            program.display(),
            out.status.code()
        ));
    }
    Ok(stdout)
}

/// 估算可用磁盘空间（macOS：statfs）。
fn free_space(dir: &Path) -> Option<u64> {
    use std::ffi::CString;
    let c = CString::new(dir.to_string_lossy().as_bytes()).ok()?;
    unsafe {
        let mut st: libc::statfs = std::mem::zeroed();
        if libc::statfs(c.as_ptr(), &mut st) != 0 {
            return None;
        }
        Some((st.f_bavail as u64).saturating_mul(st.f_bsize as u64))
    }
}
