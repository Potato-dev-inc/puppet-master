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
        title: None,
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
