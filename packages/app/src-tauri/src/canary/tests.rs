//! The check sequence against recorded screens, and the whole version-bump flow with the machine
//! faked. Screens are the profiles' own live captures (`tui/*/fixtures`).

use super::engine::{Deps, Engine, Source};
use super::model::*;
use super::run::{self, CanaryIo, Timing};
use super::state;
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
    /// The clock, in ms.
    clock: Arc<parking_lot::Mutex<i64>>,
    /// The program's path, size and modified time as the fake machine reports them.
    print: Arc<parking_lot::Mutex<Option<String>>>,
    /// How many times `--version` was read.
    reads: Arc<parking_lot::Mutex<usize>>,
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
    let (clock, print, reads) = (machine.clock.clone(), machine.print.clone(), machine.reads.clone());
    Engine::new(
        state_file,
        Deps {
            sources: Box::new(|| vec![codex_source()]),
            read_version: Box::new(move |_| {
                *reads.lock() += 1;
                Some(version.lock().clone())
            }),
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
            now_ms: Box::new(move || *clock.lock()),
            fingerprint: Box::new(move |_| print.lock().clone()),
            source_of: Box::new(|id| id.eq_ignore_ascii_case("codex").then(codex_source)),
        },
    )
}

fn machine(version: &str, screen: &'static str) -> Machine {
    Machine {
        version: Arc::new(parking_lot::Mutex::new(version.to_string())),
        after_bump: Arc::new(parking_lot::Mutex::new(screen)),
        checks: Default::default(),
        clock: Arc::new(parking_lot::Mutex::new(1_000)),
        print: Default::default(),
        reads: Default::default(),
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
        "Codex 0.159.3 \u{2192} 0.162.1: screens changed. Puppet Master will ask you before acting."
    );
    assert_eq!(status.notices[0].kind, NoticeKind::Changed);
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
    // The failure card is gone; the pass says so in its own card.
    let notices = engine.status().notices;
    assert_eq!(notices.len(), 1);
    assert_eq!((notices[0].kind, notices[0].text.as_str()), (NoticeKind::Good, "Codex 0.162.1 \u{2192} 0.162.2: checked, all good."));
    strict::replace(Vec::<String>::new());
}

// ---- results, verified versions, signals, the spawn gate -----------------------------------------

#[test]
fn a_passing_check_clears_the_untested_chip_for_that_exact_version_only() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.159.3", CODEX_READY);
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    let codex = profile_for("codex");
    engine.refresh();

    // The update lands outside the tested range: the chip is on until a check passes here.
    *machine.version.lock() = "codex-cli 0.171.0".into();
    assert_eq!(engine.refresh(), vec!["codex".to_string()]);
    let view = engine.status().agents[0].clone();
    assert_eq!((view.strict_reason, view.chip.as_deref(), view.verified), (Some(StrictReason::UntestedVersion), Some("Untested version"), false));
    assert!(strict::is_strict("codex"));
    assert_eq!(strict::startup_gate(codex.as_ref(), CODEX_UPDATE_MENU).unwrap().dismiss, None);

    let view = engine.check("codex").unwrap();
    assert!(view.verified && !view.strict && view.chip.is_none(), "{view:?}");
    assert!(!strict::is_strict("codex"), "the strict set follows");
    assert!(strict::startup_gate(codex.as_ref(), CODEX_UPDATE_MENU).unwrap().dismiss.is_some(), "the person's notices are closed for them again");
    let status = engine.status();
    assert_eq!(status.notices.len(), 1);
    assert_eq!(
        (status.notices[0].kind, status.notices[0].text.as_str()),
        (NoticeKind::Good, "Codex 0.159.3 \u{2192} 0.171.0: checked, all good.")
    );
    let change = view.change.expect("the update is shown");
    assert_eq!((change.from.as_str(), change.to.as_str(), change.verdict), ("0.159.3", "0.171.0", Verdict::Good));

    // The next update is another exact version: the chip is back until it passes too.
    *machine.version.lock() = "codex-cli 0.171.1".into();
    assert_eq!(engine.refresh(), vec!["codex".to_string()]);
    let view = engine.status().agents[0].clone();
    assert_eq!((view.chip.as_deref(), view.verified), (Some("Untested version"), false));
    assert!(strict::is_strict("codex"));

    // Going back to the version that passed here needs no new check and has no chip.
    *machine.version.lock() = "codex-cli 0.171.0".into();
    assert!(engine.refresh().is_empty());
    let view = engine.status().agents[0].clone();
    assert!(view.verified && view.chip.is_none());
    strict::replace(Vec::<String>::new());
}

