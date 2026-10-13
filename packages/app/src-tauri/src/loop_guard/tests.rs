use super::*;
use crate::tui::declarative::{parse_definition, DeclarativeProfile};
use crate::tui::TuiProfile;

/// Drives a `Guard` like the activity ticker does: a growing transcript, the last 24 lines as the
/// tail, a fake clock.
struct Rig {
    guard: Guard,
    now: u64,
    screen: Vec<String>,
    events: Vec<Event>,
    kinds: Box<dyn LineKinds>,
}

impl Rig {
    fn new() -> Self {
        Self::with(Settings::default(), Box::new(GenericKinds))
    }

    fn with(settings: Settings, kinds: Box<dyn LineKinds>) -> Self {
        Self { guard: Guard::new(settings), now: 1_000_000, screen: Vec::new(), events: Vec::new(), kinds }
    }

    fn see(&mut self, new_lines: &[&str], status: PaneStatus, advance_ms: u64) {
        self.now += advance_ms;
        self.screen.extend(new_lines.iter().map(|l| l.to_string()));
        let from = self.screen.len().saturating_sub(24);
        let tail = self.screen[from..].join("\n");
        let events = self.guard.observe("p1", &tail, status, self.kinds.as_ref(), self.now);
        self.events.extend(events);
    }

    /// One user-visible turn: running with `during` lines, then idle with `after` lines.
    fn turn(&mut self, during: &[&str], after: &[&str], secs: u64) {
        self.see(&[], PaneStatus::Running, 5_000);
        self.see(during, PaneStatus::Running, secs * 1000);
        self.see(after, PaneStatus::Idle, 1_000);
        self.now += 20_000; // the user reads and types
    }

    fn raised(&self) -> Vec<&LoopSignal> {
        self.events.iter().filter_map(|e| if let Event::Raised(s) = e { Some(s) } else { None }).collect()
    }
}

// ---- true loops ----------------------------------------------------------------------------

#[test]
fn the_same_error_across_turns_is_a_loop_with_evidence() {
    let mut rig = Rig::new();
    for i in 0..5 {
        let err = format!("error[E0432]: unresolved import `crate::store::Cache` --> src/lib.rs:{}:5", 10 + i);
        let attempt = format!("Trying again with a different import path ({} of many)", i + 1);
        rig.turn(&["cargo build", &attempt, &err], &["Build failed, will adjust the module path"], 30);
    }
    let raised = rig.raised();
    assert_eq!(raised.len(), 1, "once per episode, got {:?}", rig.events);
    let s = raised[0];
    assert_eq!(s.kind, LoopKind::RepeatedError);
    assert_eq!(s.turns, 4, "raised on the 4th repeat");
    assert!(s.evidence_lines.len() <= 3 && !s.evidence_lines.is_empty());
    assert!(s.evidence_lines[0].contains("unresolved import"), "{:?}", s.evidence_lines);
    assert_eq!(s.summary, "the same error 4 times");
    assert_eq!(rig.guard.status("p1").unwrap().kind, LoopKind::RepeatedError);
}

#[test]
fn near_duplicate_errors_with_changing_numbers_and_paths_match() {
    let mut rig = Rig::new();
    let errs = [
        "tests/user.test.ts > creates a user  AssertionError: expected 3 to equal 4 (12ms)",
        "tests/user.test.ts > creates a user  AssertionError: expected 3 to equal 5 (9ms)",
        "tests/user.test.ts > creates a user  AssertionError: expected 3 to equal 6 (14ms)",
        "tests/user.test.ts > creates a user  AssertionError: expected 3 to equal 7 (11ms)",
    ];
    for e in errs {
        rig.turn(&["npm test", e, "Tests failed"], &["Tests are still failing"], 20);
    }
    assert_eq!(rig.raised().len(), 1, "{:?}", rig.events);
    assert_eq!(rig.raised()[0].kind, LoopKind::RepeatedError);
}

#[test]
fn the_same_steps_over_and_over_without_progress_is_a_loop() {
    let mut rig = Rig::new();
    for _ in 0..4 {
        rig.turn(
            &["\u{25cf} Read(src/router.ts)", "\u{25cf} Search(pattern: \"handleRoute\")", "\u{25cf} Read(src/router.ts)"],
            &["Let me look at the router once more."],
            25,
        );
        // The lines are not new on the screen the second time; make the agent reprint them as
        // real agents do (the tail scrolls), by adding unrelated chatter between turns.
        rig.screen.extend((0..30).map(|i| format!("chatter {i}")));
    }
    let raised = rig.raised();
    assert_eq!(raised.len(), 1, "{:?}", rig.events);
    assert_eq!(raised[0].kind, LoopKind::RepeatedAction);
    assert!(raised[0].evidence_lines.iter().any(|l| l.contains("Read(src/router.ts)")));
}

