// 插件元数据增量缓存（v1.1 Task 8）
//
// SQLite 表结构与 plan.md §四 Task 8 一致。失效策略：对每个宿主条目
// 计算 `file_size` + `last_modified` 的组合哈希，与库内记录一致则直接
// 返回缓存行，否则重新提取并 UPSERT。
//
// 用 rusqlite（bundled，自带 SQLite 源码）以保持 core/ 无 Tauri 依赖，
// 使缓存逻辑可在单元测试中直接建库验证。

use super::extract::extract_metadata;
use super::model::{PluginFormat, PluginInfo};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

const CREATE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS plugin_cache (
    path TEXT PRIMARY KEY,
    file_size INTEGER,
    last_modified TEXT,
    name TEXT,
    vendor TEXT,
    version TEXT,
    format TEXT,
    scanned_at TEXT
);
"#;

/// 打开（并按需建表）缓存库。
pub fn open(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(CREATE_SQL)?;
    Ok(conn)
}

/// 内存库：供测试与"不落盘"场景使用。
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(CREATE_SQL)?;
    Ok(conn)
}

/// 失效键：路径 + 大小 + 修改时间。任一变化即视为失效。
pub fn cache_key(path: &std::path::Path, file_size: u64, last_modified: DateTime<Utc>) -> String {
    format!(
        "{}|{}|{}",
        path.to_string_lossy(),
        file_size,
        last_modified.timestamp_millis()
    )
}

/// 尝试从缓存读取一条记录；命中则回填 `info`。
/// 返回 true 表示命中（无需重新提取）。
pub fn try_load(conn: &Connection, info: &mut PluginInfo) -> bool {
    let key = cache_key(&info.path, info.file_size, info.last_modified);
    let mut stmt = match conn.prepare(
        "SELECT file_size, last_modified, name, vendor, version, format FROM plugin_cache WHERE path = ?1",
    ) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let row = stmt.query_row(params![info.path.to_string_lossy().as_ref()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
        ))
    });

    let Ok((size, mtime, name, vendor, version, format)) = row else {
        return false;
    };

    // 校验失效键
    let stored_key = format!("{}|{}|{}", info.path.to_string_lossy(), size, mtime);
    if stored_key != key {
        return false;
    }

    if let Some(n) = name {
        if !n.trim().is_empty() {
            info.name = n;
        }
    }
    info.vendor = vendor;
    info.version = version;
    if let Some(f) = format {
        if let Some(pf) = parse_format(&f) {
            info.format = pf;
        }
    }
    true
}

/// 写入（UPSERT）一条缓存记录。
pub fn store(conn: &Connection, info: &PluginInfo) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO plugin_cache (path, file_size, last_modified, name, vendor, version, format, scanned_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(path) DO UPDATE SET
           file_size = excluded.file_size,
           last_modified = excluded.last_modified,
           name = excluded.name,
           vendor = excluded.vendor,
           version = excluded.version,
           format = excluded.format,
           scanned_at = excluded.scanned_at",
        params![
            info.path.to_string_lossy().as_ref(),
            info.file_size as i64,
            info.last_modified.timestamp_millis().to_string(),
            info.name,
            info.vendor,
            info.version,
            info.format.as_str(),
            Utc::now().timestamp_millis().to_string(),
        ],
    )?;
    Ok(())
}

fn parse_format(s: &str) -> Option<PluginFormat> {
    match s.to_uppercase().as_str() {
        "AU" => Some(PluginFormat::AU),
        "VST" => Some(PluginFormat::VST),
        "VST3" => Some(PluginFormat::VST3),
        "AAX" => Some(PluginFormat::AAX),
        "CLAP" => Some(PluginFormat::CLAP),
        _ => Some(PluginFormat::Auto),
    }
}

/// 统计信息：命中/未命中数。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub total: usize,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.hits as f64 / self.total as f64
    }
}

/// 带缓存的批量提取：命中直接返回，未命中则提取并回写。
pub fn extract_with_cache(conn: &Connection, infos: &mut [PluginInfo]) -> CacheStats {
    let mut stats = CacheStats::default();
    for info in infos.iter_mut() {
        stats.total += 1;
        if try_load(conn, info) {
            stats.hits += 1;
        } else {
            stats.misses += 1;
            let meta = extract_metadata(info);
            super::extract::apply_metadata(info, meta);
            let _ = store(conn, info);
        }
    }
    stats
}

/// 清空缓存（供 UI "重新扫描"使用）。
pub fn clear(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("DELETE FROM plugin_cache;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample(path: std::path::PathBuf, name: &str, fmt: PluginFormat) -> PluginInfo {
        let mut i = PluginInfo::from_path(path, fmt);
        i.name = name.to_string();
        i
    }

    #[test]
    fn schema_creates_table() {
        let conn = open_in_memory().unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='plugin_cache'")
            .unwrap();
        let n: i64 = stmt.query_row([], |r| r.get(0)).unwrap_or(0);
        // query_row 会返回 name 文本；此处只验证表存在
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='plugin_cache'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0;
        assert!(exists, "plugin_cache 表应存在");
        let _ = n;
    }

    #[test]
    fn second_pass_is_full_cache_hit() {
        let dir = std::env::temp_dir().join("mr_cache_hit");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("X.vst3");
        fs::write(&f, b"data").unwrap();

        let conn = open_in_memory().unwrap();
        let mut a = vec![sample(f.clone(), "X", PluginFormat::VST3)];
        let s1 = extract_with_cache(&conn, &mut a);
        assert_eq!(s1.hits, 0, "首扫应未命中");
        assert_eq!(s1.misses, 1);

        let mut b = vec![sample(f.clone(), "X", PluginFormat::VST3)];
        let s2 = extract_with_cache(&conn, &mut b);
        assert_eq!(s2.hits, 1, "二次扫描应 100% 命中");
        assert_eq!(s2.misses, 0);
        assert_eq!(s2.hit_rate(), 1.0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn modified_file_invalidates_cache() {
        let dir = std::env::temp_dir().join("mr_cache_inv");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("Y.vst3");
        fs::write(&f, b"v1").unwrap();

        let conn = open_in_memory().unwrap();
        let mut a = vec![sample(f.clone(), "Y", PluginFormat::VST3)];
        extract_with_cache(&conn, &mut a);

        // 修改文件 → size/mtime 变化
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&f, b"v2 much longer content").unwrap();

        let mut b = vec![PluginInfo::from_path(f.clone(), PluginFormat::VST3)];
        let s = extract_with_cache(&conn, &mut b);
        assert_eq!(s.misses, 1, "文件变更后应重新提取");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_empties_cache() {
        let conn = open_in_memory().unwrap();
        let dir = std::env::temp_dir().join("mr_cache_clr");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("Z.clap");
        fs::write(&f, b"z").unwrap();

        let mut a = vec![sample(f.clone(), "Z", PluginFormat::CLAP)];
        extract_with_cache(&conn, &mut a);
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM plugin_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, 1);

        clear(&conn).unwrap();
        let cnt2: i64 = conn
            .query_row("SELECT COUNT(*) FROM plugin_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt2, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_key_changes_with_size_and_mtime() {
        let p = std::path::PathBuf::from("/a/b.vst3");
        let t1 = DateTime::<Utc>::from(std::time::UNIX_EPOCH);
        let t2 = t1 + chrono::Duration::seconds(5);
        assert_ne!(cache_key(&p, 100, t1), cache_key(&p, 200, t1));
        assert_ne!(cache_key(&p, 100, t1), cache_key(&p, 100, t2));
        assert_eq!(cache_key(&p, 100, t1), cache_key(&p, 100, t1));
    }
}
