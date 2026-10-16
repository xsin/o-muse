// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// Kontakt 库管理命令（Tauri 命令层）
//
// 性能约定：盘点、扫描、注册/注销涉及目录遍历与文件 IO，全部
// `async` + `spawn_blocking`，绝不阻塞 Tauri 主线程。

use crate::core::kontakt;
use crate::core::kontakt::model::{
    KontaktLibrary, NicntFormSpec, SystemRegistration, UserRegistration,
};
use crate::core::kontakt::privileged::{self, PrivilegedOp, PrivilegedRequest};
use crate::core::kontakt::{nicnt, registry};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 合并四个注册来源盘点所有库（含诊断）。
#[tauri::command]
pub async fn kontakt_inventory() -> Result<Vec<KontaktLibrary>, String> {
    tauri::async_runtime::spawn_blocking(kontakt::inventory)
        .await
        .map_err(|e| format!("盘点任务异常: {e}"))?
}

/// 扫描用户给定根目录，返回其中（多半尚未注册的）库。
#[tauri::command]
pub async fn kontakt_scan(root: String) -> Result<Vec<KontaktLibrary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = PathBuf::from(&root);
        if !dir.is_dir() {
            return Err(format!("扫描目录不存在或不是目录: {root}"));
        }
        Ok(kontakt::scan_libraries(&dir))
    })
    .await
    .map_err(|e| format!("扫描任务异常: {e}"))?
}

/// 注册一个库：读取 `.nicnt`，写入 installed_products JSON 与用户级 plist。
#[tauri::command]
pub async fn kontakt_add(nicnt_path: String) -> Result<KontaktLibrary, String> {
    tauri::async_runtime::spawn_blocking(move || kontakt::add_library(&PathBuf::from(&nicnt_path)))
        .await
        .map_err(|e| format!("添加任务异常: {e}"))?
}

/// 注销一个库：删除注册文件，**不动磁盘内容**。
#[tauri::command]
pub async fn kontakt_remove(reg_key: String, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || kontakt::remove_library(&reg_key, &name))
        .await
        .map_err(|e| format!("移除任务异常: {e}"))?
}

/// 返回 `.nicnt` 创建表单的默认模板。
#[tauri::command]
pub async fn kontakt_nicnt_spec() -> Result<NicntFormSpec, String> {
    tauri::async_runtime::spawn_blocking(kontakt::create_nicnt_form_spec)
        .await
        .map_err(|e| format!("任务异常: {e}"))
}

/// 在用户选择的库目录内生成 `.nicnt`，返回文件路径。
///
/// 同名文件已存在时直接报错（绝不覆盖）；绝不会写入 `/Library` 等系统目录。
#[tauri::command]
pub async fn kontakt_create_nicnt(dir: String, spec: NicntFormSpec) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target_dir = PathBuf::from(&dir);
        let path = kontakt::write_nicnt(&target_dir, spec)?;
        Ok(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("生成 nicnt 任务异常: {e}"))?
}

// ── 系统级注册的提权写入 ──────────────────────────────────────
//
// 免提权路径（installed_products JSON + 用户级 plist）覆盖不了 Kontakt 的
// 经典浏览器与 Kontakt 6 场景，因此需要写 `/Library` 下的两个来源。
// **用户可见行为**：永远先给出「具体文件 + 原因」的计划（`kontakt_privilege_plan`），
// 用户在前端确认后才会调 `kontakt_apply_privileged`，届时 macOS 弹出管理员密码框。

/// 计划中的一条：具体文件、动作与原因（原因同时给出 i18n key 与中文文案）。
#[derive(Debug, Clone, Serialize)]
pub struct PrivilegeEntry {
    /// 将被写入/删除的**具体绝对路径**。
    pub path: String,
    /// `"write"` 或 `"remove"`。
    pub action: String,
    /// 中文回退文案（i18n key 缺失时前端使用）。
    pub reason: String,
    /// i18n key（en/zh 两份 locale 均已提供）。
    pub reason_key: String,
}

/// 提权计划：`needed == false` 时无需提权，调用方只做免提权部分。
#[derive(Debug, Clone, Serialize)]
pub struct PrivilegePlan {
    pub needed: bool,
    pub entries: Vec<PrivilegeEntry>,
}

