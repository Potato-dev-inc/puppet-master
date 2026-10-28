//! Look-alike names against the popular lists (SPEC-n33 section 3.3). Phase 1 stub: never matches.
//! The rules are filled in by PC-4.

use super::model::{Ecosystem, Lookalike};

pub fn check(_eco: Ecosystem, _norm: &str) -> Option<Lookalike> {
    None
}

pub fn check_in(_list: &[String], _eco: Ecosystem, _norm: &str) -> Option<Lookalike> {
    None
}
