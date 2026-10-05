//! Multi-turn handles: context policy, resume vs reconstruction, session and cursor state.

use crate::operations::{self, ContextContinuity, ContextPolicy, ResultCapture};
use std::fs;
use uuid::Uuid;

use super::fixtures::*;

#[test]
fn sequential_tasks_on_one_handle_disclose_reconstructed_continuity() {
    let project = std::env::temp_dir().join(format!("pm-agent-persist-seq-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(
        &project_path,
        "reviewer",
        0,
        "summarize the outage",
    ))
    .unwrap()
    .0;
    finish_turn(&project_path, &first.operation_id, "token expired");

    let bind =
        crate::agent_runs::persist::decide_worker(&project_path, Some("reviewer"), false).unwrap();
    let crate::agent_runs::persist::WorkerBind::Adopt { handle, previous } = bind else {
        panic!("expected adopt");
    };
    assert_eq!(handle, "reviewer");
    assert_eq!(previous.turn_index, 0);

    let rendered = crate::agent_runs::persist::render_turn_prompt(
        crate::agent_runs::persist::TurnPromptArgs {
            policy: ContextPolicy::Packet,
            task: "propose a fix",
            scope: Some("auth"),
            prior_result: previous.result.as_deref(),
            prior_user: None,
            selected_history: &[],
            can_resume: crate::agent_runs::persist::snapshot_can_resume(&previous),
            is_followup: true,
        },
    );
    assert_eq!(rendered.continuity, ContextContinuity::ReconstructedSummary);
    assert!(rendered.prompt.contains("token expired"));
    assert!(rendered.prompt.contains("propose a fix"));
    assert!(!rendered.prompt.contains("# Context packet"));

    let mut second_req = persist_req(&project_path, "reviewer", 1, "propose a fix");
    second_req.worker.context_policy = ContextPolicy::Packet;
    second_req.worker.context_continuity = rendered.continuity;
    operations::create_operation(second_req).unwrap();

    let inspected = crate::agent_runs::runtime::inspect_agent(
        &project_path,
        "reviewer",
        Some("persist-session"),
    )
    .unwrap();
    assert_eq!(inspected["handle"], "reviewer");
    assert_eq!(inspected["context_policy"], "packet");
    assert_eq!(inspected["context_continuity"], "reconstructed_summary");
    assert_eq!(inspected["turn_id"], "turn-1");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn completing_turn_one_does_not_close_the_worker() {
    let project = std::env::temp_dir().join(format!("pm-agent-persist-live-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(&project_path, "keeper", 0, "first turn"))
        .unwrap()
        .0;
    finish_turn(&project_path, &first.operation_id, "done");
    let inspected =
        crate::agent_runs::runtime::inspect_agent(&project_path, "keeper", Some("persist-session"))
            .unwrap();
    assert_eq!(inspected["handle"], "keeper");
    assert_eq!(inspected["status"], "completed");
    assert_eq!(inspected["context_policy"], "packet");
    let latest = operations::latest_agent_run(&project_path, "keeper").unwrap();
    assert_eq!(latest.agent_run_id, "keeper");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn fresh_starts_a_new_conversation_when_a_name_matches() {
    let project = std::env::temp_dir().join(format!("pm-agent-persist-fresh-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(
        &project_path,
        "reviewer",
        0,
        "old conversation",
    ))
    .unwrap()
    .0;
    finish_turn(&project_path, &first.operation_id, "old answer");

    let bind =
        crate::agent_runs::persist::decide_worker(&project_path, Some("reviewer"), true).unwrap();
    let crate::agent_runs::persist::WorkerBind::Create { handle } = bind else {
        panic!("fresh must not adopt");
    };
    assert_ne!(handle, "reviewer");
    assert!(handle.starts_with("reviewer-"));

    let rendered = crate::agent_runs::persist::render_turn_prompt(
        crate::agent_runs::persist::TurnPromptArgs {
            policy: ContextPolicy::Fresh,
            task: "brand new question",
            scope: None,
            prior_result: Some("old answer"),
            prior_user: None,
            selected_history: &[],
            can_resume: false,
            is_followup: false,
        },
    );
    assert_eq!(rendered.continuity, ContextContinuity::None);
    assert_eq!(rendered.prompt, "brand new question");
    assert!(!rendered.prompt.contains("old answer"));
    let _ = fs::remove_dir_all(project);
}

#[test]
fn requested_model_is_not_silently_remapped() {
    let choice =
        crate::agent_runs::persist::resolve_model_choice(Some("luna-low"), Some("low"), None);
    assert_eq!(choice.requested_model.as_deref(), Some("luna-low"));
    assert_eq!(choice.resolved_model.as_deref(), Some("luna-low"));
    assert_eq!(choice.requested_reasoning.as_deref(), Some("low"));
    assert_eq!(choice.resolved_reasoning.as_deref(), Some("low"));
    assert!(!choice.mismatch);
    assert_ne!(choice.resolved_model.as_deref(), Some("composer-2.5"));

    let project = std::env::temp_dir().join(format!("pm-agent-persist-model-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let mut request = persist_req(&project_path, "modeler", 0, "use this model");
    request.worker.requested_model = Some("luna-low".into());
    request.worker.resolved_model = Some("composer-2.5".into());
    request.worker.requested_reasoning = Some("low".into());
    request.worker.resolved_reasoning = Some("medium".into());
    let snapshot = operations::create_operation(request).unwrap().0;
    let view = crate::agent_runs::runtime::AgentRunView::from(snapshot);
    assert_eq!(view.requested_model.as_deref(), Some("luna-low"));
    assert_eq!(view.resolved_model.as_deref(), Some("composer-2.5"));
    assert!(view.model_mismatch);
    let inspected = crate::agent_runs::runtime::inspect_agent(
        &project_path,
        "modeler",
        Some("persist-session"),
    )
    .unwrap();
    assert_eq!(inspected["requested_model"], "luna-low");
    assert_eq!(inspected["resolved_model"], "composer-2.5");
    assert_eq!(inspected["model_mismatch"], true);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn inspect_reflects_default_context_packet_policy() {
    let project = std::env::temp_dir().join(format!("pm-agent-persist-packet-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    operations::create_operation(persist_req(&project_path, "packer", 0, "inspect me")).unwrap();
    let inspected =
        crate::agent_runs::runtime::inspect_agent(&project_path, "packer", Some("persist-session"))
            .unwrap();
    assert_eq!(inspected["context_policy"], "packet");
    assert_eq!(inspected["context_continuity"], "none");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn native_session_followup_reports_resume_continuity() {
    let rendered = crate::agent_runs::persist::render_turn_prompt(
        crate::agent_runs::persist::TurnPromptArgs {
            policy: ContextPolicy::Packet,
            task: "continue",
            scope: None,
            prior_result: Some("earlier"),
            prior_user: None,
            selected_history: &[],
            can_resume: true,
            is_followup: true,
        },
    );
    assert_eq!(rendered.continuity, ContextContinuity::Resume);
    assert!(!rendered.prompt.contains("earlier"));
}

#[test]
fn ack_only_user_marker_is_reconstructed_when_resume_is_unavailable() {
    let project = std::env::temp_dir().join(format!("pm-ack-marker-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let first = operations::create_operation(persist_req(
        &project_path,
        "luna",
        0,
        "remember ORBIT-74 and reply only ACK",
    ))
    .unwrap()
    .0;
    finish_turn(&project_path, &first.operation_id, "ACK");

    let facts = crate::agent_runs::persist::conversation_facts(&project_path, "luna").unwrap();
    assert!(facts.contains("ORBIT-74"));
    let previous = operations::latest_agent_run(&project_path, "luna").unwrap();
    let rendered = crate::agent_runs::persist::render_turn_prompt(
        crate::agent_runs::persist::TurnPromptArgs {
            policy: ContextPolicy::Packet,
            task: "What is the marker?",
            scope: None,
            prior_result: previous.result.as_deref(),
            prior_user: Some(facts.as_str()),
            selected_history: &[],
            can_resume: crate::agent_runs::persist::snapshot_can_resume(&previous),
            is_followup: true,
        },
    );
    assert_eq!(rendered.continuity, ContextContinuity::ReconstructedSummary);
    assert!(rendered.prompt.contains("ORBIT-74"));
    assert!(rendered.prompt.contains("What is the marker?"));
    assert_eq!(
        crate::agent_runs::persist::followup_policy(None, ContextPolicy::Fresh),
        ContextPolicy::Packet
    );
    assert_eq!(
        crate::agent_runs::persist::followup_policy(
            Some(ContextPolicy::Fresh),
            ContextPolicy::Packet
        ),
        ContextPolicy::Fresh
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn captured_codex_thread_resumes_instead_of_reconstructing() {
    let project = std::env::temp_dir().join(format!("pm-codex-resume-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let mut request = persist_req(&project_path, "luna", 0, "remember ORBIT-73");
    request.agent_type = "codex".into();
    request.worker.provider_session_id = Some("0199thread".into());
    let first = operations::create_operation(request).unwrap().0;
    finish_turn(&project_path, &first.operation_id, "ACK");
    let previous = operations::latest_agent_run(&project_path, "luna").unwrap();
    assert!(crate::agent_runs::persist::snapshot_can_resume(&previous));
    let rendered = crate::agent_runs::persist::render_turn_prompt(
        crate::agent_runs::persist::TurnPromptArgs {
            policy: ContextPolicy::Packet,
            task: "What is the marker?",
            scope: None,
            prior_result: previous.result.as_deref(),
            prior_user: crate::agent_runs::persist::conversation_facts(&project_path, "luna")
                .as_deref(),
            selected_history: &[],
            can_resume: true,
            is_followup: true,
        },
    );
    assert_eq!(rendered.continuity, ContextContinuity::Resume);
    assert_eq!(rendered.prompt, "What is the marker?");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn session_reset_is_visible_and_blocks_resume() {
    let project = std::env::temp_dir().join(format!("pm-session-reset-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let native = running_turn(
        &project_path,
        "native",
        "opencode_native",
        Some("pane-native"),
        true,
    );
    let rebound = operations::rebind_provider_session(
        &project_path,
        &native.operation_id,
        "ses-new",
        Some("ses-live"),
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        rebound.worker.provider_session_id.as_deref(),
        Some("ses-new")
    );
    assert_eq!(rebound.stage.as_deref(), Some("session_replaced"));
    assert_eq!(
        rebound
            .required_action
            .as_ref()
            .and_then(|value| value["kind"].as_str()),
        Some("session_replaced")
    );
    operations::mark_operation_finished(
        &project_path,
        &native.operation_id,
        Some("recovered".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let reset = operations::rebind_provider_session(
        &project_path,
        &native.operation_id,
        "ses-new",
        Some("ses-live"),
        true,
        None,
    )
    .unwrap();
    assert!(reset.worker.session_reset);
    assert!(!crate::agent_runs::persist::snapshot_can_resume(&reset));
    let view = crate::agent_runs::runtime::AgentRunView::from(reset.clone());
    assert!(view.session_reset);
    assert_eq!(view.provider_session_id.as_deref(), Some("ses-new"));
    let inspected =
        crate::agent_runs::runtime::inspect_agent(&project_path, "native", Some("persist-session"))
            .unwrap();
    assert_eq!(inspected["session_reset"], true);
    assert_eq!(inspected["provider_session_id"], "ses-new");
    let rendered = crate::agent_runs::persist::render_turn_prompt(
        crate::agent_runs::persist::TurnPromptArgs {
            policy: ContextPolicy::Packet,
            task: "continue",
            scope: None,
            prior_result: reset.result.as_deref(),
            prior_user: crate::agent_runs::persist::conversation_facts(&project_path, "native")
                .as_deref(),
            selected_history: &[],
            can_resume: crate::agent_runs::persist::snapshot_can_resume(&reset),
            is_followup: true,
        },
    );
    assert_eq!(rendered.continuity, ContextContinuity::ReconstructedSummary);
    let _ = fs::remove_dir_all(project);
}

#[test]
fn fresh_context_policy_renders_plain_task_without_packet_sections() {
    let rendered = crate::agent_runs::persist::render_turn_prompt(
        crate::agent_runs::persist::TurnPromptArgs {
            policy: ContextPolicy::Fresh,
            task: "Review docs/orchestrator/quickstart.md",
            scope: None,
            prior_result: None,
            prior_user: None,
            selected_history: &[],
            can_resume: false,
            is_followup: false,
        },
    );
    assert_eq!(rendered.prompt, "Review docs/orchestrator/quickstart.md");
}
