//! Crews: a saved team of agents. "Save this team" turns a live pane and its helpers into a crew
//! (a TOML file in the profile's app data, `crews/<slug>.toml`); "Start crew" starts it again in any
//! folder, the root role first and then each helper under the pane of its parent.
//!
//! - `model`: the file shape (`Crew`, `Role`) and `CrewError`.
//! - `validate`: the checks a file must pass before it is saved or started (agent types exist in the
//!   registry, at most 8 roles, one root, no cycles, plain text only).
//! - `store`: the folder of crew files (`list`, `load`, `save`, `remove`).
//! - `draft`: a live tree of panes becomes a draft crew (folder path -> `{folder}`, redacted).
//! - `start`: the order and the stop-on-first-failure loop, over a `Starter`.
//! - `live`: the real `Starter` (the `run_agent` path) and the registry reader for drafts.
//!
//! Nothing here names an agent: any registered agent, wizard-added ones included, can fill a role.
//! A started pane is a normal pane: its trust and permission questions are never answered here.

pub mod draft;
pub mod live;
pub mod model;
pub mod start;
pub mod store;
pub mod validate;

pub use model::{Crew, CrewError, Role, MAX_ROLES, PLACEHOLDER};

#[cfg(test)]
mod tests;
