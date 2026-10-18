//! The Agent Wizard back end (docs/specs/agent-wizard/SPEC.md): turns "the command I type to start an
//! agent" into an UNAPPROVED definition plus fixtures. Everything here is deterministic. The AI that
//! writes the draft lives in the front end (`lib/agentWizard.ts`); Rust finds the program, runs a fixed
//! set of probes, checks drafts, runs the live test and writes files.
//!
//! One file per step: `locate` (Resolve), `probe`, `verify`, `livetest`, `finish`; `session` holds the
//! in-memory session store and the state machine, `exec` runs a child with a timeout and an output cap,
//! `fixtures` builds the fixture files, `hints` the "how to fix it" advice, `report` the error format an
//! AI can act on, and `api` the JSON entry points shared by the bridge route and the Tauri commands.
//!
//! Safety (SPEC section 3): a session can only be started by the local desktop app (the route layer
//! refuses MCP sessions and remote devices); probes run fixed argument lists only; every run uses a temp
//! work folder, never the person's project; the written definition is never approved here.

pub mod agent_error;
pub mod api;
pub mod ask;
pub mod exec;
pub mod features;
pub mod finish;
pub mod fixtures;
pub mod hints;
pub mod livetest;
pub mod locate;
pub mod probe;
pub mod report;
pub mod session;
pub mod verify;
pub mod visible;

#[cfg(test)]
pub(crate) mod tests;

pub use report::WizardError;
pub use session::{State, Wizard, WizardEnv};
