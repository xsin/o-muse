// 逻辑参考开源 Kontakt 库管理工具（KLM, https://github.com/cloinse/klm）在 MIT 许可下实现，本模块为独立重写，不复制其源码。
//
// `.nicnt`（NI FC 容器）解析与构造。
//
// 容器实测结构（Kontakt 8，macOS）：
//   [0x00]  `/\ NI FC MTD  /\` 魔数
//   [0x40]  UTF-16LE 版本号 "1.0.0"
//   [0x100] `<?xml ... ?>\n<ProductHints ...>…</ProductHints>`（NUL 填充对齐）
//   [0x7D100] 第二个 MTD 区块
//   [0x7D220] `/\ NI FC TOC  /\` 目录区块
//   [0x7FA20] 第二个目录区块
//   [0x7FBC0] 起为 PNG 等资源
// 解析只读；仅在用户明确提交表单时构造新的最小容器，绝不改写既有文件。

use crate::core::kontakt::model::{NicntFormSpec, ProductMetadata};
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use quick_xml::XmlVersion;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

/// 元数据区块魔数。
pub const NI_FC_MTD_MAGIC: &[u8] = b"/\\ NI FC MTD  /\\";
/// 目录区块魔数。
pub const NI_FC_TOC_MAGIC: &[u8] = b"/\\ NI FC TOC  /\\";
/// XML 声明：Service Center XML 与本容器内的片段一致，逐字节保留。
const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n";
/// XML 片段在容器中的起始偏移。
const XML_OFFSET: usize = 0x100;
/// 第一个元数据区块结束（第二个 MTD 魔数位置）。
const MTD_BLOCK_END: usize = 512_256;
/// 第一个目录魔数位置。
const TOC1_OFFSET: usize = 512_544;
/// 第二个目录魔数位置。
const TOC2_OFFSET: usize = 522_784;
/// 最小容器总长度（其后通常跟随 PNG 资源）。
const CONTAINER_END: usize = 523_392;

type XmlR = XmlReader<BufReader<Cursor<Vec<u8>>>>;

/// `.nicnt` / `*_info.nkx` 的解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicntPayload {
    pub metadata: ProductMetadata,
    /// 原始 XML 片段（含 XML 声明），逐字节保留，写入时原样落盘。
    pub raw_xml: String,
}

// ── 解析 ──────────────────────────────────────────────────────

/// 解析 `.nicnt` 或 `*_info.nkx` 文件。
pub fn parse_metadata_file(path: &Path) -> Result<NicntPayload, String> {
    let data =
        std::fs::read(path).map_err(|e| format!("读取元数据文件 {} 失败: {e}", path.display()))?;
    parse_nicnt_bytes(&data).map_err(|e| format!("解析 {} 失败: {e}", path.display()))
}

/// 从字节流解析 NI FC 容器中的 `ProductHints`。
///
/// 只读：不会修改任何文件。
pub fn parse_nicnt_bytes(data: &[u8]) -> Result<NicntPayload, String> {
    let fragment = extract_xml_fragment(data)?;
    let raw_xml =
        String::from_utf8(fragment).map_err(|_| "ProductHints 片段不是合法 UTF-8".to_string())?;
    let metadata = parse_product_hints(&raw_xml)?;
    validate_required_metadata(&metadata)?;
    Ok(NicntPayload { metadata, raw_xml })
}

/// 取出 `<ProductHints …>` 到 `</ProductHints>` 的原始片段（含 XML 声明），并去掉 NUL 填充。
///
/// 保留原始字节意味着保留属性顺序、缩进与换行——Kontakt 依赖这些细节。
fn extract_xml_fragment(data: &[u8]) -> Result<Vec<u8>, String> {
    const OPEN: &[u8] = b"<ProductHints";
    const CLOSE: &[u8] = b"</ProductHints>";

    let open = find_subslice(data, OPEN).ok_or_else(|| "未找到 <ProductHints> 片段".to_string())?;
    let close_rel = find_subslice(&data[open..], CLOSE)
        .ok_or_else(|| "ProductHints 片段不完整（缺 </ProductHints>）".to_string())?;
    let end = open + close_rel + CLOSE.len();

    // 向前回溯 XML 声明；仅当声明与根元素之间只有 NUL 或空白时才并入片段。
    let mut start = open;
    let window_start = open.saturating_sub(512);
    if let Some(decl_rel) = rfind_subslice(&data[window_start..open], b"<?xml") {
        let decl_abs = window_start + decl_rel;
        let between = &data[decl_abs..open];
        let decl_end = find_subslice(between, b"?>")
            .map(|i| i + 2)
            .unwrap_or(between.len());
        if between[decl_end..]
            .iter()
            .all(|b| *b == 0 || b.is_ascii_whitespace())
        {
            start = decl_abs;
        }
    }

    Ok(data[start..end]
        .iter()
        .copied()
        .filter(|b| *b != 0)
        .collect())
}

