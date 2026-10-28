//! The usual saved login of an agent in this project, and the evidence of one Scores row. Only
//! the numbers are copied: no other field of the row is read. Scaffold placeholders for now.

use super::model::Evidence;
use crate::scores::model::ScoreRow;
use crate::scores::stats::rate;

/// The usual login from `(account_id, turns)` rows (`None` = the agent's own login). Scaffold
/// placeholder: no login is chosen yet.
pub fn usual_login(_rows: &[(Option<String>, u32)], _valid: &dyn Fn(&str) -> bool) -> Option<String> {
    None
}

/// The evidence of one Scores row. Scaffold placeholder: an empty evidence.
pub fn evidence_of(_row: &ScoreRow) -> Evidence {
    Evidence { turns: 0, checks_green: rate(0, 0), merged: rate(0, 0), ended_ok: rate(0, 0) }
}
