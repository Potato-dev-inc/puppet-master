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
pub(crate) mod pane_api;
pub(crate) mod persist;
pub(crate) mod quick;
pub mod readiness;
mod recovery;
mod routes;
mod runtime;
pub mod transcript;

#[cfg(test)]
mod cursor_capture_tests;

#[cfg(test)]
mod codex_delivery_tests;
#[cfg(test)]
mod result_footer_tests;

#[cfg(test)]
mod cursor_delivery_tests;

#[cfg(test)]
mod delivery_contract_tests;

#[cfg(test)]
mod delivery_summary_tests;

#[cfg(test)]
mod side_panel_reply_tests;

pub(crate) use native::stable_prompt_id;
pub(crate) use persist::resolve_worker_name;
pub use recovery::on_native_pane_replaced;
pub use routes::{handle_request, handle_request_as};
pub(crate) use routes::answer_for_pane;
pub(crate) use routes::latest_handle_for_pane;
#[cfg(test)]
pub(crate) use routes::visible_runs;
pub(crate) use runtime::{convert_to_fallback, start_fallback_headless};
pub use runtime::{record_user_task, run_agent, supervise_existing, AgentRunRequest, AgentRunView};
pub(crate) use runtime::{cancel_agent, send_agent};
pub use capabilities::{read_only_supported, read_only_supported_for, read_only_unsupported_message};

/// Seams for `handoff::tests` into private modules of this one.
#[cfg(test)]
pub(crate) mod handoff_seams {
    pub fn receipt_usage_limit(snapshot: &crate::operations::OperationSnapshot) -> Option<super::runtime::ReceiptLimit> {
        super::runtime::receipt_usage_limit(snapshot)
    }

    /// What the turn watch decides when a pane that was seen working then reads idle on `after`, for a
    /// few ticks: `complete`, `ambiguous_idle`, `not_submitted`, `waiting_input`, `retry_submit` or `continue`.
    pub fn idle_turn_verdict(agent: &str, baseline: &str, task: &str, working: &str, after: &str) -> &'static str {
        use super::completion::{observe_tui_turn, TuiTurnDecision, TuiTurnWatch};
        let mut watch = TuiTurnWatch::from_baseline(baseline).with_task(task);
        observe_tui_turn(&mut watch, "running", working, false, agent);
        let mut last = TuiTurnDecision::Continue;
        for _ in 0..80 {
            last = observe_tui_turn(&mut watch, "idle", after, false, agent);
            if !matches!(last, TuiTurnDecision::Continue) {
                break;
            }
        }
        match last {
            TuiTurnDecision::Continue => "continue",
            TuiTurnDecision::WaitingInput => "waiting_input",
            TuiTurnDecision::Complete { .. } => "complete",
            TuiTurnDecision::AmbiguousIdle => "ambiguous_idle",
            TuiTurnDecision::RetrySubmit => "retry_submit",
            TuiTurnDecision::NotSubmitted => "not_submitted",
        }
    }

    pub fn compact_prompt(action: serde_json::Value) -> serde_json::Value {
        super::headless::compact_prompt_for_test(action)
    }
}

#[cfg(test)]
mod tests;
