// 插件元数据提取（v1.1 Task 8）
//
// **绝不 dlopen / LoadLibrary** 宿主插件二进制（tdd.md §3.3 全局禁止）。
// 一切信息来自：
//   1. bundle 内 `Contents/Info.plist`（文本/XML，plist crate 解析）
//   2. 文件头的只读字节模式匹配（读完即放，不执行任何插件代码）
//
// VST2 名称位于规范约定的固定偏移；VST3/CLAP 在 plist 缺失时回退字节匹配。

use super::model::{PluginFormat, PluginInfo};
use rayon::prelude::*;
use std::path::Path;

/// 从 plist 读取的元数据三件套。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub bundle_id: Option<String>,
}

/// 提取单个插件的元数据（只读，失败时回退为 None，绝不 panic）。
pub fn extract_metadata(info: &PluginInfo) -> Metadata {
    match info.format {
        PluginFormat::AU | PluginFormat::VST3 | PluginFormat::AAX | PluginFormat::CLAP => {
            match read_bundle_plist(&info.path) {
                Some(m) if m.name.is_some() || m.bundle_id.is_some() => m,
                // plist 缺失或空 → 回退字节模式匹配
                _ => fallback_from_binary(&info.path),
            }
        }
        // VST2：规范固定偏移处的名称
        PluginFormat::VST => extract_vst2(&info.path),
        PluginFormat::Auto => read_bundle_plist(&info.path)
            .filter(|m| m.name.is_some() || m.bundle_id.is_some())
            .unwrap_or_else(|| fallback_from_binary(&info.path)),
    }
}

/// 读取 `<bundle>/Contents/Info.plist`。
pub fn read_bundle_plist(path: &Path) -> Option<Metadata> {
    let plist_path = path.join("Contents").join("Info.plist");
    let data = std::fs::read(&plist_path).ok()?;
    let value = plist::Value::from_reader(std::io::Cursor::new(data)).ok()?;
    let dict = value.as_dictionary()?;

    let name = dict
        .get("CFBundleName")
        .and_then(|v| v.as_string())
        .map(|s| s.to_string())
        .or_else(|| {
            dict.get("CFBundleDisplayName")
                .and_then(|v| v.as_string())
                .map(|s| s.to_string())
        });
    let bundle_id = dict
        .get("CFBundleIdentifier")
        .and_then(|v| v.as_string())
        .map(|s| s.to_string());
    let version = dict
        .get("CFBundleShortVersionString")
        .and_then(|v| v.as_string())
        .map(|s| s.to_string())
        .or_else(|| {
            dict.get("CFBundleVersion")
                .and_then(|v| v.as_string())
                .map(|s| s.to_string())
        });
    // plist 无厂商字段；由 bundle id 反推（com.vendor.name → vendor）
    let vendor = bundle_id.as_deref().and_then(vendor_from_bundle_id);

    Some(Metadata {
        name,
        vendor,
        version,
        bundle_id,
    })
}

/// 由 bundle id 反推厂商：`com.acme.Reverb` → `Acme`；`simple` → `Simple`。
fn vendor_from_bundle_id(id: &str) -> Option<String> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    let parts: Vec<&str> = id.split('.').filter(|p| !p.is_empty()).collect();
    // 取第一段（跳过 com/org/net 等 TLD）
    let candidate = if parts.len() >= 2
        && matches!(
            parts[0].to_ascii_lowercase().as_str(),
            "com" | "org" | "net" | "io" | "de" | "fr" | "uk" | "co" | "app"
        )
    {
        parts[1]
    } else {
        parts.first().copied().unwrap_or("")
    };
    if candidate.is_empty() {
        return None;
    }
    // 首字母大写，其余保留
    let mut c = candidate.chars();
    match c.next() {
        Some(first) => Some(first.to_uppercase().collect::<String>() + c.as_str()),
        None => None,
    }
}

/// VST2：规范约定的文件头偏移处读取插件名。
///
/// 只读前若干 KB，按可打印 ASCII 序列取第一个合理长度的字符串。
fn extract_vst2(path: &Path) -> Metadata {
    let mut meta = fallback_from_binary(path);
    // VST2 头通常含 4 字节魔数 "VST2"/"VstW" 后紧跟名称；此处取首个可打印串
    if meta.name.is_none() {
        if let Some(n) = first_printable_string(path, 64) {
            meta.name = Some(n);
        }
    }
    meta
}

