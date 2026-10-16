// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// 库诊断：把四个注册来源的缺失、可见性、内容路径与重复情况翻译成问题清单。
// 判定本身是纯函数（只做一次 `stat` 级别的探测），不写入任何内容。

use crate::core::kontakt::model::{Issue, IssueCode, KontaktLibrary, Severity};
use std::collections::HashMap;
use std::path::Path;

/// 对整个库集合做诊断：先算重复，再逐条产出问题。
///
/// 已有的问题（例如扫描时记录的文件解析失败）会保留，同 `IssueCode` 只留一条。
pub fn diagnose(libraries: &mut [KontaktLibrary]) {
    let mut snpid_counts: HashMap<String, usize> = HashMap::new();
    let mut path_counts: HashMap<String, usize> = HashMap::new();
    for library in libraries.iter() {
        if !library.snpid.is_empty() {
            *snpid_counts
                .entry(library.snpid.to_lowercase())
                .or_insert(0) += 1;
        }
        if let Some(path) = &library.content_path {
            *path_counts.entry(path_key(path)).or_insert(0) += 1;
        }
    }

    for library in libraries.iter_mut() {
        let mut issues = std::mem::take(&mut library.issues);

        if !library.snpid.is_empty()
            && snpid_counts
                .get(&library.snpid.to_lowercase())
                .copied()
                .unwrap_or(0)
                > 1
        {
            issues.push(Issue::new(
                Severity::Error,
                IssueCode::DuplicateSnpid,
                format!("SNPID {} 被多个库共用，Kontakt 会拒绝加载", library.snpid),
            ));
        }
        if let Some(path) = &library.content_path {
            if path_counts.get(&path_key(path)).copied().unwrap_or(0) > 1 {
                issues.push(Issue::new(
                    Severity::Warning,
                    IssueCode::DuplicatePath,
                    format!("同一内容路径被多个库引用: {}", path.display()),
                ));
            }
        }

        for issue in issues_for(library) {
            if !issues.iter().any(|existing| existing.code == issue.code) {
                issues.push(issue);
            }
        }
        library.issues = issues;
    }
}

/// 单个库的来源/可见性/内容路径判定（不含跨库重复判定）。
pub fn issues_for(library: &KontaktLibrary) -> Vec<Issue> {
    let mut issues = Vec::new();
    let sources = &library.sources;

    if !sources.has_service_center && !sources.has_installed_product_json {
        issues.push(Issue::new(
            Severity::Warning,
            IssueCode::MissingProductHints,
            "缺少 Service Center XML 与 installed_products JSON，Kontakt 无法识别该库",
        ));
    }

    if !sources.has_user_plist && !sources.has_system_plist {
        issues.push(Issue::new(
            Severity::Warning,
            IssueCode::MissingLegacyRegistration,
            "缺少用户级与系统级注册 plist，该库不会出现在 Kontakt 库列表中",
        ));
    }

    if let Some(visibility) = library.visibility {
        if visibility != 3 {
            issues.push(Issue::new(
                Severity::Warning,
                IssueCode::HiddenInKontakt,
                format!("Visibility={visibility}（Kontakt 期望 3），该库在 Kontakt 中被隐藏"),
            ));
        }
    }

    match library.content_path.as_deref() {
        None => issues.push(Issue::new(
            Severity::Error,
            IssueCode::MissingContentPath,
            "注册信息中没有内容根目录，无法定位库文件",
        )),
        Some(path) => {
            if !path.exists() {
                issues.push(Issue::new(
                    Severity::Error,
                    IssueCode::ContentOffline,
                    format!("内容根目录不存在（卷未挂载或库已移动）: {}", path.display()),
                ));
            } else if !is_readable(path) {
                issues.push(Issue::new(
                    Severity::Warning,
                    IssueCode::ContentPermissionDenied,
                    format!("内容根目录不可读: {}", path.display()),
                ));
            }
        }
    }

    if sources.needs_system_write() {
        let mut missing = Vec::new();
        if !sources.has_service_center {
            missing.push("Service Center XML");
        }
        if !sources.has_system_plist {
            missing.push("系统注册 plist");
        }
        issues.push(Issue::new(
            Severity::Warning,
            IssueCode::UnprivilegedWriteRequired,
            format!(
                "缺少{}；该来源位于 /Library，需管理员授权后写入（应用会先列出具体文件与原因）",
                missing.join("、")
            ),
        ));
    }

    issues
}

