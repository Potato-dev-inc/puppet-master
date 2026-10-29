//! One What broke it job, start to finish (SPEC n33 section 3.5, steps 1 to 8). It runs on the thread
//! `live.rs` starts and never holds a registry lock while a test runs.
//!
//! Order: take the "now" save point, list the folder's save points, make the copy and its setup, then ask
//! `Search` for the next save point, run the declared command there (a failure is run again once), feed the
//! verdict back, and turn the end of the search into an `Outcome` with `blame`. The copy is removed on
//! every exit path (`CopyGuard` removes it on drop, also when the loop panics).
//!
//! S9: output is redacted by `LiveIo::redact` before it is stored; only the last `TAIL_LINES` lines of the
//! first failing run are kept, and raw output never leaves this file. S14: nothing is written to a pane.
#![allow(dead_code)] // `commands/what_broke_it.rs` (WB-16) is the first caller of `Live`.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::blame::{self, Blame};
use super::copy::CopyError;
use super::exec::RunResult;
use super::failnames;
use super::live::{
    BusyGuard, CopyHandle, Job, LiveIo, Req, Shared, WB_INTERNAL, WB_SETUP_FAILED, WB_TEST_NOT_FOUND, WB_TOO_BIG, WB_TOO_FEW,
    WB_UNAVAILABLE,
};
use super::model::{
    CountsLite, Finding, Narrowed, Outcome, Point, Probe, Span, Suspect, UnsureWhy, Verdict, TAIL_LINES,
};
use super::points::{self, Timeline};
use super::search::{Done, Next, Search, Why};
use super::views::{JobState, CODE_STILL_FAILING};
use super::words;
use crate::agent_runs::checks::counts::TestCounts;
use crate::checkpoints::engine::CheckpointError;
use crate::turn_summary::TurnSummary;

/// What a finished search hands to the saver.
struct Report {
    outcome: Outcome,
    /// Redacted already.
    tail: String,
    probes: Vec<Probe>,
    good_idx: Option<u32>,
    bad_idx: Option<u32>,
}

impl Report {
    fn of(outcome: Outcome) -> Report {
        Report { outcome, tail: String::new(), probes: Vec::new(), good_idx: None, bad_idx: None }
    }

    fn failed(code: &str, message: impl Into<String>) -> Report {
        Report::of(Outcome::Failed { code: code.to_string(), message: message.into() })
    }
}

/// Removes the copy when dropped, so a panic or an early return never leaves it behind.
struct CopyGuard(Option<Box<dyn CopyHandle>>);

impl CopyGuard {
    fn get(&self) -> &dyn CopyHandle {
        self.0.as_deref().expect("the copy is removed only at the end")
    }

    fn get_mut(&mut self) -> &mut dyn CopyHandle {
        self.0.as_deref_mut().expect("the copy is removed only at the end")
    }

    /// Remove it now; a failure is only logged (the stale sweep gets what is left).
    fn finish(&mut self) {
        if let Some(copy) = self.0.take() {
            if let Err(e) = copy.finish() {
                tracing::warn!(error = %e, "what-broke-it: the test copy could not be removed");
            }
        }
    }
}

/// How one run reads.
enum Class {
    Pass,
    Fail,
    Unsure(UnsureWhy),
}

/// The first failing run kept for the finding: the lowest save point that failed.
struct FirstFail {
    idx: usize,
    raw_tail: String,
    counts: Option<TestCounts>,
}

/// The state of one search: the copy, the budget, the answers measured so far.
struct Probing<'a> {
    io: &'a dyn LiveIo,
    job: &'a Job,
    req: &'a Req,
    copy: CopyGuard,
    runs: u32,
    start_ms: u64,
    max_ms: u64,
    points: u32,
    cache: HashMap<String, Verdict>,
    probes: Vec<Probe>,
    first_fail: Option<FirstFail>,
}

