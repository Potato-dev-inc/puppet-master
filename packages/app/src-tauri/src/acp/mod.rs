//! The ACP lane (A1, the core): drive an agent through the Agent Client Protocol instead of through its
//! terminal screen. No UI and no chat items here; those come in A2 and later. Behind the Labs switch
//! `acp_chat_enabled` (default off), and the terminal stays a full way to run every agent
//! (`lane::RunLane`).
//!
//! ```text
//!   AcpSession (session.rs)   state machine + snapshot, parked permission questions, turns, restore
//!     |  Supervisor (supervisor.rs)   sweep: turn clocks, idle TTL, live cap
//!     v
//!   AcpConnection (conn.rs)   process + JSON-RPC over stdio, 5 threads, bounded queues
//!     |-- Containment (job.rs)         Windows Job Object, KILL_ON_JOB_CLOSE (tree kill elsewhere)
//!     |-- WriteQueue (write_queue.rs)  normal + priority lane to stdin, stall watchdog
//!     `-- wire.rs, ring.rs             framing, id spaces, capped stderr ring
//! ```
//!
//! # Why our own transport and our own small types (no ACP crate)
//!
//! Both official crates were looked at and neither is used.
//!
//! * `agent-client-protocol` (the SDK) is a cooperative, `futures`-based connection builder that owns the
//!   read loop, dispatches typed handlers and answers the agent's requests itself. This lane needs the
//!   opposite on five points: a priority write path for cancel and permission replies; a stdin write
//!   timeout that kills the process; a bounded, ordered queue with backpressure onto the child's pipe;
//!   permission requests parked by us with no timeout; and process containment (the Job Object) tied to
//!   the child. Those are easier in a small transport of our own than in a hook around someone else's
//!   driver, and the rest of the app is threads and `parking_lot`, not `futures`. It also pins an exact
//!   schema version.
//! * `agent-client-protocol-schema` (types only) was tried and removed: it requires `serde_json`'s
//!   `preserve_order` feature, and Cargo unifies features across the whole app, so every JSON object in
//!   Puppet Master would keep insertion order instead of sorted order. Seven existing tests failed at
//!   once (golden files, the pinned tool-catalog version), and any signed or hashed JSON would have
//!   changed silently. `types.rs` has the dozen message types this lane reads, tolerant of unknown fields,
//!   checked against real adapter traffic in `fixtures/`; everything else in a message stays a raw
//!   `serde_json::Value` for A2's chat mapper.
//!
//! # Rules this module keeps (tests in each file)
//!
//! * Never auto-approve. `session/request_permission` is parked until `answer_permission` or `cancel`.
//! * No window: every process starts through `proc_util::hidden_command`.
//! * No process outlives its session or the app (Job Object), and none is ever killed by image name.
//! * Agent specifics are data (`config.rs`): this code never matches an agent id or name.
//! * Every queue is bounded; every wait has a limit written down in `config::Timeouts`.
//! * Only protocol v1. We offer no file or terminal capability; the agent does its own file work.
//!
//! Parts of the design (queue sizes, the two timer windows, idle TTL, respawn with `session/load`, a
//! fail-closed permission relay) follow ideas studied in `D:\pm-research\NUPHOS-ACP-STUDY.md`. No Nuphos
//! code is copied here.

// The chat (A2) is `chat/`; the callers that resume sessions, switch to the terminal and show config chips land in A3 to A5.
#![allow(dead_code)]

pub mod chat;
pub mod config;
pub mod conn;
pub mod error;
pub mod handshake;
pub mod job;
pub mod lane;
pub mod pin;
pub mod ring;
pub mod session;
pub mod state;
pub mod supervisor;
pub mod timers;
pub mod types;
pub mod wire;
pub mod write_queue;

#[cfg(test)]
mod fake_agent;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

/// The Labs switch in the public settings. Off: no ACP session starts.
pub const LABS_KEY: &str = "acp_chat_enabled";

static ENABLED: AtomicBool = AtomicBool::new(false);

/// The Labs switch in a settings value: on only when it is exactly `true`.
pub fn labs_on(public: &Value) -> bool {
    public.get(LABS_KEY).and_then(Value::as_bool).unwrap_or(false)
}

/// Called with the public settings whenever they change (and once at startup from the saved file). Turning
/// the switch off closes every live ACP session: the switch is also the kill switch.
pub fn apply_settings(public: &Value) {
    let on = labs_on(public);
    let was = ENABLED.swap(on, Ordering::SeqCst);
    if was && !on {
        chat::close_all();
        supervisor::global().close_all();
    }
}

/// Whether the Labs switch is on.
pub fn chat_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod labs_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_switch_is_off_unless_it_is_exactly_true() {
        assert!(!labs_on(&json!({})));
        assert!(!labs_on(&json!({"acp_chat_enabled": "yes"})));
        assert!(!labs_on(&json!({"acp_chat_enabled": null})));
        assert!(labs_on(&json!({"acp_chat_enabled": true})));
    }
}
