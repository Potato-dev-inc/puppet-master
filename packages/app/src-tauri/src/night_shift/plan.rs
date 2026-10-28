//! Night Shift scheduler (spec 3.4) and the usage-limit decision (spec 3.5).
//! Pure: no I/O, no clock, no threads. The runner passes in the record, the plan candidates, the
//! busy pairs and `now`, and gets back which tasks may start or what to do with a parked limit.

use std::collections::HashSet;

use super::model::{
    plan_key, AttemptEnd, NightRun, PlanOut, Task, TaskState, Who, LIMIT_GRACE_MS, MAX_ATTEMPTS,
    MAX_PARALLEL, MIN_START_LEFT_MS, SELF_RESUME_SLACK_MS,
};
use crate::missions::model::{AgentChoice, MAX_ACTIVE};

/// One plan the person ticked, as the scheduler sees it right now (built by the runner).
#[derive(Debug, Clone, PartialEq)]
pub struct Cand {
    pub agent: AgentChoice,
    pub label: String,
    /// The company behind the plan. `None` = unknown or `varies`: the plan is its own company.
    pub vendor: Option<String>,
    /// `Err` carries the plain reason the plan cannot run on this computer.
    pub runnable: Result<(), String>,
    /// `None`: not limited. `Some(None)`: limited, no reset shown. `Some(Some(t))`: limited until `t`.
    pub limited_until: Option<Option<u64>>,
}

/// A plan and project that a running attempt holds right now, from outside this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busy {
    pub plan_key: String,
    pub project: String,
}

/// "Start this task (index in `run.tasks`) on this candidate (index in `cands`)."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    pub task: usize,
    pub cand: usize,
}

/// Why a task has no plan right now. Text comes from `waiting_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitWhy {
    /// Every plan that could take the task is out of usage. `until_ms` is the earliest reset.
    OutOfUsage { until_ms: Option<u64> },
    /// Every plan that could take the task hit a gate on this project.
    Blocked,
    /// No plan for the task is ticked, or none of them can run on this computer.
    NoPlan,
}

/// Free start slots: the app-wide mission cap minus other missions, the run's parallel limit,
/// minus tasks already working.
pub fn free_slots(run: &NightRun, other_active_missions: usize) -> usize {
    let room = usize::from(run.parallel.min(MAX_PARALLEL))
        .min(MAX_ACTIVE.saturating_sub(other_active_missions));
    room.saturating_sub(working_count(run))
}

/// True while a new try can still finish: at least `MIN_START_LEFT_MS` before the stop time.
pub fn time_left_ok(run: &NightRun, now_ms: u64) -> bool {
    run.stop_at_ms.saturating_sub(now_ms) >= MIN_START_LEFT_MS
}

/// The tasks that may start now, in list order, at most `slots` of them (spec 3.4 rules 1-7).
pub fn next_starts(run: &NightRun, cands: &[Cand], busy: &[Busy], slots: usize, now_ms: u64) -> Vec<Start> {
    let mut out = Vec::new();
    if slots == 0 || !time_left_ok(run, now_ms) {
        return out;
    }
    let mut busy_plans: HashSet<String> = busy.iter().map(|b| b.plan_key.clone()).collect();
    let mut busy_projects: HashSet<String> = busy.iter().map(|b| b.project.clone()).collect();
    for task in run.tasks.iter().filter(|t| t.state == TaskState::Working) {
        busy_projects.insert(task.spec.project.clone());
        for attempt in task.attempts.iter().filter(|a| a.ended_ms.is_none()) {
            busy_plans.insert(plan_key(&attempt.plan));
        }
    }
    for (ti, task) in run.tasks.iter().enumerate() {
        if out.len() >= slots {
            break;
        }
        if !matches!(task.state, TaskState::Queued | TaskState::Waiting { .. }) {
            continue;
        }
        if busy_projects.contains(&task.spec.project) {
            continue; // rule 1: one task per project at a time
        }
        if task.attempts.len() >= usize::from(MAX_ATTEMPTS) {
            continue; // rule 7
        }
        let pick = cands.iter().enumerate().find(|(_, c)| {
            wanted(task, c) && !busy_plans.contains(&plan_key(&c.agent)) && eligible(c, task, run, cands, now_ms)
        });
        if let Some((ci, c)) = pick {
            busy_plans.insert(plan_key(&c.agent));
            busy_projects.insert(task.spec.project.clone());
            out.push(Start { task: ti, cand: ci });
        }
    }
    out
}

