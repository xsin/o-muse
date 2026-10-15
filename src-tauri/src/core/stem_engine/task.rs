// 分轨任务模型与 sidecar HTTP 通信（Task 2）
//
// 通过阻塞式 HTTP 客户端与 Python FastAPI 分轨服务交互。所有数据结构
// 均从 API 的 JSON 响应反序列化，不依赖任何第三方项目的私有协议。

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 分轨相关的错误类型（独立定义，未复用任何外部类型）。
#[derive(Debug, Error)]
pub enum Error {
    #[error("sidecar: {0}")]
    Sidecar(String),
    #[error("http: {0}")]
    Http(String),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("failed to build reqwest client")
}

/// `POST /separate` 的响应。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StartResponse {
    pub job_id: String,
}

/// `GET /status/{job_id}` 的响应。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusResponse {
    pub job_id: String,
    /// 状态机：queued | processing | done | error | demo_done
    pub status: String,
    pub progress: f32,
    pub message: Option<String>,
    /// 是否以 demo 模式运行（Demucs 不可用时的回退）。
    pub demo: bool,
    /// 已可用分轨名称列表（如 vocals / drums / bass / other）。
    pub stems: Vec<String>,
}

/// `/stems/{job_id}` 中单个分轨文件条目。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StemFileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
}

/// `GET /stems/{job_id}` 的响应。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StemsResponse {
    pub job_id: String,
    pub path: String,
    pub files: Vec<StemFileEntry>,
}

/// 提交一个分轨任务，返回 job_id。
pub fn start(base: &str, file_path: &str) -> Result<String> {
    let resp = client()
        .post(format!("{base}/separate"))
        .json(&serde_json::json!({ "file_path": file_path }))
        .send()
        .map_err(|e| Error::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(Error::Http(format!("separate returned {}", resp.status())));
    }
    let body: StartResponse = resp.json().map_err(|e| Error::Http(e.to_string()))?;
    Ok(body.job_id)
}

/// 查询任务状态。
pub fn status(base: &str, job_id: &str) -> Result<StatusResponse> {
    let resp = client()
        .get(format!("{base}/status/{job_id}"))
        .send()
        .map_err(|e| Error::Http(e.to_string()))?;
    if resp.status().as_u16() == 404 {
        return Err(Error::NotFound(job_id.to_string()));
    }
    let body: StatusResponse = resp.json().map_err(|e| Error::Http(e.to_string()))?;
    Ok(body)
}

/// 获取已完成任务的分轨文件清单。
pub fn stems(base: &str, job_id: &str) -> Result<StemsResponse> {
    let resp = client()
        .get(format!("{base}/stems/{job_id}"))
        .send()
        .map_err(|e| Error::Http(e.to_string()))?;
    if resp.status().as_u16() == 404 {
        return Err(Error::NotFound(job_id.to_string()));
    }
    let body: StemsResponse = resp.json().map_err(|e| Error::Http(e.to_string()))?;
    Ok(body)
}
