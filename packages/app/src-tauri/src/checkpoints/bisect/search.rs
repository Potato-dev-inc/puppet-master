//! The search state machine that picks the next save point to run (What broke it, SPEC n33 section 3.3).
//!
//! It never runs anything. The caller asks `next`, runs the declared command at that point, and gives the
//! answer back with `record`. Points are numbered `0..n`; the last one is "now". Pure: no I/O, no clock,
//! no agent names.
//!
//! Order of probes: (1) "now" must fail, else the tests pass; (2) the picked good point must pass, else
//! walk back from "now" with offsets 1, 2, 4, ... until a point passes; (3) halve `(good, bad)` until they
//! are neighbours; (4) run `good` once more, a different answer is `Unsure(Flaky)`. Any `Unsure` answer
//! stops the search with that reason.

use super::model::{UnsureWhy, Verdict};

/// Why a point is asked next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    BadEnd,
    PickedGood,
    Gallop,
    Halve,
    RecheckGood,
}

/// The next step: a point to run, or the end of the search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Probe { idx: usize, why: Why },
    Done(Done),
}

/// How the search ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// `bad == good + 1`, both measured, and `good` passed again.
    Culprit { good: usize, bad: usize },
    /// "Now" passes: nothing to find.
    TestsPass,
    /// Point 0 fails too: the break is older than every point we can test.
    AlwaysRed { oldest: usize },
    /// The picked good point fails.
    GoodWasRed { idx: usize },
    /// A run gave no answer, or the good point gave a different answer when run again.
    Unsure { why: UnsureWhy, good: Option<usize>, bad: usize },
    /// Fewer than two points: nothing to compare.
    TooFew,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Ask "now". It must fail.
    Now,
    /// Ask the picked good point. It must pass.
    Picked(usize),
    /// Ask `last - step`. A pass ends the walk back; a fail doubles the step.
    Gallop { step: usize },
    /// Ask the middle of `(good, bad)`.
    Halve { good: usize },
    /// Ask `good` once more.
    Recheck { good: usize },
    Over(Done),
}

/// The search over `n` save points. `known` holds the measured answers of all but the re-run.
#[derive(Debug, Clone)]
pub struct Search {
    n: usize,
    picked: Option<usize>,
    known: Vec<Option<Verdict>>,
    phase: Phase,
}

impl Search {
    /// A search over `n` points. A picked good point that is not older than "now" is ignored.
    pub fn new(n: usize, picked_good: Option<usize>) -> Search {
        let picked = picked_good.filter(|&g| g < n.saturating_sub(1));
        let phase = if n < 2 { Phase::Over(Done::TooFew) } else { Phase::Now };
        Search { n, picked, known: vec![None; n], phase }
    }

    /// The point to run next, or how the search ended.
    pub fn next(&self) -> Next {
        match &self.phase {
            Phase::Now => Next::Probe { idx: self.last(), why: Why::BadEnd },
            Phase::Picked(g) => Next::Probe { idx: *g, why: Why::PickedGood },
            Phase::Gallop { step } => Next::Probe { idx: self.last().saturating_sub(*step), why: Why::Gallop },
            Phase::Halve { good } => Next::Probe { idx: (good + self.range().1) / 2, why: Why::Halve },
            Phase::Recheck { good } => Next::Probe { idx: *good, why: Why::RecheckGood },
            Phase::Over(done) => Next::Done(done.clone()),
        }
    }

    /// The answer of one run. Only the answer for the point `next` asked for counts; any other is ignored.
    pub fn record(&mut self, idx: usize, verdict: Verdict) {
        let asked = match self.next() {
            Next::Probe { idx, .. } => idx,
            Next::Done(_) => return,
        };
        if idx != asked {
            return;
        }
        if let Verdict::Unsure { why } = verdict {
            let (good, bad) = self.range();
            self.phase = Phase::Over(Done::Unsure { why, good, bad });
            return;
        }
        let pass = verdict == Verdict::Pass;
        self.phase = match self.phase.clone() {
            Phase::Now => {
                self.known[idx] = Some(verdict);
                if pass {
                    Phase::Over(Done::TestsPass)
                } else {
                    match self.picked {
                        Some(g) => Phase::Picked(g),
                        None => Phase::Gallop { step: 1 },
                    }
                }
            }
            Phase::Picked(g) => {
                self.known[idx] = Some(verdict);
                if pass {
                    self.halve_or_recheck(g)
                } else {
                    Phase::Over(Done::GoodWasRed { idx: g })
                }
            }
            Phase::Gallop { step } => {
                self.known[idx] = Some(verdict);
                if pass {
                    self.halve_or_recheck(idx)
                } else if idx == 0 {
                    Phase::Over(Done::AlwaysRed { oldest: 0 })
                } else {
                    Phase::Gallop { step: step * 2 }
                }
            }
            Phase::Halve { good } => {
                self.known[idx] = Some(verdict);
                self.halve_or_recheck(if pass { idx } else { good })
            }
            Phase::Recheck { good } => {
                let (_, bad) = self.range();
                if pass {
                    Phase::Over(Done::Culprit { good, bad })
                } else {
                    Phase::Over(Done::Unsure { why: UnsureWhy::Flaky, good: Some(good), bad })
                }
            }
            over @ Phase::Over(_) => over,
        };
    }