#[test]
fn running_for_twenty_minutes_with_no_new_lines_is_stalled() {
    let mut rig = Rig::new();
    rig.see(&["\u{2736} Thinking\u{2026} (1s \u{b7} esc to interrupt)"], PaneStatus::Running, 1000);
    // Only the spinner timer changes; the words do not.
    for i in 2..=45 {
        let line = format!("\u{2736} Thinking\u{2026} ({}s \u{b7} esc to interrupt)", i * 30);
        rig.see(&[&line], PaneStatus::Running, 30_000);
        let events = rig.guard.tick(rig.now);
        rig.events.extend(events);
    }
    let raised = rig.raised();
    assert_eq!(raised.len(), 1, "{:?}", rig.events);
    assert_eq!(raised[0].kind, LoopKind::Stalled);
    assert!(raised[0].turns >= 20, "minutes without output: {}", raised[0].turns);
}

#[test]
fn busy_turns_without_any_progress_in_a_short_window_flag_no_progress() {
    let mut rig = Rig::with(Settings { turns: 3, ..Settings::default() }, Box::new(GenericKinds));
    // Six turns in about six minutes; a different file each time (so not a repeat), no progress.
    for f in ["a", "b", "c", "d", "e", "f"] {
        let read = format!("\u{25cf} Read(src/module_{f}.ts)");
        rig.turn(&[&read, &format!("\u{25cf} Grep(pattern: \"createSession{f}\")")], &["Not here either."], 20);
    }
    let raised = rig.raised();
    assert_eq!(raised.len(), 1, "{:?}", rig.events);
    assert_eq!(raised[0].kind, LoopKind::NoProgress);
}

// ---- healthy sessions must stay quiet ------------------------------------------------------

#[test]
fn healthy_typescript_session_with_varied_errors_that_get_fixed() {
    let mut rig = Rig::new();
    rig.turn(&["\u{25cf} Update(src/api.ts)", "src/api.ts(10,3): error TS2322: Type 'string' is not assignable to type 'number'."], &["Fixing the type."], 20);
    rig.turn(&["\u{25cf} Update(src/api.ts)", "src/api.ts(22,9): error TS2304: Cannot find name 'parseId'."], &["Importing parseId."], 20);
    rig.turn(&["\u{25cf} Update(src/ids.ts)", "src/ids.ts(3,1): error TS2307: Cannot find module './util'."], &["Creating util."], 20);
    rig.turn(&["\u{25cf} Write(src/util.ts)", "Compiled successfully"], &["All errors fixed."], 20);
    rig.turn(&["\u{25cf} Bash(npm test)", "Tests: 14 passed, 14 total"], &["Done."], 40);
    assert!(rig.raised().is_empty(), "{:?}", rig.events);
    assert!(rig.guard.status("p1").is_none());
}

#[test]
fn healthy_rust_session_repeating_one_error_twice_then_passing() {
    let mut rig = Rig::new();
    rig.turn(&["\u{25cf} Bash(cargo build)", "error[E0382]: borrow of moved value: `config`"], &["Cloning config."], 40);
    rig.turn(&["\u{25cf} Bash(cargo build)", "error[E0382]: borrow of moved value: `config`"], &["Using a reference instead."], 40);
    rig.turn(&["\u{25cf} Bash(cargo build)", "error[E0599]: no method named `len` found for struct `Handle`"], &["Adding the method."], 40);
    rig.turn(&["\u{25cf} Update(src/handle.rs)", "Finished dev profile in 12.3s"], &["Build is green, updated handle.rs."], 40);
    rig.turn(&["\u{25cf} Bash(cargo test)", "test result: ok. 31 passed; 0 failed"], &["All tests pass."], 60);
    rig.turn(&["\u{25cf} Bash(cargo test)", "test result: ok. 31 passed; 0 failed"], &["Committed."], 60);
    assert!(rig.raised().is_empty(), "{:?}", rig.events);
}

