// v1 集成测试（Task 6）
//
// 覆盖：Service Center XML 解析、Kontakt plist 解析、诊断逻辑、命令注册、
// 以及 Tauri 上下文（tauri.conf.json）可正常加载（打包冒烟前置）。

use o_muse_lib::core::library_manager::diagnose::diagnose;
use o_muse_lib::core::library_manager::model::{Diagnostic, LibraryEntry};
use o_muse_lib::core::library_manager::parser::{parse_kontakt_plist, parse_service_center_xml};

const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Applications>
  <Application>
    <Name>Analog Dreams</Name>
    <ContentDir>/Volumes/External/Libraries/Analog Dreams</ContentDir>
    <RegKey>2499</RegKey>
  </Application>
  <Application>
    <Name>Orchestral Tools</Name>
    <ContentDir>/Library/Application Support/Native Instruments/Orchestral Tools</ContentDir>
    <RegKey>2510</RegKey>
  </Application>
</Applications>
"#;

const SAMPLE_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>kLibraries</key>
  <dict>
    <key>2499</key>
    <string>/Volumes/External/Libraries/Analog Dreams</string>
    <key>2510</key>
    <string>/Library/Application Support/Native Instruments/Orchestral Tools</string>
  </dict>
</dict>
</plist>
"#;

#[test]
fn parse_service_center_xml_entries() {
    let tmp = std::env::temp_dir().join("mr_test_sc.xml");
    std::fs::write(&tmp, SAMPLE_XML).unwrap();
    let entries = parse_service_center_xml(&tmp);
    std::fs::remove_file(&tmp).ok();

    assert_eq!(entries.len(), 2);
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"Analog Dreams"));
    assert!(names.contains(&"Orchestral Tools"));
    assert!(entries.iter().any(|e| e.snpid == "2499"));
    assert!(entries.iter().any(|e| e.snpid == "2510"));
}

#[test]
fn parse_kontakt_plist_registered() {
    let tmp = std::env::temp_dir().join("mr_test_k.plist");
    std::fs::write(&tmp, SAMPLE_PLIST).unwrap();
    let libs = parse_kontakt_plist(&tmp);
    std::fs::remove_file(&tmp).ok();

    assert_eq!(libs.len(), 2);
    assert!(libs.contains_key("2499"));
    assert!(libs
        .get("2499")
        .map(|p| p.to_string_lossy().contains("Analog Dreams"))
        .unwrap_or(false));
}

#[test]
fn diagnose_missing_path_and_disconnected_volume() {
    // /Volumes/MusicRigGhost 几乎必然不存在 → 断卷诊断
    let ghost = std::path::PathBuf::from("/Volumes/MusicRigGhost/lib");
    let mut entries = vec![LibraryEntry::new(String::from("9999"), String::from("Ghost Lib"))];
    entries[0].path = Some(ghost.clone());

    diagnose(&mut entries);
    let diags = &entries[0].diagnostics;
    assert!(diags.iter().any(|d| matches!(d, Diagnostic::DisconnectedVolume(_))));
}

#[test]
fn diagnose_existing_path_ok() {
    let existing = std::env::temp_dir();
    let mut entries = vec![LibraryEntry::new(String::from("1000"), String::from("Temp Lib"))];
    entries[0].path = Some(existing.clone());

    diagnose(&mut entries);
    assert!(entries[0].diagnostics.is_empty());
}

#[test]
fn diagnose_duplicate_snpid() {
    let p = std::path::PathBuf::from("/Applications/X");
    let mut entries = vec![
        LibraryEntry::new(String::from("4242"), String::from("A")),
        LibraryEntry::new(String::from("4242"), String::from("B")),
    ];
    entries[0].path = Some(p.clone());
    entries[1].path = Some(p.clone());

    diagnose(&mut entries);
    assert!(entries
        .iter()
        .any(|e| e.diagnostics.iter().any(|d| matches!(d, Diagnostic::DuplicateSnpid(_)))));
}

#[test]
fn tauri_context_loads() {
    // 加载 tauri.conf.json 配置；若配置损坏会 panic（打包冒烟前置）。
    let _ctx: tauri::Context<tauri::Wry> = tauri::generate_context!();
}
