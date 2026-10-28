//! Night Shift: what Tonight's Start checks, and the run it builds (SPEC n32 3.10 and the `night_start`
//! row of 3.8).
//!
//! [`build`] is pure. It gets the clock, the plans, the git and undo state and the declared checks
//! through [`Env`], and returns the new run or the first reason it cannot start: a code and the plain
//! sentence the sheet shows. The desktop fills `Env` in `commands/night.rs`; the tests fill in their own.
//!
//! The page sends plan keys, never plan details. Each key must match a plan [`known_plans`] found on
//! this computer right now, so nothing the page sends can name an agent that is not here (S7).

use chrono::{Local, NaiveTime, TimeZone};
use serde::Deserialize;

use crate::missions::model::AgentChoice;
use crate::night_shift::model::{
    plan_key, valid_hhmm, NightRun, PlanChoice, RunState, Task, TaskSpec, TaskState, Who, DEFAULT_MINUTES,
    DEFAULT_TURNS, MAX_MINUTES, MAX_PARALLEL, MAX_RUN_MS, MAX_TASKS, MAX_TASK_TEXT, MAX_TURNS, MIN_MINUTES, MIN_RUN_MS,
    MIN_TURNS,
};
use crate::operations::OperationError;

/// One task as the Tonight sheet sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub project: String,
    pub text: String,
    /// `None` (or `"any"`) = any ticked plan with usage left; otherwise one plan key.
    #[serde(default)]
    pub who: Option<String>,
    #[serde(default)]
    pub may_change_tests: bool,
    #[serde(default = "default_turns")]
    pub turns: u32,
    #[serde(default = "default_minutes")]
    pub minutes: u32,
}

fn default_turns() -> u32 {
    DEFAULT_TURNS
}

fn default_minutes() -> u32 {
    DEFAULT_MINUTES
}

/// What the Tonight sheet's Start sends.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInput {
    pub tasks: Vec<TaskInput>,
    /// Plan keys (`agent` or `agent@account`) in the person's order.
    pub plans: Vec<String>,
    pub stop_hhmm: String,
    /// `None` = start now.
    #[serde(default)]
    pub start_hhmm: Option<String>,
    pub parallel: u8,
    pub keep_awake: bool,
}

/// A plan the Tonight sheet can tick: one installed agent, with no login or with one saved login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub key: String,
    pub agent: AgentChoice,
    pub label: String,
    /// The billing company, when it is known and not `varies`. Two plans of one company cannot both run.
    pub vendor: Option<String>,
    /// `Ok` when the plan can run in the folders given; otherwise the plain reason.
    pub runnable: Result<(), String>,
    /// `handoff::agent_limit`: `Some(Some(reset))` out of usage until a time, `Some(None)` out with no time shown.
    pub limited_until: Option<Option<u64>>,
}

/// The facts Start needs. The desktop fills it in (`commands/night.rs`); the tests fill in their own.
pub struct Env<'a> {
    pub now_ms: u64,
    /// The Labs switch (`night_shift_enabled`).
    pub enabled: bool,
    /// Undo is on and git is there, so a copy can be rolled back.
    pub undo_on: bool,
    pub awake_asked: bool,
    /// An unfinished run exists.
    pub busy: bool,
    pub plans: &'a [Known],
    /// The project key a task is stored under (`comparable_project_path`).
    pub key_of: &'a dyn Fn(&str) -> String,
    /// Whether the project key is a folder with a commit.
    pub is_git: &'a dyn Fn(&str) -> bool,
    /// The declared checks of a project key, as argv lists. A snapshot is taken at Start.
    pub declared: &'a dyn Fn(&str) -> Vec<Vec<String>>,
}

/// The sentence for Labs off (the sheet shows it on its disabled Start too).
pub const LABS_OFF_SENTENCE: &str = "A person can turn Night Shift on in Settings > General > Labs.";
/// The sentence for an unfinished run (the sheet shows it on its disabled Start too).
pub const BUSY_SENTENCE: &str = "Finish or stop tonight's run first.";

/// A refusal: a code the sheet can match, and the sentence to show.
fn blocked(code: &str, sentence: impl Into<String>) -> OperationError {
    OperationError::new(code, sentence, true)
}

