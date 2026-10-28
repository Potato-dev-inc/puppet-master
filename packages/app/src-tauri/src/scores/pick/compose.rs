//! Puts the steps together into one `Pick`: skips, the decision, the blocked mention and the
//! sentence. Takes no folder, pane id or task text; the labels are trusted registry data.
//! Scaffold placeholder: returns an empty answer that says nothing is ready.

use super::model::{Basis, Cand, Pick, Usual};
use crate::scores::model::Sample;

/// The answer for the candidates. Scaffold placeholder.
pub fn pick(_cands: &[Cand], _usual: &Usual, _when: &dyn Fn(u64) -> String) -> Pick {
    Pick {
        choice: None,
        reason: String::new(),
        basis: Basis::NoneReady,
        metric: None,
        sample: Sample::None,
        considered: Vec::new(),
        notes: Vec::new(),
    }
}
