//! The decision from the person's own numbers: the first measure with two candidates that have
//! enough runs decides, with a strict 90% gap; otherwise the usual agent. Scaffold placeholders.

use super::model::{Cand, Decision, Usual, UsualKind};

/// The decision among the ready candidates, or `None` when there are none. Scaffold placeholder.
pub fn decide(_ready: &[&Cand], _usual: &Usual) -> Option<Decision> {
    None
}

/// The usual agent among the ready ones: its index and how it was found. Scaffold placeholder.
pub fn usual_of(_ready: &[&Cand], _usual: &Usual) -> (usize, UsualKind) {
    (0, UsualKind::First)
}