/// The run Start would create, or the first reason it cannot start. Nothing is written here.
pub fn build(input: &StartInput, env: &Env) -> Result<NightRun, OperationError> {
    if !env.enabled {
        return Err(blocked("NIGHT_SHIFT_OFF", LABS_OFF_SENTENCE));
    }
    if env.busy {
        return Err(blocked("RUN_ACTIVE", BUSY_SENTENCE));
    }
    if !env.undo_on {
        return Err(blocked(
            "MISSION_UNDO_OFF",
            "Missions need Undo to go back when a turn makes things worse. Undo is off for this folder.",
        ));
    }
    if input.tasks.is_empty() {
        return Err(blocked("NO_TASKS", "Add at least one task."));
    }
    if input.tasks.len() > MAX_TASKS {
        return Err(blocked(
            "TOO_MANY_TASKS",
            format!("Night Shift takes up to {MAX_TASKS} tasks."),
        ));
    }
    let plans = ticked_plans(input, env)?;
    let (start_at, stop_at) = times(input, env)?;
    if input.keep_awake && !env.awake_asked {
        return Err(blocked(
            "AWAKE_NOT_CONFIRMED",
            "Confirm that the computer may stay awake first.",
        ));
    }
    let mut tasks = Vec::new();
    for (index, task_input) in input.tasks.iter().enumerate() {
        tasks.push(task(index + 1, task_input, &plans, env)?);
    }
    Ok(NightRun {
        id: NightRun::new_id(),
        created_ms: env.now_ms,
        start_at_ms: start_at,
        stop_at_ms: stop_at,
        state: RunState::Scheduled { start_at_ms: start_at },
        plans,
        parallel: input.parallel.clamp(1, MAX_PARALLEL),
        keep_awake: input.keep_awake,
        awake_ok: false,
        tasks,
        plan_out: Vec::new(),
        blocked: Vec::new(),
        events: Vec::new(),
        last_tick_ms: env.now_ms,
        finished_ms: None,
        seen_ms: None,
        push_sent: false,
        rev: 0,
    })
}

/// The ticked plans, in the person's order. Each must be known and runnable, and no two may be
/// logins of one company.
fn ticked_plans(input: &StartInput, env: &Env) -> Result<Vec<PlanChoice>, OperationError> {
    let mut chosen: Vec<PlanChoice> = Vec::new();
    for key in &input.plans {
        if chosen.iter().any(|plan| plan_key(&plan.agent) == *key) {
            continue;
        }
        let known = env.plans.iter().find(|plan| plan.key == *key).ok_or_else(|| {
            blocked(
                "PLAN_UNKNOWN",
                "That plan is not on this computer. Pick it again under Plans.",
            )
        })?;
        if let Err(reason) = &known.runnable {
            return Err(blocked("PLAN_NOT_READY", reason.clone()));
        }
        if let Some(vendor) = &known.vendor {
            if chosen
                .iter()
                .any(|plan| plan.vendor.as_deref() == Some(vendor.as_str()))
            {
                return Err(blocked(
                    "PLAN_SAME_COMPANY",
                    "Puppet Master won't switch between two logins of one company. Untick one.",
                ));
            }
        }
        chosen.push(PlanChoice {
            agent: known.agent.clone(),
            label: known.label.clone(),
            vendor: known.vendor.clone(),
        });
    }
    if chosen.is_empty() {
        return Err(blocked("NO_PLAN", "Tick at least one plan to use."));
    }
    Ok(chosen)
}

/// The start and stop times (epoch ms). The stop is the first `stop_hhmm` at or after the start, and it
/// must come 30 minutes to 14 hours after the start.
fn times(input: &StartInput, env: &Env) -> Result<(u64, u64), OperationError> {
    let start = match &input.start_hhmm {
        None => env.now_ms,
        Some(hhmm) => at_local(env.now_ms, hhmm).ok_or_else(|| blocked("BAD_TIME", "Pick a time like 23:30."))?,
    };
    let stop = at_local(start, &input.stop_hhmm).ok_or_else(|| blocked("BAD_TIME", "Pick a time like 07:00."))?;
    let span = stop.saturating_sub(start);
    if span < MIN_RUN_MS {
        return Err(blocked(
            "STOP_TOO_SOON",
            "Pick a stop time at least 30 minutes after the start.",
        ));
    }
    if span > MAX_RUN_MS {
        return Err(blocked(
            "STOP_TOO_LATE",
            "Pick a stop time within 14 hours of the start.",
        ));
    }
    Ok((start, stop))
}

