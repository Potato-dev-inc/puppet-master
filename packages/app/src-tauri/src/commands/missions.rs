//! Tauri commands for Missions (Preview): the start sheet, the mission view and its cards.

use crate::missions::{self, routes};
use serde_json::Value;
use tauri::AppHandle;

fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> impl std::future::Future<Output = Result<T, String>> {
    async move { tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())? }
}

fn plain(error: crate::operations::OperationError) -> String {
    error.message
}

#[tauri::command]
pub async fn mission_list(project: Option<String>) -> Result<Value, String> {
    blocking(move || Ok(routes::list_views(project.as_deref()))).await
}

#[tauri::command]
pub async fn mission_get(id: String) -> Result<Value, String> {
    blocking(move || routes::get_value(&id).map_err(plain)).await
}

#[tauri::command]
pub async fn mission_detect(project: String) -> Result<Value, String> {
    blocking(move || Ok(routes::detect_value(&project))).await
}

#[tauri::command]
pub async fn mission_start(app: AppHandle, input: Value) -> Result<Value, String> {
    let settings = crate::settings_store::read_public_settings(&app);
    blocking(move || {
        let body: routes::StartBody = serde_json::from_value(input).map_err(|e| e.to_string())?;
        routes::start_from_sheet(body, &settings).map(|m| serde_json::to_value(missions::view::view(&m)).unwrap_or_default()).map_err(plain)
    })
    .await
}

fn view_of(result: Result<missions::model::Mission, crate::operations::OperationError>) -> Result<Value, String> {
    result.map(|m| serde_json::to_value(missions::view::view(&m)).unwrap_or_default()).map_err(plain)
}

#[tauri::command]
pub async fn mission_stop(id: String) -> Result<Value, String> {
    blocking(move || view_of(missions::stop(&id, "you"))).await
}

#[tauri::command]
pub async fn mission_pause(id: String) -> Result<Value, String> {
    blocking(move || view_of(missions::pause(&id))).await
}

#[tauri::command]
pub async fn mission_resume(id: String) -> Result<Value, String> {
    blocking(move || view_of(missions::resume(&id))).await
}

#[tauri::command]
pub async fn mission_decide(id: String, choice: String) -> Result<Value, String> {
    blocking(move || view_of(missions::decide_card(&id, &choice, "you"))).await
}
