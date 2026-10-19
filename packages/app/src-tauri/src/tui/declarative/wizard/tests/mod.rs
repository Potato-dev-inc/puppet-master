//! Wizard tests. No tokens, no network: the "agent" is a tiny fake CLI compiled from
//! `tests/fake-cli/fake_cli.rs` (see `support`).

pub(crate) mod fake_host;
pub(crate) mod support;

mod agent_error_tests;
mod email_redaction_tests;
mod auto_tests;
mod auto_verdict_tests;
mod ask_tests;
mod benchmark;
mod e2e;
mod exec_tests;
mod finish_tests;
mod livetest_tests;
mod probe_tests;
mod report_tests;
mod state_tests;
mod verify_tests;
mod visible_answer_tests;
mod visible_autoanswer_tests;
mod visible_api_tests;
mod visible_capture_tests;
mod visible_finish_tests;
mod visible_livetest_tests;
mod visible_menu_shape_tests;
mod visible_screen_agent_tests;
mod visible_screen_tests;
mod visible_verify_tests;
mod visible_work_capture_tests;
mod visible_work_tests;
