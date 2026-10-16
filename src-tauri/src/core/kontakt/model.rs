// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// Kontakt 库管理数据模型。
//
// 本文件只定义数据结构与纯函数判定，不做任何磁盘 IO，
// 便于单元测试直接构造夹具。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 从 `.nicnt` / Service Center XML 解析出的产品元数据。
///
/// 字段名对应 NI `ProductHints` XML 的元素名；缺省字段为 `None`，
/// 以便区分「明确为空」与「不存在」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProductMetadata {
    /// `<Name>`：库显示名。
    pub name: String,
    /// `<RegKey>`：注册键，同时决定注册文件名。
    pub reg_key: String,
    /// `<SNPID>`：NI 产品 ID。
    pub snpid: String,
    /// 生效可见性：优先取 `ProductSpecific/Visibility`（Kontakt 以此判定库是否隐藏）。
    pub visibility: Option<i32>,
    /// `ProductSpecific/HU`：授权哈希，只读解析，绝不伪造。
    pub hu: Option<String>,
    /// `ProductSpecific/JDX`：授权签名，只读解析，绝不伪造。
    pub jdx: Option<String>,
    /// `<UPID>`：产品唯一 ID。
    pub upid: Option<String>,
    /// `<AuthSystem>`：授权体系（如 RAS3）。
    pub auth_system: Option<String>,
    /// `<Company>`：厂商。
    pub company: Option<String>,
    /// `<PoweredBy>`：承载宿主（通常为 Kontakt）。
    pub powered_by: Option<String>,
    /// `<Type>`：产品类型（Content / Plugin / Utility）。
    pub product_type: Option<String>,
    /// `<Icon>`：图标资源名。
    pub icon: Option<String>,
    /// `Relevance` 中 kontakt 条目的 `minVersion`。
    pub minimum_kontakt_version: Option<String>,
    /// `Relevance` 中列出的宿主名（原样保留大小写）。
    pub applications: Vec<String>,
}

impl ProductMetadata {
    /// 是否为 Kontakt 内容库：`<Type>` 明确为 Content，或未标注类型（来自 plist/JSON 的条目）。
    pub fn is_content_library(&self) -> bool {
        match &self.product_type {
            Some(t) => t.eq_ignore_ascii_case("Content"),
            None => true,
        }
    }
}

/// 一个库在四个注册来源中的存在情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LibrarySources {
    /// `/Library/Application Support/Native Instruments/Service Center/<Name>.xml`
    pub has_service_center: bool,
    /// `/Library/Preferences/com.native-instruments.<RegKey>.plist`
    pub has_system_plist: bool,
    /// `~/Library/Preferences/com.native-instruments.<RegKey>.plist`
    pub has_user_plist: bool,
    /// `/Users/Shared/Native Instruments/installed_products/<Name>.json`
    pub has_installed_product_json: bool,
}

impl LibrarySources {
    /// 是否需要向 `/Library` 写入才能补全注册（缺 Service Center XML 或系统 plist）。
    pub fn needs_system_write(&self) -> bool {
        !self.has_service_center || !self.has_system_plist
    }
}

/// 合并四个注册来源后的库条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct KontaktLibrary {
    pub name: String,
    pub reg_key: String,
    pub snpid: String,
    /// 内容根目录（库根），来自 JSON / 系统 plist / 磁盘扫描结果。
    pub content_path: Option<PathBuf>,
    pub visibility: Option<i32>,
    pub sources: LibrarySources,
    pub issues: Vec<Issue>,
    /// 用户级 plist 中的列表序号。
    pub user_list_index: Option<i32>,
    /// 内容根目录总占用字节数（扫描时计算，跨来源合并时可能为 None）。
    pub content_size_bytes: Option<u64>,
}

