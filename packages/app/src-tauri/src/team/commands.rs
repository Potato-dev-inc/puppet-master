//! Cockpit commands for Team (Tauri `invoke`, local to this app). Team traffic between teammates
//! never goes through these or through the bridge: they are the person's own clicks.

use super::service;
use serde_json::Value;

async fn blocking<T: serde::Serialize + Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Value, String> {
    let out = tauri::async_runtime::spawn_blocking(job).await.map_err(|e| e.to_string())??;
    serde_json::to_value(out).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn team_state() -> Result<Value, String> {
    blocking(service::view).await
}

#[tauri::command]
pub async fn team_config_get() -> Result<Value, String> {
    blocking(service::config_get).await
}

#[tauri::command]
pub async fn team_config_set(config: service::ConfigInput) -> Result<Value, String> {
    blocking(move || service::config_set(config)).await
}

#[tauri::command]
pub async fn team_send(draft_id: String, note: Option<String>) -> Result<Value, String> {
    blocking(move || service::send(&draft_id, note)).await
}

#[tauri::command]
pub async fn team_draft_dismiss(draft_id: String) -> Result<Value, String> {
    blocking(move || service::dismiss_draft(&draft_id)).await
}

#[tauri::command]
pub async fn team_card_dismiss(card_id: String) -> Result<Value, String> {
    blocking(move || service::dismiss_card(&card_id)).await
}

#[tauri::command]
pub async fn team_pull(card_id: String) -> Result<Value, String> {
    blocking(move || service::pull(&card_id)).await
}

#[tauri::command]
pub async fn team_queue(card_id: String, pane_id: String, when: String) -> Result<Value, String> {
    blocking(move || service::queue(&card_id, &pane_id, &when)).await
}

#[tauri::command]
pub async fn team_job_apply(job_id: String) -> Result<Value, String> {
    blocking(move || service::apply(&job_id)).await
}

#[tauri::command]
pub async fn team_job_discard(job_id: String) -> Result<Value, String> {
    blocking(move || service::discard(&job_id)).await
}

#[tauri::command]
pub async fn team_job_diff(job_id: String) -> Result<Value, String> {
    blocking(move || service::diff(&job_id)).await
}
