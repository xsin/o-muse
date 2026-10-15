// Dodo Payments 订阅校验（Task 5，自研实现；不引用任何上游私有代码）
// review 修复：校验必须把响应**绑定到提交的 license key**，
// 否则"账户下任意有效订阅"都会授予 Pro（越权风险）。

use std::time::Duration;

const DEFAULT_BASE: &str = "https://live.dodopayments.com";
/// 请求超时：宁可回退 Free，也不让 UI 卡住。
const TIMEOUT: Duration = Duration::from_secs(8);

/// 调用 Dodo 验证 license key 的有效性。
///
/// 约定：仅当响应中存在**与 key 对应**且状态为 active/trialing 的订阅时，
/// 才判定为 Pro。任何网络/解析/缺凭据异常均返回 false（回退 Free），
/// 绝不把错误误判为已付费。
pub async fn verify_with_dodo(key: &str) -> bool {
    let (api_key, product_id) = match credentials() {
        Some(c) => c,
        None => return false,
    };
    let base = std::env::var("DODO_API_BASE").unwrap_or_else(|_| DEFAULT_BASE.to_string());

    let client = match reqwest::Client::builder().timeout(TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return false,
    };

    // license key 走请求头而非 query string，避免泄漏到日志/代理。
    let resp = match client
        .get(format!("{base}/subscriptions"))
        .bearer_auth(&api_key)
        .header("x-license-key", key)
        .query(&[("product_id", product_id.as_str())])
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return false,
    };

    if !resp.status().is_success() {
        return false;
    }
    let body = match resp.text().await {
        Ok(t) => t,
        Err(_) => return false,
    };
    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return false,
    };

    has_active_subscription_for(&parsed, key, &product_id)
}

/// 读取 Dodo 凭据；缺失/为空则返回 None（回退 Free）。
fn credentials() -> Option<(String, String)> {
    let api_key = std::env::var("DODO_API_KEY").ok()?;
    let product_id = std::env::var("DODO_PRODUCT_ID").ok()?;
    if api_key.trim().is_empty() || product_id.trim().is_empty() {
        return None;
    }
    Some((api_key.trim().to_string(), product_id.trim().to_string()))
}

/// 判定响应中是否存在**绑定该 key** 的有效订阅。
///
/// 兼容 `items` / `subscriptions` 数组与单对象三种形态。
/// 若响应里没有任何可识别的 key 字段，则**拒绝**授予 Pro（保守策略）；
/// 这样即使上游忽略我们的过滤参数也不会误放行。
fn has_active_subscription_for(v: &serde_json::Value, key: &str, _product_id: &str) -> bool {
    let items: Vec<&serde_json::Value> = if let Some(arr) = v.get("items").and_then(|x| x.as_array())
    {
        arr.iter().collect()
    } else if let Some(arr) = v.get("subscriptions").and_then(|x| x.as_array()) {
        arr.iter().collect()
    } else {
        vec![v]
    };

    items.iter().any(|item| item_matches_key(item, key) && item_active(item))
}

/// 订阅项是否与提交的 key 对应。
///
/// 识别常见承载字段；任一匹配即视为绑定。
fn item_matches_key(item: &serde_json::Value, key: &str) -> bool {
    const KEY_FIELDS: &[&str] = &[
        "license_key",
        "license_key_id",
        "licenseKey",
        "key",
        "id",
        "subscription_id",
        "metadata.license_key",
    ];
    KEY_FIELDS.iter().any(|f| match lookup(item, f) {
        Some(s) => s == key,
        None => false,
    })
}

/// 支持一级别名路径（如 `metadata.license_key`）。
fn lookup<'a>(item: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    if let Some(rest) = field.strip_prefix("metadata.") {
        return item.get("metadata").and_then(|m| m.get(rest)).and_then(|v| v.as_str());
    }
    item.get(field).and_then(|v| v.as_str())
}

fn item_active(v: &serde_json::Value) -> bool {
    let status = v
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or_default();
    status.eq_ignore_ascii_case("active") || status.eq_ignore_ascii_case("trialing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn grants_pro_when_key_matches_and_active() {
        let v = json!({ "items": [ { "license_key": "ABC", "status": "active" } ] });
        assert!(has_active_subscription_for(&v, "ABC", "p1"));
    }

    #[test]
    fn denies_when_key_does_not_match() {
        // 账户下有其他有效订阅，但与提交的 key 无关 → 不得授予 Pro
        let v = json!({ "items": [ { "license_key": "OTHER", "status": "active" } ] });
        assert!(!has_active_subscription_for(&v, "ABC", "p1"));
    }

    #[test]
    fn denies_when_matched_subscription_not_active() {
        let v = json!({ "items": [ { "license_key": "ABC", "status": "canceled" } ] });
        assert!(!has_active_subscription_for(&v, "ABC", "p1"));
    }

    #[test]
    fn denies_when_no_key_field_present() {
        // 保守：无法绑定 key 时拒绝（防止上游忽略过滤参数导致越权）
        let v = json!({ "items": [ { "status": "active", "product_id": "p1" } ] });
        assert!(!has_active_subscription_for(&v, "ABC", "p1"));
    }

    #[test]
    fn trialing_matching_key_is_pro() {
        let v = json!({ "subscriptions": [ { "key": "ABC", "status": "trialing" } ] });
        assert!(has_active_subscription_for(&v, "ABC", "p1"));
    }

    #[test]
    fn single_object_form_works() {
        let v = json!({ "license_key": "ABC", "status": "active" });
        assert!(has_active_subscription_for(&v, "ABC", "p1"));
    }

    #[test]
    fn metadata_nested_key_works() {
        let v = json!({ "items": [ { "metadata": { "license_key": "ABC" }, "status": "active" } ] });
        assert!(has_active_subscription_for(&v, "ABC", "p1"));
    }

    #[test]
    fn no_credentials_means_free() {
        // 未配置凭据时直接回退 Free（不发起请求）
        assert!(!credentials()
            .map(|_| true)
            .unwrap_or_else(|| std::env::var("DODO_API_KEY").is_ok()));
    }
}
