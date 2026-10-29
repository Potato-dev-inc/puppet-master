//! Which candidates are skipped, and why: cannot run, its own login out of usage, or sharing a plan
//! with a ready agent that is out of usage. The first rule that matches wins, in that order. Both
//! lists keep the input order. Pure: no I/O, no agent names.

use super::model::{Cand, Skip};

/// The candidates split into the ready ones and the skipped ones (with the reason).
#[derive(Debug, Clone, PartialEq)]
pub struct Split<'a> {
    pub ready: Vec<&'a Cand>,
    pub skipped: Vec<(&'a Cand, Skip)>,
}

/// A vendor counts only when it is a real name: not empty and not `varies`.
fn known(vendor: Option<&str>) -> Option<&str> {
    vendor.filter(|v| !v.is_empty() && *v != "varies")
}

/// Splits the candidates. A plan is blocked only by a READY candidate that is out of usage, so an
/// agent that cannot run never blocks the others of its vendor.
pub fn split(cands: &[Cand]) -> Split<'_> {
    // (vendor, label) of each ready candidate that is out of usage.
    let blockers: Vec<(&str, &str)> = cands
        .iter()
        .filter(|c| c.ready.is_ok() && c.limited_until.is_some())
        .filter_map(|c| known(c.vendor.as_deref()).map(|v| (v, c.label.as_str())))
        .collect();

    let mut out = Split { ready: Vec::new(), skipped: Vec::new() };
    for cand in cands {
        if let Err(reason) = &cand.ready {
            out.skipped.push((cand, Skip::NotReady(reason.clone())));
            continue;
        }
        if let Some(until) = cand.limited_until {
            out.skipped.push((cand, Skip::OutOfUsage(until)));
            continue;
        }
        let shared = known(cand.vendor.as_deref())
            .and_then(|vendor| blockers.iter().find(|(blocked_vendor, _)| *blocked_vendor == vendor));
        match shared {
            Some((_, with)) => out.skipped.push((cand, Skip::SharedPlan { with: with.to_string() })),
            None => out.ready.push(cand),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::keep_going::model::Target;

    /// A fixture candidate: `ready` is `Err(reason)` when it cannot run; `limited` is the limit.
    fn cand(agent: &str, vendor: Option<&str>, ready: Result<(), &str>, limited: Option<Option<u64>>) -> Cand {
        Cand {
            target: Target::parse(agent).unwrap(),
            label: agent.to_string(),
            vendor: vendor.map(str::to_string),
            ready: ready.map_err(str::to_string),
            limited_until: limited,
            evidence: None,
        }
    }

    fn ready_labels<'a>(split: &'a Split<'_>) -> Vec<&'a str> {
        split.ready.iter().map(|c| c.label.as_str()).collect()
    }

    fn skipped_labels<'a>(split: &'a Split<'_>) -> Vec<&'a str> {
        split.skipped.iter().map(|(c, _)| c.label.as_str()).collect()
    }

    #[test]
    fn not_ready_beats_a_limit() {
        let cands = [cand("fake_a", Some("vendor_x"), Err("Not installed on this computer"), Some(Some(5)))];
        let s = split(&cands);
        assert!(s.ready.is_empty());
        assert_eq!(s.skipped.len(), 1);
        assert_eq!(s.skipped[0].1, Skip::NotReady("Not installed on this computer".to_string()));
    }

    #[test]
    fn a_not_ready_limited_agent_does_not_block_its_vendor() {
        let cands = [
            cand("fake_a", Some("vendor_x"), Err("Not installed on this computer"), Some(Some(5))),
            cand("fake_b", Some("vendor_x"), Ok(()), None),
        ];
        let s = split(&cands);
        assert_eq!(ready_labels(&s), vec!["fake_b"]);
        assert_eq!(skipped_labels(&s), vec!["fake_a"]);
    }

    #[test]
    fn own_limit_with_a_time_is_out_of_usage_until_that_time() {
        let cands = [cand("fake_a", None, Ok(()), Some(Some(1_700_000_000_000)))];
        let s = split(&cands);
        assert!(s.ready.is_empty());
        assert_eq!(s.skipped[0].1, Skip::OutOfUsage(Some(1_700_000_000_000)));
    }

    #[test]
    fn own_limit_without_a_time_is_out_of_usage() {
        let cands = [cand("fake_a", None, Ok(()), Some(None))];
        let s = split(&cands);
        assert!(s.ready.is_empty());
        assert_eq!(s.skipped[0].1, Skip::OutOfUsage(None));
    }

    #[test]
    fn a_ready_agent_is_ready_when_nothing_blocks_it() {
        let cands = [cand("fake_a", Some("vendor_x"), Ok(()), None)];
        let s = split(&cands);
        assert_eq!(ready_labels(&s), vec!["fake_a"]);
        assert!(s.skipped.is_empty());
    }

    #[test]
    fn shared_plan_between_two_agents_of_one_vendor() {
        let cands = [
            cand("fake_a", Some("vendor_x"), Ok(()), Some(Some(1_700_000_000_000))),
            cand("fake_b", Some("vendor_x"), Ok(()), None),
        ];
        let s = split(&cands);
        assert!(s.ready.is_empty());
        assert_eq!(s.skipped[0].1, Skip::OutOfUsage(Some(1_700_000_000_000)));
        assert_eq!(s.skipped[1].1, Skip::SharedPlan { with: "fake_a".to_string() });
    }

    #[test]
    fn shared_plan_names_the_limited_agent_only_when_it_is_ready() {
        // The limited one is not READY, so it cannot block; fake_b stays ready.
        let cands = [
            cand("fake_a", Some("vendor_x"), Err("Approve this agent in Settings > Agents first."), Some(None)),
            cand("fake_b", Some("vendor_x"), Ok(()), None),
        ];
        let s = split(&cands);
        assert_eq!(ready_labels(&s), vec!["fake_b"]);
    }

    #[test]
    fn unknown_vendor_is_never_shared() {
        // No vendor, an empty vendor and "varies" never match, on either side.
        for unknown in [None, Some(""), Some("varies")] {
            let cands = [cand("fake_a", unknown, Ok(()), Some(None)), cand("fake_b", unknown, Ok(()), None)];
            let s = split(&cands);
            assert_eq!(ready_labels(&s), vec!["fake_b"], "vendor {unknown:?}");
            assert_eq!(skipped_labels(&s), vec!["fake_a"], "vendor {unknown:?}");
        }
    }

    #[test]
    fn two_vendors_are_independent() {
        let cands = [
            cand("fake_a", Some("vendor_x"), Ok(()), Some(None)),
            cand("fake_b", Some("vendor_y"), Ok(()), None),
            cand("fake_c", Some("vendor_x"), Ok(()), None),
        ];
        let s = split(&cands);
        assert_eq!(ready_labels(&s), vec!["fake_b"]);
        assert_eq!(s.skipped[1].1, Skip::SharedPlan { with: "fake_a".to_string() });
    }

    #[test]
    fn input_order_is_kept_in_both_lists() {
        let cands = [
            cand("fake_c", Some("vendor_x"), Ok(()), None),
            cand("fake_a", None, Ok(()), Some(None)),
            cand("fake_d", None, Ok(()), None),
            cand("fake_b", None, Err("Not installed on this computer"), None),
            cand("fake_e", Some("vendor_y"), Ok(()), None),
        ];
        let s = split(&cands);
        assert_eq!(ready_labels(&s), vec!["fake_c", "fake_d", "fake_e"]);
        assert_eq!(skipped_labels(&s), vec!["fake_a", "fake_b"]);
    }

    #[test]
    fn empty_input_gives_empty_lists() {
        let s = split(&[]);
        assert!(s.ready.is_empty());
        assert!(s.skipped.is_empty());
    }
}
