// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// 磁盘扫描：在用户给定根目录下广度优先寻找库目录，识别未注册的库。
//
// 预算与安全：
//  - 最大深度 4 层、目录预算 2500 个，避免在大目录树上失控；
//  - 跳过隐藏项与内容子目录（samples / instruments / …），只在库根附近搜索；
//  - 全程只读，绝不修改任何文件。

use crate::core::kontakt::diagnose;
use crate::core::kontakt::model::{Issue, IssueCode, KontaktLibrary, Severity};
use crate::core::kontakt::nicnt;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// 扫描最大深度。
const MAX_DEPTH: usize = 4;
/// 访问目录数量上限。
const DIR_BUDGET: usize = 2500;
/// 计算占用空间时遍历的条目上限（防御性上限，避免超大库拖慢界面）。
const ENTRY_BUDGET: usize = 400_000;

/// 目录黑名单（小写比较）：内容子目录与无关目录，避免下钻。
const BLACKLIST: &[&str] = &[
    "samples",
    "instruments",
    "sounds",
    "data",
    "resource",
    "resources",
    "wav",
    "ncw",
    "waves",
    ".git",
    ".svn",
    "__macosx",
    "$recycle.bin",
    "system volume information",
    "lost+found",
    "node_modules",
];

/// 内容根的特征子目录（小写比较）。
const CONTENT_DIRS: &[&str] = &[
    "instruments",
    "samples",
    "sounds",
    "data",
    "resource",
    "resources",
];

/// 内容根的特征文件扩展名（小写比较）。
const CONTENT_EXTS: &[&str] = &["nki", "nkm", "nkc", "ncw", "wav"];

/// 扫描根目录下的库（含未注册的库）。
///
/// 结果按内容路径去重，并附带诊断信息（未注册的来源缺失会体现在 `issues` 中）。
pub fn scan_libraries(root: &Path) -> Vec<KontaktLibrary> {
    let mut libraries: Vec<KontaktLibrary> = Vec::new();
    if !root.is_dir() {
        return libraries;
    }

    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));

    let mut visited = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        visited += 1;
        if visited > DIR_BUDGET {
            break;
        }

        if let Some(metadata_file) = nicnt::find_metadata_file(&dir) {
            let library = build_library(&dir, &metadata_file);
            let key = library
                .content_path
                .as_deref()
                .map(path_key)
                .unwrap_or_else(|| path_key(&dir));
            if seen_paths.insert(key) {
                libraries.push(library);
            }
        }

        if depth >= MAX_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // 用 file_type 而非 is_dir：不跟随符号链接，避免绕出根目录或形成环。
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let path = entry.path();
            let name = file_name_lower(&path);
            if name.starts_with('.') || BLACKLIST.contains(&name.as_str()) {
                continue;
            }
            queue.push_back((path, depth + 1));
        }
    }

    libraries.sort_by_key(|library| library.name.to_lowercase());
    libraries
}

/// 由库目录与元数据文件构造库条目。
fn build_library(dir: &Path, metadata_file: &Path) -> KontaktLibrary {
    let fallback = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("未命名库")
        .to_string();
    let mut library = KontaktLibrary::new(fallback.clone(), fallback);

    match nicnt::parse_metadata_file(metadata_file) {
        Ok(payload) => {
            library.name = payload.metadata.name.clone();
            library.reg_key = payload.metadata.reg_key.clone();
            library.snpid = payload.metadata.snpid.clone();
            library.visibility = payload.metadata.visibility;
        }
        Err(err) => library.push_issue(Issue::new(
            Severity::Warning,
            IssueCode::MissingProductHints,
            format!("元数据文件 {} 解析失败: {err}", metadata_file.display()),
        )),
    }

    let content_root = resolve_content_root(dir);
    library.content_size_bytes = directory_size(&content_root);
    library.content_path = Some(content_root);

    let derived = diagnose::issues_for(&library);
    for issue in derived {
        library.push_issue(issue);
    }
    library
}

