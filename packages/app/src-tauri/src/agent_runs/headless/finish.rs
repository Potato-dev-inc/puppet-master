//! Settling a finished headless turn from the child's recorded output and exit code, and building
//! the failure text when it produced no result.

use super::output::stream_text;
use crate::agent_runs::{messaging, native, transcript};
use crate::operations::{self, OperationError, OperationStatus, StateSource};

/// Settles a finished headless turn from the child's recorded output and exit code.
pub(super) fn finish_from_output(
    project: &str,
    operation_id: &str,
    profile: &std::sync::Arc<dyn crate::tui::TuiProfile>,
    program: &str,
    code: i32,
) {
    let transcript = stream_text(project, operation_id, "stdout");
    let stderr = stream_text(project, operation_id, "stderr");
    if let Some(session) =
        profile.extract_provider_session(&transcript)
    {
        let _ = operations::set_provider_session(project, operation_id, &session);
    }
    // A model the CLI itself reported is a confirmation; record it even when the turn failed.
    if let Some(model) = profile.extract_model(&transcript) {
        let _ = operations::record_terminal_pane_state(project, operation_id, Some(model), false);
    }
    if let Some(result) = profile.extract_result(&transcript, code) {
        if result.trim().is_empty() {
            // The worker reported an empty result: that is an honest, authoritative answer.
            // Completion with no evidence text is refused by mark_operation_completed.
            let _ = operations::mark_operation_completed_empty(project, operation_id);
            return;
        }
        // A rejected shell call means the task may not have done what it says: keep the run
        // `completed`, but do not call its result verified (an explicit check can still verify).
        let rejected = profile.rejected_call_warning(&transcript);
        let result = match rejected {
            Some(warning) => format!("{result}\n\n{warning}"),
            None => result,
        };
        transcript::append(project, operation_id, "assistant_final", &crate::redact::text(&result));
        if let Ok(snapshot) = operations::get_operation(project, operation_id) {
            let handle = if snapshot.agent_run_id.is_empty() {
                snapshot.operation_id.as_str()
            } else {
                snapshot.agent_run_id.as_str()
            };
            messaging::mark_open_steers_processed_for_turn(
                project,
                handle,
                &result,
                snapshot.turn_index,
            );
        }
        let _ = operations::mark_operation_completed(
            project,
            operation_id,
            &result,
            rejected.is_none(),
        );
    } else if let Some(mut prompt) = profile
        .detect_headless_trust(&format!("{stderr}\n{transcript}"))
    {
        if let Some(id) = native::stable_prompt_id(&prompt) {
            prompt["prompt_id"] = serde_json::Value::String(id);
        }
        if let Err(error) = operations::mark_operation_waiting_input(
            project,
            operation_id,
            Some("waiting_input".into()),
            prompt,
            StateSource::Native,
        ) {
            tracing::warn!(operation_id = %operation_id, code = %error.code, message = %error.message, "could not park headless run on workspace trust prompt");
        }
    } else if let Some((error_code, message)) =
        profile.classify_headless_failure(&transcript, code)
    {
        // The definition says what went wrong: the CLI reported an error, or exited cleanly with
        // output nothing in it could read. The run fails rather than guessing a result.
        let _ = operations::mark_operation_state(
            project,
            operation_id,
            OperationStatus::Failed,
            StateSource::Native,
            Some("failed".into()),
            None,
            Some(OperationError::new(error_code, message, false)),
        );
    } else {
        let error = OperationError::new(
            if code == 0 {
                "UNVERIFIED_RESULT"
            } else {
                "WORKER_EXIT_FAILED"
            },
            failure_message(code, &stderr, Some(program)),
            code != 0,
        );
        let _ = operations::mark_operation_state(
            project,
            operation_id,
            OperationStatus::Failed,
            StateSource::Native,
            Some("failed".into()),
            None,
            Some(error),
        );
    }
}