    /// The best known pair: `good` is the highest point below `bad` that passed, `bad` the lowest point
    /// that failed (the last point while none has failed). Used for "narrowed" in the words.
    pub fn range(&self) -> (Option<usize>, usize) {
        let bad = self.known.iter().position(|v| *v == Some(Verdict::Fail)).unwrap_or(self.last());
        let good = (0..bad).rev().find(|&i| self.known[i] == Some(Verdict::Pass));
        (good, bad)
    }

    fn last(&self) -> usize {
        self.n.saturating_sub(1)
    }

    /// Halve while there is a point between `good` and `bad`; otherwise re-run `good`.
    fn halve_or_recheck(&self, good: usize) -> Phase {
        let (_, bad) = self.range();
        if bad <= good + 1 {
            Phase::Recheck { good }
        } else {
            Phase::Halve { good }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the search with a fake test function until it is done. Returns the end and every probe asked.
    fn drive(mut s: Search, mut run: impl FnMut(usize) -> Verdict) -> (Done, Vec<(usize, Why)>) {
        let mut probes = Vec::new();
        for _ in 0..1000 {
            match s.next() {
                Next::Done(d) => return (d, probes),
                Next::Probe { idx, why } => {
                    probes.push((idx, why));
                    let v = run(idx);
                    s.record(idx, v);
                }
            }
        }
        panic!("the search did not finish");
    }

    /// Tests pass before `b` and fail from `b` on.
    fn break_at(b: usize) -> impl FnMut(usize) -> Verdict {
        move |i| if i >= b { Verdict::Fail } else { Verdict::Pass }
    }

    /// One verdict per point: `true` passes, `false` fails.
    fn from_pattern(p: &'static [bool]) -> impl FnMut(usize) -> Verdict {
        move |i| if p[i] { Verdict::Pass } else { Verdict::Fail }
    }

    fn halving_points(probes: &[(usize, Why)]) -> Vec<usize> {
        let mut out = Vec::new();
        for (idx, why) in probes {
            if *why == Why::Halve {
                out.push(*idx);
            }
        }
        out
    }

    #[test]
    fn eight_turns_break_at_five_with_picked_good_takes_three_halving_probes() {
        let (done, probes) = drive(Search::new(8, Some(0)), break_at(5));
        assert_eq!(done, Done::Culprit { good: 4, bad: 5 });
        assert_eq!(halving_points(&probes), vec![3, 5, 4]);
    }

    #[test]
    fn eight_turns_break_at_five_without_picked_good_finds_the_pair() {
        let (done, probes) = drive(Search::new(8, None), break_at(5));
        assert_eq!(done, Done::Culprit { good: 4, bad: 5 });
        // now, then the walk back 6, 5, 3, then halve 4, then the good re-run.
        let idxs: Vec<usize> = probes.iter().map(|(i, _)| *i).collect();
        assert_eq!(idxs, vec![7, 6, 5, 3, 4, 4]);
        assert_eq!(probes[5].1, Why::RecheckGood);
    }

    #[test]
    fn flaky_good_recheck_is_unsure() {
        let mut seen = [0usize; 8];
        // Point 4 passes the first time and fails the second time.
        let run = move |i: usize| {
            seen[i] += 1;
            if i >= 5 || (i == 4 && seen[i] > 1) {
                Verdict::Fail
            } else {
                Verdict::Pass
            }
        };
        let (done, _) = drive(Search::new(8, None), run);
        assert_eq!(done, Done::Unsure { why: UnsureWhy::Flaky, good: Some(4), bad: 5 });
    }

    #[test]
    fn all_red_is_always_red() {
        let (done, _) = drive(Search::new(8, None), |_| Verdict::Fail);
        assert_eq!(done, Done::AlwaysRed { oldest: 0 });
    }

    #[test]
    fn red_from_the_start_with_picked_good_is_good_was_red() {
        let (done, probes) = drive(Search::new(8, Some(0)), |_| Verdict::Fail);
        assert_eq!(done, Done::GoodWasRed { idx: 0 });
        let idxs: Vec<usize> = probes.iter().map(|(i, _)| *i).collect();
        assert_eq!(idxs, vec![7, 0]);
    }

    #[test]
    fn non_monotone_history_still_ends_on_an_adjacent_measured_pair() {
        const P: [&[bool]; 4] = [
            &[true, false, true, false],
            &[true, false, true, false, true, false, true, false],
            &[true, true, false, true, false, true, false, false],
            &[true, false, false, true, false, true, false, false],
        ];
        for pattern in P {
            let (done, _) = drive(Search::new(pattern.len(), None), from_pattern(pattern));
            match done {
                Done::Culprit { good, bad } => {
                    assert_eq!(bad, good + 1, "pattern {pattern:?}");
                    assert!(pattern[good], "pattern {pattern:?}: good must pass");
                    assert!(!pattern[bad], "pattern {pattern:?}: bad must fail");
                }
                other => panic!("pattern {pattern:?} ended with {other:?}"),
            }
        }
    }

    #[test]
    fn hundred_points_need_at_most_sixteen_runs_and_find_every_break() {
        for b in 1..100 {
            let (done, probes) = drive(Search::new(100, None), break_at(b));
            assert_eq!(done, Done::Culprit { good: b - 1, bad: b }, "break at {b}");
            assert!(probes.len() <= 16, "break at {b} took {} runs", probes.len());
        }
        let (done, probes) = drive(Search::new(100, None), break_at(0));
        assert_eq!(done, Done::AlwaysRed { oldest: 0 });
        assert!(probes.len() <= 16, "all red took {} runs", probes.len());
        let (done, probes) = drive(Search::new(100, None), break_at(100));
        assert_eq!(done, Done::TestsPass);
        assert_eq!(probes.len(), 1);
    }

    #[test]
    fn passing_now_is_tests_pass() {
        let (done, probes) = drive(Search::new(8, Some(0)), |_| Verdict::Pass);
        assert_eq!(done, Done::TestsPass);
        assert_eq!(probes.len(), 1);
    }

    #[test]
    fn fewer_than_two_points_is_too_few() {
        assert_eq!(Search::new(0, None).next(), Next::Done(Done::TooFew));
        assert_eq!(Search::new(1, Some(0)).next(), Next::Done(Done::TooFew));
    }

    #[test]
    fn picked_good_that_is_not_older_than_now_is_ignored() {
        let (done, probes) = drive(Search::new(8, Some(7)), break_at(5));
        assert_eq!(done, Done::Culprit { good: 4, bad: 5 });
        assert!(probes.iter().all(|(_, why)| *why != Why::PickedGood));
    }

    #[test]
    fn unsure_answer_stops_the_search_with_its_reason() {
        let (done, _) = drive(Search::new(8, None), |i| {
            if i == 7 {
                Verdict::Fail
            } else {
                Verdict::Unsure { why: UnsureWhy::TimedOut }
            }
        });
        assert_eq!(done, Done::Unsure { why: UnsureWhy::TimedOut, good: None, bad: 7 });
    }

    #[test]
    fn unsure_while_halving_keeps_the_known_pair() {
        let (done, _) = drive(Search::new(8, None), |i| {
            if i == 4 {
                Verdict::Unsure { why: UnsureWhy::OutOfRuns }
            } else if i >= 5 {
                Verdict::Fail
            } else {
                Verdict::Pass
            }
        });
        assert_eq!(done, Done::Unsure { why: UnsureWhy::OutOfRuns, good: Some(3), bad: 5 });
    }

    #[test]
    fn answer_for_a_point_that_was_not_asked_is_ignored() {
        let mut s = Search::new(8, None);
        s.record(3, Verdict::Fail);
        assert_eq!(s.next(), Next::Probe { idx: 7, why: Why::BadEnd });
    }

    #[test]
    fn range_tracks_the_best_known_pair() {
        let mut s = Search::new(8, None);
        assert_eq!(s.range(), (None, 7));
        s.record(7, Verdict::Fail);
        s.record(6, Verdict::Fail);
        s.record(5, Verdict::Fail);
        assert_eq!(s.range(), (None, 5));
        assert_eq!(s.next(), Next::Probe { idx: 3, why: Why::Gallop });
        s.record(3, Verdict::Pass);
        assert_eq!(s.range(), (Some(3), 5));
    }

    #[test]
    fn pattern_helper_matches_its_verdicts() {
        static PAT: [bool; 3] = [true, false, true];
        let mut run = from_pattern(&PAT);
        assert_eq!(run(0), Verdict::Pass);
        assert_eq!(run(1), Verdict::Fail);
    }
}
