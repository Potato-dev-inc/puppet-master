//! Tests of the pick module. This file holds the scaffold check of the fixed constants; the guards
//! (privacy, reachability, end to end) are added by a later task.

use super::model::{Metric, MAX_CONSIDERED, MAX_LABEL_CHARS, METRIC_ORDER, PICK_DAYS};

#[test]
fn scaffold_constants_match_the_spec() {
    assert_eq!(PICK_DAYS, 90);
    assert_eq!(MAX_CONSIDERED, 24);
    assert_eq!(MAX_LABEL_CHARS, 40);
    assert_eq!(METRIC_ORDER, [Metric::ChecksGreen, Metric::Merged, Metric::EndedOk]);
}
