//! "How agent runs work": a per-agent choice between a visible pane (today's behaviour, the
//! default) and one-task mode (a headless run whose answer, model and session come from the
//! agent's own output).
//!
//! * `store` keeps the choice per agent id in `<app data>/agent_run_mode.json`. Like the authority
//!   switches it is NOT in the phone-patchable settings mirror: it changes how an agent runs, so
//!   only the Settings window (a Tauri command) writes it.
//! * `verify` decides `headless_verified`: the definition has a `[headless]` table with an output
//!   parser AND the wizard's live test passed for exactly this file.
//! * `decide` / `apply` turn the choice into the run: `run_agent` calls [`apply`] when the caller
//!   did not pass `headless` itself. An explicit `headless: true/false` always wins. A follow-up
//!   continues in the mode of its run and never reads the setting.
//!
//! One-task mode runs the very same approved headless argv as `headless: true` does today; nothing
//! new is executed. When the choice is one-task but the verification no longer holds, the run
//! falls back to the visible pane and the receipt says so (`fallback: {from: "one_task",
//! reason: "not_verified"}`).

mod store;
#[cfg(test)]
mod tests;
mod verify;

use crate::operations::{RunFallback, RunModeNote};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use store::{set_for_agent, setting_for};
#[cfg(feature = "dev-fake-tui")]
pub use verify::e2e;
pub use verify::{verification_for, Verification};

/// Where the receipt says the mode came from.
pub const SOURCE_AGENT_SETTING: &str = "agent_setting";
/// The fallback reason when one-task mode was chosen but is not verified (any more).
pub const NOT_VERIFIED: &str = "not_verified";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// A pane the person can watch. Today's behaviour.
    #[default]
    Visible,
    /// A headless run: verified answer, model and session; no pane.
    OneTask,
}

impl RunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RunMode::Visible => "visible",
            RunMode::OneTask => "one_task",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "visible" => Some(RunMode::Visible),
            "one_task" => Some(RunMode::OneTask),
            _ => None,
        }
    }
}

/// What the setting does to one run that did not say `headless` itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Decision {
    /// Run headless (one-task mode in force and verified).
    pub headless: bool,
    /// What the receipt reports; absent when the setting played no part (the default).
    pub note: Option<RunModeNote>,
    /// Set when one-task mode was chosen but could not be used.
    pub fallback: Option<RunFallback>,
}

/// The pure rule. `setting` is the agent's choice, `verified` whether its headless mode is
/// verified, `headless_possible` whether the agent can run headless at all.
pub fn decide(setting: RunMode, verified: bool, headless_possible: bool) -> Decision {
    if setting != RunMode::OneTask {
        return Decision::default();
    }
    if verified && headless_possible {
        return Decision {
            headless: true,
            note: Some(RunModeNote::new("one_task", SOURCE_AGENT_SETTING)),
            fallback: None,
        };
    }
    Decision {
        headless: false,
        note: Some(RunModeNote::new("visible", SOURCE_AGENT_SETTING)),
        fallback: Some(RunFallback::one_task_to_visible(NOT_VERIFIED)),
    }
}

/// Applies the agent's setting to a new run that did not pass `headless` itself. Only a first turn
/// that is not adopting a pane and not part of a mission is touched; everything else keeps the
/// mode it was given (an explicit `headless`, the mode of the run being continued).
pub(crate) fn apply(request: &mut crate::agent_runs::AgentRunRequest) -> Decision {
    if !request.headless_unset
        || request.headless
        || request.turn_index > 0
        || request.pane_id.is_some()
        || request.join_worktree.is_some()
        || request.until_checks_pass
    {
        return Decision::default();
    }
    let agent = request.agent_type.as_str();
    let setting = setting_for(agent);
    if setting != RunMode::OneTask {
        return Decision::default();
    }
    let verified = verification_for(agent).verified;
    let possible = crate::tui::run_modes(agent).headless;
    let decision = decide(setting, verified, possible);
    request.headless = decision.headless;
    decision
}

/// The run-mode facts of one agent type, as `GET /agents/types` and `list_agents` show them.
pub fn agent_fields(id: &str, is_declarative: bool, headless_supported: bool) -> Value {
    let verification = if is_declarative {
        verification_for(id)
    } else {
        Verification::built_in()
    };
    json!({
        "headless_supported": headless_supported,
        "headless_verified": verification.verified,
        "headless_verified_reason": verification.reason,
        "run_mode": setting_for(id).as_str(),
    })
}

/// Adds the run-mode fields to every agent of a `GET /agents/types` body.
pub fn decorate_listing(mut listing: Value) -> Value {
    let Some(agents) = listing.get_mut("agents").and_then(Value::as_array_mut) else {
        return listing;
    };
    for agent in agents {
        let Some(id) = agent.get("id").and_then(Value::as_str).map(str::to_string) else {
            continue;
        };
        let declarative = agent.get("definition").is_some_and(|d| !d.is_null());
        let headless = agent
            .pointer("/modes/headless")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let (Some(target), Some(fields)) = (
            agent.as_object_mut(),
            agent_fields(&id, declarative, headless).as_object().cloned(),
        ) {
            target.extend(fields);
        }
    }
    listing
}

/// `agent_types` for `list_agents`: one short row per dispatchable agent, so a lead sees the
/// choice even when no worker is live.
pub fn types_summary() -> Value {
    let listing = decorate_listing(crate::tui::agent_types_listing());
    let rows: Vec<Value> = listing
        .get("agents")
        .and_then(Value::as_array)
        .map(|agents| {
            agents
                .iter()
                .filter(|a| a.get("dispatchable").and_then(Value::as_bool).unwrap_or(false))
                .map(|a| {
                    json!({
                        "id": a.get("id"),
                        "headless_supported": a.get("headless_supported"),
                        "headless_verified": a.get("headless_verified"),
                        "run_mode": a.get("run_mode"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(rows)
}

/// The Settings view: one row per custom agent with what it can do and what is chosen.
pub fn settings_view() -> Value {
    let listing = decorate_listing(crate::tui::agent_types_listing());
    let rows: Vec<Value> = listing
        .get("agents")
        .and_then(Value::as_array)
        .map(|agents| {
            agents
                .iter()
                .filter(|a| a.get("definition").is_some_and(|d| !d.is_null()))
                .map(|a| {
                    json!({
                        "id": a.get("id"),
                        "label": a.get("label"),
                        "headless_supported": a.get("headless_supported"),
                        "headless_verified": a.get("headless_verified"),
                        "headless_verified_reason": a.get("headless_verified_reason"),
                        "run_mode": a.get("run_mode"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "agents": rows })
}

/// Saves one agent's choice. One-task mode is refused unless the agent's headless mode is verified.
pub fn set_checked(id: &str, mode: RunMode) -> Result<(), String> {
    if mode == RunMode::OneTask {
        let verification = verification_for(id);
        if !verification.verified {
            return Err(format!("RUN_MODE_NOT_VERIFIED: {}", verification.explain()));
        }
    }
    set_for_agent(id, mode)
}