/// One task: its project, its words, the plan it may use, the checks it is held to, and its budget.
fn task(n: usize, input: &TaskInput, plans: &[PlanChoice], env: &Env) -> Result<Task, OperationError> {
    let raw_project = input.project.trim();
    if raw_project.is_empty() {
        return Err(blocked("NO_PROJECT", format!("Pick a project for task {n}.")));
    }
    let key = (env.key_of)(raw_project);
    if !(env.is_git)(&key) {
        return Err(blocked(
            "MISSION_NOT_GIT",
            format!("Task {n}: this project is not a git project with a commit. Night Shift works in a separate copy of it."),
        ));
    }
    let text = input.text.trim();
    if text.is_empty() {
        return Err(blocked(
            "TASK_TEXT_EMPTY",
            format!("Task {n} needs words: what should it do?"),
        ));
    }
    if text.chars().count() > MAX_TASK_TEXT {
        return Err(blocked(
            "TASK_TEXT_TOO_LONG",
            format!("Task {n} is longer than {MAX_TASK_TEXT} characters."),
        ));
    }
    if !(MIN_TURNS..=MAX_TURNS).contains(&input.turns) || !(MIN_MINUTES..=MAX_MINUTES).contains(&input.minutes) {
        return Err(blocked(
            "BUDGET_RANGE",
            format!("Task {n}: turns must be {MIN_TURNS} to {MAX_TURNS}, and minutes {MIN_MINUTES} to {MAX_MINUTES}."),
        ));
    }
    let who = match input.who.as_deref().map(str::trim) {
        None | Some("") | Some("any") => Who::Any,
        Some(wanted) => {
            let chosen = plans
                .iter()
                .find(|plan| plan_key(&plan.agent) == wanted)
                .ok_or_else(|| {
                    blocked(
                        "AGENT_NOT_TICKED",
                        format!("Task {n} uses a plan that is not ticked under Plans."),
                    )
                })?;
            Who::Fixed {
                agent: chosen.agent.clone(),
            }
        }
    };
    Ok(Task {
        spec: TaskSpec {
            id: NightRun::task_id(),
            project: key.clone(),
            text: text.to_string(),
            who,
            may_change_tests: input.may_change_tests,
            turns: input.turns,
            minutes: input.minutes,
        },
        checks: (env.declared)(&key),
        state: TaskState::Queued,
        attempts: Vec::new(),
        outcome: None,
        review: Default::default(),
    })
}

/// The first local `HH:MM` at or after `from_ms` (epoch ms). `None` when the text is not a time.
pub fn at_local(from_ms: u64, hhmm: &str) -> Option<u64> {
    if !valid_hhmm(hhmm) {
        return None;
    }
    let time = NaiveTime::parse_from_str(hhmm, "%H:%M").ok()?;
    let from = Local.timestamp_millis_opt(i64::try_from(from_ms).ok()?).single()?;
    let mut day = from.date_naive();
    for _ in 0..2 {
        let at = day
            .and_time(time)
            .and_local_timezone(Local)
            .earliest()?
            .timestamp_millis();
        if at >= from.timestamp_millis() {
            return u64::try_from(at).ok();
        }
        day = day.succ_opt()?;
    }
    None
}

