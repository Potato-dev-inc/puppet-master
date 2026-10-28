//! Crews: a saved team of agents. "Save this team" turns a live pane and its helpers into a crew
//! (a TOML file in the profile's app data, `crews/<slug>.toml`); "Start crew" starts it again in any
//! folder, the root role first and then each helper under the pane of its parent.
//!
//! - `model`: the file shape (`Crew`, `Role`) and `CrewError`.
//! - `validate`: the checks a file must pass before it is saved or started (agent types exist in the
//!   registry, at most 8 roles, one root, no cycles, plain text only).
//! - `store`: the folder of crew files (`list`, `load`, `save`, `remove`).
//! - `starters`: the three built-in starter crews (compiled in, never written to app data).
//! - `agents`: `{default}` roles take the default agent at start; the Start sheet's per-role choices.
//! - `draft`: a live tree of panes becomes a draft crew (folder path -> `{folder}`, redacted).
//! - `start`: the order and the stop-on-first-failure loop, over a `Starter`.
//! - `live`: the real `Starter` (the `run_agent` path) and the registry reader for drafts.
//!
//! Nothing here names an agent: any registered agent, wizard-added ones included, can fill a role.
//! A started pane is a normal pane: its trust and permission questions are never answered here.

pub mod agents;
pub mod draft;
pub mod live;
pub mod model;
pub mod start;
pub mod starters;
pub mod store;
pub mod validate;

pub use model::{Crew, CrewError, Role, MAX_ROLES, PLACEHOLDER};

/// Idempotency keys of crew starts begin with this. The dispatcher (`bridge/dispatch.rs`) never lands a
/// run with such a key in a pane that already exists: a crew role always gets a pane of its own, so a
/// pane the person opened by hand is never handed a task, renamed or linked as a helper.
pub const KEY_PREFIX: &str = "crew-";

/// The key of the `n`th role start of the crew start `run`.
pub fn start_key(run: &str, n: usize) -> String {
    format!("{KEY_PREFIX}{run}-{n}")
}

pub fn wants_new_pane(idempotency_key: &str) -> bool {
    idempotency_key.starts_with(KEY_PREFIX)
}

#[cfg(test)]
mod starter_tests;
#[cfg(test)]
mod tests;
