//! Keep going: when an agent runs out of usage, the person can move the task to another agent.
//!
//! The limit card offers `Continue on <agent>`. One tap stops the agent in that pane (its
//! terminal stays), opens another agent in the same folder with a short fenced brief, and records
//! the hand-over. When the first agent's plan resets, a card can offer to move the task back.
//!
//! - `model`: the record, the settings, the target id (`agent` or `agent@account`) and the error codes.
//! - `store`: the private files under `<app data>/keep-going/` (records, settings, briefs).
//! - `brief` (pure): the brief text the new agent reads, with every agent or tool string fenced.
//! - `offers` (pure): which agents may take over, in what order.
//! - `chain` (pure): hop counts, limits from records, come-back due list.
//! - `live`: the hand-over itself (guards, brief, end the old run, stop the old agent, start the new one),
//!   the offer on the card, the come-back check and the status lines. Everything it touches sits behind
//!   `live_io::LiveIo`; `live_app` is the real app behind it and `live_gather` reads the old pane's work.
//! - `view` (pure): the records as the windows see them (no brief, no path, no goal).
//!
//! Invariants (SPEC n32 section 4; each one is tested where it is built):
//! - S1: agent text is only ever inside a fence, and the fence text says it is notes, not instructions.
//! - S2: the brief has no tool output, raw diff, scrollback or environment. Every string is redacted.
//! - S3: briefs stay in the private store. No bridge route, push, team or phone code reads them.
//! - S4: one writer per folder. The old run ends first, the old agent is stopped by PID, and nothing
//!   new starts unless the old agent exited.
//! - S5: Puppet Master never answers an agent's own menu, trust, permission or login prompt. This
//!   module has no write path into a pane.
//! - S6: only a person's answer on a limit card (desktop or a paired phone) or the desktop command
//!   can start a hand-over. No agent, companion, loopback or MCP caller.
//! - S7: no automatic hand-over. At most `HOP_CAP` hops per chain, and each hop needs a tap.
//! - S8: the same agent or the same billing company is never offered.
//! - S9: only the pane's own agent process is stopped, by PID, never by image name.
//! - S10: a hand-over never discards the copy the old run worked in.
//! - S11: file names come only from a validated record id.
//! - S12: records hold ids, labels and the local folder. No e-mail, no personal paths.
//! - S13: generic code never names an agent. Ranking uses `limits.vendor` and registry data only.

pub mod brief;
pub mod chain;
pub mod live;
mod live_app;
mod live_gather;
pub mod live_io;
pub mod model;
pub mod offers;
pub mod store;
pub mod view;

#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod store_tests;
