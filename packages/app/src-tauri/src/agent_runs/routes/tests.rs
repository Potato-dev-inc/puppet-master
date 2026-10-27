//! Route helper tests: list entries, pane/handle resolution, query decoding.

use super::list::pane_worker_entry;
use super::wire::percent_decode;
use super::*;
use serde_json::json;

#[test]
fn percent_decode_handles_project_paths_and_plus() {
    assert_eq!(
        percent_decode("C%3A%2FMy+Project%2Frepo"),
        "C:/My Project/repo"
    );
}

#[test]
fn pane_worker_entry_exposes_adoption_fields() {
    let pane = crate::pty::PaneInfo {
        id: "30313db4-8896-46ee-8bd6-556b9cb0a014".into(),
        agent_type: "opencode_native".into(),
        pid: 1,
        status: "running".into(),
        created_at: 0,
        last_output_at: None,
        cwd: "C:/work/puppet-master/packages/app/src-tauri".into(),
        cols: 80,
        rows: 24,
        viewer_held: false,
        reprints_on_resize: false,
        account: None,
        account_label: None,
        title: None,
        parent_pane_id: None,
    };
    let entry = pane_worker_entry(&pane, Some("mcp-session"));
    assert_eq!(entry["worker_id"], pane.id);
    assert_eq!(entry["backend"], "opencode_native");
    assert_eq!(entry["workspace"], pane.cwd);
    assert_eq!(entry["grant_required"], true);
    assert_eq!(entry["read_only_supported"], false);
    assert_eq!(entry["adoptable"], true);
}

