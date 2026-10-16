// 插件扫描数据模型（v1.1 Task 7 / v1.2 空间管理）
//
// 自研实现。仅做"文件发现 + 元数据读取"，绝不 dlopen / LoadLibrary
// 宿主插件二进制（tdd.md §3.3 全局禁止）。
//
// v1.2 增补：真实占用（`size_bytes`）、安装时间（`installed_at`）、
// 软链状态（`is_symlink` / `real_path` / `is_broken_link`），用于
// "系统盘空间焦虑"场景——让用户看清每个插件多大、能否安全移走。

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 单个 bundle 递归统计大小的条目上限。
///
/// 上限存在的意义：异常目录树（几十万个小文件、巨大资源包）不应该让
/// "打开插件管理器"变成一次不可取消的全盘 stat。达到上限时返回的是
/// **下界**（对超大插件略小于真实占用），换取扫描的可预期耗时。
pub const MAX_SIZE_ENTRIES: usize = 50_000;

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
///
/// 序列化统一为 camelCase（前端 TypeScript 接口一致）：`sizeBytes`、
/// `installedAt`、`isSymlink`、`realPath`、`isBrokenLink`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// 缓存指纹大小：bundle 为 `Contents/Info.plist` 的大小（O(1)，见 [`path_stats`]）。
    ///
    /// **不要**拿它当"占用空间"显示——真实占用看 `size_bytes`。
    pub file_size: u64,
    /// 插件 bundle 的真实占用（递归求和，字节）。
    ///
    /// 软链条目统计的是**真实目标**的占用（软链自身只占几十字节，没有展示价值）。
    pub size_bytes: u64,
    /// 安装/更新时间：取 bundle 内主可执行文件的 mtime（比目录 mtime 可靠）。
    pub installed_at: Option<DateTime<Utc>>,
    /// 该路径本身是否为符号链接
    pub is_symlink: bool,
    /// 若是软链，解析出的真实路径（`canonicalize`）
    pub real_path: Option<PathBuf>,
    /// 软链但目标不存在（失效链接，必须让用户看见）
    pub is_broken_link: bool,
}

impl PluginInfo {
    /// 由文件路径构建一个"仅发现、未读元数据"的条目。
    pub fn from_path(path: PathBuf, format: PluginFormat) -> Self {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let (size, mtime) = path_stats(&path);

        let (is_symlink, real_path, is_broken_link) = link_state(&path);
        // 软链 → 统计真实目标；失效链接 → real_path 为 None，回退自身（大小为 0）
        let payload = real_path.clone().unwrap_or_else(|| path.clone());

        Self {
            name,
            vendor: None,
            version: None,
            format,
            path,
            bundle_id: None,
            last_modified: mtime,
            file_size: size,
            size_bytes: dir_size_bytes(&payload),
            installed_at: installed_at(&payload),
            is_symlink,
            real_path,
            is_broken_link,
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

/// 软链状态：`(是否软链, 真实路径, 是否失效)`。
///
/// - 非软链 → `(false, None, false)`
/// - 正常软链 → `(true, Some(canonical), false)`
/// - 失效软链 → `(true, None, true)`（目标被删除/卷未挂载）
pub fn link_state(path: &Path) -> (bool, Option<PathBuf>, bool) {
    let is_symlink = std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if !is_symlink {
        return (false, None, false);
    }
    match std::fs::canonicalize(path) {
        Ok(real) => (true, Some(real), false),
        Err(_) => (true, None, true),
    }
}

/// 递归统计文件/目录占用（字节）。
///
/// - 普通文件：自身大小
/// - 软链：不跟随（统计链接本身，避免被恶意的链环拖死）
/// - 目录：walkdir 递归，最多 [`MAX_SIZE_ENTRIES`] 个条目（超出取前 N 个的和）
pub fn dir_size_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() || meta.file_type().is_symlink() {
        return meta.len();
    }

    let mut total: u64 = 0;
    let mut seen: usize = 0;
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.depth() == 0 {
            continue;
        }
        seen += 1;
        if seen > MAX_SIZE_ENTRIES {
            break;
        }
        // follow_links(false)：软链条目返回链接自身的元数据
        if let Ok(m) = entry.metadata() {
            if m.is_file() {
                total = total.saturating_add(m.len());
            }
        }
    }
    total
}

/// 安装/更新时间：bundle 主可执行文件的 mtime。
///
/// 依次尝试：
///   1. 自身是普通文件（VST2 `.vst` 单文件）→ 自身 mtime
///   2. `Contents/Info.plist` 的 `CFBundleExecutable` 指向的可执行文件
///   3. `Contents/MacOS/<bundle 名>`
///   4. `Contents/MacOS/` 下字典序第一个普通文件
///   5. 回退到路径自身（目录）mtime
pub fn installed_at(path: &Path) -> Option<DateTime<Utc>> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_file() {
        return mtime_of(&meta);
    }

