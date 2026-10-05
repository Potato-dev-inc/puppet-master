//! Operations tests: queries.

use super::*;

#[test]
fn unknown_agent_run_error_names_the_id_and_expected_kind() {
    let dir = temp();
    let error = latest_agent_run(&dir.to_string_lossy(), "57c03490").unwrap_err();
    assert_eq!(error.code, "AGENT_NOT_FOUND");
    assert!(error.message.contains("57c03490"));
    assert!(error.message.contains("agent run handle"));
    assert_eq!(
        error.context.get("handle").and_then(Value::as_str),
        Some("57c03490")
    );
}

fn listed_run(
    dir: &Path,
    key: &str,
    owner: &str,
    status: OperationStatus,
    finished_at_ms: Option<u64>,
) -> OperationSnapshot {
    let mut req = request(dir, key);
    req.owner_session_id = Some(owner.into());
    let mut snapshot = delegate_work(req, |_| Ok(())).unwrap();
    snapshot.status = status;
    snapshot.finished_at_ms = finished_at_ms;
    snapshot
}

#[test]
fn list_agents_keeps_recent_ended_runs_marks_ownership_and_expires_old_ones() {
    use crate::agent_runs::visible_runs;
    let dir = temp();
    let now = 10 * 24 * 60 * 60 * 1000;
    let hour = 60 * 60 * 1000;
    let runs = vec![
        listed_run(&dir, "active", "me", OperationStatus::Running, None),
        listed_run(
            &dir,
            "failed-recent",
            "me",
            OperationStatus::Failed,
            Some(now - hour),
        ),
        listed_run(
            &dir,
            "cancelled-old",
            "me",
            OperationStatus::Cancelled,
            Some(now - 48 * hour),
        ),
        listed_run(
            &dir,
            "other-conn",
            "someone-else",
            OperationStatus::Completed,
            Some(now - hour),
        ),
    ];
    let ids: Vec<_> = runs.iter().map(|run| run.operation_id.clone()).collect();
    let visible = visible_runs(runs, Some("me"), now);
    let owned_of = |id: &str| {
        visible
            .iter()
            .find(|(run, _)| run.operation_id == id)
            .map(|(_, owned)| *owned)
    };
    assert_eq!(owned_of(&ids[0]), Some(true));
    assert_eq!(owned_of(&ids[1]), Some(true), "recent failure stays listed");
    assert_eq!(
        owned_of(&ids[2]),
        None,
        "ended run past retention is dropped"
    );
    assert_eq!(
        owned_of(&ids[3]),
        Some(false),
        "other connection's run is listed but not owned"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn compact_output_baseline_strips_tui_box_art() {
    let raw = "   \n┌──────────────────────────────┐\n│ Add follow-up                │\n└──────────────────────────────┘\n\nDeepSeek V4.1 Flash · Working\n";
    let compact = super::compact_output_baseline(raw).unwrap_or_default();
    assert!(!compact.contains('┌'));
    assert!(compact.contains("DeepSeek") || compact.contains("Working"));
}

#[test]
fn compact_output_baseline_omitted_when_only_chrome() {
    let raw = "   
╭────────╮
│  ▀▀▀▀  │
╰────────╯

   
";
    assert!(super::compact_output_baseline(raw).is_none());
    let mut snapshot = create_operation(request(&temp(), "baseline-api")).unwrap().0;
    snapshot.output_baseline = Some("x".into());
    let api = super::snapshot_for_api(snapshot);
    assert!(!serde_json::to_value(api).unwrap().as_object().unwrap().contains_key("output_baseline"));
}

#[test]
fn applied_delegate_context_policy_reflects_history() {
    use super::applied_delegate_context_policy as applied;
    assert_eq!(applied(None, 0, 0), ContextPolicy::Fresh);
    assert_eq!(applied(None, 1, 0), ContextPolicy::Resume);
    assert_eq!(applied(None, 0, 2), ContextPolicy::Resume);
    assert_eq!(applied(Some(ContextPolicy::Packet), 0, 0), ContextPolicy::Packet);
}

#[test]
fn compact_output_baseline_truncates_on_char_boundary() {
    let raw = format!("a{}", "ä".repeat(400));
    let compact = super::compact_output_baseline(&raw).unwrap_or_default();
    assert!(compact.starts_with('…'));
    assert!(compact.len() <= 480 + '…'.len_utf8());
}
