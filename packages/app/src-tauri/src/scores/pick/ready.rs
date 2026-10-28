//! Which candidates are skipped, and why: cannot run, its own login out of usage, or sharing a plan
//! with a ready agent that is out of usage. Scaffold: the split returns empty lists for now.

use super::model::{Cand, Skip};

/// Splits the candidates into `(ready, skipped)`, both in input order. Scaffold placeholder.
pub fn split(_cands: &[Cand]) -> (Vec<&Cand>, Vec<(&Cand, Skip)>) {
    (Vec::new(), Vec::new())
}
