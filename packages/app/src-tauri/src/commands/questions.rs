//! Tauri commands for questions agents ask the human (`ask_user`). See `crate::questions`.

use super::*;
use crate::questions::{self, now_ms, Question, Resolution};

#[tauri::command]
pub async fn questions_list() -> Result<Vec<Question>, String> {
    Ok(questions::store().list_pending(now_ms()))
}

#[tauri::command]
pub async fn question_answer(
    app: AppHandle,
    id: String,
    choice_id: Option<String>,
    text: Option<String>,
) -> Result<Resolution, String> {
    let result = questions::store()
        .answer(&id, choice_id.as_deref(), text.as_deref(), "desktop", now_ms())
        .map_err(|e| format!("{}: {}", e.code, e.message));
    questions::routes::publish_changed(&app);
    result
}

#[tauri::command]
pub async fn question_dismiss(app: AppHandle, id: String) -> Result<Resolution, String> {
    let result = questions::store().dismiss(&id, now_ms()).map_err(|e| format!("{}: {}", e.code, e.message));
    questions::routes::publish_changed(&app);
    result
}
