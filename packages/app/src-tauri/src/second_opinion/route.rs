//! "Send to <original agent>": where a review comment goes.
//!
//! - The pane belongs to a run (it has a handle): `followup_task` semantics, i.e. `send_agent` on
//!   that handle. That starts the next turn on the same conversation, or queues behind a turn
//!   that is still running.
//! - The pane was opened by hand (no run): the comment is typed into the pane and submitted, as if
//!   the user had written it.
//!
//! Lookup and delivery are traits so the routing is tested without panes or agents.

use crate::operations::OperationError;
use serde::Serialize;

/// One line, so a TUI never submits half of it: `Review comment on file:line: text. Please address it.`
pub fn compose(file: &str, line: Option<u32>, text: &str) -> String {
    let flat = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let place = match line {
        Some(n) => format!("{}:{n}", flat(file)),
        None => flat(file),
    };
    let body = flat(text);
    let body = body.trim_end_matches(['.', '!', '?']);
    format!("Review comment on {place}: {body}. Please address it.")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `(project, handle)` of the run bound to the pane.
    Run { project: String, handle: String },
    Pane { pane_id: String },
}

pub trait Targets {
    /// The latest run bound to the pane, as `(project, handle)`.
    fn run_of_pane(&self, pane_id: &str) -> Option<(String, String)>;
    fn pane_exists(&self, pane_id: &str) -> bool;
}

pub trait Sender {
    /// Start (or queue) the next turn of a run.
    fn followup(&self, project: &str, handle: &str, task: &str) -> Result<(), OperationError>;
    /// Type `text` into the pane and press Enter.
    fn pane_input(&self, pane_id: &str, text: &str) -> Result<(), String>;
}

pub fn decide(targets: &dyn Targets, pane_id: &str) -> Result<Route, OperationError> {
    if let Some((project, handle)) = targets.run_of_pane(pane_id) {
        return Ok(Route::Run { project, handle });
    }
    if targets.pane_exists(pane_id) {
        return Ok(Route::Pane { pane_id: pane_id.to_string() });
    }
    Err(OperationError::new(
        "AGENT_GONE",
        "That agent's pane is closed, so there is nobody to send this to.",
        false,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sent {
    /// `run` (a follow-up on the run's handle) or `pane` (typed into the pane).
    pub route: &'static str,
    /// The handle or the pane id.
    pub target: String,
}

/// Deliver an already-composed one-line message (see `service::send_text_to_author`).
pub fn deliver_text(
    targets: &dyn Targets,
    sender: &dyn Sender,
    pane_id: &str,
    message: &str,
) -> Result<Sent, OperationError> {
    match decide(targets, pane_id)? {
        Route::Run { project, handle } => {
            sender.followup(&project, &handle, message)?;
            Ok(Sent { route: "run", target: handle })
        }
        Route::Pane { pane_id } => {
            sender
                .pane_input(&pane_id, message)
                .map_err(|e| OperationError::new("SEND_FAILED", e, true))?;
            Ok(Sent { route: "pane", target: pane_id })
        }
    }
}

pub fn deliver(
    targets: &dyn Targets,
    sender: &dyn Sender,
    pane_id: &str,
    file: &str,
    line: Option<u32>,
    text: &str,
) -> Result<Sent, OperationError> {
    if text.trim().is_empty() {
        return Err(OperationError::new("INVALID_ARGUMENT", "Write a comment first.", false));
    }
    let message = compose(file, line, text);
    match decide(targets, pane_id)? {
        Route::Run { project, handle } => {
            sender.followup(&project, &handle, &message)?;
            Ok(Sent { route: "run", target: handle })
        }
        Route::Pane { pane_id } => {
            sender
                .pane_input(&pane_id, &message)
                .map_err(|e| OperationError::new("SEND_FAILED", e, true))?;
            Ok(Sent { route: "pane", target: pane_id })
        }
    }
}