#[test]
fn same_screens_after_the_update_is_all_good_and_changed_screens_is_not() {
    let _lock = super::test_lock();
    // "Version 2 with the same screens."
    let same = machine("codex-cli 0.159.3", CODEX_READY);
    let engine = engine_on(&same, None);
    wire_strict(&engine);
    engine.refresh();
    *same.version.lock() = "codex-cli 0.170.0".into();
    let view = engine.run_due().remove(0);
    assert_eq!(view.last.as_ref().unwrap().outcome, Outcome::Ready);
    assert!(view.verified && view.chip.is_none());
    assert_eq!(engine.status().notices[0].text, "Codex 0.159.3 \u{2192} 0.170.0: checked, all good.");

    // "Version 2 with a changed input box."
    let changed = machine("codex-cli 0.159.3", UNRECOGNISED);
    let engine = engine_on(&changed, None);
    wire_strict(&engine);
    engine.refresh();
    *changed.version.lock() = "codex-cli 0.170.0".into();
    let view = engine.run_due().remove(0);
    assert_eq!(view.last.as_ref().unwrap().outcome, Outcome::Unrecognised);
    assert!(!view.verified && view.strict);
    assert_eq!(view.strict_reason, Some(StrictReason::ChangedScreens));
    assert_eq!(
        engine.status().notices[0].text,
        "Codex 0.159.3 \u{2192} 0.170.0: screens changed. Puppet Master will ask you before acting."
    );
    assert!(strict::is_strict("codex"));
    strict::replace(Vec::<String>::new());
}

#[test]
fn the_same_failure_twice_marks_the_screens_changed() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.159.3", "");
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    engine.refresh();
    *machine.version.lock() = "codex-cli 0.162.1".into();
    let first = engine.check("codex").unwrap();
    assert_eq!(first.last.as_ref().unwrap().outcome, Outcome::NeverDrew);
    assert!(!first.strict && engine.status().notices.is_empty(), "one slow start proves nothing");
    let second = engine.check("codex").unwrap();
    assert_eq!(second.strict_reason, Some(StrictReason::ChangedScreens), "{second:?}");
    assert_eq!(engine.status().notices.len(), 1);
    assert!(strict::is_strict("codex"));
    // A pass on the same version ends it.
    *machine.after_bump.lock() = CODEX_READY;
    let third = engine.check("codex").unwrap();
    assert!(!third.strict && third.verified, "{third:?}");
    strict::replace(Vec::<String>::new());
}

#[test]
fn a_screen_signal_starts_one_check_and_a_storm_starts_no_more() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.162.1", CODEX_READY);
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    engine.refresh();

    let started = (0..50).filter(|_| engine.note_signal("codex", Signal::NotReady)).count();
    assert_eq!(started, 1, "fifty signals, one check");
    assert!(engine.check("codex").is_ok());
    // Later signals on the same version: nothing, however much time passes.
    *machine.clock.lock() += state::SIGNAL_GAP_MS * 10;
    assert!(!engine.note_signal("codex", Signal::UnreadableScreen));
    assert!(!engine.note_signal("codex", Signal::AmbiguousIdle));

    // A new version is a new chance...
    *machine.version.lock() = "codex-cli 0.162.2".into();
    engine.refresh();
    assert!(engine.note_signal("codex", Signal::NotReady));
    assert!(!engine.note_signal("codex", Signal::NotReady));
    // ...but even another new version waits for the gap since the last check a signal started.
    *machine.version.lock() = "codex-cli 0.162.3".into();
    engine.refresh();
    *machine.clock.lock() += 1_000;
    assert!(!engine.note_signal("codex", Signal::NotReady), "too soon after the last one");
    *machine.clock.lock() += state::SIGNAL_GAP_MS;
    assert!(engine.note_signal("codex", Signal::NotReady));
    strict::replace(Vec::<String>::new());
}

