//! The check sequence against recorded screens, and the whole version-bump flow with the machine
//! faked. Screens are the profiles' own live captures (`tui/*/fixtures`).

use super::engine::{Deps, Engine, Source};
use super::model::*;
use super::run::{self, CanaryIo, Timing};
use super::strict;
use crate::tui::profile_for;
use std::sync::Arc;
use std::time::Duration;

const CODEX_UPDATE_MENU: &str = include_str!("../tui/codex/fixtures/update_prompt_live.txt");
const CODEX_READY: &str = include_str!("../tui/codex/fixtures/standby_ready_live.txt");
const CLAUDE_TRUST: &str = include_str!("../tui/claude/fixtures/trust_dialog_2_1_299.txt");
const UNRECOGNISED: &str = "  Codex 0.171\n\n  Pick a colour theme to continue\n    a) light     b) dark\n";
const HOST_SHELL: &str = "PS D:\\pm-scratch-canary\\codex>";

// ---- a fake CLI on virtual time ----------------------------------------------------------

#[derive(Clone)]
enum When {
    At(Duration),
    /// This long after the nth key was written (1-based).
    AfterKey(usize, Duration),
}

struct FakeIo {
    now: Duration,
    timeline: Vec<(When, String)>,
    key_times: Vec<Duration>,
    written: Vec<String>,
    dies_at: Option<Duration>,
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

impl FakeIo {
    fn new(timeline: Vec<(When, &str)>) -> Self {
        Self {
            now: Duration::ZERO,
            timeline: timeline.into_iter().map(|(w, s)| (w, s.to_string())).collect(),
            key_times: Vec::new(),
            written: Vec::new(),
            dies_at: None,
        }
    }
}

impl CanaryIo for FakeIo {
    fn screen(&mut self) -> String {
        let mut shown = String::new();
        for (when, screen) in &self.timeline {
            let due = match when {
                When::At(at) => self.now >= *at,
                When::AfterKey(n, after) => self.key_times.get(n - 1).is_some_and(|t| self.now >= *t + *after),
            };
            if due {
                shown = screen.clone();
            }
        }
        shown
    }
    fn write(&mut self, keys: &str) -> Result<(), String> {
        self.key_times.push(self.now);
        self.written.push(keys.to_string());
        Ok(())
    }
    fn alive(&mut self) -> bool {
        self.dies_at.is_none_or(|at| self.now < at)
    }
    fn pause(&mut self, duration: Duration) {
        self.now += duration;
    }
    fn elapsed(&mut self) -> Duration {
        self.now
    }
}

fn drive(io: &mut FakeIo, agent: &str) -> run::Drive {
    run::drive(io, profile_for(agent).as_ref(), &Timing::default())
}

// ---- the sequence --------------------------------------------------------------------------

#[test]
fn codex_update_menu_is_skipped_with_its_own_key_and_the_input_box_is_reached() {
    let mut io = FakeIo::new(vec![
        (When::At(ms(0)), ""),
        (When::At(ms(900)), CODEX_UPDATE_MENU),
        (When::AfterKey(1, ms(400)), CODEX_READY),
    ]);
    let result = drive(&mut io, "codex");
    assert_eq!(result.outcome, Outcome::Ready, "{result:?}");
    assert_eq!(result.dismissed, vec!["update_available"]);
    // Exactly the profile's skip key, nothing else: no text, no Enter, no prompt.
    let skip = profile_for("codex").startup_gate(CODEX_UPDATE_MENU).unwrap().dismiss.unwrap();
    assert_eq!(io.written, vec![skip.to_string()]);
    assert_eq!(result.keys_sent.len(), 1);
}

#[test]
fn a_menu_that_draws_after_the_first_paint_is_still_met_before_ready_is_declared() {
    // Live, Codex 0.159.3: the composer was up, and the whole update menu drew over it a moment later.
    let mut io = FakeIo::new(vec![
        (When::At(ms(300)), CODEX_READY),
        (When::At(ms(2000)), CODEX_UPDATE_MENU),
        (When::AfterKey(1, ms(300)), CODEX_READY),
    ]);
    let result = drive(&mut io, "codex");
    assert_eq!(result.outcome, Outcome::Ready, "{result:?}");
    assert_eq!(result.dismissed, vec!["update_available"], "the late menu was closed, not typed over");
}

#[test]
fn a_clean_start_reaches_ready_and_sends_nothing_at_all() {
    let mut io = FakeIo::new(vec![(When::At(ms(600)), CODEX_READY)]);
    let result = drive(&mut io, "codex");
    assert_eq!(result.outcome, Outcome::Ready);
    assert!(io.written.is_empty(), "nothing is written to a CLI that needs nothing: {:?}", io.written);
    assert!(result.dismissed.is_empty());
}

#[test]
fn a_question_only_the_person_may_answer_is_recognised_and_left_alone() {
    let mut io = FakeIo::new(vec![(When::At(ms(500)), CLAUDE_TRUST)]);
    let result = drive(&mut io, "claude");
    assert_eq!(result.outcome, Outcome::AsksYou);
    assert_eq!(result.gate, Some("workspace_trust"));
    assert!(io.written.is_empty(), "a trust question is never answered: {:?}", io.written);
    assert!(result.outcome.passed() && !result.outcome.changed_screens());
}

#[test]
fn a_screen_the_profile_cannot_read_is_a_failure_after_it_holds_still() {
    let mut io = FakeIo::new(vec![(When::At(ms(500)), UNRECOGNISED)]);
    let result = drive(&mut io, "codex");
    assert_eq!(result.outcome, Outcome::Unrecognised);
    assert!(io.written.is_empty(), "nothing is pressed on a screen nobody recognises");
    assert!(result.elapsed >= Timing::default().stuck_after);
    assert!(result.outcome.changed_screens());
}

#[test]
fn a_gate_that_ignores_its_keys_is_a_failure_not_a_loop() {
    let mut io = FakeIo::new(vec![(When::At(ms(500)), CODEX_UPDATE_MENU)]);
    let result = drive(&mut io, "codex");
    assert_eq!(result.outcome, Outcome::Unrecognised);
    assert_eq!(result.gate, Some("update_available"));
    assert_eq!(io.written.len(), 1, "the keys are pressed once, not hammered");
}

#[test]
fn a_cli_that_keeps_showing_new_gates_stops_after_the_limit() {
    // Each key reveals the menu again.
    let mut io = FakeIo::new(vec![
        (When::At(ms(500)), CODEX_UPDATE_MENU),
        (When::AfterKey(1, ms(300)), "A different screen\nsaying nothing\n"),
        (When::AfterKey(1, ms(2500)), CODEX_UPDATE_MENU),
        (When::AfterKey(2, ms(300)), "A different screen\nsaying nothing\n"),
        (When::AfterKey(2, ms(2500)), CODEX_UPDATE_MENU),
        (When::AfterKey(3, ms(300)), "Another screen\n"),
        (When::AfterKey(3, ms(2500)), CODEX_UPDATE_MENU),
        (When::AfterKey(4, ms(300)), "Yet another\n"),
        (When::AfterKey(4, ms(2500)), CODEX_UPDATE_MENU),
    ]);
    let result = drive(&mut io, "codex");
    assert_eq!(result.outcome, Outcome::Unrecognised, "{result:?}");
    assert_eq!(io.written.len(), 4, "at most four rounds");
}

#[test]
fn nothing_drawn_is_not_proof_of_a_change() {
    let mut io = FakeIo::new(vec![(When::At(ms(0)), "")]);
    let result = drive(&mut io, "codex");
    assert_eq!(result.outcome, Outcome::NeverDrew);
    assert!(!result.outcome.changed_screens(), "a slow start must not put an agent in strict mode");
}

#[test]
fn a_cli_that_quits_by_itself_is_reported_as_such() {
    let mut io = FakeIo::new(vec![(When::At(ms(100)), "Booting...\n")]);
    io.dies_at = Some(ms(800));
    assert_eq!(drive(&mut io, "codex").outcome, Outcome::Exited);
}

#[test]
fn a_fallback_to_the_host_shell_is_the_cli_having_quit() {
    let mut io = FakeIo::new(vec![(When::At(ms(100)), HOST_SHELL)]);
    assert_eq!(drive(&mut io, "codex").outcome, Outcome::Exited);
}

#[test]
fn an_agent_with_no_readable_ready_screen_cannot_be_judged() {
    assert!(run::checkable(profile_for("codex").as_ref()));
    assert!(run::checkable(profile_for("claude").as_ref()));
    assert!(!run::checkable(profile_for("not_a_registered_agent").as_ref()));
}

#[test]
fn the_stored_result_is_redacted_and_records_no_prompt() {
    let secret = "  token: sk-live-abcdefghijklmnopqrstuvwxyz\n";
    let drive = run::Drive {
        outcome: Outcome::Unrecognised,
        gate: None,
        dismissed: vec![],
        keys_sent: vec![],
        screens: vec![("after 3s".into(), format!("{UNRECOGNISED}{secret}"))],
        elapsed: Duration::from_secs(12),
    };
    let profile = profile_for("codex");
    let result = run::into_result(
        drive,
        "codex",
        "Codex",
        Some("0.171.0".into()),
        5,
        profile.as_ref(),
        &super::captures::redact_screen,
    );
    assert_eq!(result.prompts_sent, 0);
    assert!(result.keys_sent.is_empty());
    assert!(!result.screens[0].screen.contains("sk-live"), "{}", result.screens[0].screen);
    assert!(result.screens[0].screen.contains("[REDACTED]"));
    assert_eq!(result.screens[0].label, "after 3s");
}

// ---- a faked version bump, end to end ---------------------------------------------------------

struct Machine {
    version: Arc<parking_lot::Mutex<String>>,
    /// What the fake CLI shows after the bump.
    after_bump: Arc<parking_lot::Mutex<&'static str>>,
    checks: Arc<parking_lot::Mutex<Vec<String>>>,
}

fn codex_source() -> Source {
    let profile = profile_for("codex");
    Source {
        facts: Facts {
            id: "codex".into(),
            label: "Codex".into(),
            installed: true,
            tested: profile.tested_versions().map(|r| r.as_str().to_string()),
        },
        version_command: None,
    }
}

fn engine_on(machine: &Machine, state_file: Option<std::path::PathBuf>) -> Engine {
    let (version, screen, checks) = (machine.version.clone(), machine.after_bump.clone(), machine.checks.clone());
    let version_for_check = version.clone();
    Engine::new(
        state_file,
        Deps {
            sources: Box::new(|| vec![codex_source()]),
            read_version: Box::new(move |_| Some(version.lock().clone())),
            run_check: Box::new(move |source, seen| {
                checks.lock().push(seen.clone().unwrap_or_default());
                // The check drives the fake CLI through the real driver and the real profile.
                let mut io = FakeIo::new(vec![(When::At(ms(400)), *screen.lock())]);
                let profile = profile_for(&source.facts.id);
                let drive = run::drive(&mut io, profile.as_ref(), &Timing::default());
                let _ = &version_for_check;
                run::into_result(drive, &source.facts.id, &source.facts.label, seen, 1_000, profile.as_ref(), &|s| s.to_string())
            }),
            captures: Box::new(|_| 0),
            now_ms: Box::new(|| 1_000),
        },
    )
}

fn machine(version: &str, screen: &'static str) -> Machine {
    Machine {
        version: Arc::new(parking_lot::Mutex::new(version.to_string())),
        after_bump: Arc::new(parking_lot::Mutex::new(screen)),
        checks: Default::default(),
    }
}

fn wire_strict(engine: &Engine) {
    engine.on_change(Box::new(|_, strict_agents| {
        strict::replace(strict_agents.iter().cloned());
    }));
}

#[test]
fn a_faked_version_bump_outside_the_range_turns_on_the_chip_and_stops_gate_dismissal() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.159.3", CODEX_READY);
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    let codex = profile_for("codex");