/// 解析 `ProductHints` XML 文本。
///
/// 容错策略：未知元素忽略；`Visibility` 优先取 `ProductSpecific/Visibility`
/// （Kontakt 的生效值），缺失时回退到顶层 `Visibility target="Standalone"`。
pub fn parse_product_hints(xml: &str) -> Result<ProductMetadata, String> {
    let mut md = ProductMetadata::default();
    let mut reader: XmlR =
        XmlReader::from_reader(BufReader::new(Cursor::new(xml.as_bytes().to_vec())));

    let mut buf = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut app_min: Option<String> = None;
    let mut top_visibility: Option<i32> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                if name == "Application" {
                    app_min = None;
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == "minVersion" {
                            app_min = attr
                                .normalized_value(XmlVersion::Implicit1_0)
                                .ok()
                                .map(|v| v.into_owned());
                        }
                    }
                }
                stack.push(name);
                text.clear();
            }
            Ok(Event::Empty(e)) => {
                // 自闭合元素没有文本内容；只记录可能的 Application 属性。
                if local_name(e.name().as_ref()) == "Application" {
                    app_min = None;
                }
            }
            Ok(Event::Text(t)) => text.push_str(&t),
            Ok(Event::CData(t)) => {
                text.push_str(t.as_ref());
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some(ch) = resolve_entity(r.as_ref()) {
                    text.push(ch);
                }
            }
            Ok(Event::End(_)) => {
                let path = stack.join("/");
                let value = text.trim();
                if !value.is_empty() {
                    apply_field(
                        &mut md,
                        &mut top_visibility,
                        &path,
                        value,
                        app_min.as_deref(),
                    );
                }
                stack.pop();
                text.clear();
            }
            Ok(Event::Eof) => break,
            // 声明、注释、PI、DOCTYPE 一律忽略。
            Ok(_) => {}
            Err(e) => return Err(format!("ProductHints XML 解析失败: {e}")),
        }
        buf.clear();
    }

    md.visibility = md.visibility.or(top_visibility);
    Ok(md)
}

/// 把某个元素路径上的文本落到元数据字段。
fn apply_field(
    md: &mut ProductMetadata,
    top_visibility: &mut Option<i32>,
    path: &str,
    value: &str,
    app_min: Option<&str>,
) {
    match path {
        "ProductHints/Product/Name" => md.name = value.to_string(),
        "ProductHints/Product/RegKey" => md.reg_key = value.to_string(),
        "ProductHints/Product/SNPID" => md.snpid = value.to_string(),
        "ProductHints/Product/UPID" => md.upid = Some(value.to_string()),
        "ProductHints/Product/AuthSystem" => md.auth_system = Some(value.to_string()),
        "ProductHints/Product/Company" => md.company = Some(value.to_string()),
        "ProductHints/Product/PoweredBy" => md.powered_by = Some(value.to_string()),
        "ProductHints/Product/Type" => md.product_type = Some(value.to_string()),
        "ProductHints/Product/Icon" => md.icon = Some(value.to_string()),
        "ProductHints/Product/Visibility" => *top_visibility = value.parse::<i32>().ok(),
        "ProductHints/Product/ProductSpecific/HU" => md.hu = Some(value.to_string()),
        "ProductHints/Product/ProductSpecific/JDX" => md.jdx = Some(value.to_string()),
        "ProductHints/Product/ProductSpecific/Visibility" => {
            md.visibility = value.parse::<i32>().ok()
        }
        "ProductHints/Product/Relevance/Application" => {
            md.applications.push(value.to_string());
            if value.eq_ignore_ascii_case("kontakt") {
                if let Some(v) = app_min {
                    md.minimum_kontakt_version = Some(v.to_string());
                }
            }
        }
        _ => {}
    }
}

