//! Review Desk (SPEC-n33): one calm list of finished agent work, riskiest first.
//!
//! This part has the data model the risk table reads (`model`) and the fixed risk table (`risk`).
//! Nothing calls it yet; the sources, actions and UI come in later tasks, so the items are unused for now.
#![allow(dead_code)]

pub mod model;
pub mod risk;