#[test]
fn healthy_long_refactor_and_a_chat_with_no_tools() {
    let mut rig = Rig::new();
    let files = ["auth", "session", "cookies", "tokens", "refresh", "logout", "profile", "billing"];
    for f in files {
        let upd = format!("\u{25cf} Update(src/{f}.ts)");
        let ok = format!("Updated src/{f}.ts with 12 additions");
        rig.turn(&[&upd, &ok], &["Next file."], 30);
    }
    // Plain conversation: no tool rows, no errors.
    for q in ["What does the session module do?", "Why a cookie and not a header?", "Can you explain refresh tokens?", "Thanks, and the logout flow?", "OK and billing?"] {
        rig.turn(&[q, "It stores the session id and rotates it on every login."], &["Anything else?"], 8);
    }
    assert!(rig.raised().is_empty(), "{:?}", rig.events);
}

#[test]
fn an_old_error_that_merely_stays_on_screen_never_repeats() {
    let mut rig = Rig::new();
    rig.turn(&["error: connection refused while calling the payments API"], &["Starting the mock server."], 20);
    // Five more turns; the error line is still in the 24-line tail but nothing new says it again.
    for i in 0..5 {
        let l = format!("Step {i}: checking the mock server logs");
        rig.turn(&[&l], &["Looks fine."], 20);
    }
    assert!(rig.raised().is_empty(), "{:?}", rig.events);
}

#[test]
fn a_long_but_busy_run_is_not_stalled() {
    let mut rig = Rig::new();
    rig.see(&[], PaneStatus::Running, 1000);
    for i in 0..40 {
        let l = format!("\u{25cf} Read(src/file_{}.ts)", "x".repeat(i + 1));
        rig.see(&[&l], PaneStatus::Running, 40_000);
        let events = rig.guard.tick(rig.now);
        rig.events.extend(events);
    }
    assert!(rig.raised().is_empty(), "{:?}", rig.events);
}

// ---- hysteresis, dismiss, settings ---------------------------------------------------------

#[test]
fn one_signal_per_episode_and_it_re_arms_after_it_clears() {
    let mut rig = Rig::new();
    let loop_turn = |rig: &mut Rig, n: u32| {
        rig.turn(&["cargo build", &format!("error: linking with `cc` failed (exit code {n})")], &["Retrying the link step."], 20)
    };
    for n in 0..4 {
        loop_turn(&mut rig, n);
    }
    assert_eq!(rig.raised().len(), 1);
    // Two more bad turns: still the same episode, nothing new emitted.
    loop_turn(&mut rig, 5);
    loop_turn(&mut rig, 6);
    assert_eq!(rig.raised().len(), 1, "no re-alert while it persists");
    // A different outcome clears it.
    rig.turn(&["cargo build", "Finished dev profile"], &["Build is done."], 20);
    assert!(rig.events.iter().any(|e| matches!(e, Event::Cleared(p) if p == "p1")));
    assert!(rig.guard.status("p1").is_none());
    // A brand new loop alerts again.
    for n in 0..4 {
        rig.turn(&["npm test", &format!("Error: Cannot find module 'left-pad' (try {n})")], &["Installing it again."], 20);
    }
    assert_eq!(rig.raised().len(), 2, "{:?}", rig.events);
}

#[test]
fn dismiss_hides_the_episode_but_not_the_next_one() {
    let mut rig = Rig::new();
    for n in 0..4 {
        rig.turn(&[&format!("fatal: unable to access remote (attempt {n})")], &["Retrying the push."], 20);
    }
    assert!(rig.guard.status("p1").is_some());
    assert!(rig.guard.dismiss("p1"));
    assert!(rig.guard.status("p1").is_none(), "dismissed");
    assert!(rig.guard.all_active().is_empty());
    rig.turn(&["fatal: unable to access remote (attempt 9)"], &["Retrying the push."], 20);
    assert_eq!(rig.raised().len(), 1, "a dismissed episode does not alert again");
    rig.turn(&["Pushed to origin"], &["Done."], 20);
    for n in 0..4 {
        rig.turn(&[&format!("Error: ECONNRESET talking to registry (retry {n})")], &["Retrying."], 20);
    }
    assert_eq!(rig.raised().len(), 2);
    assert!(rig.guard.status("p1").is_some());
}

#[test]
fn sensitivity_and_off_switch() {
    // Six turns of the same error: N=8 stays quiet, N=3 alerts.
    let run = |settings: Settings| {
        let mut rig = Rig::with(settings, Box::new(GenericKinds));
        for n in 0..6 {
            rig.turn(&[&format!("error: cannot open file 'data.json' (pass {n})")], &["Trying again."], 20);
        }
        rig.raised().len()
    };
    assert_eq!(run(Settings { turns: 8, ..Settings::default() }), 0);
    assert_eq!(run(Settings { turns: 3, ..Settings::default() }), 1);
    assert_eq!(run(Settings { enabled: false, turns: 3, ..Settings::default() }), 0);
    assert_eq!(Settings { turns: 1, stalled_minutes: 1, ..Settings::default() }.clamped().turns, 3);
}

