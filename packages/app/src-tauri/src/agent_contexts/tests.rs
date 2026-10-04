use super::*;

#[test]
fn lists_all_agent_profiles() {
    let profiles = list_agent_context_profiles();
    assert_eq!(profiles.len(), 8);
    assert!(profiles
        .iter()
        .any(|profile| profile.agent_type == AgentType::Codex));
    assert!(profiles.iter().all(|profile| !profile.best_for.is_empty()));
}

#[test]
fn detects_model_from_buffer_text() {
    assert_eq!(
        detect_model_from_buffer("current model: claude-sonnet-4-6").as_deref(),
        Some("claude-sonnet-4-6")
    );
    assert_eq!(
        detect_model_from_buffer("using GPT-5.1 for this session").as_deref(),
        Some("GPT-5.1")
    );
}

#[test]
fn detects_cursor_model_from_footer() {
    let inspected = inspect_agent_model("p1", AgentType::CursorAgent, "hi\nComposer 2.5 · 7.2%");
    assert_eq!(inspected.detected_model.as_deref(), Some("Composer 2.5"));
}

#[test]
fn builds_complete_pane_agent_context() {
    let pane = PaneInfo {
        id: "pane-1".to_string(),
        agent_type: "codex".to_string(),
        pid: 42,
        status: "idle".to_string(),
        created_at: 1,
        last_output_at: Some(2),
        cwd: "/tmp".to_string(),
        cols: 120,
        rows: 30,
    };
    let context = build_pane_agent_context(None, pane, "line 1\nmodel: o3\nline 3").unwrap();
    assert_eq!(context.context.agent_type, AgentType::Codex);
    assert_eq!(context.model.detected_model.as_deref(), Some("o3"));
    assert_eq!(context.model.source, "buffer");
    assert!(context.recent_buffer_preview.contains("line 3"));
}
