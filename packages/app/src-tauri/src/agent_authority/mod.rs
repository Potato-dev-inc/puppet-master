//! Who may control what: the two agent-authority settings, the consent flow for an agent that wants
//! to control a pane it did not start, and the gate on an agent merging a separate copy.
//!
//! # The two switches
//!
//! The shipped defaults are the safe side. Flipping them is the two constants below, nothing else:
//!
//! | setting | default |
//! |---|---|
//! | A  "Ask me before an agent controls a pane it didn't start" | **on**  |
//! | B  "Let agents merge separate copies"                       | **off** |
//!
//! A saved choice in `<app data>/agent_authority.json` always wins over these defaults, so flipping
//! a constant changes only people who never touched the setting (a file missing a key takes the
//! default for that key).
//!
//! # How it fits together
//!
//! * `mcp_sessions::lineage` knows who started each pane (`PaneOrigin`) and which connections are
//!   helpers of which.
//! * `gate` is called at the two places an agent starts controlling something it did not create
//!   (`take_over` in `bridge/handoff.rs`, adopting a pane in `agent_runs/headless/bind.rs`) and when
//!   an agent answers a prompt (`agent_runs/routes/answer.rs`). With A off it does nothing.
//! * `consent` raises a card through the existing questions store (`questions::ConsentInfo`), so the
//!   Inbox, the phone's question list and the "Needs you" count show it with no new plumbing. The
//!   agent's call fails with `CONSENT_PENDING` until a person answers, then `CONSENT_DENIED` or goes
//!   through. A card is never answerable from a lock screen (a question push has no buttons) and not
//!   over plain loopback HTTP (`questions::routes`).
//! * `merge` is the B gate, used by `review_worktree(action=merge)`. A person merging in the
//!   cockpit goes through a Tauri command and is never gated.
//! * Every decision is written to the `audit` log.

pub mod config;
pub mod consent;
pub mod gate;
pub mod merge;
#[cfg(test)]
mod tests;

/// Setting A. Default: on.
pub const DEFAULT_ASK_BEFORE_AGENT_CONTROL: bool = true;
/// Setting B. Default: off.
pub const DEFAULT_AGENTS_MAY_MERGE: bool = false;
