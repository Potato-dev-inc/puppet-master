//! The decision from the person's own numbers: the first measure with two candidates that have
//! enough runs decides, with the same 90% gap as Scores (`stats::ahead`); otherwise the usual agent.
//! Pure functions only. The loop stops at the first measure that has two candidates, so no other
//! measure is tried to find a favourable one.

use super::model::{Basis, Cand, Decision, Usual, UsualKind, METRIC_ORDER};
use crate::scores::model::{Rate, MIN_SAMPLE};
use crate::scores::stats::ahead;

/// The decision among the ready candidates, or `None` when there are none. `ready` is the ready
/// list of `ready::split`, in input order.
pub fn decide(ready: &[&Cand], usual: &Usual) -> Option<Decision> {
    match ready.len() {
        0 => return None,
        1 => {
            return Some(Decision {
                index: 0,
                basis: Basis::OnlyOne,
                metric: None,
                rival: None,
                usual: None,
            })
        }
        _ => {}
    }
    for metric in METRIC_ORDER {
        // Candidates with enough runs for this measure, as (index in `ready`, rate).
        let have: Vec<(usize, Rate)> = ready
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.evidence.map(|e| (i, e.rate(metric))))
            .filter(|(_, r)| r.total >= MIN_SAMPLE)
            .collect();
        if have.len() < 2 {
            continue;
        }
        // The first measure with two candidates decides, and the loop stops here.
        return Some(match ahead(&have, false).as_slice() {
            [leader] => Decision {
                index: *leader,
                basis: Basis::Clear,
                metric: Some(metric),
                rival: rival_of(&have, *leader),
                usual: None,
            },
            _ => {
                let (index, kind) = usual_of(ready, usual);
                Decision {
                    index,
                    basis: Basis::CloseCall,
                    metric: Some(metric),
                    rival: None,
                    usual: Some(kind),
                }
            }
        });
    }
    let (index, kind) = usual_of(ready, usual);
    Some(Decision {
        index,
        basis: Basis::TooEarly,
        metric: None,
        rival: None,
        usual: Some(kind),
    })
}

/// The usual agent among the ready ones: the preferred id, else the last one used, else the first
/// ready one. Ids match ignoring case. `ready` must not be empty (the caller checks first); with
/// no ready candidate the index is 0 and is never read.
pub fn usual_of(ready: &[&Cand], usual: &Usual) -> (usize, UsualKind) {
    let find = |id: &Option<String>| -> Option<usize> {
        let id = id.as_deref()?;
        ready
            .iter()
            .position(|c| c.target.agent_id.eq_ignore_ascii_case(id))
    };
    if let Some(i) = find(&usual.preferred) {
        return (i, UsualKind::Chosen);
    }
    if let Some(i) = find(&usual.last) {
        return (i, UsualKind::Last);
    }
    (0, UsualKind::First)
}

