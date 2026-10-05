//! Agent run operations behind the MCP agent tools: the request and view types, the
//! query/command functions (run, send, steer, interrupt, cancel, close, inspect), and the
//! per-handle lock that serializes turn changes.
//!
//! - `request`, `view`, `receipt`, `present`: the data shapes and how they are built.
//! - `access`: handle resolution with the session ownership gate, transcript reads.
//! - `control`, `steer`, `followup`, `queue`: the commands that change a run.

mod access;
mod control;
mod followup;
mod present;
mod queue;
mod receipt;
mod request;
mod steer;
mod view;

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

pub use super::headless::{run_agent, supervise_existing};
pub(super) use access::check_snapshot_mutate_access;
pub use access::{
    current_agent_for_read, current_agent_for_wait, record_user_task, transcript_for_handle,
    transcript_page,
};
pub(super) use control::settle_native_abort;
#[cfg(test)]
pub(super) use control::{active_turns, interrupt_active_turns};
pub use control::{cancel_agent, close_agent, inspect_agent};
#[cfg(test)]
pub(super) use followup::enqueue_followup;
pub use followup::send_agent;
pub use present::agent_view_after_wait;
#[cfg(test)]
pub(super) use present::pane_presentation;
#[cfg(test)]
pub use receipt::RunReceipt;
pub(super) use request::default_agent;
pub(crate) use request::normalize_context_policy_fields;
pub use request::AgentRunRequest;
pub use steer::send_message;
pub use view::AgentRunView;

/// Warning code on a pane run whose captured result is only the tail of the reply.
pub(super) const RESULT_TRUNCATED: &str = "RESULT_TRUNCATED";
/// Warning on a visible turn that finished with no reply text on the pane (nothing was inferred).
pub(super) const NO_REPLY_TEXT: &str = "NO_REPLY_TEXT";

static HANDLE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

/// One lock per run handle, so concurrent turn changes on the same handle are serialized.
fn handle_lock(handle: &str) -> Arc<Mutex<()>> {
    HANDLE_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .entry(handle.to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

#[cfg(test)]
pub(crate) fn run_warnings_for_tests(
    snapshot: &crate::operations::OperationSnapshot,
) -> Vec<receipt::RunWarning> {
    receipt::run_warnings(snapshot)
}