impl Probing<'_> {
    fn elapsed(&self) -> u64 {
        self.io.now_ms().saturating_sub(self.start_ms)
    }

    /// Why the next `need` runs cannot start, if they cannot.
    fn budget(&self, need: u32) -> Option<UnsureWhy> {
        if self.job.cancel.load(Ordering::SeqCst) {
            Some(UnsureWhy::Stopped)
        } else if self.elapsed() >= self.max_ms {
            Some(UnsureWhy::OutOfTime)
        } else if self.runs + need > self.req.limits.max_runs {
            Some(UnsureWhy::OutOfRuns)
        } else {
            None
        }
    }

    fn progress(&self, point_no: usize) {
        {
            let mut info = self.job.info.lock();
            info.state = JobState::Running;
            info.point_no = point_no as u32;
            info.run = self.runs;
            info.line = words::progress(point_no as u32, self.points, self.runs, self.req.limits.max_runs, (self.elapsed() / 60_000) as u32);
        }
        self.io.emit(&self.job.id, None);
    }

    /// One run of the declared command at `tree`.
    fn run_once(&mut self, tree: &str, idx: usize) -> Result<(RunResult, bool), Report> {
        self.copy.get_mut().set_tree(tree).map_err(|e| copy_report(self.io, e))?;
        let remaining = self.max_ms.saturating_sub(self.elapsed());
        let check_ms = u64::from(self.req.check.timeout_min).max(1) * 60_000;
        let timeout_ms = check_ms.min(remaining).max(1_000);
        self.runs += 1;
        let shown = if idx == usize::MAX { self.job.info.lock().point_no as usize } else { idx + 1 };
        self.progress(shown);
        let result = self.io.run_test(self.copy.get(), &self.req.check.argv, timeout_ms, &self.job.cancel);
        // A run cut short by the job's own minutes (not the check's limit) is "out of time".
        let capped = result.timed_out && timeout_ms < check_ms;
        Ok((result, capped))
    }

    fn classify(&self, r: &RunResult, capped: bool) -> Result<Class, Report> {
        if r.not_started.is_some() {
            let program = self.req.check.argv.first().map(String::as_str).unwrap_or("The test command");
            return Err(Report::failed(WB_TEST_NOT_FOUND, words::program_not_found(program)));
        }
        Ok(if r.exit_ok {
            Class::Pass
        } else if r.stopped {
            Class::Unsure(UnsureWhy::Stopped)
        } else if r.timed_out {
            Class::Unsure(if capped { UnsureWhy::OutOfTime } else { UnsureWhy::TimedOut })
        } else {
            Class::Fail
        })
    }

    fn remember(&mut self, tree: &str, verdict: &Verdict) {
        if matches!(verdict, Verdict::Unsure { .. }) {
            return;
        }
        if self.cache.insert(tree.to_string(), verdict.clone()).is_none() {
            self.probes.push(Probe { tree: tree.to_string(), verdict: verdict.clone() });
        } else if let Some(p) = self.probes.iter_mut().find(|p| p.tree == tree) {
            p.verdict = verdict.clone();
        }
    }

    fn keep_fail(&mut self, idx: usize, r: &RunResult) {
        if self.first_fail.as_ref().is_some_and(|f| f.idx <= idx) {
            return;
        }
        self.first_fail = Some(FirstFail { idx, raw_tail: r.output_tail.clone(), counts: r.counts });
    }

    /// The answer at `tree`. A cached answer costs no run unless `fresh`. A failing run is run again once
    /// (`confirm`); a different second answer is flaky (S7).
    fn measure(&mut self, tree: &str, idx: usize, fresh: bool, confirm: bool) -> Result<Verdict, Report> {
        if !fresh {
            if let Some(v) = self.cache.get(tree) {
                return Ok(v.clone());
            }
        }
        let need = if confirm { 2.min(self.req.limits.max_runs) } else { 1 };
        if let Some(why) = self.budget(need) {
            return Ok(Verdict::Unsure { why });
        }
        let (first, capped) = self.run_once(tree, idx)?;
        let verdict = match self.classify(&first, capped)? {
            Class::Unsure(why) => Verdict::Unsure { why },
            Class::Pass => Verdict::Pass,
            Class::Fail => {
                self.keep_fail(idx, &first);
                if !confirm {
                    Verdict::Fail
                } else if let Some(why) = self.budget(1) {
                    Verdict::Unsure { why }
                } else {
                    let (second, capped) = self.run_once(tree, idx)?;
                    match self.classify(&second, capped)? {
                        Class::Pass => Verdict::Unsure { why: UnsureWhy::Flaky },
                        Class::Unsure(why) => Verdict::Unsure { why },
                        Class::Fail => Verdict::Fail,
                    }
                }
            }
        };
        self.remember(tree, &verdict);
        Ok(verdict)
    }
}