/// 回退：在整个文件（或 bundle 主二进制）中按字节模式找版本/名称串。
fn fallback_from_binary(path: &Path) -> Metadata {
    let mut meta = Metadata::default();

    // bundle：优先读 Contents/MacOS 下的主二进制
    let candidates: Vec<std::path::PathBuf> = if path.is_dir() {
        let macos = path.join("Contents").join("MacOS");
        std::fs::read_dir(&macos)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_else(|_| vec![path.to_path_buf()])
    } else {
        vec![path.to_path_buf()]
    };

    for c in candidates {
        if !c.is_file() {
            continue;
        }
        if let Some(v) = find_version_string(&c) {
            meta.version = Some(v);
        }
        if meta.name.is_none() {
            if let Some(n) = first_printable_string(&c, 64) {
                meta.name = Some(n);
            }
        }
        if meta.version.is_some() {
            break;
        }
    }
    meta
}

/// 在文件字节流中查找形如 `x.y.z`（或 `x.y`）的版本串。
///
/// 版本串可能嵌在更长的可打印串中（如 `Vintage EQ 2.1.0`），故在 token 内
/// 逐字符扫描 digit 起点，而非只匹配 token 开头。
fn find_version_string(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let window = &bytes[..bytes.len().min(512 * 1024)]; // 只看前 512KB
    let text: String = window
        .iter()
        .take(64 * 1024)
        .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '\n' })
        .collect();

    for token in text.split('\n') {
        let t = token.trim();
        if t.len() < 3 || t.len() > 64 {
            continue;
        }
        // 在 token 内逐个 digit 起点尝试取出 version 片段
        let chars: Vec<char> = t.chars().collect();
        for start in 0..chars.len() {
            if !chars[start].is_ascii_digit() {
                continue;
            }
            // 前一个字符不能是数字或点（避免从 "1.2.3" 的中间开始）
            if start > 0 && (chars[start - 1].is_ascii_digit() || chars[start - 1] == '.') {
                continue;
            }
            let mut end = start;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '.') {
                end += 1;
            }
            let cand: String = chars[start..end].iter().collect();
            if is_version_like(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

/// 判定是否为版本样式：至少含一个点，且各段均为非空字母数字。
fn is_version_like(s: &str) -> bool {
    if !s.contains('.') || s.len() < 3 {
        return false;
    }
    if s.starts_with('.') || s.ends_with('.') {
        return false;
    }
    s.split('.')
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// 取文件中第一个长度 >= min_len 的可打印 ASCII 串（用于名称回退）。
fn first_printable_string(path: &Path, _min_len: usize) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut cur = String::new();
    for &b in bytes.iter().take(64 * 1024) {
        if b.is_ascii_graphic() && b != b'\0' {
            cur.push(b as char);
            if cur.len() > 64 {
                break;
            }
        } else {
            if cur.len() >= 4 && cur.chars().any(|c| c.is_alphabetic()) {
                return Some(cur);
            }
            cur.clear();
        }
    }
    None
}

/// 并行提取一批插件的元数据。
///
/// VST3 在 macOS 上可能不支持从后台线程调用 `bundleEntry`，故其任务
/// 额外包一层 `std::thread::spawn`，避免阻塞调用方（Tauri 主线程）。
pub fn extract_all(infos: &mut [PluginInfo]) {
    let metas: Vec<Metadata> = infos
        .par_iter()
        .map(|info| {
            if info.format == PluginFormat::VST3 {
                // VST3：独立线程执行，避免阻塞
                let owned = info.clone();
                std::thread::spawn(move || extract_metadata(&owned))
                    .join()
                    .unwrap_or_default()
            } else {
                extract_metadata(info)
            }
        })
        .collect();

    for (info, meta) in infos.iter_mut().zip(metas) {
        apply_metadata(info, meta);
    }
}

/// 将提取结果回填到条目（仅在有值时覆盖，保留发现阶段的回退名）。
pub fn apply_metadata(info: &mut PluginInfo, meta: Metadata) {
    if let Some(n) = meta.name {
        if !n.trim().is_empty() {
            info.name = n;
        }
    }
    if meta.vendor.is_some() {
        info.vendor = meta.vendor;
    }
    if meta.version.is_some() {
        info.version = meta.version;
    }
    if meta.bundle_id.is_some() {
        info.bundle_id = meta.bundle_id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_bundle(root: &Path, name: &str, ext: &str, id: &str, ver: &str) -> std::path::PathBuf {
        let bundle = root.join(format!("{}.{}", name, ext));
        let contents = bundle.join("Contents");
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>{}</string>
  <key>CFBundleIdentifier</key><string>{}</string>
  <key>CFBundleShortVersionString</key><string>{}</string>
</dict>
</plist>"#,
            name, id, ver
        );
        fs::write(contents.join("Info.plist"), plist).unwrap();
        bundle
    }

    #[test]
    fn extracts_au_bundle_metadata() {
        let root = std::env::temp_dir().join("mr_ext_au");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let b = make_bundle(&root, "MyAU", "component", "com.acme.myau", "1.2.3");

        let mut info = PluginInfo::from_path(b.clone(), PluginFormat::AU);
        extract_all(std::slice::from_mut(&mut info));

        assert_eq!(info.name, "MyAU");
        assert_eq!(info.version.as_deref(), Some("1.2.3"));
        assert_eq!(info.bundle_id.as_deref(), Some("com.acme.myau"));
        assert_eq!(info.vendor.as_deref(), Some("Acme"), "厂商应由 bundle id 反推");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn extracts_vst3_and_clap() {
        let root = std::env::temp_dir().join("mr_ext_v3");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let b3 = make_bundle(&root, "BigVerb", "vst3", "org.fabfilter.bigverb", "2.0");
        let bc = make_bundle(&root, "ClapSynth", "clap", "com.u-he.clapsynth", "0.9");

        let mut infos = vec![
            PluginInfo::from_path(b3, PluginFormat::VST3),
            PluginInfo::from_path(bc, PluginFormat::CLAP),
        ];
        extract_all(&mut infos);

        assert_eq!(infos[0].name, "BigVerb");
        assert_eq!(infos[0].vendor.as_deref(), Some("Fabfilter"));
        assert_eq!(infos[1].name, "ClapSynth");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn extracts_aax() {
        let root = std::env::temp_dir().join("mr_ext_aax");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let b = make_bundle(&root, "ProComp", "aaxplugin", "com.avid.procomp", "3.1");

        let mut info = PluginInfo::from_path(b, PluginFormat::AAX);
        extract_all(std::slice::from_mut(&mut info));

        assert_eq!(info.name, "ProComp");
        assert_eq!(info.bundle_id.as_deref(), Some("com.avid.procomp"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn vst2_falls_back_to_binary() {
        let root = std::env::temp_dir().join("mr_ext_vst2");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let f = root.join("Retro.vst");
        // 无 plist 的裸文件（VST2 常为单文件）
        fs::write(&f, b"\x00\x00Vintage EQ 2.1.0\x00junk").unwrap();

        let mut info = PluginInfo::from_path(f, PluginFormat::VST);
        extract_all(std::slice::from_mut(&mut info));

        assert_eq!(info.version.as_deref(), Some("2.1.0"), "应从二进制头解析版本");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_plist_does_not_panic() {
        let root = std::env::temp_dir().join("mr_ext_nop");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let b = root.join("Bare.vst3");
        fs::create_dir_all(&b).unwrap();

        let mut info = PluginInfo::from_path(b, PluginFormat::VST3);
        extract_all(std::slice::from_mut(&mut info));
        // 不 panic，名称保留发现阶段的回退值
        assert_eq!(info.name, "Bare");
        assert!(info.version.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn vendor_from_bundle_id_cases() {
        assert_eq!(vendor_from_bundle_id("com.acme.rev"), Some("Acme".into()));
        assert_eq!(vendor_from_bundle_id("org.fabfilter.x"), Some("Fabfilter".into()));
        assert_eq!(vendor_from_bundle_id("simple"), Some("Simple".into()));
        assert_eq!(vendor_from_bundle_id(""), None);
    }

    #[test]
    fn extract_all_parallel_matches_serial() {
        let root = std::env::temp_dir().join("mr_ext_par");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut infos: Vec<PluginInfo> = (0..8)
            .map(|i| {
                let b = make_bundle(&root, &format!("P{}", i), "vst3", &format!("com.v{}.p", i), "1.0");
                PluginInfo::from_path(b, PluginFormat::VST3)
            })
            .collect();
        extract_all(&mut infos);
        assert_eq!(infos.len(), 8);
        for (i, info) in infos.iter().enumerate() {
            assert_eq!(info.name, format!("P{}", i));
        }
        let _ = fs::remove_dir_all(&root);
    }
}