/// 原因文案（中文回退 + i18n key），前后端一致。
const XML_WRITE_REASON: &str =
    "Kontakt 的经典浏览器读取此文件识别音色库；缺少它时该库不会出现在 Libraries 列表";
const XML_WRITE_REASON_KEY: &str = "kontakt.priv.reasonXml";
const PLIST_WRITE_REASON: &str = "记录该库的内容路径与可见性";
const PLIST_WRITE_REASON_KEY: &str = "kontakt.priv.reasonPlist";
const XML_REMOVE_REASON: &str = "删除后，该库将从 Kontakt 经典浏览器中消失";
const XML_REMOVE_REASON_KEY: &str = "kontakt.priv.reasonXmlRemove";
const PLIST_REMOVE_REASON: &str = "删除该库的系统级内容路径与可见性记录";
const PLIST_REMOVE_REASON_KEY: &str = "kontakt.priv.reasonPlistRemove";

/// 计划内的一条操作（内部表示，含可直接执行的 [`PrivilegedOp`]）。
struct PlannedOp {
    path: PathBuf,
    action: &'static str,
    reason: &'static str,
    reason_key: &'static str,
    op: PrivilegedOp,
}

/// 生成系统级注册的写入内容（Service Center XML + 系统 plist）。
#[derive(Debug)]
struct RegistrationPayload {
    xml: String,
    plist: Vec<u8>,
}

/// 查询系统级写入计划：返回**每一条具体路径与原因**，不做任何写入。
#[tauri::command]
pub async fn kontakt_privilege_plan(
    reg_key: String,
    name: String,
) -> Result<PrivilegePlan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let planned = plan_system_ops(&reg_key, &name)?;
        let entries = planned
            .iter()
            .map(|item| PrivilegeEntry {
                path: item.path.to_string_lossy().to_string(),
                action: item.action.to_string(),
                reason: item.reason.to_string(),
                reason_key: item.reason_key.to_string(),
            })
            .collect();
        Ok(PrivilegePlan {
            needed: !planned.is_empty(),
            entries,
        })
    })
    .await
    .map_err(|e| format!("提权计划任务异常: {e}"))?
}

/// 在用户**显式授权**后补全/清理系统级注册。
///
/// `allow == false` 表示用户拒绝提权：直接返回空结果（免提权部分已完成），
/// 绝不发起任何提权动作。
#[tauri::command]
pub async fn kontakt_apply_privileged(
    app: tauri::AppHandle,
    reg_key: String,
    name: String,
    allow: bool,
) -> Result<Vec<String>, String> {
    if !allow {
        return Ok(Vec::new());
    }

    let plan_key = reg_key.clone();
    let plan_name = name.clone();
    let planned = tauri::async_runtime::spawn_blocking(move || plan_system_ops(&plan_key, &plan_name))
        .await
        .map_err(|e| format!("提权计划任务异常: {e}"))??;
    if planned.is_empty() {
        return Ok(Vec::new());
    }

    let operations: Vec<PrivilegedOp> = planned.iter().map(|item| item.op.clone()).collect();
    let request = PrivilegedRequest {
        version: privileged::REQUEST_VERSION,
        operations,
        reason: format!(
            "Kontakt 系统级注册: name={name} reg_key={reg_key} 操作数={}",
            planned.len()
        ),
    };
    privileged::run_privileged(&app, &request).await
}