fn pane_bound_run(with_run: bool) -> (String, Arc<Mutex<crate::pty::PaneRegistry>>, String) {
    let project = std::env::temp_dir()
        .join(format!("pm-pane-as-handle-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();
    std::fs::create_dir_all(&project).unwrap();
    let pane_id = format!("pane-{}", uuid::Uuid::new_v4());
    let registry = Arc::new(Mutex::new(crate::pty::PaneRegistry::default()));
    registry.lock().panes.insert(
        pane_id.clone(),
        crate::pty::PaneRegistry::test_pane_stub(&pane_id),
    );
    if with_run {
        let req = crate::operations::DelegateWorkRequest {
            project_path: project.clone(),
            task: "t".into(),
            agent_type: "opencode_native".into(),
            pane_id: Some(pane_id.clone()),
            idempotency_key: format!("k-{pane_id}"),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 60_000,
            read_only: false,
            keep_pane: true,
            placement: None,
            owner_session_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: Some("run-handle-1".into()),
            turn_index: 0,
            worker: crate::operations::WorkerPersist::default(),
            context_policy: None,
            checks: Vec::new(),
        };
        crate::operations::create_operation(req).unwrap();
    }
    (project, registry, pane_id)
}

fn assert_pane_as_handle_message(error: &OperationError, pane_id: &str, run: Option<&str>) {
    assert_eq!(error.code, "INVALID_AGENT_HANDLE");
    assert!(error.message.contains("a pane id"), "{}", error.message);
    assert!(error.message.contains("not an agent run handle"));
    assert!(error.message.contains("run_agent result field `handle`"));
    assert!(error.message.contains("list_agents `handle`"));
    assert!(error.message.contains(pane_id));
    match run {
        Some(handle) => assert!(error.message.contains(&format!("`{handle}`"))),
        None => assert!(error.message.contains("no known run")),
    }
}

#[test]
fn wait_agents_inspect_and_transcript_explain_pane_id_given_as_handle() {
    let (project, registry, pane_id) = pane_bound_run(true);
    let request: WaitAgentsRequest =
        serde_json::from_value(json!({"handles": [pane_id.clone()]})).unwrap();
    let wait = wait_agents(&project, request, None, &registry).unwrap_err();
    assert_pane_as_handle_message(&wait, &pane_id, Some("run-handle-1"));
    let inspect = resolve_run_handle(&project, &pane_id, &registry)
        .and_then(|handle| runtime::inspect_agent(&project, &handle, None))
        .unwrap_err();
    assert_pane_as_handle_message(&inspect, &pane_id, Some("run-handle-1"));
    let transcript = resolve_run_handle(&project, &pane_id, &registry)
        .and_then(|handle| transcript(&project, &handle, "", None))
        .unwrap_err();
    assert_pane_as_handle_message(&transcript, &pane_id, Some("run-handle-1"));
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn pane_id_without_known_run_still_names_kinds_and_sources() {
    let (project, registry, pane_id) = pane_bound_run(false);
    let error = resolve_run_handle(&project, &pane_id, &registry).unwrap_err();
    assert_pane_as_handle_message(&error, &pane_id, None);
    let _ = std::fs::remove_dir_all(&project);
}

fn named_run(project: &str, handle: &str, name: &str, turn: u32) {
    let req = crate::operations::DelegateWorkRequest {
        project_path: project.to_string(),
        task: "t".into(),
        agent_type: "opencode_native".into(),
        pane_id: None,
        idempotency_key: format!("k-{handle}-{turn}"),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 60_000,
        read_only: false,
        keep_pane: true,
        placement: None,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: Some(handle.into()),
        turn_index: turn,
        worker: crate::operations::WorkerPersist {
            name: Some(name.into()),
            ..Default::default()
        },
        context_policy: None,
        checks: Vec::new(),
    };
    crate::operations::create_operation(req).unwrap();
}

#[test]
fn persisted_worker_name_resolves_to_handle_when_unique() {
    let (project, registry, _pane) = pane_bound_run(false);
    let uniq = uuid::Uuid::new_v4().simple().to_string();
    let (luna, nova) = (format!("luna-{uniq}"), format!("nova-{uniq}"));
    let (run_luna, run_luna_2, run_nova) = (
        format!("run-{luna}"),
        format!("run-{luna}-2"),
        format!("run-{nova}"),
    );
    named_run(&project, &run_luna, &luna, 0);
    named_run(&project, &run_luna, &luna, 1);
    named_run(&project, &run_nova, &nova, 0);
    // Same resolution a fresh process uses after restart: name -> persisted handle.
    assert_eq!(
        resolve_run_handle(&project, &luna, &registry).unwrap(),
        run_luna
    );
    assert_eq!(
        super::super::persist::find_worker(&project, &luna)
            .unwrap()
            .agent_run_id,
        run_luna
    );
    assert!(resolve_run_handle(&project, "unknown-name", &registry).is_err());
    named_run(&project, &run_luna_2, &luna, 0);
    let error = resolve_run_handle(&project, &luna, &registry).unwrap_err();
    assert_eq!(error.code, "AMBIGUOUS_WORKER_NAME");
    let _ = std::fs::remove_dir_all(&project);
}

/// The index names every project that ever ran an operation, including ones other tests (or, in
/// the app, the user) are deleting right now. On Windows a file in a directory mid-`remove_dir_all`
/// reads as "access denied". Such a foreign project must never fail a lookup that its own project
/// can answer: this was the rare `persisted_worker_name_resolves_to_handle_when_unique` flake.
#[test]
fn unreadable_foreign_project_does_not_fail_handle_or_name_lookup() {
    struct Index(std::path::PathBuf);
    impl Drop for Index {
        fn drop(&mut self) {
            crate::operations::set_test_index_dir(None);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let index = std::env::temp_dir().join(format!("pm-foreign-index-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&index).unwrap();
    crate::operations::set_test_index_dir(Some(index.clone()));
    let _index = Index(index);

    let (project, registry, _pane) = pane_bound_run(false);
    let (foreign, _registry, _pane) = pane_bound_run(false);
    let uniq = uuid::Uuid::new_v4().simple().to_string();
    let (luna, run_luna) = (format!("luna-{uniq}"), format!("run-luna-{uniq}"));
    named_run(&foreign, "run-foreign", &format!("foreign-{uniq}"), 0);
    named_run(&project, &run_luna, &luna, 0);
    // Make every record of the foreign project unreadable: replace each with a directory.
    let records = std::path::Path::new(&foreign).join(".puppet-master").join("operations");
    for entry in std::fs::read_dir(&records).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "json") {
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
        }
    }
    let by_name = super::super::persist::find_worker(&project, &luna).unwrap();
    assert_eq!(by_name.agent_run_id, run_luna);
    assert_eq!(
        super::super::persist::find_worker(&project, &run_luna).unwrap().agent_run_id,
        run_luna
    );
    assert_eq!(resolve_run_handle(&project, &luna, &registry).unwrap(), run_luna);
    // A project that cannot be read is skipped, not reported as a hit.
    assert!(super::super::persist::find_worker(&project, "no-such-worker").is_err());
    let _ = std::fs::remove_dir_all(&project);
    let _ = std::fs::remove_dir_all(&foreign);
}

/// R8 verify finding: `POST /agents/answer {handle, choice}` (no `prompt_id`) answered 503
/// INVALID_JSON. A body the caller got wrong is a 4xx with a typed code that names the field.
#[test]
fn a_request_body_with_a_missing_field_is_a_typed_4xx_that_names_the_field() {
    use super::wire::{parse, AnswerPromptRequest};
    let error = parse::<AnswerPromptRequest>(br#"{"handle":"h1","choice":"deny"}"#)
        .expect_err("prompt_id is required");
    assert_eq!(error.code, "MISSING_FIELD");
    assert_eq!(error.context["field"], "prompt_id");
    assert!(!error.recoverable);
}

#[test]
fn a_request_body_that_is_not_json_or_has_a_wrong_type_is_a_typed_4xx() {
    use super::wire::{parse, AnswerPromptRequest};
    for (body, code) in [
        (&br#"{"handle": "#[..], "INVALID_JSON"),
        (&b"not json"[..], "INVALID_JSON"),
        (
            &br#"{"handle":"h","prompt_id":7,"choice":"deny"}"#[..],
            "INVALID_FIELD",
        ),
    ] {
        let error = parse::<AnswerPromptRequest>(body).expect_err("must not parse");
        assert_eq!(error.code, code, "{error:?}");
    }
}

mod list_without_a_project {
    //! `list_agents` / `list_workers` without `project_path`: ended runs inside the retention window
    //! are listed too (an outside agent finished a run and could not find it again without its handle).
    use super::super::list::{list_agents, AgentListFilter};
    use crate::operations::{self, OperationStatus, ResultCapture};
    use parking_lot::Mutex;
    use serde_json::Value;
    use std::sync::Arc;

    fn run(project: &std::path::Path, key: &str, owner: &str) -> operations::OperationSnapshot {
        let request = operations::DelegateWorkRequest {
            project_path: project.to_string_lossy().into_owned(),
            task: "Reply with PONG".into(),
            agent_type: "claude".into(),
            pane_id: None,
            idempotency_key: key.into(),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 600_000,
            read_only: false,
            keep_pane: false,
            placement: None,
            owner_session_id: Some(owner.into()),
            worker_has_mcp_tools: false,
            agent_run_id: None,
            turn_index: 0,
            worker: Default::default(),
            context_policy: None,
            checks: Vec::new(),
        };
        operations::delegate_work(request, |_| Ok(())).unwrap()
    }

    fn finish(run: &operations::OperationSnapshot) {
        operations::mark_operation_finished(&run.project_path, &run.operation_id, Some("PONG".into()), ResultCapture::Authoritative, false).unwrap();
    }

    fn handles(project: &str, session: Option<&str>, explicit: bool, filter: AgentListFilter) -> Vec<String> {
        let registry = Arc::new(Mutex::new(crate::pty::PaneRegistry::new()));
        let listed = list_agents(project, session, &registry, explicit, filter).unwrap();
        listed["workers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|w| w["handle"].as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn a_finished_run_is_listed_without_a_project_path_and_the_filters_still_apply() {
        let dir = std::env::temp_dir().join(format!("pm-list-{}", uuid::Uuid::new_v4()));
        let project = dir.to_string_lossy().into_owned();
        let done = run(&dir, "list-done", "client-claude-desktop");
        finish(&done);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let done2 = run(&dir, "list-done-2", "client-claude-desktop");
        finish(&done2);
        let active = run(&dir, "list-active", "client-claude-desktop");
        assert_eq!(active.status, OperationStatus::Running);
        let theirs = run(&dir, "list-theirs", "someone-else");
        finish(&theirs);

        let all = AgentListFilter::default();
        let me = Some("client-claude-desktop");
        for explicit in [true, false] {
            let listed = handles(&project, me, explicit, all);
            for want in [&done, &done2, &active, &theirs] {
                assert!(listed.contains(&want.operation_id), "explicit={explicit}: {listed:?}");
            }
        }
        // Order: active first, then ended runs newest first.
        let listed: Vec<String> = handles(&project, me, false, all)
            .into_iter()
            .filter(|h| [&done, &done2, &active, &theirs].iter().any(|r| &r.operation_id == h))
            .collect();
        let at = |r: &operations::OperationSnapshot| listed.iter().position(|h| *h == r.operation_id).unwrap();
        assert!(at(&active) < at(&theirs) && at(&theirs) < at(&done2) && at(&done2) < at(&done), "{listed:?}");

        let active_only = handles(&project, me, false, AgentListFilter { active_only: true, ..all });
        assert!(active_only.contains(&active.operation_id));
        assert!(!active_only.contains(&done.operation_id), "active_only hides ended runs");
        let mine = handles(&project, me, false, AgentListFilter { mine_only: true, ..all });
        assert!(mine.contains(&done.operation_id) && !mine.contains(&theirs.operation_id), "mine_only hides the other session's run");
        let recent = handles(&project, me, false, AgentListFilter { ended_within_ms: Some(10), ..all });
        assert!(!recent.contains(&done.operation_id), "ended_within_ms hides runs that ended longer ago");
        assert!(recent.contains(&active.operation_id));
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn headless_left_out_is_true_only_when_the_caller_did_not_name_it() {
    use super::wire::headless_left_out;
    assert!(headless_left_out(br#"{"task":"x"}"#));
    assert!(headless_left_out(br#"{"task":"x","headless":null}"#));
    // An explicit false is a choice (a pane), not an absence: the agent's setting must not override it.
    assert!(!headless_left_out(br#"{"task":"x","headless":false}"#));
    assert!(!headless_left_out(br#"{"task":"x","headless":true}"#));
}
