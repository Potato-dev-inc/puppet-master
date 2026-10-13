use serde_json::json;

use super::approvals::{
    explicit_manual_approval_required, native_status_unavailable, startup_manual_approval_required,
};
use super::errors::operation_http_error;
use super::http::{query_param, requested_project_path};
use super::locks::parse_operation_lock;
use super::pane_util::{pane_buffer_view, register_spawned_pane};
use super::routes::mcp::registry_route as mcp_registry_route;
use super::tool_names::bridge_tool_name;

#[test]
fn a_trust_dialog_on_screen_blocks_startup_dispatch_whatever_the_status_says() {
    // Real captures of a visible cursor_agent pane. Right after the dialog is drawn the pane
    // status still reads `running`; typing the task then hits the dialog's hotkeys.
    let pending = include_str!("../tui/cursor/fixtures/workspace_trust_visible_pending.txt");
    let resolved = include_str!("../tui/cursor/fixtures/workspace_trust_visible_resolved.txt");
    assert!(startup_manual_approval_required(
        "cursor_agent",
        Some("running"),
        pending,
        &[]
    ));
    assert!(!startup_manual_approval_required(
        "cursor_agent",
        Some("running"),
        resolved,
        &[]
    ));
    assert!(!startup_manual_approval_required(
        "cursor_agent",
        Some("idle"),
        "Cursor Agent
→ Plan, search, build anything
",
        &[]
    ));
}

#[test]
fn pane_buffer_default_follows_the_pane_type_and_explicit_views_override_it() {
    assert_eq!(pane_buffer_view(None, "codex"), Ok("screen"));
    assert_eq!(pane_buffer_view(None, "bash"), Ok("scrollback"));
    assert_eq!(
        pane_buffer_view(Some("scrollback"), "codex"),
        Ok("scrollback")
    );
    assert_eq!(pane_buffer_view(Some("screen"), "powershell"), Ok("screen"));
    assert!(pane_buffer_view(Some("raw"), "codex").is_err());
}

#[test]
fn spawned_shells_register_as_shell_panes_for_the_request_session() {
    let session = uuid::Uuid::new_v4().to_string();
    let pane_id = uuid::Uuid::new_v4().to_string();
    register_spawned_pane(Some(&session), "powershell", &pane_id).unwrap();
    crate::mcp_sessions::set_mode(&session, "shell").unwrap();
    assert!(crate::mcp_sessions::authorize(
        Some(&session),
        "POST",
        &format!("/panes/{pane_id}/input"),
        &json!({"text":"Get-Location"})
    )
    .is_ok());
}

#[test]
fn spawned_agents_register_as_agent_panes_for_the_request_session() {
    let session = uuid::Uuid::new_v4().to_string();
    let pane_id = uuid::Uuid::new_v4().to_string();
    register_spawned_pane(Some(&session), "codex", &pane_id).unwrap();
    crate::mcp_sessions::set_mode(&session, "shell").unwrap();
    assert!(crate::mcp_sessions::authorize(
        Some(&session),
        "POST",
        &format!("/panes/{pane_id}/input"),
        &json!({"text":"unexpected takeover"})
    )
    .is_err());
}

#[test]
fn mcp_resources_route_returns_session_resource() {
    let (status, value) = mcp_registry_route("GET", &["mcp", "resources"]).unwrap();
    assert_eq!(status, 200);
    let resources = value.as_array().unwrap();
    assert!(resources.iter().any(|resource| {
        resource.get("uri").and_then(serde_json::Value::as_str) == Some("puppet-master://session")
    }));
}

#[test]
fn mcp_prompts_route_returns_status_check_prompt() {
    let (status, value) = mcp_registry_route("GET", &["mcp", "prompts"]).unwrap();
    assert_eq!(status, 200);
    let prompts = value.as_array().unwrap();
    assert!(prompts.iter().any(|prompt| {
        prompt.get("name").and_then(serde_json::Value::as_str) == Some("status_check")
    }));
}

