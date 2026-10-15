// Kontakt 库盘点解析（Task 3）
//
// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）
// 在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// 解析目标（只读）：
//  - Native Instruments Service Center 目录下的 XML（<Application> 条目）
//  - Kontakt 偏好 plist（com.native-instruments.Kontakt*.plist）
//  - 已安装产品 JSON（installed products 类信息）

use crate::core::library_manager::model::{Diagnostic, LibraryEntry};
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use std::collections::HashMap;
use std::io::BufReader;
use std::path::PathBuf;

type XmlR = XmlReader<BufReader<std::io::Cursor<Vec<u8>>>>;

/// 从单个 Service Center XML 解析 <Application> 条目。
///
/// 期望结构（字段名按 NI 实际约定，稳健容错）：
/// <Application><Name>..</Name><ContentDir>..</ContentDir>..</Application>
pub fn parse_service_center_xml(path: &std::path::Path) -> Vec<LibraryEntry> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut reader: XmlR = XmlReader::from_reader(BufReader::new(std::io::Cursor::new(
        content.into_bytes(),
    )));

    let mut entries: Vec<LibraryEntry> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref().eq_ignore_ascii_case("Application") => {
                if let Some(entry) = read_application(&mut reader) {
                    entries.push(entry);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    entries
}

fn read_application(reader: &mut XmlR) -> Option<LibraryEntry> {
    let mut name: Option<String> = None;
    let mut dir: Option<String> = None;
    let mut reg: Option<String> = None;
    let mut buf = Vec::new();
    let mut cur_tag: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                cur_tag = Some(e.name().as_ref().to_string());
            }
            Ok(Event::End(e)) => {
                let tag = e.name().as_ref().to_string();
                if tag.eq_ignore_ascii_case("Application") {
                    break;
                }
                cur_tag = None;
            }
            Ok(Event::Text(t)) => {
                if let Some(ct) = &cur_tag {
                    let val = t.as_ref().trim().to_string();
                    match ct.as_str() {
                        "Name" => name = Some(val),
                        "ContentDir" => dir = Some(val),
                        "RegKey" | "Registered" => reg = Some(val),
                        _ => {}
                    }
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(ct) = &cur_tag {
                    let val = t.as_ref().trim().to_string();
                    match ct.as_str() {
                        "Name" => name = Some(val),
                        "ContentDir" => dir = Some(val),
                        _ => {}
                    }
                }
            }
            Err(_) | Ok(Event::Eof) => break,
            _ => {}
        }
        buf.clear();
    }

    let name = name.unwrap_or_default();
    let snpid = reg.clone().unwrap_or_else(|| name.clone());
    let mut entry = LibraryEntry::new(snpid, name.clone());
    if let Some(d) = dir {
        entry.path = Some(PathBuf::from(d));
    } else {
        entry
            .diagnostics
            .push(Diagnostic::MissingMetadata("ContentDir".to_string()));
    }
    Some(entry)
}

/// 从 plist 读取 Kontakt 已注册库（macOS 偏好）。
/// 返回 HashMap<snpID, 路径>。
pub fn parse_kontakt_plist(path: &std::path::Path) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return out,
    };
    let plist = match plist::Value::from_reader(std::io::Cursor::new(data)) {
        Ok(v) => v,
        Err(_) => return out,
    };
    if let plist::Value::Dictionary(dict) = plist {
        // Native Instruments Kontakt 偏好中通常以 "kLibraries" / "Libraries" 形式存字典
        for key in ["kLibraries", "Libraries", "libs"] {
            if let Some(plist::Value::Dictionary(libs)) = dict.get(key) {
                for (snp, val) in libs.iter() {
                    if let plist::Value::String(p) = val {
                        out.insert(snp.clone(), PathBuf::from(p));
                    }
                }
            }
        }
    }
    out
}

/// 解析已安装产品 JSON（结构稳健容错）。
/// v1 盘点以 Service Center XML + 偏好 plist 为主；该函数为 Task 7 预留的通用入口。
#[allow(dead_code)]
pub fn parse_installed_products_json(path: &std::path::Path) -> Vec<LibraryEntry> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let v: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    let arr = match &v {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(o) if o.get("products").is_some() => {
            o["products"].as_array().cloned().unwrap_or_default()
        }
        _ => Vec::new(),
    };
    for item in arr {
        let snpid = item
            .get("snpID")
            .or_else(|| item.get("id"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let name = item
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if snpid.is_empty() && name.is_empty() {
            continue;
        }
        let mut entry = LibraryEntry::new(
            if snpid.is_empty() { name.clone() } else { snpid },
            name,
        );
        if let Some(p) = item.get("path").and_then(|x| x.as_str()) {
            entry.path = Some(PathBuf::from(p));
        }
        out.push(entry);
    }
    out
}
