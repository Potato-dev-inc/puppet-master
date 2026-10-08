//! Tauri command for the agent registry listing (SPEC UI-2): the same body as `GET /agents/types`.

use super::*;

/// `{agents, dispatchable_ids}` for every registered agent, in registry order (SPEC 7.5).
#[tauri::command]
pub fn agents_list() -> Value {
    crate::tui::agent_types_listing()
}

fn command_error(error: crate::tui::declarative::service::ServiceError) -> String {
    format!("{}: {}", error.code, error.message)
}

/// `GET /agents/definitions`: every definition with state and errors.
#[tauri::command]
pub fn agents_definitions_list() -> Value {
    crate::tui::declarative::service::listing_global()
}

/// `POST /agents/definitions/reload`: rescan and re-register; the listing plus a diff.
#[tauri::command]
pub fn agents_definitions_reload() -> Value {
    crate::tui::declarative::service::reload_global_with_diff()
}

/// `POST /agents/definitions/validate`: registers and approves nothing.
#[tauri::command]
pub fn agents_definitions_validate(toml: String, source: Option<String>) -> Result<Value, String> {
    crate::tui::declarative::service::validate_global(&toml, source.as_deref().unwrap_or("user"))
        .map_err(command_error)
}

/// `POST /agents/definitions/approve`. Tauri commands are the webview's own channel, so this is
/// the app-session path of APR-7.
#[tauri::command]
pub fn agents_definition_approve(file: String, fingerprint: String) -> Result<Value, String> {
    crate::tui::declarative::service::approve_global(&file, &fingerprint).map_err(command_error)
}

#[tauri::command]
pub fn agents_definition_revoke(id: String) -> Result<Value, String> {
    crate::tui::declarative::service::revoke_global(&id).map_err(command_error)
}

#[tauri::command]
pub fn agents_definitions_consent(kind: String, path: String, enabled: bool) -> Result<Value, String> {
    crate::tui::declarative::service::consent(&serde_json::json!({"kind": kind, "path": path, "enabled": enabled}))
        .map_err(command_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_list_returns_the_nine_code_agents() {
        let body = agents_list();
        let ids: Vec<&str> = body["agents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|agent| agent["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "claude",
                "codex",
                "opencode",
                "opencode_native",
                "cursor_agent",
                "cursor",
                "cmd",
                "powershell",
                "bash"
            ]
        );
        assert_eq!(body["agents"][0]["label"], "Claude Code");
        assert_eq!(body["dispatchable_ids"].as_array().unwrap().len(), 5);
    }
}
