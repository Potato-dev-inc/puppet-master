//! "Pick for me": the first row of the "+" menu. It suggests ONE agent, and the saved login the
//! person uses here, from the person's own results in THIS project (last 90 days), and says why in
//! one sentence. Nothing starts until the person presses Start. Read-only: no file is written, no
//! audit entry is made and no run is created.
//!
//! - `model`: the types and constants. The answer is camel case JSON.
//! - `ready`: which candidates are skipped (cannot run, out of usage, shares a plan).
//! - `login`: the usual saved login here, and the evidence of one Scores row.
//! - `select`: the decision from the person's own numbers (clear gap, too early, close call).
//! - `reason`: the one sentence, the lines and the notes, in simple English.
//! - `compose`: puts the steps together into a `Pick`.
//! - `live`: reads the live sources (Scores rows, agent registry, accounts, limits).
//! - `tests`: the scaffold check, and later the privacy and reachability guards.
//!
//! Private: the answer holds no folder, pane id, run handle, prompt, reply, file name or approval
//! text. It holds ids, labels from the registry and accounts store, counts and a reset time.
//! Account labels can be e-mail addresses, so a future share or copy path must use `redact::strict`.
//! It looks at the whole project. The menu has no task text, and this module never reads any.
//!
//! Agents never see it: no bridge route, no MCP tool and no prompt uses it. The only entry is the
//! Tauri command `pick_agent` in `commands/scores.rs`.

pub mod compose;
pub mod live;
pub mod login;
pub mod model;
pub mod ready;
pub mod reason;
pub mod select;
#[cfg(test)]
mod tests;

// The entry points are read by the command (a later task); until then the re-exports are unused.
#[allow(unused_imports)]
pub use compose::pick;
#[allow(unused_imports)]
pub use live::pick_for_folder;