/// 计算需要提权的操作清单。
///
/// 意图判定（调用方无需额外传参）：用户级来源存在 → 刚注册，目标是**补全**系统级注册；
/// 用户级来源不存在 → 刚注销（或从未注册），目标是**清理**系统级残留。
/// 因此 `kontakt_add` / `kontakt_remove` 之后调用都能得到正确语义。
fn plan_system_ops(reg_key: &str, name: &str) -> Result<Vec<PlannedOp>, String> {
    let reg_key = reg_key.trim();
    let name = name.trim();
    nicnt::validate_component(reg_key, "RegKey")?;
    nicnt::validate_component(name, "Name")?;

    let xml_path = registry::service_center_xml_path(reg_key);
    let plist_path = registry::system_plist_path(reg_key);

    let user_registration = registry::read_user_plist(reg_key);
    let content_dir =
        registry::read_installed_product_json(&registry::installed_product_json_path(name));
    let registered = user_registration.is_some() || content_dir.is_some();

    let mut planned = Vec::new();
    if registered {
        let payload = build_registration_payload(
            reg_key,
            name,
            user_registration.as_ref(),
            content_dir.as_deref(),
        )?;
        if !xml_path.exists() {
            planned.push(PlannedOp {
                path: xml_path.clone(),
                action: "write",
                reason: XML_WRITE_REASON,
                reason_key: XML_WRITE_REASON_KEY,
                op: PrivilegedOp::WriteFile {
                    path: xml_path.to_string_lossy().to_string(),
                    contents_base64: privileged::base64_encode(payload.xml.as_bytes()),
                },
            });
        }
        if !plist_path.exists() {
            planned.push(PlannedOp {
                path: plist_path.clone(),
                action: "write",
                reason: PLIST_WRITE_REASON,
                reason_key: PLIST_WRITE_REASON_KEY,
                op: PrivilegedOp::WriteFile {
                    path: plist_path.to_string_lossy().to_string(),
                    contents_base64: privileged::base64_encode(&payload.plist),
                },
            });
        }
    } else {
        if xml_path.exists() {
            planned.push(PlannedOp {
                path: xml_path.clone(),
                action: "remove",
                reason: XML_REMOVE_REASON,
                reason_key: XML_REMOVE_REASON_KEY,
                op: PrivilegedOp::RemoveFile {
                    path: xml_path.to_string_lossy().to_string(),
                },
            });
        }
        if plist_path.exists() {
            planned.push(PlannedOp {
                path: plist_path.clone(),
                action: "remove",
                reason: PLIST_REMOVE_REASON,
                reason_key: PLIST_REMOVE_REASON_KEY,
                op: PrivilegedOp::RemoveFile {
                    path: plist_path.to_string_lossy().to_string(),
                },
            });
        }
    }
    Ok(planned)
}

