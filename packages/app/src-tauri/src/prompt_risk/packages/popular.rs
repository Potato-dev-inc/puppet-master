//! Offline top lists of package names (SPEC-n33 section 3.3). Phase 1 stub: every list is empty.
//! The list files arrive with PC-2 and are read by PC-4.

use super::model::Ecosystem;

/// Names in rank order (most used first). Empty until PC-4.
pub fn list(_eco: Ecosystem) -> &'static [String] {
    &[]
}

/// Position of a normalized name in the list, 0 = most used.
pub fn rank(_eco: Ecosystem, _norm: &str) -> Option<u32> {
    None
}

pub fn size(_eco: Ecosystem) -> usize {
    0
}
