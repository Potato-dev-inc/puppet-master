//! Clip It: a short video of one agent turn, made on this PC.
//!
//! - `model`: the `ClipPlan` JSON the webview draws (camelCase).
//! - `screens`: replays the pane's terminal record into frames.
//! - `scrub`: redacts every frame row before it leaves Rust.
//! - `window`: finds the turn's time window in the chat items.
//! - `steps`: tool steps and the request text, redacted.
//! - `plan`: builds the whole plan.
//! - `store`: plan tokens, staged video bytes and the one-time save.
//!
//! Nothing here types into a pane, answers a prompt, or sends data off the PC.

pub mod model;
pub mod plan;
pub mod screens;
pub mod scrub;
pub mod steps;
pub mod store;
pub mod window;

#[cfg(test)]
mod guard_tests;