#[test]
fn bridge_tool_names_include_session_context_routes() {
    assert_eq!(
        bridge_tool_name("GET", &["session", "context"]).as_deref(),
        Some("read_session_context")
    );
    assert_eq!(
        bridge_tool_name("PATCH", &["session", "context"]).as_deref(),
        Some("update_session_context")
    );
    assert_eq!(
        bridge_tool_name("POST", &["panes", "pane-1", "role"]).as_deref(),
        Some("set_pane_role")
    );
    assert_eq!(
        bridge_tool_name("POST", &["delegate-task"]).as_deref(),
        Some("delegate_task")
    );
    assert_eq!(
        bridge_tool_name("POST", &["operations", "delegate"]).as_deref(),
        Some("delegate_work")
    );
    assert_eq!(
        bridge_tool_name("GET", &["operations", "op-1"]).as_deref(),
        Some("get_operation")
    );
    assert_eq!(
        bridge_tool_name("POST", &["operations", "op-1", "wait"]).as_deref(),
        Some("wait_for_operation")
    );
    assert_eq!(
        bridge_tool_name("POST", &["operations", "op-1", "cancel"]).as_deref(),
        Some("cancel_operation")
    );
}

#[test]
fn operation_lock_contract_requires_typed_nonempty_resources() {
    assert_eq!(
        parse_operation_lock("file:src/main.rs").unwrap(),
        ("file".into(), "src/main.rs".into())
    );
    assert_eq!(
        parse_operation_lock(" :name ").unwrap_err().code,
        "INVALID_LOCK"
    );
    assert_eq!(
        parse_operation_lock("missing-type").unwrap_err().code,
        "INVALID_LOCK"
    );
}

#[test]
fn readiness_distinguishes_normal_agent_prompt_from_real_permission() {
    assert!(!explicit_manual_approval_required(
        "codex",
        "How can I help? >",
        &[]
    ));
    assert!(explicit_manual_approval_required(
        "codex",
        "Allow this command to run? (y/n)",
        &[],
    ));
    assert!(explicit_manual_approval_required(
        "opencode_native",
        "Waiting for input",
        &["permission-123".into()],
    ));
}

#[test]
fn native_pending_permission_blocks_startup_even_when_pane_looks_idle() {
    assert!(startup_manual_approval_required(
        "opencode_native",
        Some("idle"),
        "",
        &["permission-123".into()],
    ));
    assert!(!startup_manual_approval_required(
        "codex",
        Some("idle"),
        "Allow this command?",
        &[],
    ));
    assert!(startup_manual_approval_required(
        "codex",
        Some("waiting_input"),
        "Allow this command?",
        &[],
    ));
}

#[test]
fn operation_errors_use_the_structured_retry_contract() {
    let mut error = crate::operations::OperationError::new("RESOURCE_LOCKED", "busy", true);
    error.retry_after_ms = Some(750);
    error.context = json!({"resource_id":"file:src/main.rs"});
    let (status, body) = operation_http_error(error);
    assert_eq!(status, 409);
    assert_eq!(body["code"], "RESOURCE_LOCKED");
    assert_eq!(body["message"], "busy");
    assert_eq!(body["error"], "busy");
    assert_eq!(body["recoverable"], true);
    assert_eq!(body["retry_after_ms"], 750);
    assert_eq!(body["context"]["resource_id"], "file:src/main.rs");
    for (code, expected_status) in [
        ("IDEMPOTENCY_KEY_CONFLICT", 409),
        ("PANE_UNAVAILABLE", 409),
        ("INVALID_AGENT_TYPE", 400),
        ("UNKNOWN_AGENT_TYPE", 400),
        ("AGENT_DEF_APPROVAL_STALE", 409),
        ("AGENT_DEF_BINARY_CHANGED", 409),
        ("AGENT_DEF_UNAPPROVED", 409),
        ("AGENT_NOT_INSTALLED", 400),
    ] {
        let (status, body) = operation_http_error(crate::operations::OperationError::new(
            code, "invalid", false,
        ));
        assert_eq!(status, expected_status, "wrong mapping for {code}");
        assert_eq!(body["code"], code);
    }
}

