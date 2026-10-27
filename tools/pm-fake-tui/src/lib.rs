//! `pm-fake-tui`: a scriptable fake agent TUI for Puppet Master's end-to-end tests.
//!
//! It runs on a real console (ConPTY when a pane spawns it), draws the screens of Claude Code or
//! Codex as captured live, and plays a script: on this prompt show that, wait, ask for approval,
//! on this key do that, write a file, hit the usage limit, exit, crash. It spends no tokens and is
//! deterministic. It is a development tool: nothing in the shipped app refers to it.

pub mod engine;
pub mod keys;
pub mod render;
pub mod runtime;
pub mod script;
pub mod term;
pub mod transcript;