/// Builds the run error text, surfacing the first useful CLI diagnostic so
/// `--help` filler does not hide the actual cause.
pub(in crate::agent_runs) fn failure_message(
    code: i32,
    stderr: &str,
    program: Option<&str>,
) -> String {
    let base = if code == 0 {
        "agent exited successfully without structured completion evidence"
    } else {
        "agent process exited with a failure status"
    };
    let Some(diagnostic) = first_cli_diagnostic(stderr) else {
        return base.to_string();
    };
    let mut message = format!("{base}: {diagnostic}");
    if diagnostic.to_ascii_lowercase().contains("trust") {
        message.push_str(" (trust the workspace folder in the agent first, then retry)");
    }
    if diagnostic
        .to_ascii_lowercase()
        .contains("unexpected argument")
        || diagnostic.to_ascii_lowercase().contains("unknown option")
    {
        if let Some(program) = program {
            if let Some(version) = cli_version(program) {
                message.push_str(&format!(" ({version})"));
            }
            message.push_str(&format!(
                "; check `{program} exec --help` — top-level flags belong before `exec`"
            ));
        }
    }
    message
}

fn first_cli_diagnostic(stderr: &str) -> Option<&str> {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let is_help = |line: &str| {
        let lower = line.to_ascii_lowercase();
        lower.contains("try '--help'")
            || lower.contains("for more information")
            || lower.contains("usage:")
    };
    let is_cause = |line: &str| {
        let lower = line.to_ascii_lowercase();
        lower.contains("unexpected argument")
            || lower.contains("unknown option")
            || lower.contains("unrecognized")
            || lower.contains("error:")
            || lower.contains("not trusted")
            || lower.contains("failed")
    };
    lines
        .iter()
        .copied()
        .find(|line| is_cause(line) && !is_help(line))
        .or_else(|| lines.iter().copied().rev().find(|line| !is_help(line)))
        .or_else(|| lines.first().copied())
}