#[test]
fn native_permission_status_failure_is_recoverable_and_fails_closed() {
    let error = native_status_unavailable("worker API is unreachable");
    assert_eq!(error.code, "NATIVE_STATUS_UNAVAILABLE");
    assert!(error.recoverable);
    let (status, body) = operation_http_error(error);
    assert_eq!(status, 503);
    assert_eq!(body["code"], "NATIVE_STATUS_UNAVAILABLE");
    assert_eq!(body["recoverable"], true);
}

#[test]
fn query_parameters_decode_url_encoded_paths_and_reject_malformed_values() {
    assert_eq!(
        query_param(
            "project_path=C%3A%2FUsers%2FDev%20Folder%2Fapp",
            "project_path"
        ),
        Some("C:/Users/Dev Folder/app".into())
    );
    assert_eq!(query_param("project_path=%ZZ", "project_path"), None);
    assert_eq!(query_param("other=value", "project_path"), None);
}

#[test]
fn wait_project_path_prefers_body_then_query() {
    assert_eq!(
        requested_project_path(Some("C:/body"), "project_path=C%3A%2Fquery"),
        Some("C:/body".into())
    );
    assert_eq!(
        requested_project_path(None, "project_path=C%3A%2Fquery"),
        Some("C:/query".into())
    );
    assert_eq!(requested_project_path(Some("  "), ""), None);
}

fn assert_unknown_agent_type_response(status: u16, body: &serde_json::Value) {
    assert_eq!(status, 400);
    assert_eq!(body["code"], "UNKNOWN_AGENT_TYPE");
    assert_eq!(body["recoverable"], false);
    assert_eq!(body["context"]["hint"], "GET /agents/types");
    let valid: Vec<String> = body["context"]["valid"]
        .as_array()
        .expect("valid ids")
        .iter()
        .map(|value| value.as_str().expect("id").to_string())
        .collect();
    assert_eq!(valid.len(), 9);
    assert_eq!(valid, crate::tui::registered_agent_ids());
}

