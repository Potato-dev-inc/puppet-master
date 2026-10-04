//! Worker-handle adoption, context modes, and requested-vs-resolved model choice. Turn prompt
//! rendering lives in `prompt`, the `inspect_agent` field set in `inspect`.

mod inspect;
mod prompt;

use crate::operations::{
    self, ContextContinuity, ContextPolicy, OperationError, OperationSnapshot, WorkerPersist,
};
use serde_json::Value;

pub use inspect::apply_inspect_fields;
#[cfg(test)]
pub(super) use prompt::original_task_for_test;
pub use prompt::{conversation_facts, render_turn_prompt, TurnPrompt, TurnPromptArgs};

pub(super) const PARENT_SUMMARY_LIMIT: usize = 2000;
pub(crate) const FOLLOWUP_WAIT_MS: u64 = 120_000;
/// Foreground wait budget for run_agent and followup_task (MCP + bridge defaults).
pub(crate) const FOREGROUND_WAIT_MS: u64 = FOLLOWUP_WAIT_MS;

#[derive(Debug, Clone)]
pub enum WorkerBind {
    Adopt {
        handle: String,
        #[allow(dead_code)]
        previous: OperationSnapshot,
    },
    Create {
        handle: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct FollowupOpts {
    pub context_policy: Option<ContextPolicy>,
    pub selected_history: Option<Vec<String>>,
    pub requested_model: Option<String>,
    pub requested_reasoning: Option<String>,
    pub role: Option<String>,
    pub scope: Option<String>,
    pub background: bool,
    pub wait_ms: Option<u64>,
    pub idempotency_key: Option<String>,
    /// Carried through so a run that adopts an existing worker keeps its acceptance gate and locks.
    pub acceptance_criteria: Option<Vec<String>>,
    pub checks: Option<Vec<super::checks::Check>>,
    pub locks: Vec<String>,
    /// Re-run a run that was parked on the workspace-trust prompt with `--trust`.
    pub trust_workspace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub requested_model: Option<String>,
    pub resolved_model: Option<String>,
    pub requested_reasoning: Option<String>,
    pub resolved_reasoning: Option<String>,
    pub mismatch: bool,
}

pub fn worker_key(
    agent_run_id: Option<&str>,
    handle: Option<&str>,
    name: Option<&str>,
) -> Option<String> {
    [agent_run_id, handle, name]
        .into_iter()
        .find_map(|value| value.map(str::trim).filter(|value| !value.is_empty()))
        .map(str::to_string)
}

/// A handle derived from the project and the caller's idempotency key. A retry of the same
/// `run_agent` call (after a transport timeout, a reconnect) must see the same handle, or the
/// request fingerprint differs and the retry is rejected as IDEMPOTENCY_KEY_CONFLICT instead of
/// replaying the run that is already in flight. Uuid-shaped so it reads like any other handle.
pub fn stable_handle(project: &str, idempotency_key: &str) -> String {
    fn fnv1a(seed: u64, parts: &[&str]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
        for part in parts {
            for byte in part.bytes().chain(std::iter::once(0u8)) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }
    let parts = [project, idempotency_key.trim()];
    let (high, low) = (fnv1a(0, &parts), fnv1a(0x9e37_79b9_7f4a_7c15, &parts));
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (high >> 32) as u32,
        (high >> 16) as u16,
        (high & 0x0fff) as u16,
        0x8000 | ((low >> 48) as u16 & 0x3fff),
        low & 0xffff_ffff_ffff
    )
}

pub fn decide_worker(
    project: &str,
    key: Option<&str>,
    fresh: bool,
) -> Result<WorkerBind, OperationError> {
    let Some(key) = key.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(WorkerBind::Create {
            handle: uuid::Uuid::new_v4().to_string(),
        });
    };
    match find_worker(project, key) {
        Ok(previous) if previous.worker.closed || previous.stage.as_deref() == Some("closed") => {
            Ok(WorkerBind::Create {
                handle: format!("{key}-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
            })
        }
        Ok(previous) if !fresh => Ok(WorkerBind::Adopt {
            handle: previous.agent_run_id.clone(),
            previous,
        }),
        Ok(_) => Ok(WorkerBind::Create {
            handle: format!("{key}-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
        }),
        Err(error) if error.code == "AGENT_NOT_FOUND" => Ok(WorkerBind::Create {
            handle: key.to_string(),
        }),
        Err(error) => Err(error),
    }
}

pub fn find_worker(project: &str, key: &str) -> Result<OperationSnapshot, OperationError> {
    match operations::resolve_agent_run(project, key) {
        Ok(snapshot) => Ok(snapshot),
        Err(error) if error.code == "AGENT_NOT_FOUND" => {
            operations::list_indexed_operations(project)?
                .into_iter()
                .filter(|snapshot| snapshot.worker.name.as_deref() == Some(key))
                .max_by_key(|snapshot| (snapshot.turn_index, snapshot.created_at_ms))
                .ok_or(error)
        }
        Err(error) => Err(error),
    }
}

/// Resolve a persisted worker `name` to its run handle. Names survive app restarts (pane ids
/// do not). `Ok(None)` when nothing carries the name; an error when several distinct runs do.
pub fn resolve_worker_name(project: &str, name: &str) -> Result<Option<String>, OperationError> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    let mut handles: Vec<String> = operations::list_indexed_operations(project)?
        .into_iter()
        .filter(|snapshot| snapshot.worker.name.as_deref() == Some(name) && !snapshot.worker.closed)
        .map(|snapshot| {
            if snapshot.agent_run_id.is_empty() {
                snapshot.operation_id
            } else {
                snapshot.agent_run_id
            }
        })
        .collect();
    handles.sort();
    handles.dedup();
    match handles.len() {
        0 => Ok(None),
        1 => Ok(handles.pop()),
        n => {
            let mut error = OperationError::new(
                "AMBIGUOUS_WORKER_NAME",
                format!("worker name `{name}` matches {n} runs; pass the run handle from list_agents instead"),
                false,
            );
            error.context = serde_json::json!({"name": name, "handles": handles});
            Err(error)
        }
    }
}

pub fn resolve_model_choice(
    requested_model: Option<&str>,
    requested_reasoning: Option<&str>,
    profile_default_model: Option<&str>,
) -> ModelChoice {
    let requested_model = nonempty(requested_model);
    let requested_reasoning = nonempty(requested_reasoning);
    // ponytail: never remap a requested model/reasoning; a later resolver may set
    // a different resolved_* only if it also sets mismatch=true.
    let resolved_model = requested_model
        .clone()
        .or_else(|| nonempty(profile_default_model));
    let resolved_reasoning = requested_reasoning.clone();
    ModelChoice {
        mismatch: visible_mismatch(
            requested_model.as_deref(),
            resolved_model.as_deref(),
            requested_reasoning.as_deref(),
            resolved_reasoning.as_deref(),
        ),
        requested_model,
        resolved_model,
        requested_reasoning,
        resolved_reasoning,
    }
}

pub fn visible_mismatch(
    requested_model: Option<&str>,
    resolved_model: Option<&str>,
    requested_reasoning: Option<&str>,
    resolved_reasoning: Option<&str>,
) -> bool {
    differs(requested_model, resolved_model) || differs(requested_reasoning, resolved_reasoning)
}

pub fn snapshot_can_resume(snapshot: &OperationSnapshot) -> bool {
    !snapshot.worker.session_reset
        && super::capabilities::for_snapshot(snapshot).session_resume
        && snapshot
            .worker
            .provider_session_id
            .as_deref()
            .is_some_and(|id| !id.is_empty())
}

/// A follow-up that goes back into the same live pane continues that pane's own conversation, so
/// pasting a rebuilt "Prior context" block would repeat what the TUI already shows. Native opencode
/// resumes through its provider session instead.
pub fn pane_keeps_context(previous: &OperationSnapshot) -> bool {
    previous.pane_id.is_some()
        && previous.agent_type != "opencode_native"
        && !previous.worker.session_reset
}

pub fn resolve_continuity(
    policy: ContextPolicy,
    can_resume: bool,
    is_followup: bool,
) -> ContextContinuity {
    if !is_followup || policy == ContextPolicy::Fresh {
        ContextContinuity::None
    } else if can_resume {
        ContextContinuity::Resume
    } else {
        ContextContinuity::ReconstructedSummary
    }
}

/// Ordinary follow-ups must not inherit an initial `fresh` launch.
pub fn followup_policy(requested: Option<ContextPolicy>, previous: ContextPolicy) -> ContextPolicy {
    match requested {
        Some(policy) => policy,
        None if previous == ContextPolicy::Fresh => ContextPolicy::Packet,
        None => previous,
    }
}

pub fn build_worker(
    handle: &str,
    name: Option<&str>,
    role: Option<&str>,
    scope: Option<&str>,
    workspace: Option<&str>,
    previous: Option<&OperationSnapshot>,
    rendered: &TurnPrompt,
    choice: &ModelChoice,
) -> WorkerPersist {
    let mut worker = previous
        .map(|snapshot| snapshot.worker.clone())
        .unwrap_or_default();
    worker.name = nonempty(name)
        .or(worker.name.take())
        .or_else(|| Some(handle.to_string()));
    if let Some(role) = nonempty(role) {
        worker.role = Some(role);
    }
    if let Some(scope) = nonempty(scope) {
        worker.scope = Some(scope);
    }
    if let Some(workspace) = nonempty(workspace) {
        worker.workspace = Some(workspace);
    } else if worker.workspace.is_none() {
        worker.workspace = previous.map(|snapshot| snapshot.project_path.clone());
    }
    if let Some(session) = previous.and_then(|snapshot| snapshot.worker.provider_session_id.clone())
    {
        worker.provider_session_id = Some(session);
    }
    worker.session_reset = false;
    worker.requested_model = choice.requested_model.clone();
    worker.resolved_model = choice.resolved_model.clone();
    worker.requested_reasoning = choice.requested_reasoning.clone();
    worker.resolved_reasoning = choice.resolved_reasoning.clone();
    worker.context_policy = rendered.policy;
    worker.context_continuity = rendered.continuity;
    worker
}

/// Single source for the model fields shown in run/list/wait/inspect views.
/// `requested` is the model argument persisted on the snapshot. `resolved` is the live pane
/// model when one is known (opencode_native: authoritative), else the last persisted value,
/// else the live profile/buffer guess.
pub fn resolve_model_fields(
    requested: Option<&str>,
    persisted_resolved: Option<&str>,
    live: Option<String>,
    live_authoritative: bool,
) -> (Option<String>, Option<String>) {
    let requested = nonempty(requested);
    let persisted = nonempty(persisted_resolved);
    let live = live.filter(|model| !model.trim().is_empty());
    let resolved = if live_authoritative {
        live.or(persisted)
    } else {
        persisted.or(live)
    };
    (requested, resolved)
}

/// Overlay live model data onto an already serialized view (`requested_model`/`resolved_model`).
pub fn overlay_live_model(
    value: &mut Value,
    snapshot: &OperationSnapshot,
    registry: Option<&std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>>,
) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let live = registry.and_then(|registry| live_resolved_model(registry, snapshot));
    let (requested, resolved) = resolve_model_fields(
        snapshot.worker.requested_model.as_deref(),
        snapshot.worker.resolved_model.as_deref(),
        live,
        snapshot.agent_type == "opencode_native",
    );
    object.insert("requested_model".into(), opt_string(requested.as_deref()));
    object.insert("resolved_model".into(), opt_string(resolved.as_deref()));
}

pub fn live_resolved_model(
    registry: &std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
    snapshot: &OperationSnapshot,
) -> Option<String> {
    let pane_id = snapshot.pane_id.as_deref()?;
    let agent_type = crate::pty::agents::AgentType::parse(&snapshot.agent_type)?;
    let buffer = crate::pty::registry_read_snapshot(registry, pane_id).unwrap_or_default();
    let inspection = crate::agent_contexts::inspect_agent_model_with_registry(
        Some(registry),
        pane_id,
        agent_type,
        &buffer,
    );
    if let Some(model) = inspection.last_user_model.or(inspection.session_model) {
        return Some(format!("{}/{}", model.provider_id, model.model_id));
    }
    inspection.detected_model
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn differs(requested: Option<&str>, resolved: Option<&str>) -> bool {
    matches!((requested, resolved), (Some(left), Some(right)) if left != right)
}

pub(super) fn opt_string(value: Option<&str>) -> Value {
    value
        .map(str::to_string)
        .map(Value::String)
        .unwrap_or(Value::Null)
}