    // Day one, a tested version: normal behaviour, nothing launched.
    assert!(engine.refresh().is_empty(), "first sighting inside the range checks nothing");
    let view = &engine.status().agents[0];
    assert!(!view.strict && view.chip.is_none(), "{view:?}");
    assert_eq!(strict::startup_gate(codex.as_ref(), CODEX_UPDATE_MENU).unwrap().dismiss, codex.startup_gate(CODEX_UPDATE_MENU).unwrap().dismiss);
    assert!(machine.checks.lock().is_empty());

    // The CLI updates to a version nobody tested.
    *machine.version.lock() = "codex-cli 0.171.0".into();
    let wanted = engine.refresh();
    assert_eq!(wanted, vec!["codex".to_string()], "the version change asks for a check");
    let view = &engine.status().agents[0];
    assert!(view.strict, "{view:?}");
    assert_eq!(view.strict_reason, Some(StrictReason::UntestedVersion));
    assert_eq!(view.chip.as_deref(), Some("Untested version"));
    assert!(view.plain.contains("ask you before closing a start-up notice"), "{}", view.plain);
    // The profile still recognises the update menu, but nothing may close it for the person.
    let gate = strict::startup_gate(codex.as_ref(), CODEX_UPDATE_MENU).expect("still recognised");
    assert_eq!(gate.id, "update_available");
    assert_eq!(gate.dismiss, None);

