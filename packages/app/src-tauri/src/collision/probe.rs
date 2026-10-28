//! Collision Alert: the git reads behind the engine, hardened. Stub from CA-1; the real code comes with CA-11.
//!
//! `ProbeError` is defined here already (SPEC-n33 3.7) because `compare` returns it. CA-11 adds the
//! `Probe` trait and `GitProbe` to this file.

/// Why a git read failed. Internal only; never shown to the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// The folder is not a git checkout.
    NotGit,
    /// The start commit is gone (rewritten or pruned).
    StartLost,
    /// A git process went over `PROBE_TIMEOUT_MS`.
    TooSlow,
    /// Anything else, with a short internal reason.
    Failed(String),
}
