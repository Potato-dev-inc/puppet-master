//! What a finished run proved and did not (`RunReceipt`) and the caveats on it (`RunWarning`),
//! derived only from recorded facts on the snapshot.

use crate::operations::{OperationSnapshot, OperationStatus, ResultCapture};
use serde::{Deserialize, Serialize};

/// What a run proved and what it did not. Every `not_verified` entry is derived from a fact the
/// run recorded; nothing here is a guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReceipt {
    pub backend: String,
    /// The model that ran when the backend confirmed it, else the one requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `enforced`, `unenforced` (requested but the backend cannot enforce it) or `off`.
    pub read_only: String,
    /// `none configured` or `<passed>/<total> passed`.
    pub checks: String,
    pub result_capture: String,
    pub not_verified: Vec<String>,
    /// One field for "did it work": `passed_checks`, `completed_unchecked`, `failed_checks`,
    /// `failed`, `cancelled`, `needs_input` or `in_progress`. Only `passed_checks` means the
    /// requested behaviour was proven; `completed_unchecked` means the worker finished but nothing
    /// verified it.
    #[serde(default)]
    pub outcome: String,
    /// Puppet Master build that produced the run, so a result can be tied to the code under test.
    #[serde(default)]
    pub build_id: String,
    /// A warm standby worker (`crate::standby`) ran the turn: no pane spawn or CLI startup was
    /// paid for it. Absent when the run used a freshly spawned worker.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub standby_used: bool,
    /// Set when the run wanted a visible pane and ran headless instead: `{from, to, reason}` with
    /// reason `no_visible_mode` or `visible_failed`. Null otherwise.
    #[serde(default)]
    pub fallback: Option<crate::operations::RunFallback>,
    /// The definition (id, source, fingerprint, pinned binary) a declarative agent ran from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_definition: Option<crate::tui::DefinitionRef>,
    /// Set when the run worked in an isolated git worktree (`isolation: "worktree"`): where, on
    /// which branch, what it changed, and whether the changes were merged. Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<ReceiptWorktree>,
}

/// The worktree facts of a receipt. `state` is `unreviewed` (still on disk, not merged),
/// `merged` or `discarded`. The change counts are those recorded when the run finished (absent
/// for a run that did not complete); `worktree_diff` always reads the live state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptWorktree {
    pub path: String,
    pub branch: String,
    pub base: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_changed: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u32>,
}

pub(super) fn build_receipt(
    snapshot: &OperationSnapshot,
    warnings: &[RunWarning],
    model_verified: Option<bool>,
    result_capture: &str,
) -> RunReceipt {
    let terminal = snapshot.status.terminal();
    // Prefer the facts recorded when the turn started; old snapshots fall back to the live registry.
    let held = snapshot.worker.run_features.unwrap_or_else(|| {
        crate::operations::RunFeatures::of(crate::tui::profile_for(&snapshot.agent_type).as_ref())
    });
    let read_only = if !snapshot.read_only {
        "off"
    } else if held.read_only_headless
        || (held.read_only_spawned_pane
            && snapshot
                .pane_id
                .as_deref()
                .is_some_and(crate::tui::opencode::read_only::is_enforced))
    {
        "enforced"
    } else {
        "unenforced"
    };
    let total = snapshot.checks.len();
    let checks = if total == 0 {
        "none configured".to_string()
    } else if terminal {
        let passed = snapshot
            .check_results
            .iter()
            .filter(|result| result.passed)
            .count();
        format!("{passed}/{total} passed")
    } else {
        format!("0/{total} passed (pending)")
    };
    let mut not_verified = Vec::new();
    if total == 0 {
        not_verified.push("no checks configured".to_string());
    }
    if terminal && result_capture == "inferred" {
        not_verified.push("result read from terminal screen".to_string());
    }
    if terminal && snapshot.worker.requested_model.is_some() && model_verified != Some(true) {
        not_verified.push("model not confirmed".to_string());
    }
    if let Some(fallback) = &snapshot.worker.fallback {
        not_verified.push(format!(
            "ran headless, not in a visible pane ({})",
            fallback.reason
        ));
    }
    if held.experimental {
        not_verified.push("experimental agent: screen heuristics only".to_string());
    }
    if read_only == "unenforced" {
        not_verified.push("read_only not enforced".to_string());
    }
    if warnings
        .iter()
        .any(|warning| warning.code == "SHELL_COMMAND_REJECTED")
    {
        not_verified.push("shell commands rejected".to_string());
    }
    if warnings
        .iter()
        .any(|warning| warning.code == "AGENT_REPORTED_ERROR")
    {
        not_verified.push("agent reported a tool failure".to_string());
    }
    let outcome = receipt_outcome(snapshot, total);
    RunReceipt {
        backend: snapshot.agent_type.clone(),
        model: snapshot
            .worker
            .resolved_model
            .clone()
            .or_else(|| snapshot.worker.requested_model.clone()),
        read_only: read_only.into(),
        checks,
        result_capture: result_capture.into(),
        not_verified,
        outcome: outcome.into(),
        build_id: env!("PM_BUILD_ID").to_string(),
        standby_used: snapshot.worker.standby_used,
        fallback: snapshot.worker.fallback.clone(),
        agent_definition: snapshot.worker.agent_definition.clone(),
        worktree: snapshot.worker.worktree.as_ref().map(|worktree| ReceiptWorktree {
            path: worktree.path.clone(),
            branch: worktree.branch.clone(),
            base: worktree.base.clone(),
            state: worktree
                .closed
                .clone()
                .unwrap_or_else(|| "unreviewed".to_string()),
            files_changed: worktree.changes.map(|c| c.files_changed),
            additions: worktree.changes.map(|c| c.additions),
            deletions: worktree.changes.map(|c| c.deletions),
        }),
    }
}