/// The job thread's body: run, save the finding, free the slot, tell the window.
pub(super) fn run(shared: &Arc<Shared>, job: &Arc<Job>, req: Req, guard: BusyGuard) {
    let report = match catch_unwind(AssertUnwindSafe(|| search(shared, job, &req))) {
        Ok(Ok(report)) | Ok(Err(report)) => report,
        Err(_) => Report::failed(WB_INTERNAL, words::went_wrong()),
    };
    job.info.lock().state = JobState::Finishing;
    let finished_ms = shared.io.now_ms();
    let (started_ms, runs, points) = {
        let info = job.info.lock();
        (info.started_ms, info.run, info.points)
    };
    let finding = Finding {
        id: req.finding_id.clone(),
        project: req.project_key.clone(),
        folder: req.folder.clone(),
        pane_key: req.pane_key.clone(),
        started_ms,
        finished_ms,
        check_name: req.check.name.clone(),
        check_argv: req.check.argv.clone(),
        runs,
        minutes_x10: (finished_ms.saturating_sub(started_ms) / 6_000) as u32,
        points,
        outcome: report.outcome,
        tail: report.tail,
        probes: report.probes,
        undone_ms: None,
        good_idx: report.good_idx,
        bad_idx: report.bad_idx,
    };
    let saved = match shared.store.put(&finding) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(error = %e, "what-broke-it: the result could not be saved");
            false
        }
    };
    {
        let mut info = job.info.lock();
        info.state = JobState::Done;
        info.ended_ms = Some(finished_ms);
        if saved {
            info.finding_id = Some(finding.id.clone());
        }
    }
    // Free the slot first, so "Keep looking" pressed on the event never meets a busy job.
    drop(guard);
    shared.io.emit(&job.id, saved.then_some(finding.id.as_str()));
}

/// Steps 1 to 8 of the spec. `Err` is a finished early report (a refusal or a failed setup).
fn search(shared: &Shared, job: &Job, req: &Req) -> Result<Report, Report> {
    let io = shared.io.as_ref();
    let now = io.now_ms();
    let now_tree = io.snapshot_now(&req.project_dir, &req.finding_id, now).map_err(|e| checkpoint_report(&e))?;
    let all = io.list_all(&req.project_dir).map_err(|e| checkpoint_report(&e))?;
    let timeline = points::build(&all, &now_tree, now);
    let summaries = io.summaries(&req.project_dir);

    let tree = if req.recheck { now_tree.clone() } else { timeline.points.last().map(|p| p.tree.clone()).unwrap_or_default() };
    let n = if req.recheck { 1 } else { timeline.points.len() };
    job.info.lock().points = n as u32;
    if !req.recheck && n < 2 {
        return Ok(Report::failed(WB_TOO_FEW, words::too_few()));
    }

    let copy = io.new_copy(&req.project_dir, &shared.store.copies_root()).map_err(|e| copy_report(io, e))?;
    let (start_ms, max_ms) = {
        let info = job.info.lock();
        (info.started_ms, info.max_ms)
    };
    let mut probing = Probing {
        io,
        job,
        req,
        copy: CopyGuard(Some(copy)),
        runs: 0,
        start_ms,
        max_ms,
        points: n as u32,
        cache: HashMap::new(),
        probes: Vec::new(),
        first_fail: None,
    };
    if let Some(prev) = &req.resume {
        for probe in &prev.probes {
            probing.cache.insert(probe.tree.clone(), probe.verdict.clone());
            probing.probes.push(probe.clone());
        }
    }
    let result = prepare(&mut probing, &tree).and_then(|()| {
        if req.recheck {
            recheck(&mut probing, &tree)
        } else {
            find(&mut probing, &timeline, &all, &summaries)
        }
    });
    probing.copy.finish();
    let mut report = result?;
    report.probes = std::mem::take(&mut probing.probes);
    if let Some(fail) = probing.first_fail.take() {
        let (failing, counts) = failure_facts(io, &fail);
        report.tail = tail_of(io, &fail.raw_tail);
        if let Outcome::Culprit { failing: names, counts: c, .. } = &mut report.outcome {
            *names = failing;
            *c = counts;
        }
    } else if let Some(prev) = &req.resume {
        report.tail = prev.tail.clone();
    }
    Ok(report)
}

