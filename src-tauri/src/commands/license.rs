// 许可相关命令（Task 5）：tier 查询 + license 激活。
// Pro 专属命令入口统一经 core::license::require_pro 拦截。

use crate::core::license::{activate_license, current_tier, LicenseTier};
use serde::Serialize;
use tauri::AppHandle;

#[derive(Serialize)]
pub struct TierInfo {
    pub tier: LicenseTier,
    pub is_pro: bool,
}

#[tauri::command]
pub async fn get_tier(app: AppHandle) -> Result<TierInfo, String> {
    let tier = current_tier(&app).await;
    Ok(TierInfo {
        tier,
        is_pro: tier.is_pro(),
    })
}

#[tauri::command]
pub async fn activate_license_key(app: AppHandle, key: String) -> Result<TierInfo, String> {
    let tier = activate_license(&app, key).await?;
    Ok(TierInfo {
        tier,
        is_pro: tier.is_pro(),
    })
}
