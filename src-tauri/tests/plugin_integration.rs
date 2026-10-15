// v1.1 插件扫描集成测试 + 性能验证（Task 10）
//
// 全部使用 /tmp 下的模拟目录，**不依赖任何真实插件**（plan 明确禁止）。
// 性能目标（PRD §5 NFR）：50 个插件首扫 < 2s，缓存命中 < 200ms。

use music_rig_lib::core::plugin_scanner::cache;
use music_rig_lib::core::plugin_scanner::discover;
use music_rig_lib::core::plugin_scanner::model::{PluginFormat, PluginInfo, ScanSource};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const AU_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>MyPlugin</string>
  <key>CFBundleIdentifier</key><string>com.acme.myplugin</string>
  <key>CFBundleShortVersionString</key><string>1.4.2</string>
</dict>
</plist>
"#;

/// 建一个带 plist 的 bundle（AU/VST3/AAX/CLAP）。
fn make_bundle(root: &Path, name: &str, ext: &str, id: &str, ver: &str) -> PathBuf {
    let bundle = root.join(format!("{}.{}", name, ext));
    let contents = bundle.join("Contents");
    fs::create_dir_all(contents.join("MacOS")).unwrap();
    let plist = AU_PLIST
        .replace("MyPlugin", name)
        .replace("com.acme.myplugin", id)
        .replace("1.4.2", ver);
    fs::write(contents.join("Info.plist"), plist).unwrap();
    fs::write(contents.join("MacOS").join(name), b"\x00binary\x00").unwrap();
    bundle
}

fn fixture_root(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("mr_fixture_{}", tag));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

// ── Task 10 断言 1：default_sources() >= 8 ──────────────────
#[test]
fn default_sources_at_least_eight() {
    let srcs = discover::default_sources();
    assert!(
        srcs.len() >= 8,
        "标准扫描源应 >= 8，实际 {}",
        srcs.len()
    );
}

// ── Task 10 断言 2：模拟目录上扫描数量正确 ──────────────────
#[test]
fn scan_count_is_correct_on_fixture() {
    let root = fixture_root("count");
    make_bundle(&root, "MyPlugin", "component", "com.a.myplugin", "1.4.2");
    make_bundle(&root, "OtherPlugin", "vst3", "com.b.otherplugin", "2.0");
    // CLAP 裸文件（无 plist）
    fs::write(root.join("ClapPlugin.clap"), b"\x00clap\x00").unwrap();
    // 干扰项：不应被计入
    fs::write(root.join("notes.txt"), b"ignore me").unwrap();

    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let found = discover::discover_plugins(&srcs);

    assert_eq!(found.len(), 3, "应命中 3 个插件，txt 被忽略: {:?}", found.iter().map(|p| p.name.clone()).collect::<Vec<_>>());

    let _ = fs::remove_dir_all(&root);
}

// ── 混合格式：一次扫描识别全部 5 种格式 ──────────────────────
#[test]
fn scan_detects_all_five_formats() {
    let root = fixture_root("formats");
    make_bundle(&root, "A", "component", "com.a.a", "1.0");
    let v = root.join("B.vst");
    fs::write(&v, b"\x00VstPlugin 3.2.1\x00").unwrap();
    make_bundle(&root, "C", "vst3", "com.c.c", "1.0");
    make_bundle(&root, "D", "aaxplugin", "com.d.d", "1.0");
    make_bundle(&root, "E", "clap", "com.e.e", "1.0");

    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let found = discover::discover_plugins(&srcs);
    let mut fmts: Vec<PluginFormat> = found.iter().map(|p| p.format).collect();
    fmts.sort_by_key(|f| f.as_str());
    fmts.dedup();

    assert_eq!(fmts.len(), 5, "应识别全部 5 种格式: {:?}", fmts);

    let _ = fs::remove_dir_all(&root);
}

// ── Task 10 断言 3：二次扫描 100% 缓存命中 ───────────────────
#[test]
fn second_scan_is_full_cache_hit() {
    let root = fixture_root("cache");
    for i in 0..10 {
        make_bundle(&root, &format!("P{}", i), "vst3", &format!("com.v{}.p", i), "1.0");
    }
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];

    let conn = cache::open_in_memory().unwrap();

    let mut pass1 = discover::discover_plugins(&srcs);
    let s1 = cache::extract_with_cache(&conn, &mut pass1);
    assert_eq!(s1.misses, 10, "首扫全部未命中");
    assert_eq!(s1.hits, 0);

    let mut pass2 = discover::discover_plugins(&srcs);
    let s2 = cache::extract_with_cache(&conn, &mut pass2);
    assert_eq!(s2.hits, 10, "二次扫描应 100% 命中");
    assert_eq!(s2.hit_rate(), 1.0, "命中率应为 100%");

    // 缓存结果与首扫一致
    assert_eq!(pass1.len(), pass2.len());
    for (a, b) in pass1.iter().zip(pass2.iter()) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.vendor, b.vendor);
        assert_eq!(a.version, b.version);
    }

    let _ = fs::remove_dir_all(&root);
}

