//! Night Shift: the start side of the live adapter (SPEC 3.5, NS-7). Pure: it builds what a task
//! starts with and reads the plan candidates from values the caller passes in. The live wrapper
//! (NS-10) calls `missions::start` or `agent_runs::run_agent` with what is built here.
//!
//! Safety (SPEC 4, S1): neither request carries `trust_workspace`, `allow_shell` or
//! `allowed_commands`. A gate or question is never answered on the way in.

use serde_json::{json, Value};

use super::model::{plan_key, NightRun, PlanOut, Task, CHECK_TIMEOUT_MS, ROUND_TIMEOUT_MS};
use super::plan::Cand;
use crate::accounts::Account;
use crate::agent_runs::checks::Check;
use crate::missions::model::{AgentChoice, MissionCheck, Starter};
use crate::missions::routes::name_for;
use crate::missions::StartRequest;
use crate::operations::OperationError;
use crate::tui::Descriptor;

/// Why a task could not start right now (SPEC 3.5). `NoSlot` is retried later, not a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartFail {
    NoSlot,
    Refused { code: String, sentence: String },
}

/// `MISSION_LIMIT` means the app-wide mission slots are full: retry later. Any other refusal ends
/// the task with the plain sentence the mission layer wrote.
pub fn start_fail_from(err: OperationError) -> StartFail {
    if err.code == "MISSION_LIMIT" {
        StartFail::NoSlot
    } else {
        StartFail::Refused { code: err.code, sentence: err.message }
    }
}

/// The key that makes a retried start return the same mission (`night:<run>:<task>:<n>`).
fn idempotency_key(run: &NightRun, task: &Task, n: u8) -> String {
    format!("night:{}:{}:{}", run.id, task.spec.id, n)
}

/// The mission request for a task with declared checks. One agent, quiet, rollback on, the test
/// guard as the person set it, and the checks exactly as snapshotted at Start.
pub fn mission_request(run: &NightRun, task: &Task, plan: &AgentChoice, n: u8, undo_on: bool) -> StartRequest {
    let checks = task
        .checks
        .iter()
        .map(|argv| MissionCheck {
            name: name_for(argv),
            check: Check::CommandExits { command: argv.clone(), expect_exit: 0, timeout_ms: Some(CHECK_TIMEOUT_MS) },
        })
        .collect();
    let mut req = StartRequest::new(&task.spec.project, &task.spec.text, checks, vec![plan.clone()]);
    req.may_change_tests = Some(task.spec.may_change_tests);
    req.budget_turns = Some(task.spec.turns);
    req.budget_minutes = Some(task.spec.minutes);
    req.round_timeout_ms = ROUND_TIMEOUT_MS;
    req.check_timeout_ms = CHECK_TIMEOUT_MS;
    req.undo_on = undo_on;
    req.started_by = Starter::Night { run: run.id.clone() };
    req.idempotency_key = Some(idempotency_key(run, task, n));
    req.quiet = true;
    req
}

/// The single-turn request for a task with no declared checks: a separate copy, a visible pane kept
/// open, no waiting. Built as JSON for `AgentRunRequest`. Only the fields listed here are set.
pub fn single_request(run: &NightRun, task: &Task, plan: &AgentChoice, n: u8) -> Value {
    let mut req = json!({
        "project_path": task.spec.project,
        "task": task.spec.text,
        "agent_type": plan.agent_type,
        "headless": false,
        "background": true,
        "keep_pane": true,
        "isolation": "worktree",
        "wait_ms": 0,
        "timeout_ms": ROUND_TIMEOUT_MS,
        "budget_turns": task.spec.turns,
        "budget_minutes": task.spec.minutes,
        "idempotency_key": idempotency_key(run, task, n),
    });
    if let Some(account) = &plan.account {
        req["account"] = json!(account);
    }
    if let Some(model) = &plan.model {
        req["model"] = json!(model);
    }
    req
}

/// The plans the run ticked, in the person's order, as the scheduler sees them now.
///
/// `descriptors` (installed agents) and `accounts` (saved logins) say whether a plan can run at all.
/// `limit_of` is the live limit for a plan (`handoff::agent_limit`). A plan's own `plan_out` note
/// from this run is used when `limit_of` has nothing. `agent_ok` is the folder check for the
/// agent, given by the caller; its `Err` text is the plain reason.
pub fn candidates(
    run: &NightRun,
    descriptors: &[Descriptor],
    accounts: &[Account],
    limit_of: &dyn Fn(&AgentChoice) -> Option<Option<u64>>,
    agent_ok: &dyn Fn(&AgentChoice) -> Result<(), String>,
) -> Vec<Cand> {
    run.plans
        .iter()
        .map(|choice| {
            let key = plan_key(&choice.agent);
            let limited_until = limit_of(&choice.agent).or_else(|| {
                run.plan_out.iter().find(|p: &&PlanOut| p.plan_key == key).map(|p| p.until_ms)
            });
            Cand {
                agent: choice.agent.clone(),
                label: choice.label.clone(),
                vendor: choice.vendor.clone(),
                runnable: runnable(&choice.agent, descriptors, accounts, agent_ok),
                limited_until,
            }
        })
        .collect()
}

