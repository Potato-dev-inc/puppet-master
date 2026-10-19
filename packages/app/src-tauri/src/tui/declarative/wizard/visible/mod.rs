//! The visible (TUI) path of the Agent Wizard (SPEC section 11): a guided capture of the program in a
//! real pane, the screens as fixtures, and the live test of the drafted profile in a new pane.
//!
//! `screen` reads screens (kind, stability), `host` is the pane abstraction (the real registry, or a
//! script in tests), `capture` is the step state machine (pure), `runner` drives it on a thread,
//! `api` holds the `capture` operations, `fixtures` writes the screens and their expectations, and
//! `livetest` runs the checks against a pane started from the draft.

pub mod ai_menu;
pub mod answer;
pub mod api;
pub mod capture;
pub mod fixtures;
pub mod host;
pub mod livetest;
pub mod menu;
pub mod registry_host;
pub mod runner;
pub mod verdict;
pub mod screen;
pub mod styled;
