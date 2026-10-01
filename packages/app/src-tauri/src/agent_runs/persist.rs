//! Worker-handle adoption, context modes, and requested-vs-resolved model choice.

use crate::operations::{
    self, ContextContinuity, ContextPolicy, OperationError, OperationSnapshot, OperationStatus,
    WorkerPersist,
};
use serde_json::{json, Value};

const PARENT_SUMMARY_LIMIT: usize = 2000;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub requested_model: Option<String>,
    pub resolved_model: Option<String>,
    pub requested_reasoning: Option<String>,
    pub resolved_reasoning: Option<String>,
    pub mismatch: bool,
}

#[derive(Debug, Clone)]
pub struct TurnPromptArgs<'a> {
    pub policy: ContextPolicy,
    pub task: &'a str,
    pub scope: Option<&'a str>,
    pub prior_result: Option<&'a str>,
    pub prior_user: Option<&'a str>,
    pub selected_history: &'a [String],
    pub can_resume: bool,
    pub is_followup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnPrompt {
    pub prompt: String,
    pub policy: ContextPolicy,
    pub continuity: ContextContinuity,
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
        Err(error) if error.code == "AGENT_NOT_FOUND" => operations::list_indexed_operations(project)?
            .into_iter()
            .filter(|snapshot| snapshot.worker.name.as_deref() == Some(key))
            .max_by_key(|snapshot| (snapshot.turn_index, snapshot.created_at_ms))
            .ok_or(error),
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

pub fn conversation_facts(project: &str, handle: &str) -> Option<String> {
    let turns = operations::turns_for_handle(project, handle).ok()?;
    let mut parts = Vec::new();
    for turn in turns {
        let task = turn.task.trim();
        if !task.is_empty() {
            parts.push(format!("User: {task}"));
        }
        if let Some(result) = turn
            .result
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            parts.push(format!("Assistant: {result}"));
        }
    }
    let text = parts.join("\n");
    if text.is_empty() {
        None
    } else {
        Some(owned_bound_summary(&text))
    }
}

