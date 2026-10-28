//! Night Shift: a queue of tasks that run while the person sleeps (SPEC n32, section 3).
//!
//! Invariants (section 4): Night Shift never answers an agent's gate, never merges, and never
//! deletes a copy. Its only way to keep the computer on is `power`, which is released when the run
//! ends, is stopped, or the app exits.

#[allow(dead_code)] // the runner calls these from its own task (NS-5 and later); nothing else yet
pub mod power;