/// 解析 XML 实体引用（`GeneralRef` 事件只给出 `&` 与 `;` 之间的内容）。
fn resolve_entity(raw: &str) -> Option<char> {
    match raw {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let num = raw.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse::<u32>().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// 去掉命名空间前缀，仅保留局部名。
fn local_name(raw: &str) -> String {
    match raw.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => raw.to_string(),
    }
}

/// 必填字段校验：Name / RegKey / SNPID 非空且可作为文件名。
fn validate_required_metadata(md: &ProductMetadata) -> Result<(), String> {
    for (field, value) in [
        ("Name", md.name.as_str()),
        ("RegKey", md.reg_key.as_str()),
        ("SNPID", md.snpid.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(format!("ProductHints 缺少必填字段 {field}"));
        }
        validate_component(value, field)?;
    }
    Ok(())
}

/// 校验名称类字段：非空、不含路径分隔符与控制字符、不是 `.` / `..`。
pub fn validate_component(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{field} 不能为空"));
    }
    if value == "." || value == ".." {
        return Err(format!("{field} 不能是 `.` 或 `..`"));
    }
    for ch in value.chars() {
        if matches!(ch, '/' | '\\' | '\n' | '\r') {
            return Err(format!("{field} 不能包含字符 `{ch}`"));
        }
    }
    Ok(())
}

/// 在目录内查找元数据文件：优先 `.nicnt`（忽略 `._` 前缀），否则 `*_info.nkx`。
pub fn find_metadata_file(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut nicnt: Vec<PathBuf> = Vec::new();
    let mut nkx: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with("._") {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".nicnt") {
            nicnt.push(path);
        } else if lower.ends_with("_info.nkx") {
            nkx.push(path);
        }
    }
    nicnt.sort();
    nkx.sort();
    nicnt.into_iter().next().or_else(|| nkx.into_iter().next())
}

// ── 构造（仅用于用户显式创建，且只写用户目录）────────────────

/// 生成 UUID v4 格式字符串。
///
/// 不引入新依赖：以时间戳、进程号与自增序号做 SHA-256，取前 16 字节，
/// 按 RFC 4122 设置版本位与变体位。
pub fn generate_upid() -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(seq.to_le_bytes());
    let digest = hasher.finalize();

    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    let mut hex = String::with_capacity(32);
    for b in bytes {
        hex.push_str(&format!("{b:02x}"));
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// 校验可选的授权字段：只接受十六进制串，避免写入无意义的垃圾。
fn validate_hex_optional(value: &Option<String>, field: &str) -> Result<(), String> {
    let raw = match value {
        Some(v) if !v.trim().is_empty() => v.trim(),
        _ => return Ok(()),
    };
    if !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{field} 只能包含十六进制字符"));
    }
    Ok(())
}

