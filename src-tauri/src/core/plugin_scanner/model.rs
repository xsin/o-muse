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

    /// 判重键：同一路径 + 同一格式视为同一源。
    pub fn key(&self) -> String {
        let p = self.path.to_string_lossy().to_string();
        format!("{}|{}", self.plugin_format.as_str(), p.trim_end_matches('/'))
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

/// 读取文件/目录的大小与修改时间。目录则递归累加（上限保护）。
fn path_stats(path: &std::path::Path) -> (u64, DateTime<Utc>) {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return (0, DateTime::<Utc>::MIN_UTC),
    };
    let mtime = meta
        .modified()
        .map(DateTime::<Utc>::from)
        .unwrap_or(DateTime::<Utc>::MIN_UTC);

    let size = if meta.is_file() {
        meta.len()
    } else {
        // 目录：浅层累加，避免超大 bundle 拖慢发现阶段
        dir_size_capped(path, 2000)
    };
    (size, mtime)
}

/// 递归统计目录大小，文件数超过 cap 则提前停止。
fn dir_size_capped(dir: &std::path::Path, cap: usize) -> u64 {
    let mut total = 0u64;
    let mut count = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = match std::fs::read_dir(&d) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for ent in rd.flatten() {
            if count >= cap {
                return total;
            }
            count += 1;
            if let Ok(m) = ent.metadata() {
                if m.is_file() {
                    total += m.len();
                } else if m.is_dir() {
                    stack.push(ent.path());
                }
            }
        }
    }
    total
}