/// Size check, first state, then the project's own setup steps (once).
fn prepare(p: &mut Probing, tree: &str) -> Result<(), Report> {
    let io = p.io;
    p.copy.get().tree_bytes(tree).map_err(|e| copy_report(io, e))?;
    p.copy.get_mut().set_tree(tree).map_err(|e| copy_report(io, e))?;
    let preset = io.preset(&p.req.folder);
    p.copy.get_mut().prepare(&p.req.project_dir, &preset).map_err(|e| copy_report(io, e))?;
    Ok(())
}

/// "Check it again": one probe of the folder as it is now.
fn recheck(p: &mut Probing, tree: &str) -> Result<Report, Report> {
    p.progress(1);
    let verdict = p.measure(tree, usize::MAX, true, true)?;
    let outcome = match verdict {
        Verdict::Pass => Outcome::TestsPass,
        Verdict::Fail => Outcome::Failed { code: CODE_STILL_FAILING.into(), message: words::still_failing().into() },
        Verdict::Unsure { why } => Outcome::NotSure { why, narrowed: None },
    };
    Ok(Report::of(outcome))
}

/// The search proper (steps 3 to 5).
fn find(p: &mut Probing, timeline: &Timeline, all: &[crate::checkpoints::Checkpoint], summaries: &[TurnSummary]) -> Result<Report, Report> {
    let points = &timeline.points;
    let picked = p.req.picked.as_ref().and_then(|(pane, id)| picked_index(points, all, pane, id));
    let mut search = Search::new(points.len(), picked);
    let done = loop {
        match search.next() {
            Next::Done(done) => break done,
            Next::Probe { idx, why } => {
                let recheck_good = why == Why::RecheckGood;
                let verdict = p.measure(&points[idx].tree, idx, recheck_good, !recheck_good)?;
                search.record(idx, verdict);
            }
        }
    };
    let (good, bad) = search.range();
    let mut report = match done {
        Done::TooFew => Report::failed(WB_TOO_FEW, words::too_few()),
        Done::TestsPass => Report::of(Outcome::TestsPass),
        Done::AlwaysRed { oldest } => Report::of(Outcome::AlwaysRed { oldest_ms: points[oldest].at_ms }),
        Done::GoodWasRed { idx } => Report::of(Outcome::GoodWasRed { at_ms: points[idx].at_ms }),
        Done::Unsure { why, good, bad } => {
            let narrowed = good.and_then(|g| narrowed_between(&timeline.spans, summaries, points[g].at_ms, points[bad].at_ms));
            Report::of(Outcome::NotSure { why, narrowed })
        }
        Done::Culprit { good, bad } => culprit(p, timeline, summaries, good, bad)?,
    };
    report.good_idx = good.map(|g| g as u32);
    report.bad_idx = Some(bad as u32);
    Ok(report)
}

