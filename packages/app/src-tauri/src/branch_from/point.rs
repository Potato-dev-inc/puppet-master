//! Resolving the save point a branch starts from (SPEC n33 section 3.4). Pure.
//!
//! Only the `Cut` type is here so far: the brief reads it. `resolve` and `checkpoint_id_ok` come next.

/// Which chat turns go into the brief. The indexes are positions in the chat items (`User` items).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    /// Before this turn: the turn that starts at this item is not in the copy.
    BeforeTurnAt(usize),
    /// After this turn: the turn that starts at this item is the last one in the copy.
    ThroughTurnAt(usize),
    /// Up to a time (unix ms), for a History row: a turn is in when it started at or before it, and an
    /// item of a turn is in when its time is at or before it.
    UpToMs(u64),
}
