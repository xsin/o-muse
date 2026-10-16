// 插件目录自动发现（v1.1 Task 7）
//
// 只按扩展名匹配宿主条目，**不读取任何元数据**（元数据提取属 Task 8），
// 绝不 dlopen / LoadLibrary。根目录不可读/不存在时静默跳过，不抛错。

use super::model::{PluginFormat, PluginInfo, ScanSource};
use crate::core::platform_paths::default_plugin_scan_sources;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;
use walkdir::WalkDir;

/// 递归扫描的最大深度，防止巨型目录树拖垮 UI。
const MAX_DEPTH: usize = 6;

/// 默认扫描源（委托 platform_paths，ADR-0002）。
///
/// 仅含**标准目录**；用户自定义目录由 `custom_sources` 单独持久化。
pub fn default_sources() -> Vec<ScanSource> {
    default_plugin_scan_sources()
        .into_iter()
        .map(|(path, fmt)| ScanSource::standard(path, fmt, true))
        .collect()
}

/// 只含 **VST3** 标准目录的默认扫描源（v1.2）。
///
/// 为什么默认只扫 VST3：VST3 是当前事实标准（且目录最干净），而全格式扫描
/// 要遍历 10+ 个目录并对每个 bundle 递归求和大小；"打开插件管理器"应当是
/// 一次轻量操作。其余格式由前端"格式切换"显式触发（传 `format` 参数）。
pub fn default_sources_vst3() -> Vec<ScanSource> {
    default_sources()
        .into_iter()
        .filter(|s| s.plugin_format == PluginFormat::VST3)
        .collect()
}