/// Step 4: name the turn (or turns, or the gap) between the last good and the first bad point.
fn culprit(p: &mut Probing, timeline: &Timeline, summaries: &[TurnSummary], good: usize, bad: usize) -> Result<Report, Report> {
    let (good_pt, bad_pt) = (&timeline.points[good], &timeline.points[bad]);
    let mut found = blame::blame(&timeline.spans, good_pt.at_ms, bad_pt.at_ms);
    if let Some(before_tree) = blame::needs_refine(&found, &good_pt.tree) {
        let verdict = p.measure(&before_tree, usize::MAX, false, true)?;
        match verdict {
            Verdict::Unsure { why } => {
                let narrowed = narrowed_between(&timeline.spans, summaries, good_pt.at_ms, bad_pt.at_ms);
                return Ok(Report::of(Outcome::NotSure { why, narrowed }));
            }
            Verdict::Fail => found = blame::after_refine(found, true),
            Verdict::Pass => {}
        }
    }
    let outcome = match found {
        Blame::Turn(span) => Outcome::Culprit {
            suspect: blame::join_summaries(&span, summaries),
            failing: Vec::new(),
            counts: None,
            last_good_ms: good_pt.at_ms,
        },
        Blame::Several(spans) => {
            let mut suspects: Vec<Suspect> = spans.iter().map(|s| blame::join_summaries(s, summaries)).collect();
            suspects.sort_by(|a, b| (a.at_ms, &a.pane_key, a.turn).cmp(&(b.at_ms, &b.pane_key, b.turn)));
            Outcome::Several { suspects, last_good_ms: good_pt.at_ms, first_bad_ms: bad_pt.at_ms }
        }
        Blame::Outside => Outcome::BetweenTurns { last_good_ms: good_pt.at_ms, first_bad_ms: bad_pt.at_ms },
    };
    Ok(Report::of(outcome))
}

/// The turns left as suspects between two times, for "It was one of turns 5 to 8".
fn narrowed_between(spans: &[Span], summaries: &[TurnSummary], good_at: u64, bad_at: u64) -> Option<Narrowed> {
    let mut turns: Vec<Span> = match blame::blame(spans, good_at, bad_at) {
        Blame::Turn(s) => vec![s],
        Blame::Several(v) => v,
        Blame::Outside => Vec::new(),
    };
    turns.sort_by(|a, b| (a.start_ms, &a.pane_key, a.turn).cmp(&(b.start_ms, &b.pane_key, b.turn)));
    let (first, last) = (turns.first()?, turns.last()?);
    Some(Narrowed {
        from: blame::join_summaries(first, summaries),
        to: blame::join_summaries(last, summaries),
        turns: turns.len() as u32,
    })
}

/// The index of the point the person picked: by pane and id, or by tree when equal states were merged.
fn picked_index(points: &[Point], all: &[crate::checkpoints::Checkpoint], pane: &str, id: &str) -> Option<usize> {
    points
        .iter()
        .position(|p| p.pane_key.as_deref() == Some(pane) && p.checkpoint_id.as_deref() == Some(id))
        .or_else(|| {
            let tree = &all.iter().find(|c| c.pane_id == pane && c.id == id)?.tree;
            points.iter().position(|p| &p.tree == tree)
        })
}

/// Failing test names (redacted) and the counts, from the first failing run.
fn failure_facts(io: &dyn LiveIo, fail: &FirstFail) -> (Vec<String>, Option<CountsLite>) {
    let failing = failnames::failing_tests(&fail.raw_tail).iter().map(|name| io.redact(name)).collect();
    let counts = fail.counts.map(|c| CountsLite { passed: c.passed, failed: c.failed, skipped: c.skipped, total: c.total });
    (failing, counts)
}

/// The last `TAIL_LINES` lines of a run, redacted with paths cut down (S9).
fn tail_of(io: &dyn LiveIo, raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    io.redact(&lines[start..].join("\n"))
}

fn checkpoint_report(e: &CheckpointError) -> Report {
    let code = if matches!(e, CheckpointError::TooLarge(_)) { WB_TOO_BIG } else { WB_UNAVAILABLE };
    Report::failed(code, e.to_string())
}

/// A copy problem in plain words. A failed setup keeps the redacted end of its output (S9).
fn copy_report(io: &dyn LiveIo, e: CopyError) -> Report {
    match e {
        CopyError::TooBig { .. } => Report::failed(WB_TOO_BIG, words::too_big()),
        CopyError::SetupFailed { step, tail } => {
            let mut report = Report::failed(WB_SETUP_FAILED, words::setup_failed(&io.redact(&step)));
            report.tail = tail_of(io, &tail);
            report
        }
        CopyError::UnsafeRoot => Report::failed(WB_UNAVAILABLE, words::copy_unsafe()),
        CopyError::Link { .. } => Report::of(Outcome::CopyDiffers { detail: io.redact(&e.to_string()) }),
        other => Report::failed(WB_UNAVAILABLE, other.to_string()),
    }
}