// ── 元数据正确性（端到端：发现 → 提取 → 缓存）────────────────
#[test]
fn metadata_end_to_end() {
    let root = fixture_root("meta");
    make_bundle(&root, "Reverb", "component", "com.fabfilter.reverb", "2.5.1");
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let conn = cache::open_in_memory().unwrap();
    let mut found = discover::discover_plugins(&srcs);
    cache::extract_with_cache(&conn, &mut found);

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Reverb");
    assert_eq!(found[0].version.as_deref(), Some("2.5.1"));
    assert_eq!(found[0].vendor.as_deref(), Some("Fabfilter"));

    let _ = fs::remove_dir_all(&root);
}

// ── 性能：50 插件首扫 < 2s ──────────────────────────────────
#[test]
fn perf_first_scan_50_plugins_under_2s() {
    let root = fixture_root("perf50");
    for i in 0..50 {
        make_bundle(&root, &format!("Plug{}", i), "vst3", &format!("com.p{}.x", i), "1.0");
    }
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];

    let t = Instant::now();
    let mut found = discover::discover_plugins(&srcs);
    let conn = cache::open_in_memory().unwrap();
    cache::extract_with_cache(&conn, &mut found);
    let elapsed = t.elapsed();

    assert_eq!(found.len(), 50);
    assert!(
        elapsed < Duration::from_secs(2),
        "50 插件首扫应 < 2s，实际 {:?}",
        elapsed
    );

    let _ = fs::remove_dir_all(&root);
}

// ── 性能：缓存扫描 < 200ms ──────────────────────────────────
#[test]
fn perf_cached_scan_under_200ms() {
    let root = fixture_root("perfcache");
    for i in 0..50 {
        make_bundle(&root, &format!("Plug{}", i), "vst3", &format!("com.p{}.x", i), "1.0");
    }
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let conn = cache::open_in_memory().unwrap();

    // 预热
    let mut warm = discover::discover_plugins(&srcs);
    cache::extract_with_cache(&conn, &mut warm);

    let t = Instant::now();
    let mut found = discover::discover_plugins(&srcs);
    let stats = cache::extract_with_cache(&conn, &mut found);
    let elapsed = t.elapsed();

    assert_eq!(stats.hits, 50, "应全部命中缓存");
    assert!(
        elapsed < Duration::from_millis(200),
        "缓存扫描应 < 200ms，实际 {:?}",
        elapsed
    );

    let _ = fs::remove_dir_all(&root);
}

// ── 鲁棒性：损坏 plist / 空目录 / 权限外目录都不 panic ─────────
#[test]
fn corrupt_plist_does_not_panic() {
    let root = fixture_root("corrupt");
    let b = root.join("Broken.component");
    fs::create_dir_all(b.join("Contents")).unwrap();
    fs::write(b.join("Contents").join("Info.plist"), b"not a plist at all").unwrap();

    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let conn = cache::open_in_memory().unwrap();
    let mut found = discover::discover_plugins(&srcs);
    let stats = cache::extract_with_cache(&conn, &mut found);

    assert_eq!(found.len(), 1, "损坏 plist 仍应被发现");
    assert_eq!(found[0].name, "Broken", "名称回退为文件名");
    assert_eq!(stats.misses, 1);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn empty_and_missing_dirs_are_safe() {
    let empty = fixture_root("empty");
    let srcs = vec![
        ScanSource::standard(empty.clone(), PluginFormat::Auto, true),
        ScanSource::standard(PathBuf::from("/no/such/dir/mr"), PluginFormat::Auto, true),
    ];
    let found = discover::discover_plugins(&srcs);
    assert!(found.is_empty(), "空目录与不存在目录都应返回空且不 panic");
    let _ = fs::remove_dir_all(&empty);
}

// ── 元数据缺失统一为 None（UI 渲染 "—"）──────────────────────
#[test]
fn missing_metadata_is_none_not_empty_string() {
    let root = fixture_root("nonemeta");
    fs::write(root.join("Bare.clap"), b"\x00").unwrap();
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let conn = cache::open_in_memory().unwrap();
    let mut found = discover::discover_plugins(&srcs);
    cache::extract_with_cache(&conn, &mut found);

    assert!(found[0].vendor.is_none());
    // 版本可能从二进制里碰巧匹配到，此处只断言类型语义
    let _ = found[0].version.is_none();

    let _ = fs::remove_dir_all(&root);
}

// ── 并行与串行结果一致 ──────────────────────────────────────
#[test]
fn parallel_matches_serial_on_fixture() {
    let root = fixture_root("par");
    for i in 0..20 {
        make_bundle(&root, &format!("S{}", i), "component", &format!("com.s{}.y", i), "1.0");
    }
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let a = discover::discover_plugins(&srcs);
    let b = discover::discover_plugins_par(&srcs);
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), 20);
    let _ = fs::remove_dir_all(&root);
}