impl KontaktLibrary {
    /// 以注册键（回退为名称、SNPID）构造空条目。
    pub fn new(name: impl Into<String>, reg_key: impl Into<String>) -> Self {
        let name = name.into();
        let reg_key = reg_key.into();
        Self {
            snpid: String::new(),
            name,
            reg_key,
            ..Self::default()
        }
    }

    /// 追加问题，同一 `IssueCode` 只保留首次出现的那条，避免重复噪声。
    pub fn push_issue(&mut self, issue: Issue) {
        if self.issues.iter().any(|i| i.code == issue.code) {
            return;
        }
        self.issues.push(issue);
    }

    /// 该条目是否含指定级别的问题。
    pub fn has_severity(&self, severity: Severity) -> bool {
        self.issues.iter().any(|i| i.severity == severity)
    }
}

/// 问题级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// 问题编码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueCode {
    /// 缺 Service Center XML 与 installed_products JSON。
    MissingProductHints,
    /// 缺用户级与系统级 plist 注册。
    MissingLegacyRegistration,
    /// 可见性不为 3（Kontakt 中不可见）。
    HiddenInKontakt,
    /// 无法确定内容根目录。
    MissingContentPath,
    /// 内容根目录不存在（含未挂载的卷）。
    ContentOffline,
    /// 内容根目录不可读。
    ContentPermissionDenied,
    /// 同一 SNPID 出现多次（大小写不敏感）。
    DuplicateSnpid,
    /// 同一内容路径被多个库引用（大小写不敏感）。
    DuplicatePath,
    /// 补全注册需要写入 `/Library`，需管理员权限；本应用不会提权。
    UnprivilegedWriteRequired,
}

/// 一条诊断问题。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    pub code: IssueCode,
    pub detail: String,
}

impl Issue {
    pub fn new(severity: Severity, code: IssueCode, detail: impl Into<String>) -> Self {
        Self {
            severity,
            code,
            detail: detail.into(),
        }
    }
}

/// 用户级 plist 内容。
///
/// 真实样本：`{"SNPID":"1c7","Name":"Altera Keys","UserListIndex":0,"RegKey":"Altera Keys","UserRemoved":0}`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserRegistration {
    pub snpid: String,
    pub name: String,
    pub reg_key: String,
    pub user_list_index: Option<i32>,
    /// `UserRemoved` 非 0 表示用户已在 Kontakt 中移除该库。
    pub user_removed: bool,
}

/// 系统级 plist 内容（只读来源）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SystemRegistration {
    pub reg_key: String,
    pub snpid: String,
    pub name: String,
    pub content_dir: Option<PathBuf>,
    pub visibility: Option<i32>,
    pub hu: Option<String>,
    pub jdx: Option<String>,
    pub upid: Option<String>,
    pub auth_system: Option<String>,
}

/// Service Center XML 内容（只读来源）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ServiceCenterRegistration {
    pub metadata: ProductMetadata,
    /// 原始 XML 片段（含 XML 声明），逐字节保留，写入时原样落盘。
    pub raw_xml: String,
}

/// `.nicnt` 创建表单。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NicntFormSpec {
    pub name: String,
    pub reg_key: String,
    pub snpid: String,
    pub company: String,
    pub product_type: String,
    /// 缺省时生成新的 UUID v4。
    pub upid: Option<String>,
    pub auth_system: Option<String>,
    /// 授权哈希；缺省时不写入（绝不伪造）。
    pub hu: Option<String>,
    /// 授权签名；缺省时不写入（绝不伪造）。
    pub jdx: Option<String>,
    pub visibility: i32,
}

impl Default for NicntFormSpec {
    /// 表单默认值：Content 类型、可见性 3（Kontakt 中可见）。
    fn default() -> Self {
        Self {
            name: String::new(),
            reg_key: String::new(),
            snpid: String::new(),
            company: String::new(),
            product_type: "Content".to_string(),
            upid: None,
            auth_system: None,
            hu: None,
            jdx: None,
            visibility: 3,
        }
    }
}