/// Every plan on this computer the sheet can tick: each installed agent once, and once per saved login
/// of it. `folders` are the projects the plans must run in; the agent's folder check runs for each.
pub fn known_plans(folders: &[String]) -> Vec<Known> {
    let accounts = crate::accounts::Store::default_location().list();
    let mut out = Vec::new();
    for agent in crate::tui::descriptors()
        .into_iter()
        .filter(|d| d.installed && d.dispatchable && d.kind == crate::tui::Kind::Cli.as_str())
    {
        let vendor = crate::tui::profile_for(agent.id)
            .limits()
            .filter(|limits| !limits.vendor_varies())
            .map(|limits| limits.vendor().to_string());
        let logins = std::iter::once(None).chain(accounts.iter().filter(|a| a.agent_id == agent.id).map(Some));
        for login in logins {
            let account = login.map(|saved| saved.id.clone());
            let choice = AgentChoice {
                agent_type: agent.id.to_string(),
                account: account.clone(),
                model: None,
            };
            let label = match login {
                None => agent.label.clone(),
                Some(saved) => format!("{} · {}", agent.label, saved.label),
            };
            out.push(Known {
                key: plan_key(&choice),
                runnable: runnable_in(&choice, folders),
                limited_until: crate::handoff::agent_limit(agent.id, account.as_deref()),
                agent: choice,
                label,
                vendor: vendor.clone(),
            });
        }
    }
    out
}