#[test]
fn run_agent_unknown_type_returns_unknown_agent_type() {
    let _serial = crate::tui::registry::test_serial();
    let snapshot = crate::operations::OperationSnapshot {
        operation_id: uuid::Uuid::new_v4().to_string(),
        runtime_id: String::new(),
        request_fingerprint: String::new(),
        project_path: "C:/proj".into(),
        task: "work".into(),
        agent_type: "not-a-real-agent".into(),
        pane_id: None,
        pane_created: false,
        idempotency_key: "key".into(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        status: crate::operations::OperationStatus::Queued,
        revision: 1,
        created_at_ms: 0,
        updated_at_ms: 0,
        observed_at_ms: None,
        pane_state: None,
        required_action: None,
        source: crate::operations::StateSource::Native,
        stage: None,
        progress_pct: None,
        result: None,
        error: None,
        timeout_ms: 60_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        keep_pane_requested: None,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: "run".into(),
        turn_index: 0,
        verified: false,
        started_at_ms: None,
        finished_at_ms: None,
        message_baseline_ids: vec![],
        output_baseline: None,
        result_capture: None,
        acceptance_status: crate::operations::AcceptanceStatus::NotChecked,
        worker: Default::default(),
        checks: Vec::new(),
        check_results: Vec::new(),
        warnings: Vec::new(),
    };
    let error =
        super::dispatch::check_agent_dispatchable(&snapshot, "not-a-real-agent").unwrap_err();
    let (status, body) = operation_http_error(error);
    assert_unknown_agent_type_response(status, &body);
}

#[test]
fn spawn_unknown_type_returns_unknown_agent_type() {
    let _serial = crate::tui::registry::test_serial();
    let unknown = match crate::tui::require_registered("bogus-agent") {
        Err(err) => err,
        Ok(_) => panic!("expected unknown agent type"),
    };
    let (status, body) = operation_http_error(
        crate::operations::OperationError::unknown_agent_type(unknown.id, unknown.valid),
    );
    assert_unknown_agent_type_response(status, &body);
}

#[test]
fn delegate_work_unknown_type_keeps_invalid_agent_type() {
    let dir = std::env::temp_dir().join(format!("pm-delegate-unknown-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let req = crate::operations::DelegateWorkRequest {
        project_path: dir.to_string_lossy().into_owned(),
        task: "do work".into(),
        agent_type: "not-a-real-agent".into(),
        pane_id: None,
        idempotency_key: "delegate-unknown".into(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let error = crate::operations::create_operation(req).unwrap_err();
    assert_eq!(error.code, "INVALID_AGENT_TYPE");
    assert_eq!(error.message, "agent_type is not supported");
    let (status, body) = operation_http_error(error);
    assert_eq!(status, 400);
    assert_eq!(body["code"], "INVALID_AGENT_TYPE");
    assert_eq!(body["message"], "agent_type is not supported");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn agents_types_lists_every_code_profile() {
    let _serial = crate::tui::registry::test_serial();
    let (status, body) = mcp_registry_route("GET", &["agents", "types"]).expect("route exists");
    assert_eq!(status, 200);
    let agents = body["agents"].as_array().expect("agents array");
    let ids: Vec<&str> = agents
        .iter()
        .map(|agent| agent["id"].as_str().expect("id"))
        .collect();
    let expected: Vec<&str> = crate::tui::profiles().iter().map(|p| p.id()).collect();
    assert_eq!(ids, expected, "registry order");
    for agent in agents {
        assert!(agent["installed"].is_boolean(), "{agent}");
        assert!(agent["modes"]["visible"].is_boolean(), "{agent}");
        assert!(agent["modes"]["headless"].is_boolean(), "{agent}");
        assert!(agent["features"]["read_only_headless"].is_boolean(), "{agent}");
        assert!(agent["run_options"]["allow_shell"].is_boolean(), "{agent}");
        assert!(agent["standby"]["visible"].is_boolean(), "{agent}");
        assert!(agent["label"].is_string(), "{agent}");
    }
    let dispatchable: Vec<&str> = body["dispatchable_ids"]
        .as_array()
        .expect("dispatchable_ids")
        .iter()
        .map(|id| id.as_str().expect("id"))
        .collect();
    assert_eq!(dispatchable, vec!["claude", "codex", "opencode", "opencode_native", "cursor_agent"]);
}

// ---- /agents/definitions (T19) -------------------------------------------------------------

mod definitions {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::{json, Value};

    use crate::bridge::routes::definitions::respond;
    use crate::mcp_sessions;
    use crate::tui::declarative::loader::{reload_global, LoadInputs};
    use crate::tui::declarative::service;

    /// A unique temp tree; cleanup removes only recorded entries, one at a time.
    struct Tree {
        root: PathBuf,
        files: Vec<PathBuf>,
    }

    impl Tree {
        fn new() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir()
                .join(format!("pm-defroutes-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            std::fs::create_dir_all(root.join("agents")).unwrap();
            Self { root, files: Vec::new() }
        }
        fn agents(&self) -> PathBuf {
            self.root.join("agents")
        }
        fn approvals(&mut self) -> PathBuf {
            let path = self.root.join("approvals.json");
            self.files.push(path.clone());
            path
        }
        fn file(&mut self, name: &str, text: &str) -> PathBuf {
            let path = self.agents().join(name);
            std::fs::write(&path, text).unwrap();
            self.files.push(path.clone());
            path
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            for file in &self.files {
                let _ = std::fs::remove_file(file);
            }
            let _ = std::fs::remove_dir(self.root.join("agents"));
            let _ = std::fs::remove_dir(&self.root);
        }
    }

    /// `cargo` is on PATH while tests run, so the binary resolves without any agent installed.
    fn def(id: &str, extra: &str) -> String {
        format!(
            "schema_version = 1\n[agent]\nid = \"{id}\"\nlabel = \"Test {id}\"\n[binary]\nnames = [\"cargo\"]\ninstall_hint = \"get cargo\"\n[headless]\nargv = [\"run\"]\ntask_separator = \"--\"\n[headless.output]\nformat = \"plain_text\"\n{extra}"
        )
    }

    fn inputs<'a>(dir: &'a Path, code_ids: &'a [&'a str]) -> LoadInputs<'a> {
        LoadInputs { bundled: &[], user_dir: Some(dir), code_ids, projects: &[], extra: &[] }
    }

    #[test]
    fn agents_definitions_lists_state_and_errors() {
        let mut t = Tree::new();
        let good = t.file("a.toml", &def("defa_x", ""));
        t.file("b.toml", "schema_version = 1\n[agent]\nid = \"defb_x\"\nbogus = 1\n");
        let approvals = t.approvals();
        let code = ["claude"];
        let body = service::listing(&inputs(&t.agents(), &code), &approvals);
        assert_eq!(body["sources"][1]["name"], "user");
        assert_eq!(body["sources"][1]["enabled"], true);
        let defs = body["definitions"].as_array().unwrap();
        assert_eq!(defs.len(), 2);
        let a = defs.iter().find(|d| d["id"] == "defa_x").unwrap();
        assert_eq!(a["state"], "needs_approval");
        assert_eq!(a["source"], "user");
        assert!(a["fingerprint"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(a["file"], good.to_string_lossy().as_ref());
        let bad = defs.iter().find(|d| d["state"] == "invalid").unwrap();
        let errors = bad["errors"].as_array().unwrap();
        assert!(!errors.is_empty());
        assert!(errors[0]["code"].is_string() && errors[0]["message"].is_string());
        assert!(bad["file"].as_str().unwrap().ends_with("b.toml"));
    }

    #[test]
    fn approve_stale_fingerprint_and_revoke() {
        let mut t = Tree::new();
        let file = t.file("a.toml", &def("defc_x", ""));
        let approvals = t.approvals();
        let code = ["claude"];
        let agents = t.agents();
        let inp = inputs(&agents, &code);
        let listed = service::listing(&inp, &approvals);
        let fp = listed["definitions"][0]["fingerprint"].as_str().unwrap().to_string();

        let stale = service::approve(&inp, &approvals, &file.to_string_lossy(), "sha256:nope").unwrap_err();
        assert_eq!((stale.status, stale.code), (409, "AGENT_DEF_APPROVAL_STALE"));
        let missing = service::approve(&inp, &approvals, "C:/no/such.toml", &fp).unwrap_err();
        assert_eq!(missing.code, "AGENT_DEF_NOT_FOUND");

        let ok = service::approve(&inp, &approvals, &file.to_string_lossy(), &fp).unwrap();
        assert_eq!(ok["ok"], true);
        assert_eq!(ok["id"], "defc_x");
        assert_eq!(ok["state"], "active");
        let record = crate::tui::declarative::approvals::Approvals::load(&approvals).records()[0].clone();
        assert!(!record.binary.is_empty(), "binary is pinned at approval");

        assert_eq!(service::listing(&inp, &approvals)["definitions"][0]["state"], "active");
        service::revoke(&approvals, "defc_x").unwrap();
        assert_eq!(service::listing(&inp, &approvals)["definitions"][0]["state"], "needs_approval");
        assert_eq!(service::revoke(&approvals, "defc_x").unwrap_err().code, "AGENT_DEF_NOT_FOUND");
    }

    #[test]
    fn validate_renders_argv_and_registers_nothing() {
        let good = service::validate_text(&def("defd_x", ""), "user", &["claude"]).unwrap();
        assert_eq!(good["ok"], true, "{good}");
        assert_eq!(good["rendered"]["headless_argv"], json!(["run", "--", "<task>"]));
        let clash = service::validate_text(&def("claude", ""), "user", &["claude"]).unwrap();
        assert_eq!(clash["ok"], false);
        assert!(!clash["errors"].as_array().unwrap().is_empty());
        assert_eq!(service::validate_text("x", "nowhere", &[]).unwrap_err().status, 400);
    }

    #[test]
    fn remote_device_cannot_mutate_definitions() {
        use crate::bridge::routes::definitions::is_local_app;
        let local = "POST /x HTTP/1.1\r\nHost: 127.0.0.1:17321\r\n";
        let tunnel = "POST /x HTTP/1.1\r\nX-PM-Proxied: 1\r\nAuthorization: Bearer tok\r\n";
        let bearer_only = "POST /x HTTP/1.1\r\nAuthorization: Bearer tok\r\n";
        let fwd = "POST /x HTTP/1.1\r\nCF-Connecting-IP: 1.2.3.4\r\n";
        assert!(is_local_app(local, true));
        assert!(!is_local_app(tunnel, true));
        assert!(!is_local_app(bearer_only, true));
        assert!(!is_local_app(fwd, true));
        assert!(!is_local_app(local, false));
        let body = json!({"file": "C:/x.toml", "fingerprint": "sha256:x", "id": "x", "toml": "x"});
        for action in ["approve", "revoke", "consent", "reload", "validate"] {
            let remote = is_local_app(tunnel, true);
            let (status, reply) =
                respond("POST", &["agents", "definitions", action], &body, None, remote).unwrap().unwrap_err();
            assert_eq!(status, 403, "{action}");
            assert_eq!(reply["code"], "AUTHORIZATION_DENIED");
        }
        // listing stays readable; the local app is not refused by the gate
        assert!(respond("GET", &["agents", "definitions"], &json!({}), None, false).unwrap().is_ok());
        let local_reply = respond("POST", &["agents", "definitions", "revoke"], &json!({"id": "zz-none"}), None, true).unwrap();
        assert_ne!(local_reply.err().map(|e| e.0), Some(403));
    }

    #[test]
    fn approve_requires_app_session() {
        let body = json!({"file": "C:/x.toml", "fingerprint": "sha256:x"});
        // an MCP session (header present) can reach none of the definition routes
        for (method, path) in [
            ("POST", "/agents/definitions/approve"),
            ("POST", "/agents/definitions/revoke"),
            ("POST", "/agents/definitions/consent"),
            ("POST", "/agents/definitions/reload"),
            ("GET", "/agents/definitions"),
        ] {
            let denied = mcp_sessions::authorize(Some("mcp-session-1"), method, path, &body).unwrap_err();
            assert_eq!(denied.code, "AUTHORIZATION_DENIED", "{method} {path}");
        }
        // belt and braces: the route layer refuses a session too, before touching any file
        for action in ["approve", "revoke", "consent"] {
            let (status, reply) =
                respond("POST", &["agents", "definitions", action], &body, Some("mcp-session-1"), true).unwrap().unwrap_err();
            assert_eq!(status, 403, "{action}");
            assert_eq!(reply["code"], "AUTHORIZATION_DENIED");
        }
        // the app session (no MCP header) is let through the gate
        assert!(mcp_sessions::authorize(None, "POST", "/agents/definitions/approve", &body).is_ok());
        assert!(mcp_sessions::authorize(None, "GET", "/agents/definitions", &json!({})).is_ok());
        // other agent APIs still demand a session
        assert!(mcp_sessions::authorize(None, "GET", "/agents/types", &json!({})).is_err());
        // unknown paths under it are not routes
        assert!(respond("POST", &["agents", "definitions", "nope"], &body, None, true).is_none());
    }

    #[test]
    fn agents_types_includes_declarative_after_approval() {
        let _serial = crate::tui::registry::test_serial();
        let mut t = Tree::new();
        let file = t.file("a.toml", &def("defe_x", ""));
        let approvals = t.approvals();
        let code = crate::tui::registry::registry().code_ids();
        let agents = t.agents();
        let inp = inputs(&agents, &code);

        reload_global(&inp, &approvals);
        let before: Value = crate::tui::agent_types_listing();
        let row = before["agents"].as_array().unwrap().iter().find(|a| a["id"] == "defe_x");
        assert!(row.is_none(), "unapproved definitions are not listed as agents");

        let fp = service::listing(&inp, &approvals)["definitions"][0]["fingerprint"].as_str().unwrap().to_string();
        service::approve(&inp, &approvals, &file.to_string_lossy(), &fp).unwrap();
        reload_global(&inp, &approvals);
        let after: Value = crate::tui::agent_types_listing();
        let row = after["agents"].as_array().unwrap().iter().find(|a| a["id"] == "defe_x").cloned();
        reload_global(&inputs(&t.root.join("none"), &code), &approvals);

        let row = row.expect("approved definition is listed");
        assert_eq!(row["source"], "user");
        assert_eq!(row["label"], "Test defe_x");
        assert_eq!(row["definition"]["state"], "active");
        assert_eq!(row["install_hint"], "get cargo");
        assert_eq!(row["modes"]["visible"], false, "no [tui] table");
        assert!(row["definition"]["fingerprint"].as_str().unwrap().starts_with("sha256:"));
    }
}

#[test]
fn agents_health_route_lists_cli_agents_with_hints() {
    let _serial = crate::tui::registry::test_serial();
    let (status, body) =
        super::routes::health::static_route("GET", &["agents", "health"]).expect("route exists");
    assert_eq!(status, 200);
    let agents = body["agents"].as_array().expect("agents array");
    let ids: Vec<&str> = agents.iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"claude") && ids.contains(&"codex"), "{ids:?}");
    assert!(!ids.contains(&"bash"), "shells are not agents to set up: {ids:?}");
    for agent in agents {
        assert!(
            ["unchecked", "ready", "not_installed", "needs_login", "error"]
                .contains(&agent["state"].as_str().unwrap()),
            "{agent}"
        );
        assert!(agent["reason"].as_str().is_some_and(|r| !r.is_empty()), "{agent}");
        assert!(agent["can_test"].is_boolean(), "{agent}");
    }
    let claude = agents.iter().find(|a| a["id"] == "claude").unwrap();
    assert_eq!(claude["install_hint"], "npm i -g @anthropic-ai/claude-code");
    assert!(super::routes::health::static_route("POST", &["agents", "health"]).is_none());
}

#[test]
fn previews_route_returns_the_detected_dev_servers_per_pane() {
    let _serial = crate::pane_activity::TEST_SERIAL.lock();
    crate::pane_preview::observe("route-pane", "  Local:   http://localhost:5173/
  Network: http://192.168.1.9:5173/
");
    let (status, body) = super::routes::previews::static_route("GET", &["previews"]).expect("route exists");
    assert_eq!(status, 200);
    let urls = &body["previews"]["route-pane"];
    assert_eq!(urls[0]["url"], "http://localhost:5173/");
    assert_eq!(urls[0]["port"], 5173);
    assert_eq!(urls[0]["networkUrl"], "http://192.168.1.9:5173/");
    assert!(super::routes::previews::static_route("POST", &["previews"]).is_none());
    crate::pane_preview::retain_alive(&|_| false);
}

#[test]
fn stop_all_route_is_authorised_like_other_remote_control_routes() {
    use crate::mobile_pairing::authorize_bridge_request;
    let segs = ["panes", "stop-all"];
    // Remote peer without a pairing token: refused.
    let (status, body) = authorize_bridge_request("", false, &segs, "POST").unwrap_err();
    assert_eq!((status, body["error"].as_str()), (401, Some("pairing_required")));
    // The phone PWA arrives through the loopback proxy: the proxy marker still demands a token.
    let (status, _) = authorize_bridge_request("X-PM-Proxied: 1\r\n", true, &segs, "POST").unwrap_err();
    assert_eq!(status, 401);
    // A wrong token is refused too.
    let (status, _) =
        authorize_bridge_request("Authorization: Bearer nope\r\n", false, &segs, "POST").unwrap_err();
    assert!(status == 401 || status == 503, "{status}");
    // The desktop app itself (loopback, no proxy marker) is allowed.
    assert!(authorize_bridge_request("", true, &segs, "POST").is_ok());
}

#[test]
fn stop_all_route_is_not_exposed_to_mcp_sessions() {
    // Agents must not be able to stop their siblings through the bridge: the route is not a tool.
    let session = uuid::Uuid::new_v4().to_string();
    let err = crate::mcp_sessions::authorize(Some(&session), "POST", "/panes/stop-all", &json!({}))
        .expect_err("MCP session refused");
    assert_eq!(err.code, "AUTHORIZATION_DENIED");
    assert!(crate::mcp_sessions::authorize(None, "POST", "/panes/stop-all", &json!({})).is_ok());
}

#[test]
fn stop_report_wire_shape() {
    let report = crate::panic::StopReport {
        stopped: vec![crate::panic::StoppedPane { pane_id: "p1".into(), agent_type: "claude".into(), cwd: "/w".into() }],
        failed: vec![],
    };
    assert_eq!(
        serde_json::to_value(report).unwrap(),
        json!({"stopped":[{"pane_id":"p1","agent_type":"claude","cwd":"/w"}],"failed":[]})
    );
}