/// The earliest reset that frees a plan or a company for this run, if any.
pub fn wait_until(run: &NightRun, cands: &[Cand], now_ms: u64) -> Option<u64> {
    let limits = cands.iter().filter_map(|c| match c.limited_until {
        Some(Some(reset)) if reset > now_ms => Some(reset),
        _ => None,
    });
    let outs = run.plan_out.iter().filter_map(|p| p.until_ms.filter(|t| *t > now_ms));
    limits.chain(outs).min()
}

/// Why a task has no plan now, for the `Waiting` state.
pub fn wait_why(run: &NightRun, task: &Task, cands: &[Cand], now_ms: u64) -> WaitWhy {
    let wanted_cands: Vec<&Cand> = cands
        .iter()
        .filter(|c| wanted(task, c) && c.runnable.is_ok())
        .collect();
    if wanted_cands.is_empty() {
        return WaitWhy::NoPlan;
    }
    let blocked_here = |c: &Cand| {
        let key = plan_key(&c.agent);
        run.blocked.iter().any(|b| b.plan_key == key && b.project == task.spec.project)
    };
    if wanted_cands.iter().all(|&c| blocked_here(c)) {
        return WaitWhy::Blocked;
    }
    WaitWhy::OutOfUsage {
        until_ms: wait_until(run, cands, now_ms),
    }
}

/// The task's `why` text. `when` formats a reset time (the caller knows the local clock).
pub fn waiting_text(why: &WaitWhy, when: &dyn Fn(u64) -> String) -> String {
    match why {
        WaitWhy::OutOfUsage { until_ms: Some(t) } => {
            format!("Out of usage. Waiting for the reset at {}.", when(*t))
        }
        WaitWhy::OutOfUsage { until_ms: None } => "Out of usage. No reset time was shown.".to_string(),
        WaitWhy::Blocked => "A plan stopped on a question for this project. Open its terminal to answer.".to_string(),
        WaitWhy::NoPlan => "No plan for this task is ready. Tick one in Tonight.".to_string(),
    }
}

/// What to do with a limit that has parked an attempt (spec 3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitAction {
    /// Keep the copy and the pane; check again at `until_ms`.
    WaitInPlace { until_ms: u64 },
    /// End the try as out of usage; the plan is marked out until its reset.
    EndAttempt { reset_at_ms: Option<u64> },
    /// Continue in the same copy on another plan (v1.1, Keep Going).
    KeepGoing { to: AgentChoice },
}

/// What the runner knows about a parked limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitFacts {
    pub reset_at_ms: Option<u64>,
    pub self_resume_armed: bool,
    pub parked_ms: u64,
}

/// Pure decision for a parked limit. `keep_going` is the Keep Going target when Keep Going is on.
/// `other_plan_ok` says a hop to another company is allowed right now (no same-company hop, S9).
pub fn limit_action(
    l: &LimitFacts,
    now_ms: u64,
    stop_at_ms: u64,
    other_plan_ok: bool,
    keep_going: Option<&AgentChoice>,
) -> LimitAction {
    if l.parked_ms < LIMIT_GRACE_MS {
        return LimitAction::WaitInPlace {
            until_ms: now_ms + (LIMIT_GRACE_MS - l.parked_ms),
        };
    }
    if l.self_resume_armed {
        if let Some(reset) = l.reset_at_ms {
            // The CLI continues by itself after the reset. If it is still parked at reset + slack,
            // it did not resume, so the wait below stops applying and the try ends.
            let due = reset + SELF_RESUME_SLACK_MS;
            if due < stop_at_ms && now_ms < due {
                return LimitAction::WaitInPlace { until_ms: due };
            }
        }
    }
    if other_plan_ok {
        if let Some(to) = keep_going {
            return LimitAction::KeepGoing { to: to.clone() };
        }
    }
    LimitAction::EndAttempt {
        reset_at_ms: l.reset_at_ms,
    }
}