/// `Ok` when the plan can run on this computer; otherwise the plain reason.
fn runnable(
    agent: &AgentChoice,
    descriptors: &[Descriptor],
    accounts: &[Account],
    agent_ok: &dyn Fn(&AgentChoice) -> Result<(), String>,
) -> Result<(), String> {
    let Some(known) = descriptors.iter().find(|d| d.id == agent.agent_type) else {
        return Err("Puppet Master doesn't know this agent.".to_string());
    };
    if !known.installed {
        return Err(format!("{} isn't installed.", known.label));
    }
    if let Some(account) = agent.account.as_deref() {
        let saved = accounts.iter().any(|a| a.id == account && a.agent_id == agent.agent_type);
        if !saved {
            return Err(format!("{} has no saved login for that account.", known.label));
        }
    }
    agent_ok(agent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::night_shift::model::{sample_run, sample_task, PlanChoice, DEFAULT_MINUTES, DEFAULT_TURNS};
    use crate::tui::{FeatureFlags, Modes, RunOptionSupport, StandbySupport};

    const RUN: &str = "ns-0123456789ab";
    const TASK: &str = "nt-00000001";

    fn agent(name: &str, account: Option<&str>) -> AgentChoice {
        AgentChoice { agent_type: name.to_string(), account: account.map(str::to_string), model: None }
    }

    fn desc(id: &'static str, label: &str, installed: bool) -> Descriptor {
        Descriptor {
            id,
            label: label.to_string(),
            icon: None,
            kind: "cli",
            source: "code",
            experimental: false,
            dispatchable: true,
            not_dispatchable_reason: None,
            installed,
            install_hint: None,
            install_fallback: None,
            login_hint: None,
            modes: Modes { headless: true, visible: true },
            features: FeatureFlags { read_only_headless: false, read_only_spawned_pane: false, resumable_headless: false },
            run_options: RunOptionSupport::default(),
            standby: StandbySupport::default(),
            definition: None,
        }
    }

    fn saved_login(id: &str, agent_id: &str) -> Account {
        Account { id: id.into(), agent_id: agent_id.into(), label: "Work".into(), home: "C:\\Users\\you\\acct".into() }
    }

    fn plan(agent: AgentChoice, label: &str, vendor: Option<&str>) -> PlanChoice {
        PlanChoice { agent, label: label.to_string(), vendor: vendor.map(str::to_string) }
    }

    fn fixture() -> (NightRun, Task) {
        let mut run = sample_run(RUN);
        let mut task = sample_task(TASK);
        task.checks = vec![vec!["npm".to_string(), "test".to_string()]];
        task.spec.may_change_tests = true;
        task.spec.turns = 7;
        task.spec.minutes = 30;
        run.tasks = vec![task.clone()];
        (run, task)
    }

    #[test]
    fn mission_request_is_quiet_and_carries_the_task() {
        let (run, task) = fixture();
        let plan = agent("fake_a", None);
        let req = mission_request(&run, &task, &plan, 1, true);
        assert!(req.quiet);
        assert!(req.rollback);
        assert!(req.undo_on);
        assert_eq!(req.may_change_tests, Some(true));
        assert_eq!(req.budget_turns, Some(7));
        assert_eq!(req.budget_minutes, Some(30));
        assert_eq!(req.round_timeout_ms, ROUND_TIMEOUT_MS);
        assert_eq!(req.check_timeout_ms, CHECK_TIMEOUT_MS);
        assert_eq!(req.project, task.spec.project);
        assert_eq!(req.objective, task.spec.text);
        assert_eq!(req.agents, vec![plan]);
        assert_eq!(req.started_by, Starter::Night { run: RUN.to_string() });
        assert_eq!(req.idempotency_key.as_deref(), Some("night:ns-0123456789ab:nt-00000001:1"));
    }

    #[test]
    fn mission_request_uses_the_snapshot_checks_exactly() {
        let (run, task) = fixture();
        let req = mission_request(&run, &task, &agent("fake_a", None), 1, true);
        assert_eq!(req.checks.len(), 1);
        assert_eq!(req.checks[0].name, "Tests");
        assert_eq!(req.checks[0].argv(), Some(task.checks[0].as_slice()));
        match &req.checks[0].check {
            Check::CommandExits { expect_exit, timeout_ms, .. } => {
                assert_eq!(*expect_exit, 0);
                assert_eq!(*timeout_ms, Some(CHECK_TIMEOUT_MS));
            }
            other => panic!("expected a command check, got {other:?}"),
        }
    }

    #[test]
    fn mission_request_keeps_the_undo_setting_and_the_test_guard() {
        let (run, mut task) = fixture();
        task.spec.may_change_tests = false;
        let req = mission_request(&run, &task, &agent("fake_a", None), 2, false);
        assert!(!req.undo_on);
        assert_eq!(req.may_change_tests, Some(false));
        assert_eq!(req.idempotency_key.as_deref(), Some("night:ns-0123456789ab:nt-00000001:2"));
    }

    #[test]
    fn single_request_is_a_background_worktree_turn_with_budget_and_key() {
        let (run, task) = fixture();
        let v = single_request(&run, &task, &agent("fake_a", Some("work")), 2);
        assert_eq!(v["isolation"], "worktree");
        assert_eq!(v["background"], true);
        assert_eq!(v["headless"], false);
        assert_eq!(v["keep_pane"], true);
        assert_eq!(v["wait_ms"], 0);
        assert_eq!(v["agent_type"], "fake_a");
        assert_eq!(v["account"], "work");
        assert_eq!(v["project_path"], task.spec.project);
        assert_eq!(v["task"], task.spec.text);
        assert_eq!(v["budget_turns"], 7);
        assert_eq!(v["budget_minutes"], 30);
        assert_eq!(v["idempotency_key"], "night:ns-0123456789ab:nt-00000001:2");
    }

    #[test]
    fn single_request_never_carries_trust_shell_or_command_fields() {
        let (run, _) = fixture();
        let mut task = sample_task(TASK);
        task.spec.turns = DEFAULT_TURNS;
        task.spec.minutes = DEFAULT_MINUTES;
        let text = single_request(&run, &task, &agent("fake_a", None), 1).to_string();
        for banned in ["trust_workspace", "allow_shell", "allowed_commands"] {
            assert!(!text.contains(banned), "{banned} must not be in a night request");
        }
        assert!(single_request(&run, &task, &agent("fake_a", None), 1).get("account").is_none());
    }

    #[test]
    fn candidates_follow_the_persons_plan_order_and_carry_limits() {
        let (mut run, _) = fixture();
        run.plans = vec![
            plan(agent("fake_b", None), "Fake B", Some("beta")),
            plan(agent("fake_a", Some("work")), "Fake A · Work", Some("acme")),
            plan(agent("fake_a", None), "Fake A", Some("acme")),
        ];
        run.plan_out = vec![PlanOut { plan_key: "fake_a".into(), until_ms: Some(5_000), since_ms: 1 }];
        let descriptors = vec![desc("fake_a", "Fake A", true), desc("fake_b", "Fake B", true)];
        let accounts = vec![saved_login("work", "fake_a")];
        let limit_of = |a: &AgentChoice| if plan_key(a) == "fake_b" { Some(Some(9_000)) } else { None };
        let cands = candidates(&run, &descriptors, &accounts, &limit_of, &|_| Ok(()));
        let labels: Vec<&str> = cands.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Fake B", "Fake A · Work", "Fake A"]);
        assert_eq!(cands[0].limited_until, Some(Some(9_000)));
        assert_eq!(cands[1].limited_until, None);
        assert_eq!(cands[2].limited_until, Some(Some(5_000)));
        assert_eq!(cands[0].vendor.as_deref(), Some("beta"));
        assert!(cands.iter().all(|c| c.runnable.is_ok()));
    }

    #[test]
    fn candidates_say_plainly_why_a_plan_cannot_run() {
        let (mut run, _) = fixture();
        run.plans = vec![
            plan(agent("fake_x", None), "Fake X", None),
            plan(agent("fake_c", None), "Fake C", None),
            plan(agent("fake_a", Some("gone")), "Fake A · Gone", None),
            plan(agent("fake_a", None), "Fake A", None),
            plan(agent("fake_b", None), "Fake B", None),
        ];
        let descriptors = vec![desc("fake_a", "Fake A", true), desc("fake_b", "Fake B", true), desc("fake_c", "Fake C", false)];
        let accounts = vec![saved_login("work", "fake_a")];
        let agent_ok = |a: &AgentChoice| {
            if a.agent_type == "fake_a" {
                Err("Needs approval first.".to_string())
            } else {
                Ok(())
            }
        };
        let cands = candidates(&run, &descriptors, &accounts, &|_| None, &agent_ok);
        assert_eq!(cands[0].runnable, Err("Puppet Master doesn't know this agent.".to_string()));
        assert_eq!(cands[1].runnable, Err("Fake C isn't installed.".to_string()));
        assert_eq!(cands[2].runnable, Err("Fake A has no saved login for that account.".to_string()));
        assert_eq!(cands[3].runnable, Err("Needs approval first.".to_string()));
        assert_eq!(cands[4].runnable, Ok(()));
    }

    #[test]
    fn mission_limit_is_no_slot_and_other_refusals_keep_their_words() {
        assert_eq!(start_fail_from(OperationError::new("MISSION_LIMIT", "Two missions are already running.", false)), StartFail::NoSlot);
        assert_eq!(
            start_fail_from(OperationError::new("MISSION_NOT_GIT", "Missions need a git project.", false)),
            StartFail::Refused { code: "MISSION_NOT_GIT".into(), sentence: "Missions need a git project.".into() }
        );
    }
}
