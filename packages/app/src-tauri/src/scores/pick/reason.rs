//! The sentence, the "Why this one" lines and the short labels, in simple English (the wording is
//! fixed by the spec's table, so every string is built from templates, never from a path or an id).
//! Scaffold placeholders for now.

use super::model::{Basis, Cand, Metric, Skip};

/// The text of a skipped candidate. Scaffold placeholder.
pub fn skip_text(_skip: &Skip, _when: &dyn Fn(u64) -> String) -> String {
    String::new()
}

/// One line of "Why this one" for a candidate. Scaffold placeholder.
pub fn line_of(_cand: &Cand, _metric: Option<Metric>) -> String {
    String::new()
}

/// A label cut to the label limit with "...". Scaffold placeholder.
pub fn short(_label: &str) -> String {
    String::new()
}

/// The one sentence of the answer. Scaffold placeholder: only the basis is taken for now.
pub fn sentence(_basis: Basis, _when: &dyn Fn(u64) -> String) -> String {
    String::new()
}