#[test]
fn a_signal_does_nothing_when_auto_check_is_off_or_the_agent_is_not_known() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.162.1", CODEX_READY);
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    engine.refresh();
    engine.set_auto_check(false);
    assert!(!engine.note_signal("codex", Signal::NotReady));
    assert!(!engine.note_signal("shell", Signal::NotReady));
    assert!(!engine.note_signal("", Signal::NotReady));
    engine.set_auto_check(true);
    assert!(engine.note_signal("CODEX", Signal::NotReady), "the switch was the only thing in the way");
    assert!(machine.checks.lock().is_empty(), "noting a signal launches nothing by itself");
    strict::replace(Vec::<String>::new());
}

#[test]
fn a_signal_check_that_finds_unreadable_screens_marks_the_agent_and_ends_the_signals() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.162.1", UNRECOGNISED);
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    engine.refresh();
    assert!(!engine.status().agents[0].strict, "nothing changed, nothing checked");
    assert!(engine.note_signal("codex", Signal::UnreadableScreen));
    let view = engine.check("codex").unwrap();
    assert_eq!(view.strict_reason, Some(StrictReason::ChangedScreens));
    assert_eq!(engine.status().notices[0].text, "Codex 0.162.1 changed its screens. Puppet Master will ask you before acting.");
    *machine.clock.lock() += state::SIGNAL_GAP_MS * 5;
    assert!(!engine.note_signal("codex", Signal::UnreadableScreen), "the answer is known");
    strict::replace(Vec::<String>::new());
}

#[test]
fn before_a_run_only_a_changed_program_makes_the_version_be_read_again() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.159.3", CODEX_READY);
    *machine.print.lock() = Some("codex.exe|10|1".into());
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    engine.refresh();
    assert_eq!(*machine.reads.lock(), 1);

    // Nothing changed: no process is started, nothing is read.
    assert!(!engine.before_run("codex"));
    assert!(!engine.before_run("codex"));
    assert_eq!(*machine.reads.lock(), 1);

    // The program was replaced by one with the same version: read once, remembered.
    *machine.print.lock() = Some("codex.exe|11|2".into());
    assert!(!engine.before_run("codex"));
    assert_eq!(*machine.reads.lock(), 2);
    assert!(!engine.before_run("codex"));
    assert_eq!(*machine.reads.lock(), 2);

    // The update lands between probes: seen at the next run, outside the range, so a check is wanted.
    *machine.print.lock() = Some("codex.exe|12|3".into());
    *machine.version.lock() = "codex-cli 0.171.0".into();
    *machine.clock.lock() = 5_000;
    assert!(engine.before_run("codex"));
    assert_eq!(*machine.reads.lock(), 3);
    let view = engine.status().agents[0].clone();
    assert_eq!(view.version.as_deref(), Some("0.171.0"));
    assert_eq!(view.strict_reason, Some(StrictReason::UntestedVersion), "strict before the run's start-up screens are met");
    assert!(strict::is_strict("codex"));
    let change = view.change.unwrap();
    assert_eq!((change.from.as_str(), change.at_ms), ("0.159.3", 5_000));

    // Not an agent, or no program to look at: nothing happens.
    assert!(!engine.before_run("nope"));
    *machine.print.lock() = None;
    assert!(!engine.before_run("codex"));
    assert_eq!(*machine.reads.lock(), 3);
    strict::replace(Vec::<String>::new());
}

#[test]
fn the_first_run_before_any_probe_only_remembers_the_program() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.171.0", CODEX_READY);
    *machine.print.lock() = Some("codex.exe|10|1".into());
    let engine = engine_on(&machine, None);
    assert!(!engine.before_run("codex"), "nothing to compare with");
    assert_eq!(*machine.reads.lock(), 0, "no process for a first look");
    // The probe that follows takes it from there: the first sighting outside the range is checked.
    assert_eq!(engine.refresh(), vec!["codex".to_string()]);
    assert!(!engine.before_run("codex"));
    assert_eq!(*machine.reads.lock(), 1);
}

#[test]
fn a_program_change_with_auto_check_off_notes_the_version_and_starts_nothing() {
    let _lock = super::test_lock();
    let machine = machine("codex-cli 0.159.3", CODEX_READY);
    *machine.print.lock() = Some("codex.exe|10|1".into());
    let engine = engine_on(&machine, None);
    wire_strict(&engine);
    engine.refresh();
    engine.set_auto_check(false);
    *machine.print.lock() = Some("codex.exe|11|2".into());
    *machine.version.lock() = "codex-cli 0.171.0".into();
    assert!(!engine.before_run("codex"));
    let view = engine.status().agents[0].clone();
    assert_eq!(view.version.as_deref(), Some("0.171.0"));
    assert!(view.strict, "an untested version is still handled carefully");
    strict::replace(Vec::<String>::new());
}

