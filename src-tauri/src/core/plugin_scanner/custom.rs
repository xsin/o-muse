// 自定义扫描源持久化（v1.1 Task 9）
//
// 自定义目录为 **Pro 专属**（plan §四 Task 9、handsoff §5）。
// 持久化于 tauri-plugin-store；本模块不含任何 Apache-2.0 原始代码。
//
// 注意：`add_custom_source` **不做递归扫描**（plan 明确禁止），
// 递归发现统一在 `scan_all_plugins` 中处理。

use super::model::{PluginFormat, ScanSource};
use std::path::{Path, PathBuf};

/// store 中保存自定义源的键名。
pub const STORE_KEY: &str = "custom_plugin_sources";

/// 序列化：把自定义源列表写入 store（JSON 数组）。
pub fn serialize(sources: &[ScanSource]) -> Result<String, String> {
    serde_json::to_string(sources).map_err(|e| format!("序列化自定义源失败: {e}"))
}

/// 反序列化：从 store 读取自定义源列表（损坏/缺失时返回空列表，不报错）。
pub fn deserialize(raw: Option<&str>) -> Vec<ScanSource> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str(raw).unwrap_or_default()
}

/// 追加一个自定义源；按 `key()` 去重，重复则覆盖。
pub fn upsert(mut sources: Vec<ScanSource>, new: ScanSource) -> Vec<ScanSource> {
    let key = new.key();
    sources.retain(|s| s.key() != key);
    sources.push(new);
    sources
}

/// 移除指定路径的自定义源。
///
/// 与 `upsert` 一样用 `key()` 归一化比较，容忍结尾斜杠差异
/// （否则 `/tmp/plug` 无法移除 `/tmp/plug/`，用户点了没反应）。
pub fn remove(sources: Vec<ScanSource>, path: &Path) -> Vec<ScanSource> {
    let target = normalize_path_key(path);
    sources.into_iter().filter(|s| s.key() != target).collect()
}

/// 路径判重键（与 `ScanSource::key()` 一致：去结尾斜杠）。
pub fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy().trim_end_matches('/').to_string()
}

/// 自动检测目录主要承载的格式：统计各扩展名出现次数，取最多者。
///
/// 仅看一层（不递归），与 plan "不在 add_custom_plugin_dir 中递归扫描"一致。
/// 无法检测时返回 `Auto`。
pub fn detect_format(path: &Path) -> PluginFormat {
    let rd = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => return PluginFormat::Auto,
    };

    let mut counts = [(PluginFormat::AU, 0u32), (PluginFormat::VST, 0u32), (PluginFormat::VST3, 0u32), (PluginFormat::AAX, 0u32), (PluginFormat::CLAP, 0u32)];

    for ent in rd.flatten().take(500) {
        let p = ent.path();
        let ext = match p.extension().and_then(|e| e.to_str()) {
            Some(e) => e.to_ascii_lowercase(),
            None => continue,
        };
        if let Some(f) = PluginFormat::from_extension(&ext) {
            if let Some(slot) = counts.iter_mut().find(|(fmt, _)| *fmt == f) {
                slot.1 += 1;
            }
        }
    }

    counts
        .iter()
        .filter(|(_, c)| *c > 0)
        .max_by_key(|(_, c)| *c)
        .map(|(f, _)| *f)
        .unwrap_or(PluginFormat::Auto)
}

/// 构造一个自定义源（自动检测格式）。
pub fn build_custom(path: PathBuf) -> ScanSource {
    let fmt = detect_format(&path);
    ScanSource::custom(path, fmt, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detect_format_picks_dominant_extension() {
        let root = std::env::temp_dir().join("mr_detect");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for i in 0..3 {
            fs::write(root.join(format!("a{}.vst3", i)), b"x").unwrap();
        }
        fs::write(root.join("b.component"), b"x").unwrap();

        assert_eq!(detect_format(&root), PluginFormat::VST3);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_format_returns_auto_when_empty() {
        let root = std::env::temp_dir().join("mr_detect_empty");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert_eq!(detect_format(&root), PluginFormat::Auto);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_format_missing_dir_is_auto() {
        assert_eq!(
            detect_format(Path::new("/nope/mr/missing")),
            PluginFormat::Auto
        );
    }

    #[test]
    fn upsert_dedupes_by_key() {
        let p = PathBuf::from("/tmp/plug");
        let mut v = Vec::new();
        v = upsert(v, ScanSource::custom(p.clone(), PluginFormat::VST3, true));
        v = upsert(v, ScanSource::custom(p.clone(), PluginFormat::AU, true));
        assert_eq!(v.len(), 1, "同一路径应去重");
        assert_eq!(v[0].plugin_format, PluginFormat::AU, "后者覆盖前者");
    }

    #[test]
    fn remove_by_path_works() {
        let a = PathBuf::from("/tmp/a");
        let b = PathBuf::from("/tmp/b");
        let v = vec![
            ScanSource::custom(a.clone(), PluginFormat::AU, true),
            ScanSource::custom(b.clone(), PluginFormat::VST3, true),
        ];
        let left = remove(v, &a);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].path, b);
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let v = vec![ScanSource::custom(
            PathBuf::from("/tmp/x"),
            PluginFormat::CLAP,
            true,
        )];
        let s = serialize(&v).unwrap();
        let back = deserialize(Some(&s));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].path, PathBuf::from("/tmp/x"));
        assert!(back[0].is_custom);
    }

    #[test]
    fn deserialize_tolerates_garbage() {
        assert!(deserialize(None).is_empty());
        assert!(deserialize(Some("")).is_empty());
        assert!(deserialize(Some("not json")).is_empty());
    }

    #[test]
    fn build_custom_sets_custom_flag() {
        let root = std::env::temp_dir().join("mr_build_custom");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("z.clap"), b"x").unwrap();
        let s = build_custom(root.clone());
        assert!(s.is_custom);
        assert_eq!(s.plugin_format, PluginFormat::CLAP);
        let _ = fs::remove_dir_all(&root);
    }
}