/// 组装待写入的系统级内容。
///
/// 元数据优先级：库目录内的 `.nicnt`（最权威，能拿到原始 `ProductHints` 片段）
/// ＞ 用户级 plist。**绝不伪造** `HU` / `JDX` 等授权字段：只有原始片段里
/// 或 `.nicnt` 里确实有才写入。
fn build_registration_payload(
    reg_key: &str,
    name: &str,
    user_registration: Option<&UserRegistration>,
    content_dir: Option<&Path>,
) -> Result<RegistrationPayload, String> {
    let payload = content_dir
        .and_then(nicnt::find_metadata_file)
        .and_then(|path| nicnt::parse_metadata_file(&path).ok());

    let snpid = payload
        .as_ref()
        .map(|p| p.metadata.snpid.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            user_registration
                .map(|u| u.snpid.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| {
            format!("未能确定「{name}」的 SNPID（缺少 .nicnt 与用户级注册），无法生成系统级注册文件")
        })?;

    let display_name = payload
        .as_ref()
        .map(|p| p.metadata.name.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            user_registration
                .map(|u| u.name.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| name.to_string());

    let visibility = payload
        .as_ref()
        .and_then(|p| p.metadata.visibility)
        .unwrap_or(3);

    let xml = match &payload {
        // 优先原样落盘 `.nicnt` 里的 ProductHints 片段（保留属性顺序与缩进）。
        Some(payload) => payload.raw_xml.clone(),
        None => nicnt::render_product_hints(&NicntFormSpec {
            name: display_name.clone(),
            reg_key: reg_key.to_string(),
            snpid: snpid.clone(),
            company: String::new(),
            product_type: "Content".to_string(),
            upid: None,
            auth_system: None,
            hu: None,
            jdx: None,
            visibility,
        })?,
    };

    let plist = registry::system_plist_bytes(&SystemRegistration {
        reg_key: reg_key.to_string(),
        snpid,
        name: display_name,
        content_dir: content_dir.map(Path::to_path_buf),
        visibility: Some(visibility),
        hu: payload.as_ref().and_then(|p| p.metadata.hu.clone()),
        jdx: payload.as_ref().and_then(|p| p.metadata.jdx.clone()),
        upid: payload.as_ref().and_then(|p| p.metadata.upid.clone()),
        auth_system: payload
            .as_ref()
            .and_then(|p| p.metadata.auth_system.clone()),
    })?;

    Ok(RegistrationPayload { xml, plist })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root(tag: &str) -> PathBuf {
        let base = std::fs::canonicalize(std::env::temp_dir()).expect("临时目录可用");
        let dir = base.join(format!("omuse_kontakt_cmd_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn 命令层默认表单可获取() {
        let spec = tauri::async_runtime::block_on(kontakt_nicnt_spec()).expect("应返回默认表单");
        assert_eq!(spec.product_type, "Content");
        assert_eq!(spec.visibility, 3);
    }

    #[test]
    fn 命令层拒绝不存在的扫描目录() {
        let missing = std::env::temp_dir().join("omuse_definitely_missing_dir_9f8e7d");
        let err =
            tauri::async_runtime::block_on(kontakt_scan(missing.to_string_lossy().to_string()))
                .unwrap_err();
        assert!(err.contains("不存在"), "错误信息应说明目录不存在: {err}");
    }

    #[test]
    fn 命令层生成并扫描_nicnt() {
        let root = fixture_root("create");
        let spec = NicntFormSpec {
            name: "CmdLib".to_string(),
            reg_key: "CmdLib".to_string(),
            snpid: "C1".to_string(),
            company: "O Muse".to_string(),
            ..NicntFormSpec::default()
        };

        let created = tauri::async_runtime::block_on(kontakt_create_nicnt(
            root.to_string_lossy().to_string(),
            spec,
        ))
        .expect("应能生成 nicnt");
        assert!(PathBuf::from(&created).is_file());

        std::fs::create_dir_all(root.join("Samples")).unwrap();
        let found =
            tauri::async_runtime::block_on(kontakt_scan(root.to_string_lossy().to_string()))
                .expect("应能扫描");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "CmdLib");

        std::fs::remove_dir_all(&root).ok();
    }

    // ── 提权计划的内容组装（不触碰 /Library）──────────────────

    #[test]
    fn 有_nicnt_时优先复用原始_ProductHints() {
        let root = fixture_root("priv_nicnt");
        let spec = NicntFormSpec {
            name: "PrivLib".to_string(),
            reg_key: "PrivLib".to_string(),
            snpid: "P1".to_string(),
            company: "O Muse".to_string(),
            ..NicntFormSpec::default()
        };
        kontakt::write_nicnt(&root, spec).expect("应能生成 nicnt");

        let payload = build_registration_payload("PrivLib", "PrivLib", None, Some(&root))
            .expect("有 .nicnt 时应能组装内容");
        assert!(payload.xml.contains("<ProductHints"));
        assert!(payload.xml.contains("<SNPID>P1</SNPID>"));
        // 系统 plist 必须能被自家解析器读回
        let parsed = plist::Value::from_reader(std::io::Cursor::new(payload.plist.clone()))
            .expect("plist 应可解析");
        assert!(parsed.as_dictionary().is_some());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 无_nicnt_时回退到用户级注册字段() {
        let user = UserRegistration {
            snpid: "U9".to_string(),
            name: "FallbackLib".to_string(),
            reg_key: "FallbackLib".to_string(),
            user_list_index: Some(0),
            user_removed: false,
        };
        let payload = build_registration_payload("FallbackLib", "FallbackLib", Some(&user), None)
            .expect("有用户级 plist 时应能回退");
        assert!(payload.xml.contains("<RegKey>FallbackLib</RegKey>"));
        assert!(payload.xml.contains("<SNPID>U9</SNPID>"));
        std::str::from_utf8(&payload.plist).unwrap();
    }

    #[test]
    fn snpid_未知时拒绝组装而不是伪造() {
        let error = build_registration_payload("NoSnpid", "NoSnpid", None, None).unwrap_err();
        assert!(error.contains("SNPID"), "错误信息应说明缺少 SNPID: {error}");
    }

    #[test]
    fn 计划路径只落在两个系统目录内() {
        // 即便库名/RegKey 含路径分隔符，组装前就会被拒绝，不会拼出越界路径。
        assert!(plan_system_ops("../../etc/passwd", "x").is_err());
        assert!(plan_system_ops("ok", "a/b").is_err());

        let xml = registry::service_center_xml_path("Some RegKey");
        let plist = registry::system_plist_path("Some RegKey");
        assert!(xml.starts_with(registry::service_center_dir()));
        assert!(plist.starts_with(registry::system_preferences_dir()));
    }
}
