//! Durable handles and bounded transcript support for MCP agent runs.

pub(crate) mod approval_policy;
mod answer_guard;
mod capabilities;
pub(crate) mod checks;
mod completion;
pub(crate) mod delivery;
mod headless;
mod messaging;
mod native;
pub(crate) mod pane_close;
pub(crate) mod persist;
mod quick;
mod recovery;
mod routes;
mod runtime;
pub(crate) mod transcript;

#[cfg(test)]
mod cursor_capture_tests;

#[cfg(test)]
mod cursor_delivery_tests;

#[cfg(test)]
mod delivery_contract_tests;

pub(crate) use native::stable_prompt_id;
pub(crate) use persist::resolve_worker_name;
pub use recovery::on_native_pane_replaced;
pub use routes::handle_request;
pub(crate) use routes::latest_handle_for_pane;
#[cfg(test)]
pub(crate) use routes::visible_runs;
pub use runtime::{record_user_task, supervise_existing, AgentRunRequest, AgentRunView};
pub use capabilities::{read_only_supported, read_only_supported_for, read_only_unsupported_message};

#[cfg(test)]
#[path = "agent_runs/tests.rs"]
mod tests;
