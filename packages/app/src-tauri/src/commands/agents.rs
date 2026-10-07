//! Tauri command for the agent registry listing (SPEC UI-2): the same body as `GET /agents/types`.

use super::*;

/// `{agents, dispatchable_ids}` for every registered agent, in registry order (SPEC 7.5).
#[tauri::command]
pub fn agents_list() -> Value {
    crate::tui::agent_types_listing()
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
