// Free/Pro 许可判定 + Dodo Payments 订阅校验（Task 5，ADR-0003 覆盖）。
//
// 自研模块，不引用任何上游项目的私有实现；Dodo 网络调用仅在存在 license_key
// 时触发，无 key 或网络失败时回退 Free（带 TTL 缓存上次结果）。
//
// 凭据经环境变量注入（.env，gitignore）：DODO_API_KEY / DODO_PRODUCT_ID。

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub mod dodo;

const STORE_FILE: &str = "music-rig.dat";
const KEY_LICENSE: &str = "license_key";
const KEY_CACHED_TIER: &str = "cached_tier";
const KEY_CACHED_AT: &str = "cached_tier_at"; // unix ms
const TTL_MS: i64 = 24 * 60 * 60 * 1000; // 24h

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LicenseTier {
    Free,
    Pro,
}

impl LicenseTier {
    pub fn is_pro(self) -> bool {
        matches!(self, LicenseTier::Pro)
    }
}

/// 读取上次缓存的 tier；命中 TTL 则直接返回，否则 None。
fn read_cached_tier(app: &AppHandle) -> Option<LicenseTier> {
    let store = app.store(STORE_FILE).ok()?;
    let tier = store.get(KEY_CACHED_TIER)?;
    let at = store.get(KEY_CACHED_AT)?;
    let tier: LicenseTier = serde_json::from_value(tier).ok()?;
    let at: i64 = serde_json::from_value(at).ok()?;
    let now = Utc::now().timestamp_millis();
    if now - at <= TTL_MS {
        Some(tier)
    } else {
        None
    }
}

fn write_cached_tier(app: &AppHandle, tier: LicenseTier) {
    if let Ok(store) = app.store(STORE_FILE) {
        if let Ok(v) = serde_json::to_value(tier) {
            let _ = store.set(KEY_CACHED_TIER, v);
        }
        if let Ok(v) = serde_json::to_value(Utc::now().timestamp_millis()) {
            let _ = store.set(KEY_CACHED_AT, v);
        }
        let _ = store.save();
    }
}

fn read_license_key(app: &AppHandle) -> Option<String> {
    let store = app.store(STORE_FILE).ok()?;
    let v = store.get(KEY_LICENSE)?;
    v.as_str().map(|s| s.to_string())
}

/// 持久化 license_key（激活时调用）。
pub fn write_license_key(app: &AppHandle, key: &str) {
    if let Ok(store) = app.store(STORE_FILE) {
        if let Ok(v) = serde_json::to_value(key) {
            let _ = store.set(KEY_LICENSE, v);
        }
        let _ = store.save();
    }
}

/// 推导当前 tier：优先 TTL 缓存；否则读 key → Dodo 校验 → 缓存并返回。
pub async fn current_tier(app: &AppHandle) -> LicenseTier {
    if let Some(cached) = read_cached_tier(app) {
        return cached;
    }
    let tier = match read_license_key(app) {
        Some(key) if !key.trim().is_empty() => {
            let valid = dodo::verify_with_dodo(key.trim()).await;
            if valid {
                LicenseTier::Pro
            } else {
                LicenseTier::Free
            }
        }
        _ => LicenseTier::Free,
    };
    write_cached_tier(app, tier);
    tier
}

/// 激活 license_key：写入 store → Dodo 校验 → 缓存 → 返回 tier。
pub async fn activate_license(app: &AppHandle, key: String) -> Result<LicenseTier, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("license key 不能为空".into());
    }
    write_license_key(app, &key);
    let tier = if dodo::verify_with_dodo(&key).await {
        LicenseTier::Pro
    } else {
        LicenseTier::Free
    };
    write_cached_tier(app, tier);
    Ok(tier)
}

/// Pro 专属命令统一拦截：非 Pro 返回错误。
pub async fn require_pro(app: &AppHandle) -> Result<(), String> {
    if current_tier(app).await.is_pro() {
        Ok(())
    } else {
        Err("此功能需要 Music Rig Pro 订阅".into())
    }
}

#[allow(dead_code)]
pub fn project_license() -> &'static str {
    "Apache-2.0"
}