    strict::replace(Vec::<String>::new());
}

#[test]
fn a_failed_check_after_a_bump_raises_the_inbox_card_and_a_later_pass_clears_it() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.159.3", CODEX_READY);
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    engine.refresh();

    // 0.162.1 is inside the tested range, but its start screen is one the profile cannot read.
    *machine.version.lock() = "codex-cli 0.162.1".into();
    *machine.after_bump.lock() = UNRECOGNISED;
    assert_eq!(engine.refresh(), vec!["codex".to_string()]);
    let view = engine.check("codex").expect("check ran");
    assert_eq!(view.last.as_ref().unwrap().outcome, Outcome::Unrecognised);
    assert!(view.strict);
    assert_eq!(view.strict_reason, Some(StrictReason::ChangedScreens));
    let status = engine.status();
    assert_eq!(status.notices.len(), 1);
    assert_eq!(
        status.notices[0].text,
        "Codex 0.162.1 changed its screens. Puppet Master will ask you before acting."
    );
    assert_eq!(machine.checks.lock().len(), 1);
    // The check never sent a prompt.
    assert_eq!(view.last.as_ref().unwrap().screens.len(), 1);

    // The person dismisses the card; it stays gone.
    engine.dismiss_notice(&status.notices[0].id);
    assert!(engine.status().notices.is_empty());

    // The CLI is fixed upstream (a newer version reads fine): strict mode ends.
    *machine.version.lock() = "codex-cli 0.162.2".into();
    *machine.after_bump.lock() = CODEX_READY;
    assert_eq!(engine.refresh(), vec!["codex".to_string()]);
    let view = engine.check("codex").unwrap();
    assert_eq!(view.last.as_ref().unwrap().outcome, Outcome::Ready);
    assert!(!view.strict, "{view:?}");
    assert!(engine.status().notices.is_empty());
    strict::replace(Vec::<String>::new());
}

