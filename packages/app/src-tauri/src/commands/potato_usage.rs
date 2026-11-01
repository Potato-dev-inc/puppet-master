//! Tauri command of Settings > Potato Bot > Usage: the last 30 days, read only. Desktop only, like the rest of the
//! Potato Bot commands; the phone and the bridge never see it. Errors are one string, `CODE: plain sentence`.

use crate::potato::live;
use crate::potato::usage::UsageReport;

#[tauri::command]
pub async fn potato_usage() -> Result<UsageReport, String> {
    let service = live::service().ok_or_else(|| "POTATO_NOT_READY: Potato Bot is not ready yet. Try again in a moment.".to_string())?;
    tauri::async_runtime::spawn_blocking(move || service.usage_report().map_err(|e| e.to_string())).await.map_err(|e| format!("INTERNAL: {e}"))?
}