pub fn render_turn_prompt(args: TurnPromptArgs<'_>) -> TurnPrompt {
    let continuity = resolve_continuity(args.policy, args.can_resume, args.is_followup);
    let cleaned_prior = args
        .prior_result
        .and_then(|text| super::completion::clean_extracted_result(text, ""));
    let reconstructed = if args.is_followup && !args.can_resume {
        Some(reconstruct_facts(args.prior_user, cleaned_prior.as_deref()))
            .filter(|text| !text.is_empty())
    } else {
        None
    };
    let reconstructed_facts = reconstructed.as_deref();
    let prompt = match args.policy {
        ContextPolicy::Fresh => args.task.to_string(),
        ContextPolicy::Packet => {
            context_packet(args.task, args.scope, reconstructed_facts)
        }
        ContextPolicy::ParentSummary => {
            let summary = if args.is_followup {
                reconstructed_facts.or(cleaned_prior.as_deref()).unwrap_or("(none)")
            } else {
                "(none)"
            };
            format!(
                "{}\n\nParent summary:\n{}",
                args.task,
                bound_summary(summary)
            )
        }
        ContextPolicy::Resume => context_packet(args.task, args.scope, reconstructed_facts),
        ContextPolicy::SelectedHistory => {
            let mut prompt = context_packet(args.task, args.scope, reconstructed_facts);
            if !args.selected_history.is_empty() {
                prompt.push_str("\nSelected history:");
                for message in args.selected_history {
                    prompt.push_str("\n- ");
                    prompt.push_str(message);
                }
            }
            prompt
        }
    };
    TurnPrompt {
        prompt,
        policy: args.policy,
        continuity,
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
    if let Some(model) = inspection
        .last_user_model
        .or(inspection.session_model)
    {
        return Some(format!("{}/{}", model.provider_id, model.model_id));
    }
    inspection.detected_model
}

pub fn apply_inspect_fields(value: &mut Value, snapshot: &OperationSnapshot, siblings: &[OperationSnapshot]) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let handle = if snapshot.agent_run_id.is_empty() {
        snapshot.operation_id.as_str()
    } else {
        snapshot.agent_run_id.as_str()
    };
    let queued: Vec<String> = siblings
        .iter()
        .filter(|item| item.agent_run_id == handle && item.status == OperationStatus::Queued)
        .map(|item| item.operation_id.clone())
        .collect();
    object.insert(
        "backend".into(),
        Value::String(snapshot.agent_type.clone()),
    );
    object.insert(
        "current_task".into(),
        Value::String(snapshot.task.clone()),
    );
    object.insert(
        "name".into(),
        snapshot
            .worker
            .name
            .clone()
            .map(Value::String)
            .unwrap_or(Value::String(handle.to_string())),
    );
    object.insert(
        "role".into(),
        opt_string(snapshot.worker.role.as_deref()),
    );
    object.insert(
        "scope".into(),
        opt_string(snapshot.worker.scope.as_deref()),
    );
    object.insert(
        "workspace".into(),
        snapshot
            .worker
            .workspace
            .clone()
            .map(Value::String)
            .unwrap_or_else(|| Value::String(snapshot.project_path.clone())),
    );
    object.insert(
        "provider_session_id".into(),
        opt_string(snapshot.worker.provider_session_id.as_deref()),
    );
    object.insert(
        "session_reset".into(),
        Value::Bool(snapshot.worker.session_reset),
    );
    object.insert(
        "context_policy".into(),
        Value::String(snapshot.worker.context_policy.as_str().into()),
    );
    object.insert(
        "context_continuity".into(),
        Value::String(snapshot.worker.context_continuity.as_str().into()),
    );
    object.insert(
        "requested_model".into(),
        opt_string(snapshot.worker.requested_model.as_deref()),
    );
    object.insert(
        "resolved_model".into(),
        opt_string(snapshot.worker.resolved_model.as_deref()),
    );
    object.insert(
        "requested_reasoning".into(),
        opt_string(snapshot.worker.requested_reasoning.as_deref()),
    );
    object.insert(
        "resolved_reasoning".into(),
        opt_string(snapshot.worker.resolved_reasoning.as_deref()),
    );
    object.insert(
        "model_mismatch".into(),
        Value::Bool(visible_mismatch(
            snapshot.worker.requested_model.as_deref(),
            snapshot.worker.resolved_model.as_deref(),
            snapshot.worker.requested_reasoning.as_deref(),
            snapshot.worker.resolved_reasoning.as_deref(),
        )),
    );
    object.insert("current_turn".into(), json!(snapshot.turn_index));
    let pending = if snapshot.worker.pending_messages.is_empty() {
        super::messaging::pending_texts(&snapshot.project_path, handle)
    } else {
        snapshot.worker.pending_messages.clone()
    };
    object.insert("pending_messages".into(), json!(pending));
    object.insert("queued_tasks".into(), json!(queued));
    object.insert(
        "ownership".into(),
        opt_string(snapshot.owner_session_id.as_deref()),
    );
    object.insert(
        "event_cursor".into(),
        json!(snapshot.worker.event_cursor.max(snapshot.revision)),
    );
    object.insert("closed".into(), json!(snapshot.worker.closed));
    object.insert(
        "capabilities".into(),
        super::capabilities::for_snapshot(snapshot).value(),
    );
    if let Some(receipt) = super::messaging::latest_steer(&snapshot.project_path, handle) {
        object.insert(
            "last_steer".into(),
            json!({
                "message_id": receipt.message_id,
                "state": receipt.disposition.as_str(),
                "turn_id": receipt.turn_id,
                "result": receipt.result,
            }),
        );
    }
}

fn context_packet(task: &str, scope: Option<&str>, facts: Option<&str>) -> String {
    let scope = scope.map(str::trim).filter(|value| !value.is_empty());
    let facts = facts
        .map(bound_summary)
        .filter(|value| !value.is_empty());
    if scope.is_none() && facts.is_none() {
        return task.to_string();
    }
    let mut prompt = task.to_string();
    if let Some(scope) = scope {
        prompt.push_str("\n\nScope: ");
        prompt.push_str(scope);
    }
    if let Some(facts) = facts {
        prompt.push_str("\n\nPrior context:\n");
        prompt.push_str(facts);
    }
    prompt
}

fn reconstruct_facts(prior_user: Option<&str>, prior_result: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(user) = prior_user.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(user.to_string());
    }
    if let Some(result) = prior_result.map(str::trim).filter(|value| !value.is_empty()) {
        if !parts.iter().any(|part| part.contains(result)) {
            parts.push(format!("Assistant: {result}"));
        }
    }
    parts.join("\n")
}

fn bound_summary(text: &str) -> &str {
    if text.len() <= PARENT_SUMMARY_LIMIT {
        return text;
    }
    let end = text
        .char_indices()
        .map(|(index, ch)| index + ch.len_utf8())
        .take_while(|end| *end <= PARENT_SUMMARY_LIMIT)
        .last()
        .unwrap_or(0);
    &text[..end]
}

fn owned_bound_summary(text: &str) -> String {
    bound_summary(text).to_string()
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

fn opt_string(value: Option<&str>) -> Value {
    value
        .map(str::to_string)
        .map(Value::String)
        .unwrap_or(Value::Null)
}