#[test]
fn an_old_state_file_loads_and_is_upgraded_when_the_next_check_saves() {
    let _lock = super::test_lock();
    let dir = std::env::temp_dir().join(format!("pm-canary-old-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("canary.json");
    let old = r#"{"auto_check":true,"agents":{"codex":{"version":"codex-cli 0.171.0","probed_ms":5,"last_result":null,"notice":null}}}"#;
    std::fs::write(&file, old).unwrap();

    let machine = machine("codex-cli 0.171.0", CODEX_READY);
    {
        let engine = engine_on(&machine, Some(file.clone()));
        let view = engine.status().agents[0].clone();
        assert_eq!((view.version.as_deref(), view.verified, view.strict), (Some("0.171.0"), false, true));
        assert!(view.change.is_none());
        let view = engine.check("codex").unwrap();
        assert!(view.verified && !view.strict);
    }
    // A restart keeps the verified version: no chip, no check.
    let reborn = engine_on(&machine, Some(file));
    let view = reborn.status().agents[0].clone();
    assert!(view.verified && !view.strict, "{view:?}");
    assert!(reborn.refresh().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
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
    fn write(&mut self, permit: &crate::agent_runs::readiness::GatePermit) -> Result<(), String> {
        self.written.push(permit.keys().to_string());
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

#[test]
fn the_live_wrapper_weighs_one_agents_signals_at_most_every_half_minute() {
    use std::time::Instant;
    let t0 = Instant::now();
    assert!(super::live::signal_due("Signal_Throttle_Test", t0));
    assert!(!super::live::signal_due("signal_throttle_test", t0 + Duration::from_secs(1)), "once a second is too often");
    assert!(!super::live::signal_due("signal_throttle_test", t0 + Duration::from_secs(29)));
    assert!(super::live::signal_due("signal_throttle_test", t0 + Duration::from_secs(31)));
    assert!(super::live::signal_due("signal_throttle_other", t0 + Duration::from_secs(1)), "each agent has its own clock");
}

// ---- a wizard-added agent's trust screen -----------------------------------------------------------------------------

/// A definition with only a `[[tui.prompt]]` for the folder-trust question (what the Agent Wizard writes).
const TRUST_ONLY_AGENT: &str = r##"
schema_version = 1

[agent]
id = "trustprobe"
label = "Trust Probe"
experimental = true

[binary]
names = ["trustprobe-not-installed"]
install_hint = "see https://example.invalid/trustprobe"

[tui]
argv = []

[[tui.prompt]]
kind = "workspace_trust"
detect_regex = 'do you trust the contents of this project'
choices = [
  { id = "allow_once", key = "enter" },
  { id = "deny", key = "escape" },
]
"##;

const TRUST_SCREEN: &str = "\n Probe CLI v1.4.0\n\nAccessing workspace:\nC:\\Users\\you\\project\n\nDo you trust the contents of this project?\n\n> Yes, I trust this folder\n  No, exit\n\n\u{2191}/\u{2193} Navigate \u{b7} enter Confirm\n";

#[test]
fn a_trust_screen_a_definition_declares_is_a_question_for_the_person_not_changed_screens() {
    let profile = crate::tui::declarative::DeclarativeProfile::new(crate::tui::declarative::parse_definition(TRUST_ONLY_AGENT).unwrap()).unwrap();
    let mut io = FakeIo::new(vec![(When::At(ms(500)), TRUST_SCREEN)]);
    let result = run::drive(&mut io, &profile, &Timing::default());
    // Before: no startup gate, so the screen was "not recognised" and the agent got a "screens changed" card.
    assert_eq!(result.outcome, Outcome::AsksYou, "{result:?}");
    assert_eq!(result.gate, Some("workspace_trust"));
    assert!(io.written.is_empty(), "only the person passes a trust gate: {:?}", io.written);
    assert!(result.outcome.passed() && !result.outcome.changed_screens());
}