#[test]
fn turning_the_guard_off_clears_open_cards() {
    let mut rig = Rig::new();
    for n in 0..4 {
        rig.turn(&[&format!("error: cannot open file 'data.json' (pass {n})")], &["Trying again."], 20);
    }
    let events = rig.guard.set_settings(Settings { enabled: false, ..Settings::default() });
    assert_eq!(events, vec![Event::Cleared("p1".into())]);
    assert!(rig.guard.status("p1").is_none());
}

// ---- classification ------------------------------------------------------------------------

const ODDBALL: &str = r##"
schema_version = 1

[agent]
id = "oddball"
label = "Oddball"
experimental = true

[binary]
names = ["oddball"]

[tui]
argv = []
error_regex = ['BOOM-\d+']
"##;

#[test]
fn error_regex_in_a_toml_definition_teaches_the_guard() {
    let line = "BOOM-4411 the widget refused to start";
    assert!(!default_error_line(line), "the generic set does not know this wording");
    let taught = DeclarativeProfile::new(parse_definition(ODDBALL).unwrap()).unwrap();
    assert!(taught.is_error_line(line));
    assert!(!taught.is_error_line("the widget started"));
    // The generic set still applies to a declarative agent.
    assert!(taught.is_error_line("Error: Cannot find module 'x'"));
    // Without the table the same definition does not flag it.
    let plain = DeclarativeProfile::new(parse_definition(&ODDBALL.replace("error_regex = ['BOOM-\\d+']", "")).unwrap()).unwrap();
    assert!(!plain.is_error_line(line));

    // And end to end: four turns of that wording make a loop for the taught profile only.
    struct Kinds(std::sync::Arc<DeclarativeProfile>);
    impl LineKinds for Kinds {
        fn is_error(&self, l: &str) -> bool {
            self.0.is_error_line(l)
        }
        fn is_tool(&self, l: &str) -> bool {
            default_tool_line(l)
        }
        fn is_chrome(&self, _: &str) -> bool {
            false
        }
    }
    let mut taught_rig = Rig::with(Settings::default(), Box::new(Kinds(std::sync::Arc::new(taught))));
    let mut plain_rig = Rig::with(Settings::default(), Box::new(Kinds(std::sync::Arc::new(plain))));
    for rig in [&mut taught_rig, &mut plain_rig] {
        for n in 0..4 {
            rig.turn(&[&format!("BOOM-44{n} the widget refused to start")], &["Restarting the widget."], 20);
        }
    }
    assert_eq!(taught_rig.raised().len(), 1);
    assert!(plain_rig.raised().is_empty());
}

#[test]
fn a_bad_error_regex_is_rejected_at_load() {
    let bad = ODDBALL.replace("'BOOM-\\d+'", "'(unclosed'");
    let error = DeclarativeProfile::new(parse_definition(&bad).unwrap()).err().expect("rejected");
    assert_eq!(error.code, "AGENT_DEF_BAD_REGEX");
    assert!(error.path.contains("error_regex"), "{}", error.path);
}

#[test]
fn generic_error_detection_has_sane_edges() {
    for yes in [
        "error[E0432]: unresolved import `crate::x`",
        "Traceback (most recent call last):",
        "npm ERR! code ENOENT",
        "src/a.ts(1,1): error TS2322: bad",
        "fatal: not a git repository",
        "Error: Cannot find module 'x'",
    ] {
        assert!(default_error_line(yes), "{yes}");
    }
    for no in ["All good, 0 errors", "Fixed the error in handler.ts", "no failures found", "Read(src/a.ts)", ""] {
        assert!(!default_error_line(no), "{no}");
    }
}

#[test]
fn normalisation_ignores_numbers_paths_timestamps_and_hashes() {
    let a = normalize("12:01:44 error at /home/me/app/src/a.ts:12:5 request 9f3a8c21b7 failed after 3 retries", false);
    let b = normalize("12:09:02 error at C:\\work\\other\\b.ts:99:1 request 0a1b2c3d4e failed after 7 retries", false);
    assert_eq!(a, b);
    assert_eq!(normalize("\u{280b} Thinking\u{2026} (12s)", false), normalize("\u{2819} Thinking\u{2026} (13s)", false));
}