/// 渲染 `ProductHints` XML（含 XML 声明）。
///
/// 不伪造 `HU` / `JDX`：未提供时该元素整体省略。
/// 顶层 `Visibility target="Standalone"` 固定为 7（真实库一致），
/// 生效可见性写入 `ProductSpecific/Visibility`。
pub fn render_product_hints(spec: &NicntFormSpec) -> Result<String, String> {
    validate_component(&spec.name, "Name")?;
    validate_component(&spec.reg_key, "RegKey")?;
    validate_component(&spec.snpid, "SNPID")?;
    validate_hex_optional(&spec.hu, "HU")?;
    validate_hex_optional(&spec.jdx, "JDX")?;

    let upid = match &spec.upid {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => generate_upid(),
    };
    let product_type = if spec.product_type.trim().is_empty() {
        "Content"
    } else {
        spec.product_type.trim()
    };
    let company = escape_xml(spec.company.trim());

    let mut xml = String::new();
    xml.push_str(XML_DECL);
    xml.push_str("<ProductHints spec=\"1.0.16\">\n\n");
    xml.push_str("  <Product version=\"1\">\n");
    xml.push_str(&format!("    <UPID>{}</UPID>\n", escape_xml(&upid)));
    xml.push_str(&format!("    <Name>{}</Name>\n", escape_xml(&spec.name)));
    xml.push_str(&format!("    <Type>{}</Type>\n", escape_xml(product_type)));
    xml.push_str("    <NKSEnabled>true</NKSEnabled>\n");
    xml.push_str("    <Relevance>\n");
    xml.push_str(
        "      <Application minVersion=\"7.8.1.0\" nativeContent=\"true\">kontakt</Application>\n",
    );
    xml.push_str("      <Application minVersion=\"2.6.5.0\">maschine</Application>\n");
    xml.push_str("      <Application minVersion=\"1.8.2.0\">kkontrol</Application>\n");
    xml.push_str("    </Relevance>\n");
    xml.push_str("    <PoweredBy>Kontakt</PoweredBy>\n");
    xml.push_str("    <Visibility target=\"Standalone\">7</Visibility>\n");
    xml.push_str(&format!("    <Company>{company}</Company>\n"));
    if let Some(auth) = spec.auth_system.as_deref().map(str::trim) {
        if !auth.is_empty() {
            xml.push_str(&format!(
                "    <AuthSystem>{}</AuthSystem>\n",
                escape_xml(auth)
            ));
        }
    }
    xml.push_str(&format!("    <SNPID>{}</SNPID>\n", escape_xml(&spec.snpid)));
    xml.push_str(&format!(
        "    <RegKey>{}</RegKey>\n",
        escape_xml(&spec.reg_key)
    ));
    xml.push_str("    <ProductSpecific>\n");
    if let Some(hu) = spec.hu.as_deref().map(str::trim) {
        if !hu.is_empty() {
            xml.push_str(&format!("      <HU>{}</HU>\n", escape_xml(hu)));
        }
    }
    if let Some(jdx) = spec.jdx.as_deref().map(str::trim) {
        if !jdx.is_empty() {
            xml.push_str(&format!("      <JDX>{}</JDX>\n", escape_xml(jdx)));
        }
    }
    xml.push_str(&format!(
        "      <Visibility type=\"Number\">{}</Visibility>\n",
        spec.visibility
    ));
    xml.push_str("    </ProductSpecific>\n");
    xml.push_str("  </Product>\n\n");
    xml.push_str("</ProductHints>\n");
    Ok(xml)
}

/// 构造最小可用 NI FC 容器（无 PNG 资源，目录区块留空）。
pub fn build_container(spec: &NicntFormSpec) -> Result<Vec<u8>, String> {
    let xml = render_product_hints(spec)?;
    let xml_bytes = xml.as_bytes();
    if XML_OFFSET + xml_bytes.len() >= MTD_BLOCK_END {
        return Err("ProductHints 片段过长，超出元数据区块容量".to_string());
    }

    let mut buf = vec![0u8; CONTAINER_END];
    buf[..NI_FC_MTD_MAGIC.len()].copy_from_slice(NI_FC_MTD_MAGIC);

    // 0x40 处的 UTF-16LE 版本号（真实容器为 "1.0.0"）。
    let version: Vec<u8> = "1.0.0"
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    buf[0x40..0x40 + version.len()].copy_from_slice(&version);

    buf[XML_OFFSET..XML_OFFSET + xml_bytes.len()].copy_from_slice(xml_bytes);
    buf[MTD_BLOCK_END..MTD_BLOCK_END + NI_FC_MTD_MAGIC.len()].copy_from_slice(NI_FC_MTD_MAGIC);
    buf[TOC1_OFFSET..TOC1_OFFSET + NI_FC_TOC_MAGIC.len()].copy_from_slice(NI_FC_TOC_MAGIC);
    buf[TOC2_OFFSET..TOC2_OFFSET + NI_FC_TOC_MAGIC.len()].copy_from_slice(NI_FC_TOC_MAGIC);
    Ok(buf)
}

