//! Tauri commands for the attention model (see `docs/features/attention.md`).

/// The current attention snapshot: the same JSON as `GET /attention`.
#[tauri::command]
pub async fn attention_snapshot() -> Result<serde_json::Value, String> {
    Ok(crate::attention::routes::get(None))
}