fn working_count(run: &NightRun) -> usize {
    run.tasks.iter().filter(|t| t.state == TaskState::Working).count()
}

/// The person's plan rule: `Any` takes any ticked plan, `Fixed` only its own.
fn wanted(task: &Task, c: &Cand) -> bool {
    match &task.spec.who {
        Who::Any => true,
        Who::Fixed { agent } => plan_key(agent) == plan_key(&c.agent),
    }
}

/// Usage-limited right now.
fn limited(c: &Cand, now_ms: u64) -> bool {
    match c.limited_until {
        None => false,
        Some(None) => true,
        Some(Some(reset)) => reset > now_ms,
    }
}

/// A plan that is out of usage now (the plan itself, or any plan of its company).
fn out_now(p: &PlanOut, now_ms: u64) -> bool {
    match p.until_ms {
        None => true,
        Some(t) => t > now_ms,
    }
}

/// The company of a plan when it is known: a non-empty vendor other than `varies`.
/// `None` means the plan is its own company, so it never matches another plan.
fn company(c: &Cand) -> Option<&str> {
    c.vendor.as_deref().filter(|v| !v.is_empty() && *v != "varies")
}

/// Spec 3.4 rule 4: while a plan P is out, every other plan of P's company waits for P's reset.
/// "Out" means P is out in this run's `plan_out`, or P is limited in the candidates.
fn same_company_out(c: &Cand, cands: &[Cand], run: &NightRun, now_ms: u64) -> bool {
    let key = plan_key(&c.agent);
    let same_company = |o: &Cand| {
        plan_key(&o.agent) != key && matches!((company(o), company(c)), (Some(a), Some(b)) if a == b)
    };
    let own_out = run.plan_out.iter().any(|p| p.plan_key == key && out_now(p, now_ms));
    let other_out = run
        .plan_out
        .iter()
        .filter(|p| out_now(p, now_ms))
        .any(|p| cands.iter().find(|o| plan_key(&o.agent) == p.plan_key).is_some_and(same_company));
    own_out || other_out || cands.iter().any(|o| limited(o, now_ms) && same_company(o))
}

/// A try that ended on this plan blocks the plan for this task: a gate always, a limit until its reset.
fn tried_out(end: &AttemptEnd, now_ms: u64) -> bool {
    match end {
        AttemptEnd::Gate { .. } => true,
        AttemptEnd::OutOfUsage { reset_at_ms } => matches!(reset_at_ms, Some(t) if *t > now_ms),
        _ => false,
    }
}

