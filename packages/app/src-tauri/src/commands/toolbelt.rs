//! Tauri commands for the Toolbelt (MCP servers configured once, written to every agent).
//! Thin wrappers over `crate::toolbelt::service`, which the bridge routes share.

use serde_json::Value;

fn run(action: &str, body: Value) -> Result<Value, String> {
    crate::toolbelt::service::call_live(action, &body)
}

#[tauri::command]
pub fn toolbelt_list() -> Result<Value, String> {
    run("list", Value::Null)
}

#[tauri::command]
pub fn toolbelt_add(server: Value) -> Result<Value, String> {
    run("add", serde_json::json!({ "server": server }))
}

#[tauri::command]
pub fn toolbelt_update(id: String, server: Value) -> Result<Value, String> {
    run("update", serde_json::json!({ "id": id, "server": server }))
}

#[tauri::command]
pub fn toolbelt_remove(id: String) -> Result<Value, String> {
    run("remove", serde_json::json!({ "id": id }))
}

#[tauri::command]
pub fn toolbelt_sync_preview(project: Option<String>, include_pm: Option<bool>) -> Result<Value, String> {
    run("sync_preview", serde_json::json!({ "project": project, "include_pm": include_pm.unwrap_or(false) }))
}

#[tauri::command]
pub fn toolbelt_sync_apply(project: Option<String>, include_pm: Option<bool>) -> Result<Value, String> {
    run("sync_apply", serde_json::json!({ "project": project, "include_pm": include_pm.unwrap_or(false) }))
}

#[tauri::command]
pub fn toolbelt_import_preview(project: Option<String>) -> Result<Value, String> {
    run("import_preview", serde_json::json!({ "project": project }))
}

#[tauri::command]
pub fn toolbelt_import_apply(project: Option<String>, references: Vec<String>) -> Result<Value, String> {
    run("import_apply", serde_json::json!({ "project": project, "references": references }))
}
