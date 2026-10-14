//! The gate: called where an agent starts controlling something it did not create, or answers a
//! prompt in a pane that is not its own.
//!
//! With setting A off every function here returns `Ok` after writing the audit line, which is
//! today's behaviour. With A on, a pane the caller did not start (or a run it did not start) needs a
//! person's OK first: the call fails with `CONSENT_PENDING` and a card appears in the Inbox.
//!
//! Callers (keep this list in step; `no_ungated_take_over_callers` fails when a new one appears):
//! * `bridge/handoff.rs`: the `take_over` tool.
//! * `agent_runs/headless/bind.rs`: `run_agent(worker_id | pane_id)` adopting a pane that exists.
//! * `agent_runs/routes/answer.rs`: the `answer_prompt` tool.
//! * `bridge/routes/opencode.rs`: OpenCode permission and question replies (`check_answer`).
//! * `bridge/routes/panes.rs`: an agent's raw keys into a pane that shows a dialog
//!   (`check_raw_keys`).

use super::config;
use super::consent::{self, Action, Ask, Basis, Verdict};
use crate::audit::{self, Kind, Who};
use crate::mcp_sessions::{self, Relation};
use crate::operations::OperationError;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::Arc;

/// How long an agent should wait before asking again.
const RETRY_AFTER_MS: u64 = 10_000;

type Registry = Arc<Mutex<PaneRegistry>>;

/// The agent label the UI shows for an agent type, from its profile or definition.
fn agent_label(agent_type: &str) -> String {
    crate::tui::descriptors()
        .into_iter()
        .find(|d| d.id == agent_type)
        .map(|d| d.label)
        .unwrap_or_else(|| agent_type.to_string())
}

/// "Codex" for a pane running Codex; a plain fallback when the pane is not in the registry.
fn target_label(pane_id: Option<&str>, registry: Option<&Registry>) -> String {
    pane_id
        .zip(registry)
        .and_then(|(pane, registry)| registry.lock().panes.get(pane).map(|p| agent_label(&p.info.agent_type)))
        .unwrap_or_else(|| "that pane".to_string())
}

/// Who the card says is asking when the connection never said which host it is.
pub const UNKNOWN_REQUESTER: &str = "Another agent";

fn requester_label(session: &str) -> String {
    mcp_sessions::session_client_name(session).unwrap_or_else(|| UNKNOWN_REQUESTER.to_string())
}

fn needs_a_person(relation: &Relation) -> bool {
    matches!(relation, Relation::UserOpened | Relation::OtherAgent)
}

fn pending(ask: &Ask, id: &str) -> OperationError {
    let what = match ask.action {
        Action::TakeOver => format!("control {}", ask.target),
        Action::AnswerPrompt => format!("answer a question in {}", ask.target),
    };
    let mut error = OperationError::new(
        "CONSENT_PENDING",
        format!(
            "A person has to say yes first. Puppet Master is asking them whether you may {what}; that pane was not started by you, so nothing was done. Do not retry in a loop: wait about {} seconds, then make the same call again, or carry on with other work and check back. If you do not need that pane, start your own agent with run_agent instead.",
            RETRY_AFTER_MS / 1000
        ),
        true,
    );
    error.retry_after_ms = Some(RETRY_AFTER_MS);
    error.context = json!({
        "consent_id": id,
        "action": ask.action.as_str(),
        "retry_after_ms": RETRY_AFTER_MS,
        "pane_id": ask.pane_id,
        "answered_where": "the Inbox in the Puppet Master window",
    });
    error
}

fn refused(ask: &Ask, id: &str) -> OperationError {
    let mut error = OperationError::new(
        "CONSENT_DENIED",
        "The person said no, so nothing was done. Do not try again unless they ask you to. Start your own agent with run_agent if you need one, or tell the person what you needed.",
        false,
    );
    error.context = json!({ "consent_id": id, "action": ask.action.as_str(), "pane_id": ask.pane_id });
    error
}

fn run(ask: Ask, now: u64) -> Result<Basis, OperationError> {
    match consent::decide(&ask, now) {
        Verdict::Allowed(basis) => Ok(basis),
        Verdict::Pending(id) => Err(pending(&ask, &id)),
        Verdict::Denied(id) => Err(refused(&ask, &id)),
    }
}

/// An agent asks to take over a pane or a run it did not start (`grant=true`).
///
/// `pane_id` and `handle` are what the call names; either may be absent. The call is already known
/// to carry `grant=true` (without it the take-over fails on its own, and a card would be noise).
pub fn check_take_over(
    session: &str,
    pane_id: Option<&str>,
    handle: Option<&str>,
    registry: Option<&Registry>,
) -> Result<(), OperationError> {
    check_take_over_at(session, pane_id, handle, registry, questions_now())
}

