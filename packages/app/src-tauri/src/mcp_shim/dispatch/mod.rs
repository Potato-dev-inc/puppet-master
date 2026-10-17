//! Maps a `tools/call` name to the family module that owns its bridge request.

mod agents;
mod coordination;
mod opencode;
mod operations;
mod panes;
mod team;

#[cfg(test)]
pub(crate) use team::team_wait_timeout_ms;

use super::*;
use std::sync::mpsc::Receiver;

/// Dispatches one tool call; unknown names are an error.
pub(super) fn dispatch(
    name: &str,
    args: &Value,
    cancel_rx: Option<&Receiver<()>>,
) -> Result<String, String> {
    None.or_else(|| agents::dispatch(name, args, cancel_rx))
        .or_else(|| panes::dispatch(name, args, cancel_rx))
        .or_else(|| coordination::dispatch(name, args, cancel_rx))
        .or_else(|| operations::dispatch(name, args, cancel_rx))
        .or_else(|| opencode::dispatch(name, args, cancel_rx))
        .or_else(|| team::dispatch(name, args, cancel_rx))
        .unwrap_or_else(|| Err(format!("unknown tool: {name}")))
}