/// 目录可读性探测：只有明确的权限错误才算不可读，其他错误按可读处理以免误报。
fn is_readable(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(_) => true,
        Err(err) => err.kind() != std::io::ErrorKind::PermissionDenied,
    }
}

/// 路径比较键：大小写不敏感，忽略结尾分隔符。
fn path_key(path: &Path) -> String {
    path.to_string_lossy().trim_end_matches('/').to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::kontakt::model::LibrarySources;
    use std::path::PathBuf;

    fn fixture_root(tag: &str) -> PathBuf {
        let base = std::fs::canonicalize(std::env::temp_dir()).expect("临时目录可用");
        let dir = base.join(format!("omuse_kontakt_diag_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn library(name: &str, snpid: &str, path: PathBuf, visibility: Option<i32>) -> KontaktLibrary {
        KontaktLibrary {
            name: name.to_string(),
            reg_key: name.to_string(),
            snpid: snpid.to_string(),
            content_path: Some(path),
            visibility,
            sources: LibrarySources {
                has_service_center: true,
                has_system_plist: true,
                has_user_plist: true,
                has_installed_product_json: true,
            },
            ..KontaktLibrary::default()
        }
    }

    #[test]
    fn 重复_snpid_被标为_error() {
        let root = fixture_root("dup");
        let a = library("A", "W76", root.join("a"), Some(3));
        let b = library("B", "w76", root.join("b"), Some(3));

        let mut list = vec![a, b];
        diagnose(&mut list);

        for item in &list {
            let issue = item
                .issues
                .iter()
                .find(|i| i.code == IssueCode::DuplicateSnpid)
                .expect("应标出重复 SNPID");
            assert_eq!(issue.severity, Severity::Error);
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn visibility_不为3_被标为_hidden() {
        let root = fixture_root("hidden");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let mut list = vec![library("Hidden", "AAA", root.join("lib"), Some(7))];
        diagnose(&mut list);

        let issue = list[0]
            .issues
            .iter()
            .find(|i| i.code == IssueCode::HiddenInKontakt)
            .expect("应标出隐藏");
        assert_eq!(issue.severity, Severity::Warning);
        assert!(issue.detail.contains("7"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 内容路径缺失或离线分别报错() {
        let root = fixture_root("paths");
        let mut missing = library("NoPath", "A1", root.clone(), Some(3));
        missing.content_path = None;
        let mut offline = library("Offline", "A2", root.join("nope"), Some(3));
        offline.sources.has_service_center = false;
        offline.sources.has_installed_product_json = false;

        let mut list = vec![missing, offline];
        diagnose(&mut list);

        assert!(list[0]
            .issues
            .iter()
            .any(|i| i.code == IssueCode::MissingContentPath && i.severity == Severity::Error));
        assert!(list[1]
            .issues
            .iter()
            .any(|i| i.code == IssueCode::ContentOffline && i.severity == Severity::Error));
        // 缺 Service Center XML => 需要系统级写入
        assert!(list[1]
            .issues
            .iter()
            .any(|i| i.code == IssueCode::UnprivilegedWriteRequired));
        assert!(list[1]
            .issues
            .iter()
            .any(|i| i.code == IssueCode::MissingProductHints));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 全来源齐备且可见路径正常时无问题() {
        let root = fixture_root("clean");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let mut list = vec![library("Clean", "AAA", root.join("lib"), Some(3))];
        diagnose(&mut list);

        assert!(
            list[0].issues.is_empty(),
            "不应有问题: {:?}",
            list[0].issues
        );

        std::fs::remove_dir_all(&root).ok();
    }
}
