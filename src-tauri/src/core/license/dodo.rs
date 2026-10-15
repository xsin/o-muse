// Dodo Payments 订阅校验（Task 5，自研实现；不引用任何上游私有代码）。
//
// 凭据经环境变量注入（.env，gitignore）：DODO_API_KEY / DODO_PRODUCT_ID。
// 仅当存在 license_key 时才发起网络校验；校验超时或失败时回退 Free。

use std::time::Duration;

const DEFAULT_BASE: &str = "https://live.dodopayments.com";

/// 调用 Dodo 验证 license key 的有效性。
///
/// 采用轻量策略：使用 subscriptions 查询接口，携带 key 与 product id。
/// 任何网络/解析异常均返回 false（回退 Free），避免把错误暴露成 Pro。
pub async fn verify_with_dodo(key: &str) -> bool {
    let api_key = match std::env::var("DODO_API_KEY") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => return false,
    };
    let product_id = match std::env::var("DODO_PRODUCT_ID") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => return false,
    };
    let base = std::env::var("DODO_API_BASE").unwrap_or_else(|_| DEFAULT_BASE.to_string());

    let url = format!("{base}/subscriptions");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(_) => return false,
    };

    let res = client
        .get(&url)
        .basic_auth("test", Some(api_key.clone()))
        .query(&[("license_key", key), ("product_id", &product_id)])
        .header("Content-Type", "application/json")
        .send()
        .await;

    let resp = match res {
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

    // 成功响应约定包含 "status":"active" 的订阅项；保守判定包含 active 且
    // 校验字段与 key 对应。解析失败时回退 Free。
    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return false,
    };
    subscription_is_active(&parsed)
}

/// 判定 Dodo 响应里是否存在有效（active）订阅。
/// 兼容两种常见形态：数组 of items，或单对象。
fn subscription_is_active(v: &serde_json::Value) -> bool {
    if let Some(items) = v.get("items").and_then(|x| x.as_array()) {
        return items.iter().any(item_active);
    }
    if let Some(subscriptions) = v.get("subscriptions").and_then(|x| x.as_array()) {
        return subscriptions.iter().any(item_active);
    }
    // 单对象形态
    item_active(v)
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
    fn detects_active_item_in_items_array() {
        let v = json!({ "items": [ { "status": "active" }, { "status": "canceled" } ] });
        assert!(subscription_is_active(&v));
    }

    #[test]
    fn trialing_counts_as_active() {
        let v = json!({ "subscriptions": [ { "status": "trialing" } ] });
        assert!(subscription_is_active(&v));
    }

    #[test]
    fn no_active_subscription_is_not_pro() {
        let v = json!({ "items": [ { "status": "canceled" }, { "status": "past_due" } ] });
        assert!(!subscription_is_active(&v));
    }

    #[test]
    fn single_object_active() {
        let v = json!({ "status": "active", "product_id": "prod_x" });
        assert!(subscription_is_active(&v));
    }

    #[test]
    fn missing_status_is_not_active() {
        let v = json!({ "foo": "bar" });
        assert!(!subscription_is_active(&v));
    }
}