/// The other candidate with the highest low bound (the nearest rival). A tie goes to the first.
fn rival_of(have: &[(usize, Rate)], leader: usize) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (index, rate) in have {
        if *index == leader {
            continue;
        }
        match best {
            Some((_, low)) if low >= rate.low => {}
            _ => best = Some((*index, rate.low)),
        }
    }
    best.map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::super::model::{Evidence, Metric};
    use super::*;
    use crate::handoff::keep_going::model::Target;
    use crate::scores::model::Sample;
    use crate::scores::stats::rate;

    /// A candidate with the given evidence: `(count, total)` for checks, merged and ended OK.
    fn cand(id: &str, ev: Option<((u32, u32), (u32, u32), (u32, u32))>) -> Cand {
        Cand {
            target: Target {
                agent_id: id.into(),
                account: None,
            },
            label: id.into(),
            vendor: None,
            ready: Ok(()),
            limited_until: None,
            evidence: ev.map(|(checks, merged, ended)| Evidence {
                turns: checks.1,
                checks_green: rate(checks.0, checks.1),
                merged: rate(merged.0, merged.1),
                ended_ok: rate(ended.0, ended.1),
            }),
        }
    }

    /// The same `(count, total)` on all three measures.
    fn same(id: &str, count: u32, total: u32) -> Cand {
        cand(id, Some(((count, total), (count, total), (count, total))))
    }

    fn usual(preferred: Option<&str>, last: Option<&str>) -> Usual {
        Usual {
            preferred: preferred.map(String::from),
            last: last.map(String::from),
        }
    }

    fn refs(v: &[Cand]) -> Vec<&Cand> {
        v.iter().collect()
    }

    #[test]
    fn no_ready_candidate_gives_none() {
        assert_eq!(decide(&[], &usual(None, None)), None);
    }

    #[test]
    fn one_ready_candidate_is_the_only_one() {
        let cands = [same("fake_a", 0, 0)];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        assert_eq!(
            d,
            Decision {
                index: 0,
                basis: Basis::OnlyOne,
                metric: None,
                rival: None,
                usual: None
            }
        );
    }

    #[test]
    fn four_against_four_on_every_measure_is_too_early_and_uses_the_usual_one() {
        let cands = [same("fake_a", 4, 4), same("fake_b", 0, 4)];
        let d = decide(&refs(&cands), &usual(Some("fake_b"), None)).unwrap();
        assert_eq!(d.basis, Basis::TooEarly);
        assert_eq!((d.index, d.usual), (1, Some(UsualKind::Chosen)));
        assert_eq!((d.metric, d.rival), (None, None));
    }

    #[test]
    fn one_agent_with_many_runs_and_no_rival_is_too_early() {
        let cands = [same("fake_a", 20, 20), cand("fake_b", None)];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        assert_eq!(d.basis, Basis::TooEarly);
        assert_eq!(d.index, 0);
    }

    #[test]
    fn five_against_five_with_five_and_none_is_clear() {
        let cands = [same("fake_a", 5, 5), same("fake_b", 0, 5)];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        assert_eq!(d.basis, Basis::Clear);
        assert_eq!(
            (d.index, d.metric, d.rival),
            (0, Some(Metric::ChecksGreen), Some(1))
        );
    }

    #[test]
    fn ten_of_ten_against_five_of_ten_is_clear_with_a_rival() {
        let cands = [same("fake_a", 10, 10), same("fake_b", 5, 10)];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        assert_eq!(d.basis, Basis::Clear);
        assert_eq!((d.index, d.rival, d.usual), (0, Some(1), None));
    }

    #[test]
    fn a_clear_leader_wins_even_when_it_is_not_the_usual_one() {
        let cands = [same("fake_a", 10, 10), same("fake_b", 5, 10)];
        let d = decide(&refs(&cands), &usual(Some("fake_b"), Some("fake_b"))).unwrap();
        assert_eq!((d.basis, d.index), (Basis::Clear, 0));
    }

    #[test]
    fn nine_of_ten_against_eight_of_ten_is_a_close_call_and_uses_the_usual_one() {
        let cands = [same("fake_a", 9, 10), same("fake_b", 8, 10)];
        let d = decide(&refs(&cands), &usual(None, Some("fake_b"))).unwrap();
        assert_eq!(d.basis, Basis::CloseCall);
        assert_eq!(
            (d.index, d.metric, d.rival),
            (1, Some(Metric::ChecksGreen), None)
        );
        assert_eq!(d.usual, Some(UsualKind::Last));
    }

    #[test]
    fn checks_decide_before_merged_and_a_merged_gap_is_not_used() {
        // Checks are a close call (9/10 vs 8/10). Merged would be a clear gap (10/10 vs 0/10),
        // but the first measure with two candidates decides, so this is a close call on checks.
        let cands = [
            cand("fake_a", Some(((9, 10), (10, 10), (5, 5)))),
            cand("fake_b", Some(((8, 10), (0, 10), (5, 5)))),
        ];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        assert_eq!(d.basis, Basis::CloseCall);
        assert_eq!(d.metric, Some(Metric::ChecksGreen));
    }

    #[test]
    fn a_clear_ended_ok_gap_is_ignored_when_checks_are_a_close_call() {
        let cands = [
            cand("fake_a", Some(((9, 10), (5, 10), (10, 10)))),
            cand("fake_b", Some(((8, 10), (5, 10), (0, 10)))),
        ];
        let d = decide(&refs(&cands), &usual(Some("fake_b"), None)).unwrap();
        assert_eq!(
            (d.basis, d.metric, d.index),
            (Basis::CloseCall, Some(Metric::ChecksGreen), 1)
        );
    }

    #[test]
    fn a_measure_with_one_candidate_is_skipped_and_the_next_decides() {
        // Checks: only fake_a has 5+ runs (fake_b has 4). Merged: both have 5+ runs and a clear gap.
        let cands = [
            cand("fake_a", Some(((10, 10), (5, 5), (0, 0)))),
            cand("fake_b", Some(((4, 4), (0, 5), (0, 0)))),
        ];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        assert_eq!(
            (d.basis, d.metric, d.index),
            (Basis::Clear, Some(Metric::Merged), 0)
        );
    }

    #[test]
    fn ended_ok_decides_only_when_the_two_better_measures_have_no_pair() {
        let cands = [
            cand("fake_a", Some(((0, 0), (0, 0), (10, 10)))),
            cand("fake_b", Some(((0, 0), (0, 0), (5, 10)))),
        ];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        assert_eq!(
            (d.basis, d.metric, d.index),
            (Basis::Clear, Some(Metric::EndedOk), 0)
        );
    }

    #[test]
    fn three_candidates_leader_clear_against_both() {
        let cands = [
            same("fake_a", 10, 10),
            same("fake_b", 5, 10),
            same("fake_c", 5, 10),
        ];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        assert_eq!((d.basis, d.index, d.rival), (Basis::Clear, 0, Some(1)));
    }

    #[test]
    fn three_candidates_leader_not_clear_against_one_is_a_close_call() {
        let cands = [
            same("fake_a", 10, 10),
            same("fake_b", 9, 10),
            same("fake_c", 2, 10),
        ];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        assert_eq!(d.basis, Basis::CloseCall);
    }

    #[test]
    fn early_leader_gives_an_early_rate_on_the_deciding_metric() {
        let cands = [same("fake_a", 5, 5), same("fake_b", 0, 5)];
        let d = decide(&refs(&cands), &usual(None, None)).unwrap();
        let leader = refs(&cands)[d.index].evidence.unwrap();
        assert_eq!(leader.rate(d.metric.unwrap()).sample, Sample::Early);
    }

    #[test]
    fn usual_is_preferred_then_last_then_first() {
        let cands = [same("fake_a", 0, 4), same("fake_b", 0, 4)];
        let r = refs(&cands);
        assert_eq!(
            usual_of(&r, &usual(Some("FAKE_B"), Some("fake_a"))),
            (1, UsualKind::Chosen)
        );
        assert_eq!(
            usual_of(&r, &usual(None, Some("fake_b"))),
            (1, UsualKind::Last)
        );
        assert_eq!(usual_of(&r, &usual(None, None)), (0, UsualKind::First));
    }

    #[test]
    fn a_preferred_agent_that_is_not_ready_falls_to_the_next_rule() {
        let cands = [same("fake_a", 0, 4), same("fake_b", 0, 4)];
        let r = refs(&cands);
        assert_eq!(
            usual_of(&r, &usual(Some("fake_c"), Some("fake_b"))),
            (1, UsualKind::Last)
        );
        assert_eq!(
            usual_of(&r, &usual(Some("fake_c"), None)),
            (0, UsualKind::First)
        );
    }

    #[test]
    fn too_early_uses_the_preferred_agent_found_ignoring_case() {
        let cands = [
            same("fake_a", 4, 4),
            same("fake_b", 4, 4),
            same("fake_c", 0, 0),
        ];
        let d = decide(&refs(&cands), &usual(Some("Fake_C"), None)).unwrap();
        assert_eq!(d.basis, Basis::TooEarly);
        assert_eq!((d.index, d.usual), (2, Some(UsualKind::Chosen)));
    }
}