/// 在指定目录写入 `<Name>.nicnt`。
///
/// 仅写用户选择的库目录；同名文件已存在时直接报错，绝不覆盖。
pub fn write_nicnt(dir: &Path, spec: NicntFormSpec) -> Result<PathBuf, String> {
    validate_component(&spec.name, "Name")?;
    validate_component(&spec.reg_key, "RegKey")?;
    validate_component(&spec.snpid, "SNPID")?;
    if !dir.is_dir() {
        return Err(format!("目标不是目录: {}", dir.display()));
    }
    super::registry::ensure_no_symlink(dir)?;

    let target = dir.join(format!("{}.nicnt", spec.name));
    if target.exists() {
        return Err(format!("目标已存在，拒绝覆盖: {}", target.display()));
    }
    let bytes = build_container(&spec)?;
    super::registry::atomic_write_new(&target, &bytes)?;
    Ok(target)
}

// ── 工具 ──────────────────────────────────────────────────────

/// XML 文本转义。
fn escape_xml(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 子串首次出现的位置。
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 子串最后一次出现的位置。
fn rfind_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按实测布局拼一个含 ProductHints 的容器。
    fn fake_container(xml: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(NI_FC_MTD_MAGIC);
        buf.resize(XML_OFFSET, 0);
        buf.extend_from_slice(xml.as_bytes());
        buf.resize(MTD_BLOCK_END, 0);
        buf.extend_from_slice(NI_FC_MTD_MAGIC);
        buf.resize(TOC1_OFFSET, 0);
        buf.extend_from_slice(NI_FC_TOC_MAGIC);
        buf
    }

    /// 夹具根目录：规范化后使用，避免 macOS `/var` → `/private/var` 这类系统符号链接
    /// 触发 `ensure_no_symlink` 的拒绝策略。
    fn fixture_root(tag: &str) -> PathBuf {
        let base = std::fs::canonicalize(std::env::temp_dir()).expect("临时目录可用");
        let dir = base.join(format!("omuse_kontakt_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const SAMPLE_XML: &str = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n",
        "<ProductHints spec=\"1.0.16\">\n\n",
        "  <Product version=\"1\">\n",
        "    <UPID>1deba36d-3048-4d4f-9bc8-bf1b35f5eaf6</UPID>\n",
        "    <Name>Altera Keys</Name>\n",
        "    <Type>Content</Type>\n",
        "    <NKSEnabled>true</NKSEnabled>\n",
        "    <Relevance>\n",
        "      <Application minVersion=\"7.8.1.0\" nativeContent=\"true\">kontakt</Application>\n",
        "      <Application minVersion=\"2.6.5.0\">maschine</Application>\n",
        "    </Relevance>\n",
        "    <PoweredBy>Kontakt</PoweredBy>\n",
        "    <Visibility target=\"Standalone\">7</Visibility>\n",
        "    <Company>Impact Soundworks</Company>\n",
        "    <AuthSystem>RAS3</AuthSystem>\n",
        "    <SNPID>1c7</SNPID>\n",
        "    <RegKey>Altera Keys</RegKey>\n",
        "    <ProductSpecific>\n",
        "      <HU>4A2B08CEE0D9E49A5DB0F10CE5495664</HU>\n",
        "      <JDX>A5EC2DA03A8DFBEB07ECA0DF92D38F8FFED788ADD32E2391911D4B8DE300B930</JDX>\n",
        "      <Visibility type=\"Number\">3</Visibility>\n",
        "    </ProductSpecific>\n",
        "  </Product>\n\n",
        "</ProductHints>\n"
    );

    #[test]
    fn nicnt_解析基础字段() {
        let data = fake_container(SAMPLE_XML);
        let payload = parse_nicnt_bytes(&data).expect("应能解析");

        assert_eq!(payload.metadata.name, "Altera Keys");
        assert_eq!(payload.metadata.reg_key, "Altera Keys");
        assert_eq!(payload.metadata.snpid, "1c7");
        // 生效可见性取 ProductSpecific/Visibility
        assert_eq!(payload.metadata.visibility, Some(3));
        assert_eq!(
            payload.metadata.upid.as_deref(),
            Some("1deba36d-3048-4d4f-9bc8-bf1b35f5eaf6")
        );
        assert_eq!(payload.metadata.auth_system.as_deref(), Some("RAS3"));
        assert_eq!(
            payload.metadata.company.as_deref(),
            Some("Impact Soundworks")
        );
        assert_eq!(payload.metadata.product_type.as_deref(), Some("Content"));
        assert!(payload.metadata.is_content_library());
        assert_eq!(
            payload.metadata.minimum_kontakt_version.as_deref(),
            Some("7.8.1.0")
        );
        assert_eq!(payload.metadata.applications.len(), 2);
        assert!(payload.metadata.hu.is_some() && payload.metadata.jdx.is_some());
    }

    #[test]
    fn nicnt_非法输入不panic() {
        // 空文件
        assert!(parse_nicnt_bytes(b"").is_err());
        // 无 ProductHints
        assert!(parse_nicnt_bytes(b"random binary \x00\x01\x02 payload").is_err());
        // 只有开标签，缺字段
        assert!(
            parse_nicnt_bytes(b"<ProductHints spec=\"1.0.16\"><Product><Name>X</Product>").is_err()
        );
        // 损坏 XML（非法尖括号）
        assert!(parse_nicnt_bytes(b"<ProductHints><<>></ProductHints>").is_err());
        // 缺 SNPID
        let missing = fake_container(
            "<ProductHints spec=\"1.0.16\"><Product><Name>A</Name><RegKey>A</RegKey></Product></ProductHints>",
        );
        assert!(parse_nicnt_bytes(&missing).is_err());
        // 名称含路径分隔符
        let slash = fake_container(
            "<ProductHints spec=\"1.0.16\"><Product><Name>a/b</Name><RegKey>a</RegKey><SNPID>x</SNPID></Product></ProductHints>",
        );
        assert!(parse_nicnt_bytes(&slash).is_err());
    }

    #[test]
    fn nicnt_原始片段逐字节保留() {
        let data = fake_container(SAMPLE_XML);
        let payload = parse_nicnt_bytes(&data).unwrap();
        // 片段以 XML 声明开头、以 </ProductHints> 结尾，且与原文完全一致（未重新格式化）
        assert!(payload
            .raw_xml
            .starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>"));
        assert!(payload.raw_xml.ends_with("</ProductHints>"));
        assert_eq!(payload.raw_xml, SAMPLE_XML.trim_end_matches('\n'));
        assert!(!payload.raw_xml.contains('\0'));
    }

    #[test]
    fn 构造的容器可被自身解析() {
        let dir = fixture_root("nicnt");

        let spec = NicntFormSpec {
            name: "Test Library".to_string(),
            reg_key: "Test Library".to_string(),
            snpid: "9Z9".to_string(),
            company: "O Muse".to_string(),
            ..NicntFormSpec::default()
        };
        let path = write_nicnt(&dir, spec.clone()).expect("应能写入");
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "Test Library.nicnt"
        );

        let payload = parse_metadata_file(&path).expect("应能解析回读");
        assert_eq!(payload.metadata.name, "Test Library");
        assert_eq!(payload.metadata.snpid, "9Z9");
        assert_eq!(payload.metadata.visibility, Some(3));
        // 未提供授权字段 => 不写 HU/JDX
        assert!(payload.metadata.hu.is_none());
        assert!(payload.metadata.jdx.is_none());
        assert!(payload.metadata.upid.is_some());

        // 同名文件已存在 => 拒绝覆盖
        assert!(write_nicnt(&dir, spec).is_err());
        assert_eq!(std::fs::read(&path).unwrap().len(), CONTAINER_END);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 资源文件查找忽略下划线前缀() {
        let dir = fixture_root("find");

        std::fs::write(dir.join("._Hidden.nicnt"), b"x").unwrap();
        assert!(find_metadata_file(&dir).is_none());

        std::fs::write(dir.join("Real.nicnt"), b"x").unwrap();
        let found = find_metadata_file(&dir).unwrap();
        assert_eq!(found.file_name().unwrap().to_str().unwrap(), "Real.nicnt");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 生成的_upid_为_uuid_v4_格式() {
        let a = generate_upid();
        let b = generate_upid();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert_eq!(&a[14..15], "4");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"));
    }
}