fn cli_version(program: &str) -> Option<String> {
    let output = crate::proc_util::hidden_command(program)
        .arg("--version")
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{DelegateWorkRequest, OperationStatus, ResultCapture};

    fn running_cursor_turn(name: &str, turn: u32) -> (std::path::PathBuf, String, String) {
        let project =
            std::env::temp_dir().join(format!("pm-finish-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&project).unwrap();
        let project_path = project.to_string_lossy().into_owned();
        let request = DelegateWorkRequest {
            project_path: project_path.clone(),
            task: "reply with []".into(),
            agent_type: "cursor_agent".into(),
            pane_id: None,
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            acceptance_criteria: None,
            task_id: None,
            exclusive: false,
            locks: vec![],
            timeout_ms: 900_000,
            read_only: false,
            keep_pane: false,
            placement: None,
            owner_session_id: None,
            parent_pane_id: None,
            worker_has_mcp_tools: false,
            agent_run_id: Some(format!(
                "finish-{name}-{}",
                &uuid::Uuid::new_v4().to_string()[..8]
            )),
            turn_index: turn,
            worker: Default::default(),
            context_policy: None,
            checks: Vec::new(),
        };
        let (op, _) = operations::create_operation(request).unwrap();
        for status in [OperationStatus::Starting, OperationStatus::Running] {
            operations::mark_operation_state(
                &op.project_path,
                &op.operation_id,
                status,
                StateSource::Native,
                Some("process_running".into()),
                None,
                None,
            )
            .unwrap();
        }
        (project, project_path, op.operation_id)
    }

    /// SPEC: editing a definition never changes a run already started. The registry loses the
    /// agent while the turn runs; the turn finishes with the profile it holds.
    #[test]
    fn turn_keeps_its_profile_when_the_registry_is_swapped_mid_turn() {
        // A private registry, so the process-wide one (read by every other test) is never swapped.
        let registry = crate::tui::registry::Registry::seeded();
        let (project, path, id) = running_cursor_turn("swap", 0);
        let held = registry.profile_for_arc("cursor_agent");
        transcript::append(
            &path,
            &id,
            "stdout",
            &crate::redact::text(r#"{"type":"result","is_error":false,"result":"PONG","session_id":"s-swap"}"#),
        );
        registry.swap(Vec::new());
        assert_eq!(registry.profile_for_arc("cursor_agent").id(), "unknown", "control: agent is gone");
        finish_from_output(&path, &id, &held, "agent", 0);
        let done = operations::get_operation(&path, &id).unwrap();
        assert_eq!(done.status, OperationStatus::Completed, "{:?}", done.error);
        assert_eq!(done.result.as_deref(), Some("PONG"));
        assert_eq!(done.result_capture, Some(ResultCapture::Authoritative));
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn empty_result_event_completes_the_turn_honestly() {
        let (project, path, id) = running_cursor_turn("empty", 1);
        transcript::append(
            &path,
            &id,
            "stdout",
            &crate::redact::text(r#"{"type":"result","subtype":"success","is_error":false,"result":"","session_id":"s-empty"}"#),
        );
        finish_from_output(&path, &id, &crate::tui::profile_for("cursor_agent"), "agent", 0);
        let done = operations::get_operation(&path, &id).unwrap();
        assert_eq!(done.status, OperationStatus::Completed, "{:?}", done.error);
        assert_eq!(done.result.as_deref(), None);
        assert_eq!(done.result_capture, Some(ResultCapture::Authoritative));
        assert_eq!(done.worker.provider_session_id.as_deref(), Some("s-empty"));
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn result_without_trailing_newline_after_other_events_completes() {
        let (project, path, id) = running_cursor_turn("eof", 0);
        transcript::append(
            &path,
            &id,
            "stdout",
            &crate::redact::text("{\"type\":\"system\",\"subtype\":\"init\"}\n"),
        );
        transcript::append(
            &path,
            &id,
            "stdout",
            &crate::redact::text(r#"{"type":"result","is_error":false,"result":"PONG","session_id":"s1"}"#),
        );
        finish_from_output(&path, &id, &crate::tui::profile_for("cursor_agent"), "agent", 0);
        let done = operations::get_operation(&path, &id).unwrap();
        assert_eq!(done.status, OperationStatus::Completed, "{:?}", done.error);
        assert_eq!(done.result.as_deref(), Some("PONG"));
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn claude_reported_model_is_recorded_as_confirmed() {
        let (project, path, id) = running_cursor_turn("claude-model", 0);
        operations::record_unconfirmed_model(&path, &id, "guess").unwrap();
        transcript::append(
            &path,
            &id,
            "stdout",
            &crate::redact::text(r#"{"result":"PONG","session_id":"s2","modelUsage":{"claude-sonnet-5-5":{"outputTokens":14}}}"#),
        );
        finish_from_output(&path, &id, &crate::tui::profile_for("claude"), "claude", 0);
        let done = operations::get_operation(&path, &id).unwrap();
        assert_eq!(done.status, OperationStatus::Completed, "{:?}", done.error);
        assert_eq!(done.worker.resolved_model.as_deref(), Some("claude-sonnet-5-5"));
        assert!(!done.worker.model_unconfirmed);
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn a_completed_run_with_no_model_carries_an_explicit_warning_not_a_silent_null() {
        let (project, path, id) = running_cursor_turn("no-model", 0);
        transcript::append(
            &path,
            &id,
            "stdout",
            &crate::redact::text(r#"{"type":"result","is_error":false,"result":"PONG","session_id":"s3"}"#),
        );
        finish_from_output(&path, &id, &crate::tui::profile_for("cursor_agent"), "agent", 0);
        let done = operations::get_operation(&path, &id).unwrap();
        assert!(done.worker.resolved_model.is_none());
        let warnings = crate::agent_runs::runtime::run_warnings_for_tests(&done);
        let unresolved = warnings.iter().find(|w| w.code == "MODEL_UNRESOLVED").expect("warning");
        assert!(unresolved.detail.contains("did not report which model"), "{}", unresolved.detail);
        // A configured default is shown, but flagged.
        operations::record_unconfirmed_model(&path, &id, "Composer 2.5").unwrap();
        let done = operations::get_operation(&path, &id).unwrap();
        let warnings = crate::agent_runs::runtime::run_warnings_for_tests(&done);
        assert!(warnings.iter().all(|w| w.code != "MODEL_UNRESOLVED"));
        assert!(warnings.iter().any(|w| w.code == "MODEL_UNCONFIRMED"));
        let _ = std::fs::remove_dir_all(project);
    }
}