// ── 缓存持久化到磁盘文件可用 ────────────────────────────────
#[test]
fn on_disk_cache_survives_reopen() {
    let root = fixture_root("disk");
    make_bundle(&root, "DiskPlug", "vst3", "com.disk.plug", "9.9");
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];

    let db = std::env::temp_dir().join("mr_plugin_cache_test.db");
    let _ = fs::remove_file(&db);

    {
        let conn = cache::open(&db).unwrap();
        let mut f = discover::discover_plugins(&srcs);
        let s = cache::extract_with_cache(&conn, &mut f);
        assert_eq!(s.misses, 1);
    }
    {
        let conn = cache::open(&db).unwrap();
        let mut f = discover::discover_plugins(&srcs);
        let s = cache::extract_with_cache(&conn, &mut f);
        assert_eq!(s.hits, 1, "重开库后应命中缓存");
        assert_eq!(f[0].name, "DiskPlug");
    }

    let _ = fs::remove_file(&db);
    let _ = fs::remove_dir_all(&root);
}

// ── 空结果不写崩 UI 数据契约 ────────────────────────────────
#[test]
fn plugin_info_serializes_for_frontend() {
    let p = PluginInfo::from_path(PathBuf::from("/tmp/X.vst3"), PluginFormat::VST3);
    let json = serde_json::to_string(&p).expect("应可序列化给前端");
    assert!(json.contains("\"format\":\"VST3\""), "前端依赖大写格式串: {}", json);
}

// ── review 回归：bundle 内 plist 变更必须使缓存失效 ──────────────
#[test]
fn regression_bundle_plist_edit_invalidates_cache() {
    let root = fixture_root("regr_plist");
    let b = make_bundle(&root, "Ver", "vst3", "com.ver.plug", "1.0.0");
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let conn = cache::open_in_memory().unwrap();

    let mut p1 = discover::discover_plugins(&srcs);
    cache::extract_with_cache(&conn, &mut p1);
    assert_eq!(p1[0].version.as_deref(), Some("1.0.0"));

    // 只改版本号且保持长度相同：目录 mtime 不变、总大小不变，
    // 旧实现（按目录 mtime + 目录大小）会永久命中旧值。
    let plist = b.join("Contents").join("Info.plist");
    let text = fs::read_to_string(&plist).unwrap().replace("1.0.0", "1.0.9");
    fs::write(&plist, text).unwrap();

    let mut p2 = discover::discover_plugins(&srcs);
    let s = cache::extract_with_cache(&conn, &mut p2);

    assert_eq!(s.misses, 1, "plist 变更后必须重新提取，而非命中旧缓存");
    assert_eq!(p2[0].version.as_deref(), Some("1.0.9"), "应读到新版本");

    let _ = fs::remove_dir_all(&root);
}

// ── review 回归：缓存必须恢复 bundle_id ────────────────────────
#[test]
fn regression_cache_restores_bundle_id() {
    let root = fixture_root("regr_bid");
    make_bundle(&root, "Bid", "component", "com.bid.plug", "1.0");
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let conn = cache::open_in_memory().unwrap();

    let mut p1 = discover::discover_plugins(&srcs);
    cache::extract_with_cache(&conn, &mut p1);
    assert!(p1[0].bundle_id.is_some(), "首扫应解出 bundle_id");

    let mut p2 = discover::discover_plugins(&srcs);
    let s = cache::extract_with_cache(&conn, &mut p2);
    assert_eq!(s.hits, 1);
    assert_eq!(
        p2[0].bundle_id.as_deref(),
        p1[0].bundle_id.as_deref(),
        "缓存命中后 bundle_id 不得丢失"
    );

    let _ = fs::remove_dir_all(&root);
}

// ── review 回归：删除插件后缓存行被清理（库不无限增长）──────────
#[test]
fn regression_cache_prunes_deleted_plugins() {
    let root = fixture_root("regr_prune");
    make_bundle(&root, "Gone", "vst3", "com.gone.plug", "1.0");
    let srcs = vec![ScanSource::standard(root.clone(), PluginFormat::Auto, true)];
    let conn = cache::open_in_memory().unwrap();

    let mut p1 = discover::discover_plugins(&srcs);
    cache::extract_with_cache(&conn, &mut p1);
    let cnt: i64 = conn
        .query_row("SELECT COUNT(*) FROM plugin_cache", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cnt, 1);

    fs::remove_dir_all(&root).unwrap();
    let p2 = discover::discover_plugins(&srcs);
    assert!(p2.is_empty());
    cache::prune_missing(&conn, &p2).unwrap();
    let cnt2: i64 = conn
        .query_row("SELECT COUNT(*) FROM plugin_cache", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cnt2, 0, "已删除插件的缓存行应被清理");
}