fn runnable_in(agent: &AgentChoice, folders: &[String]) -> Result<(), String> {
    for folder in folders {
        crate::companion::agents::agent_ok(&agent.agent_type, folder).map_err(str::to_string)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::night_shift::model::validate_ids;

    /// A fixed instant (epoch ms). The local clock decides the `HH:MM` text the tests build from it.
    const NOW: u64 = 1_800_000_000_000;

    fn ident(text: &str) -> String {
        text.trim().to_string()
    }

    fn is_git(key: &str) -> bool {
        key != "/plain"
    }

    fn declared(_key: &str) -> Vec<Vec<String>> {
        vec![vec!["npm".to_string(), "test".to_string()]]
    }

    fn known(
        key: &str,
        agent: &str,
        account: Option<&str>,
        vendor: Option<&str>,
        runnable: Result<(), String>,
    ) -> Known {
        Known {
            key: key.into(),
            agent: AgentChoice {
                agent_type: agent.into(),
                account: account.map(str::to_string),
                model: None,
            },
            label: key.into(),
            vendor: vendor.map(str::to_string),
            runnable,
            limited_until: None,
        }
    }

    fn plans() -> Vec<Known> {
        vec![
            known("fake_a", "fake_a", None, Some("vendor_a"), Ok(())),
            known("fake_b", "fake_b", None, Some("vendor_b"), Ok(())),
            known("fake_a@work", "fake_a", Some("work"), Some("vendor_a"), Ok(())),
            known("fake_c", "fake_c", None, None, Err("Fake C isn't installed.".into())),
        ]
    }

    fn env(plans: &[Known]) -> Env<'_> {
        Env {
            now_ms: NOW,
            enabled: true,
            undo_on: true,
            awake_asked: true,
            busy: false,
            plans,
            key_of: &ident,
            is_git: &is_git,
            declared: &declared,
        }
    }

    /// `HH:MM` on the local clock, `hours` after the fixed instant.
    fn hhmm_after(hours: f64) -> String {
        let ms = NOW + (hours * 3_600_000.0) as u64;
        chrono::Local
            .timestamp_millis_opt(ms as i64)
            .single()
            .expect("local time")
            .format("%H:%M")
            .to_string()
    }

    fn task_input(project: &str, text: &str) -> TaskInput {
        TaskInput {
            project: project.into(),
            text: text.into(),
            who: None,
            may_change_tests: false,
            turns: DEFAULT_TURNS,
            minutes: DEFAULT_MINUTES,
        }
    }

    fn input() -> StartInput {
        StartInput {
            tasks: vec![task_input("/work/app", "Fix the cart total rounding.")],
            plans: vec!["fake_a".into(), "fake_b".into()],
            stop_hhmm: hhmm_after(3.0),
            start_hhmm: None,
            parallel: 2,
            keep_awake: false,
        }
    }

    /// The code of the refusal, and its sentence.
    fn refusal(result: Result<NightRun, OperationError>) -> (String, String) {
        let error = result.expect_err("should be refused");
        (error.code, error.message)
    }

    #[test]
    fn a_valid_start_builds_a_scheduled_run_with_snapshots() {
        let p = plans();
        let run = build(&input(), &env(&p)).expect("valid");
        validate_ids(&run).expect("ids");
        assert_eq!(run.tasks.len(), 1);
        assert_eq!(run.plans.len(), 2);
        assert_eq!(run.plans[0].label, "fake_a");
        assert_eq!(run.parallel, 2);
        assert!(matches!(run.state, RunState::Scheduled { start_at_ms } if start_at_ms == NOW));
        assert!(run.stop_at_ms - run.start_at_ms >= MIN_RUN_MS && run.stop_at_ms - run.start_at_ms <= MAX_RUN_MS);
        let task = &run.tasks[0];
        assert_eq!(task.spec.project, "/work/app");
        assert_eq!(task.spec.who, Who::Any);
        assert_eq!(
            task.checks,
            vec![vec!["npm".to_string(), "test".to_string()]],
            "the declared checks are snapshotted"
        );
        assert_eq!(task.state, TaskState::Queued);
    }

    #[test]
    fn labs_off_refuses_with_the_settings_sentence() {
        let p = plans();
        let mut e = env(&p);
        e.enabled = false;
        let (code, sentence) = refusal(build(&input(), &e));
        assert_eq!(code, "NIGHT_SHIFT_OFF");
        assert_eq!(sentence, LABS_OFF_SENTENCE);
    }

    #[test]
    fn an_unfinished_run_refuses() {
        let p = plans();
        let mut e = env(&p);
        e.busy = true;
        assert_eq!(
            refusal(build(&input(), &e)),
            ("RUN_ACTIVE".into(), BUSY_SENTENCE.into())
        );
    }

    #[test]
    fn undo_off_refuses_with_the_missions_sentence() {
        let p = plans();
        let mut e = env(&p);
        e.undo_on = false;
        let (code, sentence) = refusal(build(&input(), &e));
        assert_eq!(code, "MISSION_UNDO_OFF");
        assert!(sentence.starts_with("Missions need Undo to go back"));
    }

    #[test]
    fn the_task_count_is_between_one_and_ten() {
        let p = plans();
        let mut none = input();
        none.tasks.clear();
        assert_eq!(refusal(build(&none, &env(&p))).0, "NO_TASKS");
        let mut many = input();
        many.tasks = (0..=MAX_TASKS)
            .map(|i| task_input("/work/app", &format!("Task {i}")))
            .collect();
        assert_eq!(refusal(build(&many, &env(&p))).0, "TOO_MANY_TASKS");
    }

    #[test]
    fn plans_must_be_ticked_known_runnable_and_one_per_company() {
        let p = plans();
        let mut none = input();
        none.plans.clear();
        assert_eq!(refusal(build(&none, &env(&p))).0, "NO_PLAN");
        let mut unknown = input();
        unknown.plans = vec!["fake_zzz".into()];
        assert_eq!(refusal(build(&unknown, &env(&p))).0, "PLAN_UNKNOWN");
        let mut not_ready = input();
        not_ready.plans = vec!["fake_c".into()];
        assert_eq!(
            refusal(build(&not_ready, &env(&p))),
            ("PLAN_NOT_READY".into(), "Fake C isn't installed.".into())
        );
        let mut two_logins = input();
        two_logins.plans = vec!["fake_a".into(), "fake_a@work".into()];
        let (code, sentence) = refusal(build(&two_logins, &env(&p)));
        assert_eq!(code, "PLAN_SAME_COMPANY");
        assert_eq!(
            sentence,
            "Puppet Master won't switch between two logins of one company. Untick one."
        );
    }

    #[test]
    fn the_stop_time_must_be_30_minutes_to_14_hours_away() {
        let p = plans();
        let mut soon = input();
        soon.stop_hhmm = hhmm_after(0.1);
        assert_eq!(refusal(build(&soon, &env(&p))).0, "STOP_TOO_SOON");
        let mut late = input();
        late.stop_hhmm = hhmm_after(15.0);
        assert_eq!(refusal(build(&late, &env(&p))).0, "STOP_TOO_LATE");
    }

    #[test]
    fn a_bad_clock_text_is_refused() {
        let p = plans();
        let mut bad_stop = input();
        bad_stop.stop_hhmm = "7am".into();
        assert_eq!(refusal(build(&bad_stop, &env(&p))).0, "BAD_TIME");
        let mut bad_start = input();
        bad_start.start_hhmm = Some("25:00".into());
        assert_eq!(refusal(build(&bad_start, &env(&p))).0, "BAD_TIME");
    }

    #[test]
    fn keep_awake_needs_the_one_time_confirm() {
        let p = plans();
        let mut e = env(&p);
        e.awake_asked = false;
        let mut awake = input();
        awake.keep_awake = true;
        assert_eq!(refusal(build(&awake, &e)).0, "AWAKE_NOT_CONFIRMED");
        awake.keep_awake = false;
        assert!(
            build(&awake, &e).is_ok(),
            "without keep awake the confirm is not needed"
        );
    }

    #[test]
    fn a_task_needs_a_git_project_a_name_and_words() {
        let p = plans();
        let mut no_project = input();
        no_project.tasks[0].project = "   ".into();
        assert_eq!(refusal(build(&no_project, &env(&p))).0, "NO_PROJECT");
        let mut plain = input();
        plain.tasks[0].project = "/plain".into();
        let (code, sentence) = refusal(build(&plain, &env(&p)));
        assert_eq!(code, "MISSION_NOT_GIT");
        assert!(sentence.starts_with("Task 1: this project is not a git project with a commit."));
        let mut empty = input();
        empty.tasks[0].text = "  ".into();
        assert_eq!(refusal(build(&empty, &env(&p))).0, "TASK_TEXT_EMPTY");
        let mut long = input();
        long.tasks[0].text = "x".repeat(MAX_TASK_TEXT + 1);
        assert_eq!(refusal(build(&long, &env(&p))).0, "TASK_TEXT_TOO_LONG");
    }

    #[test]
    fn turns_and_minutes_stay_in_range() {
        let p = plans();
        let mut few_turns = input();
        few_turns.tasks[0].turns = 2;
        assert_eq!(refusal(build(&few_turns, &env(&p))).0, "BUDGET_RANGE");
        let mut long = input();
        long.tasks[0].minutes = 300;
        assert_eq!(refusal(build(&long, &env(&p))).0, "BUDGET_RANGE");
    }

    #[test]
    fn a_task_can_name_one_ticked_plan_only() {
        let p = plans();
        let mut not_ticked = input();
        not_ticked.tasks[0].who = Some("fake_c".into());
        assert_eq!(refusal(build(&not_ticked, &env(&p))).0, "AGENT_NOT_TICKED");
        let mut fixed = input();
        fixed.tasks[0].who = Some("fake_b".into());
        let run = build(&fixed, &env(&p)).expect("ticked");
        assert!(matches!(&run.tasks[0].spec.who, Who::Fixed { agent } if agent.agent_type == "fake_b"));
    }

    #[test]
    fn parallel_is_kept_between_one_and_two() {
        let p = plans();
        let mut many = input();
        many.parallel = 9;
        assert_eq!(build(&many, &env(&p)).expect("ok").parallel, MAX_PARALLEL);
        let mut none = input();
        none.parallel = 0;
        assert_eq!(build(&none, &env(&p)).expect("ok").parallel, 1);
    }

    #[test]
    fn a_later_start_is_scheduled_for_that_time() {
        let p = plans();
        let mut later = input();
        later.start_hhmm = Some(hhmm_after(1.0));
        let run = build(&later, &env(&p)).expect("ok");
        let RunState::Scheduled { start_at_ms } = run.state else {
            panic!("not scheduled: {:?}", run.state)
        };
        assert!(
            start_at_ms >= NOW && start_at_ms <= NOW + 2 * 3_600_000,
            "about one hour later"
        );
        assert_eq!(run.start_at_ms, start_at_ms);
    }

    #[test]
    fn each_task_gets_its_own_id() {
        let p = plans();
        let mut two = input();
        two.tasks.push(task_input("/work/app", "Second task."));
        let run = build(&two, &env(&p)).expect("ok");
        assert_ne!(run.tasks[0].spec.id, run.tasks[1].spec.id);
        assert!(run
            .tasks
            .iter()
            .all(|t| crate::night_shift::model::valid_task_id(&t.spec.id)));
    }

    #[test]
    fn at_local_finds_the_next_time_of_day() {
        let at = at_local(NOW, "00:00").expect("time");
        assert!(at >= NOW && at < NOW + 25 * 3_600_000);
        assert!(at_local(NOW, "24:00").is_none());
        assert!(at_local(NOW, "7am").is_none());
    }
}
