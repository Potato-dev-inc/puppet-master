//! Turn completion: evidence rules, `checks`/acceptance evaluation and interrupted turns.

use super::model::*;
use super::store::{bump, now_ms, root, read};
use super::{get_operation, operation_for_task_any_project};
use std::fs;
use super::store::update_lock;

pub fn mark_task_completed_any_project(
    task_id: &str,
    evidence: &str,
) -> Result<Vec<OperationSnapshot>, OperationError> {
    let operation = operation_for_task_any_project(task_id)?;
    mark_task_completed(&operation.project_path, task_id, evidence)
}

/// Complete operations tied to an explicitly completed task with evidence.
pub fn mark_task_completed(
    project: &str,
    task_id: &str,
    evidence: &str,
) -> Result<Vec<OperationSnapshot>, OperationError> {
    if evidence.trim().is_empty() {
        return Err(OperationError::new(
            "COMPLETION_EVIDENCE_REQUIRED",
            "evidence must not be empty",
            false,
        ));
    }
    let dir = root(project)?;
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(OperationError::io(e)),
    };
    let mut completed = Vec::new();
    for entry in entries {
        let entry = entry.map_err(OperationError::io)?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let op = read(&entry.path())?;
            if op.task_id.as_deref() == Some(task_id) && !op.status.terminal() {
                completed.push(mark_operation_completed(
                    project,
                    &op.operation_id,
                    evidence,
                    true,
                )?);
            }
        }
    }
    Ok(completed)
}

/// Finish a run with explicit completion evidence. `verified` is set only by a caller that
/// obtained native structured evidence (for example, a successful CLI result event).
pub fn mark_operation_completed(
    project: &str,
    id: &str,
    evidence: &str,
    verified: bool,
) -> Result<OperationSnapshot, OperationError> {
    let capture = if verified {
        ResultCapture::Authoritative
    } else {
        ResultCapture::Inferred
    };
    mark_operation_finished(
        project,
        id,
        Some(evidence.to_string()),
        capture,
        verified,
    )
}

/// Prefix on a stored result that was read from the terminal screen without any `checks`.
pub const UNVERIFIED_SCREEN_PREFIX: &str = "[unverified: read from terminal screen] ";

/// Complete a turn whose worker authoritatively reported an empty result (the model said
/// nothing). The result stays empty and unverified; checks and acceptance criteria decide.
pub fn mark_operation_completed_empty(
    project: &str,
    id: &str,
) -> Result<OperationSnapshot, OperationError> {
    finish_operation(project, id, None, ResultCapture::Authoritative, false, true)
}

/// Finish a turn. Missing capture is allowed so an idle worker is not left "running" until timeout.
pub fn mark_operation_finished(
    project: &str,
    id: &str,
    result: Option<String>,
    capture: ResultCapture,
    verified: bool,
) -> Result<OperationSnapshot, OperationError> {
    finish_operation(project, id, result, capture, verified, false)
}