/// 内容根解析：
///  1. 本目录含内容 → 自身；
///  2. 否则父目录含内容且父目录没有元数据文件 → 父目录；
///  3. 都不满足 → 自身。
fn resolve_content_root(dir: &Path) -> PathBuf {
    if dir_has_content(dir) {
        return dir.to_path_buf();
    }
    if let Some(parent) = dir.parent() {
        if dir_has_content(parent) && nicnt::find_metadata_file(parent).is_none() {
            return parent.to_path_buf();
        }
    }
    dir.to_path_buf()
}

/// 目录是否含内容（特征子目录、特征扩展名，或非 `_info.nkx` 的 NKX）。
fn dir_has_content(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = file_name_lower(&path);
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if CONTENT_DIRS.contains(&name.as_str()) {
                return true;
            }
        } else if file_type.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext = ext.to_ascii_lowercase();
                if CONTENT_EXTS.contains(&ext.as_str()) {
                    return true;
                }
                if ext == "nkx" && !name.ends_with("_info.nkx") {
                    return true;
                }
            }
        }
    }
    false
}

/// 递归统计目录占用空间（不跟随符号链接；达到条目上限即返回已统计值）。
pub fn directory_size(dir: &Path) -> Option<u64> {
    if !dir.is_dir() {
        return None;
    }
    let mut total = 0u64;
    let mut count = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            count += 1;
            if count > ENTRY_BUDGET {
                return Some(total);
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    Some(total)
}

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().trim_end_matches('/').to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::kontakt::model::NicntFormSpec;

    fn fixture_root(tag: &str) -> PathBuf {
        let base = std::fs::canonicalize(std::env::temp_dir()).expect("临时目录可用");
        let dir = base.join(format!("omuse_kontakt_scan_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn spec(name: &str) -> NicntFormSpec {
        NicntFormSpec {
            name: name.to_string(),
            reg_key: name.to_string(),
            snpid: format!("S{}", name.len()),
            company: "O Muse".to_string(),
            ..NicntFormSpec::default()
        }
    }

    #[test]
    fn 扫描识别内容根与库名() {
        let root = fixture_root("basic");
        let lib = root.join("Lib");
        std::fs::create_dir_all(lib.join("Instruments")).unwrap();
        std::fs::write(lib.join("Instruments").join("x.nki"), b"nki").unwrap();
        nicnt::write_nicnt(&lib, spec("Lib")).unwrap();

        let found = scan_libraries(&root);
        assert_eq!(found.len(), 1, "应找到一个库: {found:?}");
        assert_eq!(found[0].name, "Lib");
        assert_eq!(found[0].snpid, "S3");
        assert_eq!(found[0].content_path.as_deref(), Some(lib.as_path()));
        // 内容根 = Lib，占用空间应覆盖 Instruments/x.nki
        assert!(found[0].content_size_bytes.unwrap_or(0) >= 3);
        // 未注册：四个来源全为 false
        assert!(!found[0].sources.has_service_center);
        assert!(!found[0].sources.has_installed_product_json);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 扫描跳过黑名单与隐藏目录() {
        let root = fixture_root("skip");
        // 黑名单目录里的 nicnt 不应被发现
        let inside = root.join("Lib").join("Samples");
        std::fs::create_dir_all(&inside).unwrap();
        nicnt::write_nicnt(&inside, spec("Deep")).unwrap();
        // 隐藏目录里的 nicnt 不应被发现
        let hidden = root.join(".hidden");
        std::fs::create_dir_all(&hidden).unwrap();
        nicnt::write_nicnt(&hidden, spec("Hidden")).unwrap();

        let found = scan_libraries(&root);
        assert!(found.is_empty(), "黑名单与隐藏目录都应跳过: {found:?}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 扫描在父目录解析内容根() {
        let root = fixture_root("parent");
        // 元数据放在子目录 Content，内容在父目录 Foo
        let foo = root.join("Foo");
        let content = foo.join("Content");
        std::fs::create_dir_all(&content).unwrap();
        nicnt::write_nicnt(&content, spec("Foo")).unwrap();
        std::fs::create_dir_all(foo.join("Samples")).unwrap();

        let found = scan_libraries(&root);
        assert_eq!(found.len(), 1, "应找到一个库: {found:?}");
        assert_eq!(found[0].name, "Foo");
        assert_eq!(found[0].content_path.as_deref(), Some(foo.as_path()));

        std::fs::remove_dir_all(&root).ok();
    }
}