fn eligible(c: &Cand, task: &Task, run: &NightRun, cands: &[Cand], now_ms: u64) -> bool {
    let key = plan_key(&c.agent);
    c.runnable.is_ok()
        && !limited(c, now_ms)
        && !same_company_out(c, cands, run, now_ms)
        && !run
            .blocked
            .iter()
            .any(|b| b.plan_key == key && b.project == task.spec.project)
        && !task
            .attempts
            .iter()
            .any(|a| plan_key(&a.plan) == key && a.end.as_ref().is_some_and(|e| tried_out(e, now_ms)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::night_shift::model::{
        Attempt, AttemptKind, Blocked, Review, RunState, TaskSpec, MAX_ATTEMPTS,
    };

    // Fake plans only: `fake_a` and `fake_a2` are two logins of one company, `fake_b` is another.
    const NOW: u64 = 1_000_000;
    const STOP: u64 = NOW + 10 * 3_600_000;

    fn agent(id: &str) -> AgentChoice {
        AgentChoice {
            agent_type: id.into(),
            account: None,
            model: None,
        }
    }

    fn cand(id: &str, vendor: Option<&str>) -> Cand {
        Cand {
            agent: agent(id),
            label: id.into(),
            vendor: vendor.map(Into::into),
            runnable: Ok(()),
            limited_until: None,
        }
    }

    fn task(n: u8, project: &str, who: Who) -> Task {
        Task {
            spec: TaskSpec {
                id: format!("nt-0000000{n}"),
                project: project.into(),
                text: "Fix the parser".into(),
                who,
                may_change_tests: false,
                turns: 12,
                minutes: 90,
            },
            checks: vec![],
            state: TaskState::Queued,
            attempts: vec![],
            outcome: None,
            review: Review::Pending,
        }
    }

    fn run(tasks: Vec<Task>) -> NightRun {
        NightRun {
            id: "ns-000000000001".into(),
            created_ms: 0,
            start_at_ms: 0,
            stop_at_ms: STOP,
            state: RunState::Running,
            plans: vec![],
            parallel: 2,
            keep_awake: false,
            awake_ok: false,
            tasks,
            plan_out: vec![],
            blocked: vec![],
            events: vec![],
            last_tick_ms: NOW,
            finished_ms: None,
            seen_ms: None,
            push_sent: false,
            rev: 1,
        }
    }

    fn attempt(plan: &str, end: Option<AttemptEnd>) -> Attempt {
        let ended_ms = end.as_ref().map(|_| NOW);
        Attempt {
            n: 1,
            plan: agent(plan),
            label: plan.into(),
            kind: AttemptKind::Mission,
            mission_id: None,
            handle: None,
            pane_ids: vec![],
            started_ms: NOW - 1,
            ended_ms,
            end,
        }
    }

    fn out_of(plan: &str, until: u64) -> PlanOut {
        PlanOut {
            plan_key: plan.into(),
            until_ms: Some(until),
            since_ms: NOW,
        }
    }

    fn pick_at(r: &NightRun, c: &[Cand], now: u64) -> Vec<(usize, usize)> {
        next_starts(r, c, &[], 2, now)
            .into_iter()
            .map(|s| (s.task, s.cand))
            .collect()
    }

    fn pick(r: &NightRun, c: &[Cand]) -> Vec<(usize, usize)> {
        pick_at(r, c, NOW)
    }

    #[test]
    fn one_task_per_plan() {
        let r = run(vec![task(1, "p1", Who::Any), task(2, "p2", Who::Any)]);
        let c = vec![cand("fake_a", None)];
        assert_eq!(pick(&r, &c), vec![(0, 0)]);
        // A plan another run's attempt holds is used by no task.
        let busy = [Busy {
            plan_key: "fake_a".into(),
            project: "p9".into(),
        }];
        assert!(next_starts(&r, &c, &busy, 2, NOW).is_empty());
    }

    #[test]
    fn one_task_per_project() {
        let c = vec![cand("fake_a", None), cand("fake_b", None)];
        let r = run(vec![task(1, "p1", Who::Any), task(2, "p1", Who::Any)]);
        assert_eq!(pick(&r, &c), vec![(0, 0)]);
        // A project busy outside this run holds all its tasks.
        let busy = [Busy {
            plan_key: "fake_b".into(),
            project: "p1".into(),
        }];
        assert!(next_starts(&r, &c, &busy, 2, NOW).is_empty());
        // A task already working in the project blocks the others.
        let mut working = run(vec![task(1, "p1", Who::Any), task(2, "p1", Who::Any)]);
        working.tasks[0].state = TaskState::Working;
        assert!(pick(&working, &c).is_empty());
    }

    #[test]
    fn list_order_wins() {
        let r = run(vec![
            task(1, "p1", Who::Any),
            task(2, "p2", Who::Any),
            task(3, "p3", Who::Any),
        ]);
        let c = vec![cand("fake_a", None), cand("fake_b", None)];
        assert_eq!(pick(&r, &c), vec![(0, 0), (1, 1)]);
        let one = next_starts(&r, &c, &[], 1, NOW);
        assert_eq!(one, vec![Start { task: 0, cand: 0 }]);
        // A waiting task is queued again and starts in its turn.
        let mut waiting = run(vec![task(1, "p1", Who::Any)]);
        waiting.tasks[0].state = TaskState::Waiting {
            until_ms: Some(NOW + 1),
            why: "Out of usage.".into(),
        };
        assert_eq!(pick(&waiting, &c), vec![(0, 0)]);
    }

    #[test]
    fn fixed_waits_for_its_plan() {
        let fixed_b = Who::Fixed {
            agent: agent("fake_b"),
        };
        let r = run(vec![task(1, "p1", fixed_b)]);
        assert!(pick(&r, &[cand("fake_a", None)]).is_empty());
        assert_eq!(
            pick(&r, &[cand("fake_a", None), cand("fake_b", None)]),
            vec![(0, 1)]
        );
        // Its plan limited: it waits even though another plan is free.
        let mut limited_b = cand("fake_b", None);
        limited_b.limited_until = Some(Some(NOW + 60_000));
        assert!(pick(&r, &[cand("fake_a", None), limited_b]).is_empty());
    }

    #[test]
    fn different_company_before_and_after_reset() {
        let r0 = run(vec![task(1, "p1", Who::Any)]);
        let c = vec![
            cand("fake_a", Some("vendor_a")),
            cand("fake_a2", Some("vendor_a")),
            cand("fake_b", Some("vendor_b")),
        ];
        // fake_a is out (run record): its company waits, fake_b takes the task.
        let mut r = r0.clone();
        r.plan_out = vec![out_of("fake_a", NOW + 60_000)];
        assert_eq!(pick(&r, &c), vec![(0, 2)]);
        // Same with the limit only on the candidate.
        let mut limited = c.clone();
        limited[0].limited_until = Some(Some(NOW + 60_000));
        assert_eq!(pick(&r0, &limited), vec![(0, 2)]);
        // After the reset fake_a is first in order again.
        assert_eq!(pick_at(&r, &c, NOW + 60_001), vec![(0, 0)]);
        assert_eq!(pick_at(&r0, &limited, NOW + 60_001), vec![(0, 0)]);
    }

    #[test]
    fn plan_without_vendor_is_its_own_company() {
        let c = vec![
            cand("fake_a", None),
            cand("fake_a2", None),
            cand("fake_b", None),
        ];
        let mut r = run(vec![task(1, "p1", Who::Any)]);
        r.plan_out = vec![out_of("fake_a", NOW + 60_000)];
        assert_eq!(pick(&r, &c), vec![(0, 1)]);
    }

    #[test]
    fn varies_never_matches() {
        let mut r = run(vec![task(1, "p1", Who::Any)]);
        r.plan_out = vec![out_of("fake_a", NOW + 60_000)];
        // Both varies: never the same company.
        let both = vec![
            cand("fake_a", Some("varies")),
            cand("fake_a2", Some("varies")),
            cand("fake_b", Some("vendor_b")),
        ];
        assert_eq!(pick(&r, &both), vec![(0, 1)]);
        // Out plan is varies, the other has a vendor: still not the same company.
        let mixed = vec![
            cand("fake_a", Some("varies")),
            cand("fake_a2", Some("vendor_a")),
        ];
        assert_eq!(pick(&r, &mixed), vec![(0, 1)]);
    }

    #[test]
    fn blocked_pair_is_skipped() {
        let mut r = run(vec![task(1, "p1", Who::Any), task(2, "p2", Who::Any)]);
        r.blocked = vec![Blocked {
            plan_key: "fake_a".into(),
            project: "p1".into(),
            sentence: "Asked to trust the folder.".into(),
        }];
        let c = vec![cand("fake_a", None), cand("fake_b", None)];
        // Task 1 cannot use fake_a on p1, so it takes fake_b; task 2 takes fake_a.
        assert_eq!(pick(&r, &c), vec![(0, 1), (1, 0)]);
    }

    #[test]
    fn max_attempts_and_tried_plans() {
        let c = vec![cand("fake_a", None), cand("fake_b", None)];
        let failed = || AttemptEnd::Failed {
            code: "x".into(),
            sentence: "Failed.".into(),
        };
        // Two tries used: never again.
        let mut spent = task(1, "p1", Who::Any);
        spent.attempts = vec![attempt("fake_a", Some(failed())), attempt("fake_b", Some(failed()))];
        assert_eq!(spent.attempts.len(), usize::from(MAX_ATTEMPTS));
        assert!(pick(&run(vec![spent]), &c).is_empty());
        // One failed try: the second try may start.
        let mut second = task(1, "p1", Who::Any);
        second.attempts = vec![attempt("fake_a", Some(failed()))];
        assert_eq!(pick(&run(vec![second.clone()]), &c), vec![(0, 0)]);
        // A gate on fake_a: this task never tries fake_a again.
        let mut gated = task(1, "p1", Who::Any);
        gated.attempts = vec![attempt(
            "fake_a",
            Some(AttemptEnd::Gate {
                code: "trust".into(),
                sentence: "Asked to trust.".into(),
            }),
        )];
        assert_eq!(pick(&run(vec![gated.clone()]), &c), vec![(0, 1)]);
        // Out of usage: skipped until its reset, allowed after it.
        let mut out = task(1, "p1", Who::Any);
        out.attempts = vec![attempt(
            "fake_a",
            Some(AttemptEnd::OutOfUsage {
                reset_at_ms: Some(NOW + 60_000),
            }),
        )];
        assert_eq!(pick(&run(vec![out.clone()]), &c), vec![(0, 1)]);
        assert_eq!(pick_at(&run(vec![out]), &c, NOW + 60_001), vec![(0, 0)]);
    }

    #[test]
    fn min_start_left() {
        let c = vec![cand("fake_a", None)];
        let mut r = run(vec![task(1, "p1", Who::Any)]);
        r.stop_at_ms = NOW + MIN_START_LEFT_MS - 1;
        assert!(!time_left_ok(&r, NOW));
        assert!(pick(&r, &c).is_empty());
        r.stop_at_ms = NOW + MIN_START_LEFT_MS;
        assert!(time_left_ok(&r, NOW));
        assert_eq!(pick(&r, &c), vec![(0, 0)]);
        // After the stop time nothing starts.
        assert!(!time_left_ok(&r, STOP + 1));
    }

    #[test]
    fn free_slots_counts_other_missions_and_working_tasks() {
        let mut r = run(vec![]);
        assert_eq!(free_slots(&r, 0), 2);
        assert_eq!(free_slots(&r, 1), 1);
        assert_eq!(free_slots(&r, 2), 0);
        assert_eq!(free_slots(&r, 5), 0);
        r.parallel = 1;
        assert_eq!(free_slots(&r, 0), 1);
        let mut working = run(vec![task(1, "p1", Who::Any)]);
        working.tasks[0].state = TaskState::Working;
        assert_eq!(free_slots(&working, 0), 1);
        assert_eq!(free_slots(&working, 1), 0);
    }

    #[test]
    fn wait_until_is_the_earliest_useful_reset() {
        let mut a = cand("fake_a", None);
        a.limited_until = Some(Some(NOW + 5_000));
        let mut b = cand("fake_b", None);
        b.limited_until = Some(Some(NOW + 7_000));
        let c = [a, b];
        let mut r = run(vec![]);
        assert_eq!(wait_until(&r, &c, NOW), Some(NOW + 5_000));
        r.plan_out = vec![out_of("fake_a", NOW + 4_000)];
        assert_eq!(wait_until(&r, &c, NOW), Some(NOW + 4_000));
        // The plan-out reset has passed, the candidate's reset is next.
        assert_eq!(wait_until(&r, &c, NOW + 4_500), Some(NOW + 5_000));
        assert_eq!(wait_until(&r, &c, NOW + 8_000), None);
        // A limit with no reset time is not a time to wait for.
        let mut unknown = cand("fake_u", None);
        unknown.limited_until = Some(None);
        assert_eq!(wait_until(&run(vec![]), &[unknown], NOW), None);
    }

    #[test]
    fn wait_why_and_its_text() {
        let t = task(1, "p1", Who::Any);
        let r = run(vec![t.clone()]);
        assert_eq!(wait_why(&r, &t, &[], NOW), WaitWhy::NoPlan);
        let mut bad = cand("fake_a", None);
        bad.runnable = Err("Not installed.".into());
        assert_eq!(wait_why(&r, &t, &[bad], NOW), WaitWhy::NoPlan);
        let mut blocked = run(vec![t.clone()]);
        blocked.blocked = vec![Blocked {
            plan_key: "fake_a".into(),
            project: "p1".into(),
            sentence: "Asked to trust.".into(),
        }];
        assert_eq!(wait_why(&blocked, &t, &[cand("fake_a", None)], NOW), WaitWhy::Blocked);
        let mut a = cand("fake_a", None);
        a.limited_until = Some(Some(NOW + 5_000));
        assert_eq!(
            wait_why(&r, &t, &[a], NOW),
            WaitWhy::OutOfUsage {
                until_ms: Some(NOW + 5_000)
            }
        );
        let when = |t: u64| (t / 1000).to_string();
        assert_eq!(
            waiting_text(&WaitWhy::OutOfUsage { until_ms: Some(5_000) }, &when),
            "Out of usage. Waiting for the reset at 5."
        );
        assert_eq!(
            waiting_text(&WaitWhy::OutOfUsage { until_ms: None }, &when),
            "Out of usage. No reset time was shown."
        );
        assert_eq!(
            waiting_text(&WaitWhy::Blocked, &when),
            "A plan stopped on a question for this project. Open its terminal to answer."
        );
        assert_eq!(
            waiting_text(&WaitWhy::NoPlan, &when),
            "No plan for this task is ready. Tick one in Tonight."
        );
    }

    #[test]
    fn limit_action_table() {
        let stop = NOW + 10 * 3_600_000;
        let reset = NOW + 3_600_000;
        let other = agent("fake_a2");
        let facts = |reset: Option<u64>, armed: bool, parked: u64| LimitFacts {
            reset_at_ms: reset,
            self_resume_armed: armed,
            parked_ms: parked,
        };
        // Grace: a fresh limit may clear by itself.
        assert_eq!(
            limit_action(&facts(Some(reset), false, 0), NOW, stop, true, Some(&other)),
            LimitAction::WaitInPlace {
                until_ms: NOW + LIMIT_GRACE_MS
            }
        );
        assert_eq!(
            limit_action(&facts(None, false, 60_000), NOW, stop, true, None),
            LimitAction::WaitInPlace {
                until_ms: NOW + 120_000
            }
        );
        // Not armed, no Keep Going: end the try, plan out until the reset.
        assert_eq!(
            limit_action(&facts(Some(reset), false, LIMIT_GRACE_MS), NOW, stop, true, None),
            LimitAction::EndAttempt {
                reset_at_ms: Some(reset)
            }
        );
        // Armed and the reset is inside the night: wait for reset + slack.
        assert_eq!(
            limit_action(&facts(Some(reset), true, LIMIT_GRACE_MS), NOW, stop, true, None),
            LimitAction::WaitInPlace {
                until_ms: reset + SELF_RESUME_SLACK_MS
            }
        );
        // Armed, but reset + slack is past the stop time: not armed.
        let late = stop - 5 * 60_000;
        assert_eq!(
            limit_action(&facts(Some(late), true, LIMIT_GRACE_MS), NOW, stop, true, None),
            LimitAction::EndAttempt {
                reset_at_ms: Some(late)
            }
        );
        // Armed, no reset shown: not armed.
        assert_eq!(
            limit_action(&facts(None, true, LIMIT_GRACE_MS), NOW, stop, true, None),
            LimitAction::EndAttempt { reset_at_ms: None }
        );
        // Armed, but still parked after reset + slack: the CLI did not resume, not armed.
        let past = NOW - 60_000;
        assert_eq!(
            limit_action(&facts(Some(past), true, LIMIT_GRACE_MS), NOW + 600_000, stop, true, None),
            LimitAction::EndAttempt {
                reset_at_ms: Some(past)
            }
        );
        // Keep Going given and another company is fine: hop in the same copy.
        assert_eq!(
            limit_action(&facts(Some(reset), false, LIMIT_GRACE_MS), NOW, stop, true, Some(&other)),
            LimitAction::KeepGoing { to: other.clone() }
        );
        // Keep Going given but no other company may take it: no same-company hop (S9).
        assert_eq!(
            limit_action(&facts(Some(reset), false, LIMIT_GRACE_MS), NOW, stop, false, Some(&other)),
            LimitAction::EndAttempt {
                reset_at_ms: Some(reset)
            }
        );
    }
}