#[test]
fn the_same_version_is_checked_once() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.159.3", CODEX_READY);
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    engine.refresh();
    *machine.version.lock() = "codex-cli 0.160.0".into();
    assert_eq!(engine.run_due().len(), 1);
    assert!(engine.refresh().is_empty(), "already checked on 0.160.0");
    assert_eq!(machine.checks.lock().len(), 1);
    strict::replace(Vec::<String>::new());
}

#[test]
fn auto_check_off_notes_the_version_but_launches_nothing() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.159.3", CODEX_READY);
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    engine.refresh();
    engine.set_auto_check(false);
    *machine.version.lock() = "codex-cli 0.160.0".into();
    assert!(engine.refresh().is_empty());
    assert_eq!(engine.status().agents[0].version.as_deref(), Some("0.160.0"));
    // "Check this agent" still works on demand.
    assert!(engine.check("codex").is_ok());
    strict::replace(Vec::<String>::new());
}

#[test]
fn what_the_canary_knows_survives_a_restart() {
    let _lock = super::test_lock();
    let dir = std::env::temp_dir().join(format!("pm-canary-state-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("canary.json");
    let machine = machine("codex-cli 0.159.3", UNRECOGNISED);
    {
        let engine = engine_on(&machine, Some(file.clone()));
        engine.refresh();
        *machine.version.lock() = "codex-cli 0.162.1".into();
        engine.refresh();
        engine.check("codex").unwrap();
    }
    let reborn = engine_on(&machine, Some(file));
    let status = reborn.status();
    assert!(status.agents[0].strict, "strict mode is still on after a restart");
    assert_eq!(status.notices.len(), 1);
    assert_eq!(status.agents[0].version.as_deref(), Some("0.162.1"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_check_for_an_unknown_or_missing_agent_is_refused_with_a_sentence() {
    let machine = machine("codex-cli 0.159.3", CODEX_READY);
    let engine = engine_on(&machine, None);
    assert!(engine.check("nope").unwrap_err().contains("not an agent"));
}

#[test]
fn the_built_in_ranges_parse_and_cover_their_fixtures() {
    for (agent, fixture_version) in [("codex", "codex-cli 0.159.3"), ("claude", "2.1.299 (Claude Code)")] {
        let range = profile_for(agent).tested_versions().unwrap_or_else(|| panic!("{agent} declares a range"));
        assert_eq!(
            super::version::standing(Some(&range), Some(fixture_version)),
            super::version::Standing::Tested,
            "{agent}: the version its fixtures were captured on is inside its own range"
        );
    }
    assert!(profile_for("shell").tested_versions().is_none());
}

// ---- the live dismissal path honours strict mode -------------------------------------------------

/// A pane showing a screen until something is written, then another.
struct FakePane {
    screens: Vec<&'static str>,
    at: usize,
    written: Vec<String>,
}

impl crate::bridge::startup_gate::GateIo for FakePane {
    fn screen(&mut self) -> String {
        self.screens[self.at].to_string()
    }
    fn write(&mut self, keys: &str) -> Result<(), String> {
        self.written.push(keys.to_string());
        if self.at + 1 < self.screens.len() {
            self.at += 1;
        }
        Ok(())
    }
    fn pause(&mut self, _duration: Duration) {}
}

#[test]
fn the_run_path_closes_a_notice_normally_but_leaves_it_alone_in_strict_mode() {
    use crate::bridge::startup_gate::dismiss_startup_gates;
    let _lock = super::test_lock();
    let codex = profile_for("codex");
    strict::replace(Vec::<String>::new());

    // Control: a tested version. The profile's skip key is pressed and the notice is gone.
    let mut pane = FakePane { screens: vec![CODEX_UPDATE_MENU, CODEX_READY], at: 0, written: vec![] };
    let report = dismiss_startup_gates(&mut pane, codex.as_ref());
    assert_eq!(report.dismissed, vec!["update_available"]);
    assert_eq!(pane.written.len(), 1);

    // Strict mode (an untested version): the same screen is recognised, but nothing is pressed and
    // the run is handed the gate so it parks and shows it.
    strict::replace(["codex".to_string()]);
    let mut pane = FakePane { screens: vec![CODEX_UPDATE_MENU, CODEX_READY], at: 0, written: vec![] };
    let report = dismiss_startup_gates(&mut pane, codex.as_ref());
    strict::replace(Vec::<String>::new());
    assert!(pane.written.is_empty(), "strict mode presses nothing: {:?}", pane.written);
    assert!(report.dismissed.is_empty());
    assert_eq!(report.blocked.map(|g| g.id), Some("update_available"));
}
