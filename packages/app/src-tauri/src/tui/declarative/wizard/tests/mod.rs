//! Wizard tests. No tokens, no network: the "agent" is a tiny fake CLI compiled from
//! `tests/fake-cli/fake_cli.rs` (see `support`).

pub(crate) mod support;

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