fn finish_operation(
    project: &str,
    id: &str,
    result: Option<String>,
    capture: ResultCapture,
    verified: bool,
    allow_empty: bool,
) -> Result<OperationSnapshot, OperationError> {
    let evidence = result.as_deref().unwrap_or("").trim();
    if !allow_empty && capture != ResultCapture::Missing && evidence.is_empty() {
        return Err(OperationError::new(
            "COMPLETION_EVIDENCE_REQUIRED",
            "completion requires explicit result evidence",
            false,
        ));
    }
    if verified && evidence.is_empty() {
        return Err(OperationError::new(
            "COMPLETION_EVIDENCE_REQUIRED",
            "verified completion requires explicit result evidence",
            false,
        ));
    }
    // Command checks can run for minutes; execute them before taking the global update lock.
    let command_results = match get_operation(project, id) {
        Ok(peek) if !peek.status.terminal() && !peek.checks.is_empty() => {
            crate::agent_runs::checks::run_command_checks(
                std::path::Path::new(&peek.project_path),
                &peek.checks,
            )
        }
        _ => Default::default(),
    };
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() {
        return Ok(op);
    }
    if op.status == OperationStatus::Cancelling {
        return Err(OperationError::new(
            "CANCELLATION_IN_PROGRESS",
            "operation cancellation has reserved the lifecycle state",
            true,
        ));
    }
    if !matches!(
        op.status,
        OperationStatus::Running | OperationStatus::WaitingInput | OperationStatus::Starting
    ) {
        return Err(OperationError::new(
            "INVALID_STATE_TRANSITION",
            "operation cannot complete in its current state",
            false,
        ));
    }
    op.status = OperationStatus::Completed;
    op.result = result.filter(|value| !value.trim().is_empty());
    op.result_capture = Some(capture);
    op.verified = verified;
    op.finished_at_ms = Some(now_ms());
    op.stage = Some("completed".into());
    op.source = match capture {
        ResultCapture::Authoritative => StateSource::Native,
        ResultCapture::Inferred | ResultCapture::Missing => StateSource::Inferred,
    };
    op.progress_pct = None;
    op.required_action = None;
    evaluate_acceptance_criteria(&mut op);
    if capture == ResultCapture::Inferred && op.checks.is_empty() {
        // Text scraped off a terminal screen with nothing configured to verify it: still
        // `completed`, but the stored result says so and no acceptance claim is made.
        op.verified = false;
        op.acceptance_status = AcceptanceStatus::NotChecked;
        if let Some(result) = op.result.as_mut() {
            if !result.starts_with(UNVERIFIED_SCREEN_PREFIX) {
                result.insert_str(0, UNVERIFIED_SCREEN_PREFIX);
            }
        }
    }
    if !op.checks.is_empty() {
        let results = crate::agent_runs::checks::evaluate_with_commands(
            std::path::Path::new(&op.project_path),
            &op.checks,
            op.result.as_deref(),
            &command_results,
        );
        let failure = crate::agent_runs::checks::failure_summary(&results);
        op.check_results = results;
        if let Some(message) = failure {
            op.status = OperationStatus::Failed;
            op.verified = false;
            op.stage = Some("failed".into());
            op.acceptance_status = AcceptanceStatus::Failed;
            op.error = Some(OperationError::new("ACCEPTANCE_FAILED", message, false));
        } else {
            // Verified `checks` are the authoritative acceptance signal.
            op.acceptance_status = AcceptanceStatus::Passed;
        }
    }
    bump(&mut op)?;
    Ok(op)
}

/// Finish a turn as interrupted without treating it as a successful completion.
pub fn mark_turn_interrupted(
    project: &str,
    id: &str,
    partial: Option<String>,
) -> Result<OperationSnapshot, OperationError> {
    let _serial = update_lock();
    let mut op = get_operation(project, id)?;
    if op.status.terminal() {
        return Ok(op);
    }
    op.status = OperationStatus::Cancelled;
    op.stage = Some("interrupted".into());
    op.result = partial.filter(|value| !value.trim().is_empty()).or(op.result);
    if op.result.is_none() {
        op.result_capture = Some(ResultCapture::Missing);
    } else if op.result_capture.is_none() {
        op.result_capture = Some(ResultCapture::Inferred);
    }
    op.keep_pane = true;
    op.finished_at_ms = Some(now_ms());
    op.required_action = None;
    bump(&mut op)?;
    Ok(op)
}

fn normalize_acceptance_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn criterion_matches_result_text(criterion: &str, result: &str) -> bool {
    let needle = normalize_acceptance_text(criterion);
    if needle.is_empty() {
        return false;
    }
    let haystack = normalize_acceptance_text(result);
    if haystack.contains(&needle) {
        return true;
    }
    // ponytail: loose token gate for paraphrased replies; upgrade with structured checks (file_exists:).
    let tokens: Vec<&str> = needle
        .split_whitespace()
        .filter(|word| word.len() >= 4)
        .collect();
    if tokens.len() >= 2 {
        tokens.iter().all(|word| haystack.contains(word))
    } else {
        false
    }
}

fn evaluate_acceptance_criteria(op: &mut OperationSnapshot) {
    let Some(criteria) = op.acceptance_criteria.as_ref() else {
        op.acceptance_status = AcceptanceStatus::NotChecked;
        return;
    };
    let meaningful: Vec<&str> = criteria
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect();
    if meaningful.is_empty() {
        op.acceptance_status = AcceptanceStatus::NotChecked;
        return;
    }
    let result = op.result.as_deref().unwrap_or("");
    let workspace = std::path::Path::new(&op.project_path);
    let mut hard_fail = false;
    let mut all_verified = true;

    for criterion in &meaningful {
        if let Some(relative) = criterion.strip_prefix("file_exists:") {
            let relative = relative.trim();
            if relative.is_empty() {
                all_verified = false;
                continue;
            }
            let target = workspace.join(relative);
            if !target.exists() {
                hard_fail = true;
            }
            continue;
        }
        if criterion_matches_result_text(criterion, result) {
            continue;
        }
        all_verified = false;
    }

    op.acceptance_status = if hard_fail {
        AcceptanceStatus::Failed
    } else if all_verified {
        AcceptanceStatus::Passed
    } else {
        AcceptanceStatus::NotChecked
    };
}
