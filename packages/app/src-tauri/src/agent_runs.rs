//! Durable handles and bounded transcript support for MCP agent runs.

mod capabilities;
pub(crate) mod checks;
mod completion;
mod headless;
mod messaging;
mod native;
pub(crate) mod pane_close;
mod persist;
mod recovery;
mod routes;
mod runtime;
mod transcript;

pub(crate) use native::stable_prompt_id;
pub(crate) use persist::resolve_worker_name;
pub use recovery::on_native_pane_replaced;
pub use routes::handle_request;
pub(crate) use routes::latest_handle_for_pane;
#[cfg(test)]
pub(crate) use routes::visible_runs;
pub use runtime::{record_user_task, supervise_existing, AgentRunRequest, AgentRunView};
pub use capabilities::{read_only_supported, read_only_unsupported_message};

#[cfg(test)]
#[path = "agent_runs/tests.rs"]
mod tests;
