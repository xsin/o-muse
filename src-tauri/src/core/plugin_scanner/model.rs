// 插件扫描数据模型（v1.1 Task 7）
//
// 自研实现。仅做"文件发现 + 元数据读取"，绝不 dlopen / LoadLibrary
// 宿主插件二进制（tdd.md §3.3 全局禁止）。

use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// 支持的插件格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PluginFormat {
    /// AudioUnit（.component）
    AU,
    /// VST2（.vst）
    VST,
    /// VST3（.vst3）
    VST3,
    /// AAX（.aaxplugin）
    AAX,
    /// CLAP（.clap）
    CLAP,
    /// 未知/未识别
    Auto,
}

impl PluginFormat {
    /// 该格式对应的宿主文件扩展名（小写，无点）。
    pub fn extension(self) -> &'static str {
        match self {
            PluginFormat::AU => "component",
            PluginFormat::VST => "vst",
            PluginFormat::VST3 => "vst3",
            PluginFormat::AAX => "aaxplugin",
            PluginFormat::CLAP => "clap",
            PluginFormat::Auto => "",
        }
    }

    /// 由扩展名推断格式（不含点，大小写不敏感）。
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "component" => Some(PluginFormat::AU),
            "vst" => Some(PluginFormat::VST),
            "vst3" => Some(PluginFormat::VST3),
            "aaxplugin" => Some(PluginFormat::AAX),
            "clap" => Some(PluginFormat::CLAP),
            _ => None,
        }
    }

    /// 已知的全部宿主扩展名（用于快速过滤）。
    pub fn all_extensions() -> &'static [&'static str] {
        &["component", "vst", "vst3", "aaxplugin", "clap"]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PluginFormat::AU => "AU",
            PluginFormat::VST => "VST",
            PluginFormat::VST3 => "VST3",
            PluginFormat::AAX => "AAX",
            PluginFormat::CLAP => "CLAP",
            PluginFormat::Auto => "Auto",
        }
    }
}

/// 一个扫描源（标准目录或用户自定义目录）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanSource {
    pub path: PathBuf,
    pub plugin_format: PluginFormat,
    /// 是否递归子目录
    pub recursive: bool,
    /// 是否启用（禁用后跳过）
    pub enabled: bool,
    /// 是否为用户自定义目录（Pro 专属能力）
    pub is_custom: bool,
}

impl ScanSource {
    /// 构造一个标准（非自定义）扫描源。
    pub fn standard(path: PathBuf, plugin_format: PluginFormat, recursive: bool) -> Self {
        Self {
            path,
            plugin_format,
            recursive,
            enabled: true,
            is_custom: false,
        }
    }

    /// 构造一个用户自定义扫描源（Pro 专属）。
    pub fn custom(path: PathBuf, plugin_format: PluginFormat, recursive: bool) -> Self {
        Self {
            path,
            plugin_format,
            recursive,
            enabled: true,
            is_custom: true,
        }
    }

    /// 判重键：一个目录就是一个源，与其检测出的格式无关。
    ///
    /// 故**仅按路径**判重（去掉结尾斜杠以容忍 `/a/b` 与 `/a/b/`）。
    pub fn key(&self) -> String {
        self.path
            .to_string_lossy()
            .trim_end_matches('/')
            .to_string()
    }
}

/// 一个被发现的插件条目。
///
/// `vendor` / `version` / `bundle_id` 为 `Option`：元数据缺失时为 `None`，
/// 由 UI 统一渲染为 "—"，避免空串与"未知"语义混淆。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginInfo {
    /// 展示名（优先元数据，回退文件名）
    pub name: String,
    /// 厂商名（元数据缺失为 None）
    pub vendor: Option<String>,
    /// 版本号（元数据缺失为 None）
    pub version: Option<String>,
    pub format: PluginFormat,
    pub path: PathBuf,
    /// Bundle identifier（AU/VST3/CLAP 可从 Info.plist 读取）
    pub bundle_id: Option<String>,
    /// 宿主条目最后修改时间（用于缓存失效）
    pub last_modified: DateTime<Utc>,
    /// 宿主条目字节大小（目录则为其递归大小，用于缓存失效）
    pub file_size: u64,
}

impl PluginInfo {
    /// 由文件路径构建一个"仅发现、未读元数据"的条目。
    pub fn from_path(path: PathBuf, format: PluginFormat) -> Self {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let (size, mtime) = path_stats(&path);
        Self {
            name,
            vendor: None,
            version: None,
            format,
            path,
            bundle_id: None,
            last_modified: mtime,
            file_size: size,
        }
    }

    /// 缓存键：路径 + 大小 + 修改时间（任一变化即失效）。
    pub fn cache_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.path.to_string_lossy(),
            self.file_size,
            self.last_modified.timestamp_millis()
        )
    }
}

/// 读取文件/目录的"指纹"：大小 + 修改时间。
///
/// review 修复两点：
///  1. **性能**：此前对目录做最多 2000 次 stat 的递归求和，且发生在
///     **每次发现阶段**（早于缓存查询），导致"全部命中缓存"的重扫仍要付
///     完整遍历代价。改为：文件用自身 size+mtime；bundle 目录用
///     `Contents/Info.plist` 的 size+mtime（元数据真正来源，成本 O(1)）。
///  2. **正确性**：目录自身的 mtime 在**内部文件被修改时不变**，
///     此前会导致 `Info.plist` 改了版本（长度不变）却永远命中旧缓存。
fn path_stats(path: &std::path::Path) -> (u64, DateTime<Utc>) {
    // bundle：以 Contents/Info.plist 作为指纹来源
    let plist = path.join("Contents").join("Info.plist");
    if plist.is_file() {
        if let Ok(m) = std::fs::metadata(&plist) {
            let mtime = m.modified().map(DateTime::<Utc>::from).unwrap_or(DateTime::<Utc>::MIN_UTC);
            return (m.len(), mtime);
        }
    }

    match std::fs::metadata(path) {
        Ok(m) => {
            let mtime = m.modified().map(DateTime::<Utc>::from).unwrap_or(DateTime::<Utc>::MIN_UTC);
            let size = if m.is_file() { m.len() } else { 0 };
            (size, mtime)
        }
        Err(_) => (0, DateTime::<Utc>::MIN_UTC),
    }
}
