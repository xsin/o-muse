// Kontakt 库数据模型（Task 3）
//
// 独立定义，未复用任何第三方项目的私有类型。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 一个 Kontakt 库条目的盘点结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryEntry {
    /// 声音库产品 ID（SNPID）；缺失时回退为名称。
    pub snpid: String,
    /// 库显示名称。
    pub name: String,
    /// 库目录路径（来自注册信息或偏好 plist）。
    pub path: Option<PathBuf>,
    /// 是否在 Kontakt 偏好中注册（可见）。
    pub registered: bool,
    /// 诊断信息集合。
    pub diagnostics: Vec<Diagnostic>,
}

impl LibraryEntry {
    pub fn new(snpid: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            snpid: snpid.into(),
            name: name.into(),
            path: None,
            registered: false,
            diagnostics: Vec::new(),
        }
    }
}

/// 盘点中发现的问题。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Diagnostic {
    /// 路径不存在（非卷）。
    MissingPath(PathBuf),
    /// 同一 SNPID 出现多次。
    DuplicateSnpid(String),
    /// 路径位于 /Volumes 但卷不可访问（断卷）。
    DisconnectedVolume(PathBuf),
    /// 缺少必要元数据（如路径）。
    MissingMetadata(String),
}