/// 在给定扫描源中发现插件宿主条目。
///
/// - 按 `PluginFormat::all_extensions()` 过滤扩展名
/// - 源自带格式时以源格式为准；源为 `Auto` 时由扩展名推断
/// - 结果按路径去重并稳定排序（便于 UI 与测试）
pub fn discover_plugins(sources: &[ScanSource]) -> Vec<PluginInfo> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<PluginInfo> = Vec::new();

    for src in sources {
        if !src.enabled {
            continue;
        }
        for path in walk_source(&src.path, src.recursive) {
            if !seen.insert(path.clone()) {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let Some(fmt) = PluginFormat::from_extension(&ext) else {
                continue;
            };
            // 源指定了具体格式时以源为准（例如 Components/ 下只认 AU）
            let fmt = match src.plugin_format {
                PluginFormat::Auto => fmt,
                f if f == fmt => f,
                f => f,
            };
            out.push(PluginInfo::from_path(path, fmt));
        }
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// 遍历单个目录，返回候选路径（不读元数据）。
fn walk_source(root: &PathBuf, recursive: bool) -> Vec<PathBuf> {
    if !root.is_dir() {
        return Vec::new();
    }
    let walker = if recursive {
        WalkDir::new(root).max_depth(MAX_DEPTH).follow_links(false)
    } else {
        WalkDir::new(root).max_depth(1).follow_links(false)
    };

    walker
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.depth() > 0)
        .map(|e| e.into_path())
        .collect()
}

/// 并行版发现：源数量多（macOS 默认 12 个）时用 rayon 摊平 IO 等待。
///
/// 与 `discover_plugins` 语义一致，仅并发执行。
pub fn discover_plugins_par(sources: &[ScanSource]) -> Vec<PluginInfo> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<PluginInfo> = Vec::new();

    let per_source: Vec<Vec<PluginInfo>> = sources
        .par_iter()
        .filter(|s| s.enabled)
        .map(|src| {
            let mut v = Vec::new();
            for path in walk_source(&src.path, src.recursive) {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let Some(fmt) = PluginFormat::from_extension(&ext) else {
                    continue;
                };
                let fmt = match src.plugin_format {
                    PluginFormat::Auto => fmt,
                    f => f,
                };
                v.push(PluginInfo::from_path(path, fmt));
            }
            v
        })
        .collect();

    for v in per_source {
        for info in v {
            if seen.insert(info.path.clone()) {
                out.push(info);
            }
        }
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn default_sources_covers_all_formats() {
        let srcs = default_sources();
        assert!(srcs.len() >= 8, "标准源应 >= 8（Task 10 断言）");
        let fmts: HashSet<PluginFormat> = srcs.iter().map(|s| s.plugin_format).collect();
        for f in [PluginFormat::AU, PluginFormat::VST, PluginFormat::VST3, PluginFormat::AAX, PluginFormat::CLAP] {
            assert!(fmts.contains(&f), "缺少格式 {:?}", f);
        }
        // 标准源全部非自定义
        assert!(srcs.iter().all(|s| !s.is_custom));
        // AAX 应指向 Avid 目录
        let aax: Vec<_> = srcs
            .iter()
            .filter(|s| s.plugin_format == PluginFormat::AAX)
            .collect();
        assert_eq!(aax.len(), 1, "AAX 仅系统级 Avid 目录");
        assert!(aax[0]
            .path
            .to_string_lossy()
            .contains("Avid/Audio/Plug-Ins"));
    }

    #[test]
    fn default_sources_vst3_only_contains_vst3() {
        let srcs = default_sources_vst3();
        assert!(!srcs.is_empty(), "VST3 默认源不应为空");
        assert!(
            srcs.iter().all(|s| s.plugin_format == PluginFormat::VST3),
            "默认源只应含 VST3"
        );
        // 全部来自标准源（非自定义）
        assert!(srcs.iter().all(|s| !s.is_custom));
        // 至少覆盖系统级 /Library/Audio/Plug-Ins/VST3
        assert!(srcs
            .iter()
            .any(|s| s.path == PathBuf::from("/Library/Audio/Plug-Ins/VST3")));
    }

    #[test]
    fn discovers_by_extension() {
        let root = std::env::temp_dir().join("mr_disc_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub")).unwrap();
        for n in ["a.component", "b.vst", "c.vst3", "d.aaxplugin", "e.clap"] {
            fs::write(root.join(n), b"x").unwrap();
        }
        fs::write(root.join("ignore.txt"), b"x").unwrap();
        fs::write(root.join("sub/nested.vst3"), b"x").unwrap();

        let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
        let found = discover_plugins(&srcs);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(found.len(), 6, "5 个插件 + 1 个嵌套，txt 应被忽略: {:?}", names);
        assert!(names.contains(&"a.component".to_string()));
        assert!(names.contains(&"nested.vst3".to_string()));
        assert!(!names.iter().any(|n| n.ends_with(".txt")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn non_recursive_skips_subdirs() {
        let root = std::env::temp_dir().join("mr_disc_flat");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("top.vst3"), b"x").unwrap();
        fs::write(root.join("sub/deep.vst3"), b"x").unwrap();

        let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, false)];
        let found = discover_plugins(&srcs);
        assert_eq!(found.len(), 1, "非递归应只命中顶层");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_source_dir_is_ignored() {
        let srcs = vec![ScanSource::standard(
            PathBuf::from("/definitely/not/here/mr"),
            PluginFormat::Auto,
            true,
        )];
        assert!(discover_plugins(&srcs).is_empty());
    }

    #[test]
    fn disabled_source_is_skipped() {
        let root = std::env::temp_dir().join("mr_disc_disabled");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("x.vst3"), b"x").unwrap();

        let mut s = ScanSource::standard(root.clone(), PluginFormat::Auto, true);
        s.enabled = false;
        assert!(discover_plugins(&[s]).is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn par_matches_serial() {
        let root = std::env::temp_dir().join("mr_disc_par");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for i in 0..12 {
            fs::write(root.join(format!("p{}.vst3", i)), b"x").unwrap();
        }
        let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
        let a = discover_plugins(&srcs);
        let b = discover_plugins_par(&srcs);
        assert_eq!(a.len(), b.len());
        assert_eq!(a.len(), 12);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn format_extension_roundtrip() {
        for f in [PluginFormat::AU, PluginFormat::VST, PluginFormat::VST3, PluginFormat::AAX, PluginFormat::CLAP] {
            assert_eq!(PluginFormat::from_extension(f.extension()), Some(f));
        }
        assert_eq!(PluginFormat::from_extension("COMPONENT"), Some(PluginFormat::AU));
        assert_eq!(PluginFormat::from_extension("dll"), None);
    }
}