fn receipt_outcome(snapshot: &OperationSnapshot, checks_total: usize) -> &'static str {
    match snapshot.status {
        OperationStatus::Completed => {
            let all_passed = checks_total > 0
                && snapshot.check_results.len() >= checks_total
                && snapshot.check_results.iter().all(|result| result.passed);
            // Completed with checks that did not all pass cannot normally happen (a failing
            // check fails the run); never report it as proven.
            if all_passed {
                "passed_checks"
            } else {
                "completed_unchecked"
            }
        }
        OperationStatus::Failed => {
            if snapshot
                .error
                .as_ref()
                .is_some_and(|error| error.code == "ACCEPTANCE_FAILED")
            {
                "failed_checks"
            } else {
                "failed"
            }
        }
        OperationStatus::Cancelled => "cancelled",
        OperationStatus::WaitingInput => "needs_input",
        OperationStatus::Queued
        | OperationStatus::Starting
        | OperationStatus::Running
        | OperationStatus::Cancelling => "in_progress",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunWarning {
    pub code: String,
    /// `capture` (how the result was obtained) or `heuristic` (text match; not a fact).
    pub source: String,
    pub detail: String,
}

pub(super) fn run_warnings(snapshot: &OperationSnapshot) -> Vec<RunWarning> {
    if !snapshot.status.terminal() {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    let mut push = |code: &str, source: &str, detail: &str| {
        warnings.push(RunWarning {
            code: code.into(),
            source: source.into(),
            detail: detail.into(),
        })
    };
    for warning in &snapshot.warnings {
        push(&warning.code, "capture", &warning.detail);
    }
    let facts = crate::tui::RunFacts {
        result: snapshot.result.as_deref(),
        has_pane: snapshot.pane_id.is_some(),
        completed: snapshot.status == OperationStatus::Completed,
        result_inferred: matches!(snapshot.result_capture, Some(ResultCapture::Inferred)),
        model_requested: snapshot.worker.requested_model.is_some(),
        model_unconfirmed: snapshot.worker.model_unconfirmed,
    };
    for warning in crate::tui::profile_for(&snapshot.agent_type).run_warnings(&facts) {
        push(warning.code, warning.source, warning.detail);
    }
    if snapshot.status == OperationStatus::Completed {
        if snapshot.worker.resolved_model.is_none() {
            // Never a silent null: say whether it was asked for and that the backend stayed silent.
            let detail = if snapshot.worker.requested_model.is_some() {
                "A model was requested but the backend did not confirm which model ran.".to_string()
            } else {
                format!(
                    "{} did not report which model ran and no configured default could be determined, so resolved_model is null.",
                    snapshot.agent_type
                )
            };
            warnings.push(RunWarning {
                code: "MODEL_UNRESOLVED".into(),
                source: "capture".into(),
                detail,
            });
        } else if snapshot.worker.model_unconfirmed
            && !warnings.iter().any(|warning| warning.code == "MODEL_UNCONFIRMED")
        {
            warnings.push(RunWarning {
                code: "MODEL_UNCONFIRMED".into(),
                source: "capture".into(),
                detail: "The backend does not report which model ran; resolved_model is the CLI's configured default, not a confirmation.".into(),
            });
        }
    }
    warnings
}