    let macos = path.join("Contents").join("MacOS");
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(exe) = bundle_executable_name(path) {
        candidates.push(macos.join(exe));
    }
    if let Some(stem) = path.file_stem() {
        candidates.push(macos.join(stem));
    }
    if let Ok(rd) = std::fs::read_dir(&macos) {
        let mut names: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        names.sort();
        candidates.extend(names);
    }

    for c in candidates {
        if let Ok(m) = std::fs::metadata(&c) {
            if m.is_file() {
                if let Some(t) = mtime_of(&m) {
                    return Some(t);
                }
            }
        }
    }
    // 回退：目录自身 mtime（不如可执行文件可靠，但总比 None 好）
    mtime_of(&meta)
}

/// `Contents/Info.plist` 的 `CFBundleExecutable`。
fn bundle_executable_name(bundle: &Path) -> Option<String> {
    let plist = bundle.join("Contents").join("Info.plist");
    let value = plist::Value::from_file(&plist).ok()?;
    value
        .as_dictionary()?
        .get("CFBundleExecutable")?
        .as_string()
        .map(|s| s.to_string())
}

fn mtime_of(meta: &std::fs::Metadata) -> Option<DateTime<Utc>> {
    meta.modified().ok().map(DateTime::<Utc>::from)
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
            return (m.len(), mtime_of(&m).unwrap_or(DateTime::<Utc>::MIN_UTC));
        }
    }

    match std::fs::metadata(path) {
        Ok(m) => {
            let mtime = mtime_of(&m).unwrap_or(DateTime::<Utc>::MIN_UTC);
            let size = if m.is_file() { m.len() } else { 0 };
            (size, mtime)
        }
        Err(_) => (0, DateTime::<Utc>::MIN_UTC),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 测试根目录：放在 workspace 内的 `target/` 下，避免依赖平台 temp 策略。
    fn test_root(tag: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("plugin-model-tests")
            .join(tag);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 构造一个假 VST3 bundle：Contents/MacOS/<exe> + Contents/Resources/<res>
    fn fake_bundle(root: &Path, name: &str, exe_bytes: usize, res_bytes: usize) -> PathBuf {
        let bundle = root.join(name);
        let macos = bundle.join("Contents").join("MacOS");
        let res = bundle.join("Contents").join("Resources");
        fs::create_dir_all(&macos).unwrap();
        fs::create_dir_all(&res).unwrap();
        let stem = Path::new(name)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        fs::write(macos.join(&stem), vec![0u8; exe_bytes]).unwrap();
        fs::write(res.join("payload.dat"), vec![0u8; res_bytes]).unwrap();
        bundle
    }

    #[test]
    fn dir_size_counts_recursively() {
        let root = test_root("size");
        let bundle = fake_bundle(&root, "Big.vst3", 1000, 500);
        // 更深一层也要算进去
        let deep = bundle.join("Contents").join("Resources").join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("extra.bin"), vec![0u8; 250]).unwrap();

        assert_eq!(dir_size_bytes(&bundle), 1750, "应递归求和（1000+500+250）");
        // 单文件同样正确
        let single = root.join("single.vst");
        fs::write(&single, vec![0u8; 42]).unwrap();
        assert_eq!(dir_size_bytes(&single), 42);
        // 不存在的路径 → 0（不 panic）
        assert_eq!(dir_size_bytes(&root.join("nope.vst3")), 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn installed_at_uses_main_executable_mtime() {
        let root = test_root("installed");
        let bundle = fake_bundle(&root, "Timed.vst3", 16, 16);
        let exe = bundle.join("Contents").join("MacOS").join("Timed");

        // 给可执行文件一个明确的时间戳（2020-09-13T12:26:40Z）
        let want = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        let f = fs::OpenOptions::new().write(true).open(&exe).unwrap();
        f.set_modified(want).unwrap();
        drop(f);

        let info = PluginInfo::from_path(bundle.clone(), PluginFormat::VST3);
        let got = info.installed_at.expect("应有安装时间");
        assert_eq!(
            got,
            DateTime::<Utc>::from(want),
            "安装时间应取主可执行文件 mtime"
        );

        // 目录自身 mtime 与之不同 → 证明取的不是目录 mtime
        let dir_mtime = fs::metadata(&bundle).unwrap().modified().unwrap();
        assert_ne!(
            DateTime::<Utc>::from(dir_mtime),
            got,
            "不应退化为目录 mtime"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn installed_at_prefers_plist_executable_name() {
        let root = test_root("installed-plist");
        let bundle = root.join("Renamed.clap");
        let macos = bundle.join("Contents").join("MacOS");
        fs::create_dir_all(&macos).unwrap();
        // 可执行文件名与 bundle 名不同（真实插件常见）
        let exe = macos.join("ActualBinary");
        fs::write(&exe, vec![0u8; 8]).unwrap();
        fs::write(macos.join("Other"), vec![0u8; 8]).unwrap();
        fs::write(
            bundle.join("Contents").join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>ActualBinary</string>
</dict></plist>"#,
        )
        .unwrap();

        let want = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_500_000_000);
        let f = fs::OpenOptions::new().write(true).open(&exe).unwrap();
        f.set_modified(want).unwrap();
        drop(f);

        let info = PluginInfo::from_path(bundle, PluginFormat::CLAP);
        assert_eq!(info.installed_at, Some(DateTime::<Utc>::from(want)));

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn detects_symlink_and_broken_link() {
        let root = test_root("link");
        let real_dir = root.join("real");
        fs::create_dir_all(&real_dir).unwrap();
        let real_bundle = fake_bundle(&real_dir, "Linked.vst3", 32, 8);

        let scan_dir = root.join("scan");
        fs::create_dir_all(&scan_dir).unwrap();
        let link = scan_dir.join("Linked.vst3");
        std::os::unix::fs::symlink(&real_bundle, &link).unwrap();

        let info = PluginInfo::from_path(link.clone(), PluginFormat::VST3);
        assert!(info.is_symlink, "应识别为软链");
        assert!(!info.is_broken_link, "目标存在时不应判为失效");
        assert_eq!(
            info.real_path.as_deref(),
            Some(fs::canonicalize(&real_bundle).unwrap().as_path()),
            "real_path 应为解析后的真实路径"
        );
        // 软链条目的大小应统计真实目标（40 字节载荷），而不是链接本身
        assert_eq!(info.size_bytes, 40, "软链应统计真实目标占用");

        // 删掉目标 → 失效链接
        fs::remove_dir_all(&real_dir).unwrap();
        let broken = PluginInfo::from_path(link, PluginFormat::VST3);
        assert!(broken.is_symlink, "仍是软链");
        assert!(broken.is_broken_link, "目标消失后应判为失效链接");
        assert!(broken.real_path.is_none(), "失效链接无真实路径");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_dir_is_not_symlink() {
        let root = test_root("plain");
        let bundle = fake_bundle(&root, "Plain.vst3", 4, 4);
        let info = PluginInfo::from_path(bundle, PluginFormat::VST3);
        assert!(!info.is_symlink);
        assert!(!info.is_broken_link);
        assert!(info.real_path.is_none());
        let _ = fs::remove_dir_all(&root);
    }
}