pub(crate) fn check_take_over_at(
    session: &str,
    pane_id: Option<&str>,
    handle: Option<&str>,
    registry: Option<&Registry>,
    now: u64,
) -> Result<(), OperationError> {
    let relations = [
        pane_id.map(|pane| mcp_sessions::relation_to_pane(session, pane)),
        handle.map(|handle| mcp_sessions::relation_to_agent(session, handle)),
    ];
    let foreign = relations.iter().flatten().any(needs_a_person);
    if !foreign {
        return Ok(());
    }
    let subject = pane_id.or(handle).unwrap_or_default().to_string();
    let target = target_label(pane_id, registry);
    let requester = requester_label(session);
    let detail = json!({ "pane_id": pane_id, "handle": handle });
    if !config::ask_before_agent_control() {
        audit::record(
            Kind::TakeOver,
            Who::Agent(session.to_string()),
            &format!("{requester} took over {target}, which it did not start (not asked: the setting is off)"),
            detail,
        );
        return Ok(());
    }
    let ask = Ask {
        session: session.to_string(),
        subject,
        pane_id: pane_id.map(str::to_string),
        action: Action::TakeOver,
        prompt_key: String::new(),
        requester: requester.clone(),
        target: target.clone(),
        risk: None,
        risk_headline: None,
    };
    let basis = run(ask, now)?;
    let how = match basis {
        Basis::Once => "after you allowed it",
        Basis::Always => "because you allowed it for this pane",
    };
    audit::record(
        Kind::TakeOver,
        Who::Agent(session.to_string()),
        &format!("{requester} took over {target}, which it did not start, {how}"),
        detail,
    );
    Ok(())
}

fn questions_now() -> u64 {
    crate::questions::now_ms()
}

/// Is a prompt risky enough that an agent outside the pane's lineage needs a person's OK? Anything
/// above low, and anything whose risk is not known.
pub fn risk_needs_consent(level: Option<&str>) -> bool {
    !matches!(level, Some("low"))
}

/// An agent answers a prompt. `session` is `None` for the person's own answer (never gated).
pub fn check_answer(
    session: Option<&str>,
    pane_id: &str,
    prompt_id: &str,
    risk_level: Option<&str>,
    risk_headline: Option<&str>,
    registry: Option<&Registry>,
) -> Result<(), OperationError> {
    let Some(session) = session.filter(|s| !s.trim().is_empty()) else {
        return Ok(());
    };
    if !config::ask_before_agent_control() || !risk_needs_consent(risk_level) {
        return Ok(());
    }
    if !needs_a_person(&mcp_sessions::lineage_of_pane(session, pane_id)) {
        return Ok(());
    }
    let level = match risk_level {
        Some("high") => "high",
        _ => "medium",
    };
    let ask = Ask {
        session: session.to_string(),
        subject: pane_id.to_string(),
        pane_id: Some(pane_id.to_string()),
        action: Action::AnswerPrompt,
        prompt_key: prompt_id.to_string(),
        requester: requester_label(session),
        target: target_label(Some(pane_id), registry),
        risk: Some(level.to_string()),
        risk_headline: risk_headline.map(|h| h.chars().take(120).collect()),
    };
    run(ask, questions_now())?;
    Ok(())
}

/// Write the audit line for an answer that was delivered: who pressed it, in which pane, which
/// choice. The desktop's own answers (`session` is `None`) are recorded too. The command text is not.
pub fn note_answer(session: Option<&str>, pane_id: &str, prompt_id: &str, choice: &str, registry: Option<&Registry>) {
    let target = target_label(Some(pane_id), registry);
    let said = match choice {
        "allow_once" | "once" => "Allowed once",
        "deny" | "reject" => "Said no",
        "allow_all" | "allow_broad" | "always" => "Allowed for good",
        other => other,
    };
    let (who, by) = match session.filter(|s| !s.trim().is_empty()) {
        Some(session) => (Who::Agent(session.to_string()), requester_label(session)),
        None => (Who::Desktop, "You".to_string()),
    };
    audit::record(
        Kind::Approval,
        who,
        &format!("{by} answered a question in {target}: {said}"),
        json!({ "pane_id": pane_id, "prompt_id": prompt_id, "choice": choice }),
    );
}

/// An agent's raw keys (`write_terminal_input`, `press_key`) into a pane whose screen shows a
/// dialog (`prompt`, from `pane_prompt::detect`) answer it, so they are an answer (review R12 3.2):
/// - the gate is `check_answer`'s: with setting A on, a pane outside the agent's lineage and a
///   prompt above low risk need a person's OK (`CONSENT_PENDING`); with A off (today's default)
///   nothing is refused;
/// - every such write is written to the audit log (`Approval`, `via: raw_keys`), allowed or not
///   refused, with what was pressed described (`keys`), never the typed text.
///
/// `session` is `None` for the person (desktop, phone): never gated, not audited here (their
/// keys are theirs). Call it right before the write.
pub fn check_raw_keys(
    session: Option<&str>,
    pane_id: &str,
    prompt: &serde_json::Value,
    keys: &str,
    registry: Option<&Registry>,
) -> Result<(), OperationError> {
    let Some(session) = session.filter(|s| !s.trim().is_empty()) else {
        return Ok(());
    };
    let prompt_id = prompt.get("prompt_id").and_then(serde_json::Value::as_str).unwrap_or_default();
    let verdict = check_answer(
        Some(session),
        pane_id,
        prompt_id,
        prompt.pointer("/risk/level").and_then(serde_json::Value::as_str),
        prompt.pointer("/risk/headline").and_then(serde_json::Value::as_str),
        registry,
    );
    let target = target_label(Some(pane_id), registry);
    let by = requester_label(session);
    let outcome = match &verdict {
        Ok(()) => "sent",
        Err(error) if error.code == "CONSENT_PENDING" => "held: asking you first",
        Err(_) => "refused",
    };
    audit::record(
        Kind::Approval,
        Who::Agent(session.to_string()),
        &format!("{by} pressed keys in {target} while it showed a question, instead of answering it ({outcome})"),
        json!({
            "pane_id": pane_id,
            "prompt_id": prompt_id,
            "kind": prompt.get("kind"),
            "via": "raw_keys",
            "keys": keys,
            "outcome": outcome,
        }),
    );
    verdict
}
