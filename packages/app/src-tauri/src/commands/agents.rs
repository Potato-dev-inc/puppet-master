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

/// The validation result (`ok`, `errors`, `rendered` argv) for one listed definition file, so the
/// approve dialog can show what would run (APR-3). Only files the last scan listed can be read.
#[tauri::command]
pub fn agents_definition_preview(file: String) -> Result<Value, String> {
    let wanted = std::fs::canonicalize(&file).unwrap_or_else(|_| PathBuf::from(&file));
    let entry = crate::tui::declarative::loader::last_entries()
        .into_iter()
        .find(|e| std::fs::canonicalize(&e.path).unwrap_or_else(|_| e.path.clone()) == wanted)
        .ok_or_else(|| format!("AGENT_DEF_NOT_FOUND: no listed definition file {file}"))?;
    let text = std::fs::read_to_string(&entry.path).map_err(|e| format!("AGENT_DEF_NOT_FOUND: {e}"))?;
    crate::tui::declarative::service::validate_global(&text, entry.source).map_err(command_error)
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
    fn preview_refuses_a_file_the_scan_did_not_list() {
        let error = agents_definition_preview("C:\\definitely\\not\\listed.toml".into()).unwrap_err();
        assert!(error.starts_with("AGENT_DEF_NOT_FOUND"), "{error}");
    }

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
