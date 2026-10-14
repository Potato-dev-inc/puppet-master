//! The golden corpus: tonight's live bugs as casts (`tests/corpus/<agent>/<version>/*.cast`).
//!
//! Each test replays one cast through the code that decides, asserts the CORRECT decision (every
//! fix is merged, so these pass on this base), and snapshots the decision timeline with `insta`
//! (`tests/corpus/snapshots/*.snap`): what the code decided, in order, at which virtual time. A
//! change to the readiness, delivery, status or receipt logic that alters a verdict shows up as a
//! snapshot diff in review. Time is virtual: no test here sleeps.
//!
//! How to add one: see the module docs in `replay/mod.rs`.

use super::pane::{ReplayPane, Timeline};
use super::receipt::{replay_receipts, CaptureModel};
use super::startup::{replay_startup, Verdict};
use crate::agent_runs::delivery::{deliver_prompt_traced, Delivery};
use crate::pty::status::PaneStatus;
use crate::tui::profile_for;
use std::time::Duration;

fn snap(name: &str, timeline: &Timeline) {
    insta::with_settings!({
        snapshot_path => "../../tests/corpus/snapshots",
        prepend_module_to_snapshot => false,
    }, {
        insta::assert_snapshot!(name, timeline.render());
    });
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The statuses a pane went through after its start, repeats collapsed.
fn status_path(pane: &ReplayPane) -> Vec<PaneStatus> {
    let mut path: Vec<PaneStatus> = Vec::new();
    for (_, status) in pane.transitions.iter().skip(1) {
        if path.last() != Some(status) {
            path.push(*status);
        }
    }
    path
}

// ---- (a) the Codex update menu draws 0 / 300 / 600 ms after spawn --------------------------------
//
// Live bug #4 (run 7bef62a6): the task was typed into the menu (Enter = "Update now") and the run
// answered with the menu. Correct: the menu is closed with its own key (Esc) before anything else is
// typed, the task goes into the composer once, and one Enter starts the turn.

const PROMPT: &str = "Reply with the single word FIVE. Do not use any tools.";

fn deliver_into_menu_cast(cast: &str, snapshot: &str) {
    let mut pane = ReplayPane::corpus(cast);
    let clear = profile_for("codex").clear_input_sequence();
    let mut trace = Vec::new();
    let outcome = deliver_prompt_traced(&mut pane, clear, PROMPT, &mut trace);
    pane.note("deliver", format!("{outcome:?}"));

    assert_eq!(outcome, Delivery::Started, "{trace:#?}");
    let esc = pane.inputs.iter().position(|i| i.text == "\u{1b}" && !i.enter).expect("the menu was closed with Esc");
    let typed = pane.inputs.iter().position(|i| i.text == PROMPT).expect("the task was typed");
    assert!(esc < typed, "Esc first, then the task: {:?}", pane.inputs);
    assert!(
        pane.inputs[..esc].iter().all(|i| i.text.chars().all(|c| c == '\u{15}') && !i.enter),
        "nothing but the composer clear came before Esc: {:?}",
        pane.inputs
    );
    // While the menu was on screen only its own dismiss key and the composer clear (Ctrl-U) were sent.
    for input in pane.inputs.iter().filter(|i| i.gate.is_some()) {
        assert!(
            !input.enter && (input.text == "\u{1b}" || input.text.chars().all(|c| c == '\u{15}')),
            "typed into the update menu: {input:?}\n{}",
            pane.timeline.render()
        );
    }
    assert_eq!(pane.inputs.iter().filter(|i| i.text == PROMPT).count(), 1, "the task is typed once");
    assert_eq!(pane.inputs.iter().filter(|i| i.enter).count(), 1, "one Enter: {:?}", pane.inputs);
    snap(snapshot, &pane.timeline);
}

#[test]
fn codex_update_menu_at_spawn_is_closed_before_the_task_is_typed() {
    deliver_into_menu_cast("codex/0.159.3/update_menu_at_0ms.cast", "codex_update_menu_at_0ms");
}

#[test]
fn codex_update_menu_300ms_after_spawn_is_closed_before_the_task_is_typed() {
    deliver_into_menu_cast("codex/0.159.3/update_menu_at_300ms.cast", "codex_update_menu_at_300ms");
}

#[test]
fn codex_update_menu_600ms_after_spawn_is_closed_before_the_task_is_typed() {
    deliver_into_menu_cast("codex/0.159.3/update_menu_at_600ms.cast", "codex_update_menu_at_600ms");
}

#[test]
fn codex_update_menu_drawn_during_the_clear_pause_is_caught_by_the_last_look() {
    // 347acdc: the composer was up and settled, the menu covered it 100 ms later, inside the pause
    // after the clear keys. Nothing but the clear may have reached the pane before the menu closed.
    deliver_into_menu_cast("codex/0.159.3/update_menu_over_composer_at_100ms.cast", "codex_update_menu_over_composer_at_100ms");
}

// ---- (b) Claude's trust dialog, "No, exit", a bare PowerShell prompt ------------------------------
//
// VERIFY #2b: the pane stays alive and idle, the dialog text stays above the shell prompt, and the
// run used to be released and typed its task into PowerShell. Correct: the dialog holds the run, the
// bare prompt after it ends the run as declined, and a task is never typable at any moment.

#[test]
fn claude_trust_decline_into_the_host_shell_ends_the_run_and_nothing_is_typed() {
    let mut pane = ReplayPane::corpus("claude/2.1.299/trust_decline_to_host_shell.cast");
    let verdict = replay_startup(&mut pane, ms(1_230), ms(100), Duration::from_secs(10));
    assert_eq!(verdict, Verdict::Declined("WORKSPACE_TRUST_DECLINED"));

    // The dialog held the run until the shell prompt came; it was never released in between.
    let startup: Vec<&str> = pane.timeline.lines.iter().filter(|l| l.channel == "startup").map(|l| l.detail.as_str()).collect();
    assert_eq!(startup, ["hold (workspace_trust)", "run ends: WORKSPACE_TRUST_DECLINED"]);
    // At no moment could the task have been typed (dialog: blocked by prompt; shell prompt: agent exited).
    assert!(pane.timeline.lines.iter().all(|l| !(l.channel == "typing" && l.detail == "allowed")), "{}", pane.timeline.render());
    assert!(
        pane.timeline.lines.iter().any(|l| l.channel == "typing" && l.detail == "refused AGENT_EXITED"),
        "the bare shell prompt is refused as an exited agent: {}",
        pane.timeline.render()
    );
    snap("claude_trust_decline_to_host_shell", &pane.timeline);
}

// ---- (c) an approval-gated Claude edit: running, waiting_input, running, idle ---------------------
//
// VERIFY #5: History said "1 file" for the turn, Last turn said "0 files, you approved 1 request".
// Correct: the status path is exactly running, waiting_input, running, idle; the end-of-turn summary
// waits for ITS OWN checkpoint capture and counts the file edited after the approval.

#[test]
fn claude_approval_gated_edit_receipt_counts_the_file_edited_after_the_approval() {
    let mut pane = ReplayPane::corpus("claude/2.1.299/approval_gated_edit.cast");
    pane.run_to_end(Duration::from_secs(5));
    assert_eq!(
        status_path(&pane),
        [PaneStatus::Running, PaneStatus::WaitingInput, PaneStatus::Running, PaneStatus::Idle],
        "{}",
        pane.timeline.render()
    );

    let mut receipts = Timeline::default();
    let replay = replay_receipts(&pane, CaptureModel::slow_approval_capture(), &mut receipts);
    let (_, turn, files) = *replay.built.last().expect("a summary was built for the end of the turn");
    assert_eq!((turn, files), (1, Some(1)), "{}", pane.timeline.merged(&receipts).render());
    assert_eq!(replay.kept.len(), 1, "one entry for the turn");
    let kept = &replay.kept[0];
    assert_eq!(kept.files.as_ref().map(|f| f.count), Some(1), "Last turn agrees with History: {}", kept.sentence);
    assert_eq!(kept.approvals.len(), 1);
    assert!(kept.sentence.contains("now idle"), "{}", kept.sentence);
    snap("claude_approval_gated_edit", &pane.timeline.merged(&receipts));
}

// ---- (d) Codex idle repaint after a resize must stay idle -----------------------------------------
//
// VERIFY #9: selecting an idle Codex pane resizes it, Codex reprints, and the pane flashed
// "Working 0s". Correct: idle before, idle through the resize and the reprint, and real work after
// still flips it to running.

#[test]
fn codex_idle_pane_stays_idle_through_a_resize_repaint_and_real_work_still_flips_it() {
    let mut pane = ReplayPane::corpus("codex/0.159.3/idle_repaint_after_resize.cast");
    pane.run_to(Duration::from_secs(9));
    assert_eq!(pane.status(), PaneStatus::Idle, "settled before the resize: {}", pane.timeline.render());
    pane.run_to(Duration::from_secs(19));
    assert_eq!(pane.status(), PaneStatus::Idle, "still idle after the resize and both repaints: {}", pane.timeline.render());
    assert!(
        pane.transitions.iter().all(|(at, status)| !(*at > Duration::from_secs(9) && *status == PaneStatus::Running)),
        "no flicker to running: {}",
        pane.timeline.render()
    );
    pane.run_to(Duration::from_secs(21));
    assert_eq!(pane.status(), PaneStatus::Running, "a turn in flight is still work");
    snap("codex_idle_repaint_after_resize", &pane.timeline);
}

// ---- (e) Claude's dimmed next-prompt suggestion in the input box ----------------------------------
//
// VERIFY-R11 #1 (a regression from the typing permit): after a turn Claude draws a faint suggestion of
// the next prompt (`❯ commit this`) in the input box. Readiness read it as text the user typed, so the
// follow-up parked on `screen_not_ready` and ended PROMPT_NOT_SUBMITTED (4 times on 2 panes).
// Correct: the suggestion is an empty box; the follow-up is typed over it (Claude wipes the suggestion
// as the first key arrives), submitted once, and a turn starts. The cast is the real flight recording
// of a Claude 2.1.300 session; what the pane did when the follow-up was typed is Claude's own bytes.

const FOLLOW_UP: &str = "Reply with only the word OK. Use no tools.";

#[test]
fn claude_follow_up_typed_over_a_dimmed_suggestion_is_delivered_and_not_parked() {
    let mut pane = ReplayPane::corpus("claude/2.1.300/ghost_suggestion_followup.cast");
    pane.run_to(Duration::from_secs(10));
    assert_eq!(pane.status(), PaneStatus::Idle, "the first turn is over: {}", pane.timeline.render());

    // The premise, from the real bytes: the emulator's plain text DOES hold the suggestion in the box...
    assert!(pane.plain_text().contains("\u{276f}\u{a0}commit this"), "{}", pane.plain_text());
    // ...and readiness (which reads through the profile) must not take it for typed text.
    let profile = profile_for("claude");
    let seen = pane.screen_text();
    assert!(!seen.contains("commit this"), "{seen}");
    assert_eq!(profile.ready_recogniser(&seen), Some(crate::tui::ComposerState::Empty), "{seen}");

    let mut trace = Vec::new();
    let outcome = deliver_prompt_traced(&mut pane, profile.clear_input_sequence(), FOLLOW_UP, &mut trace);
    pane.note("deliver", format!("{outcome:?}"));
    assert_eq!(outcome, Delivery::Started, "{trace:#?}\n{}", pane.timeline.render());
    // Nothing but the task and one Enter was typed: no clearing, no retyping.
    assert_eq!(
        pane.inputs.iter().map(|i| (i.text.as_str(), i.enter)).collect::<Vec<_>>(),
        [(FOLLOW_UP, false), ("", true)],
        "{}",
        pane.timeline.render()
    );
    snap("claude_ghost_suggestion_followup", &pane.timeline);
}

// ---- (f) Claude's fullscreen renderer: the input box is pinned to the bottom of the alternate screen --
//
// Round 13 #2: some machines start Claude 2.1.300 in its "fullscreen renderer" (`/tui fullscreen`,
// `CLAUDE_CODE_NO_FLICKER=1`): alternate screen buffer, the box drawn on the bottom rows with the effort
// row above it. Readiness was only ever checked on the classic layout. Correct: the fresh pinned box is
// Empty, the task is typed into it once, submitted with one Enter, and a turn starts. The cast is a real
// session; what Claude wrote when the task was typed and submitted is its own bytes.

#[test]
fn claude_fullscreen_pinned_box_takes_a_task_and_starts_a_turn() {
    let mut pane = ReplayPane::corpus("claude/2.1.300/fullscreen_task_into_pinned_box.cast");
    pane.run_to(Duration::from_secs(4));
    let seen = pane.screen_text();
    assert!(pane.plain_text().contains("Claude Code v2.1.300"), "{}", pane.plain_text());
    let profile = profile_for("claude");
    assert_eq!(profile.ready_recogniser(&seen), Some(crate::tui::ComposerState::Empty), "{seen}");

    let mut trace = Vec::new();
    let outcome = deliver_prompt_traced(&mut pane, profile.clear_input_sequence(), FOLLOW_UP, &mut trace);
    pane.note("deliver", format!("{outcome:?}"));
    assert_eq!(outcome, Delivery::Started, "{trace:#?}\n{}", pane.timeline.render());
    assert_eq!(
        pane.inputs.iter().map(|i| (i.text.as_str(), i.enter)).collect::<Vec<_>>(),
        [(FOLLOW_UP, false), ("", true)],
        "{}",
        pane.timeline.render()
    );
    snap("claude_fullscreen_task_into_pinned_box", &pane.timeline);
}

// ---- (f) a usage limit hit mid-turn (SYNTHETIC cast) ------------------------------------------------
//
// Hand-off, detect-only (`crate::handoff`). The cast is SYNTHETIC: built by hand from the Claude Code
// 2.1.300 string-table texts quoted in the design and the layout of the live 2.1.299 captures. It is
// not a live recording; replace it with a flight recording when a real limit is captured (C1).
// Correct: the limit is not acted on until it has held still, it fires ONCE, its own-wait line makes the card say Claude continues by itself, nothing is
// ever typed into the pane (not by detection, and a task offered at the limit is refused), and when
// Claude carries on by itself it stops counting, and the old notice left above does not fire again.

/// The fixed noon clock, moved forward with the cast's virtual time.
struct CastClock {
    base: crate::handoff::reset::FixedClock,
    elapsed: Duration,
}

impl crate::handoff::reset::Clock for CastClock {
    fn now_ms(&self) -> i64 {
        self.base.now_ms + self.elapsed.as_millis() as i64
    }
    fn zone_name(&self) -> Option<String> {
        self.base.zone_name()
    }
    fn local_at(&self, ms: i64) -> chrono::NaiveDateTime {
        self.base.local_at(ms)
    }
    fn to_ms(&self, local: chrono::NaiveDateTime) -> Option<i64> {
        self.base.to_ms(local)
    }
}

#[test]
fn claude_usage_limit_mid_turn_fires_once_after_it_holds_types_nothing_and_stops_when_claude_carries_on() {
    use crate::handoff::watch::{Cleared, Event, Standing, Tracker};
    use crate::handoff::detect::LimitKind;
    use crate::handoff::reset::Clock;
    let profile = profile_for("claude");
    let mut pane = ReplayPane::corpus("claude/2.1.300/usage_limit_mid_turn.cast");
    let mut tracker = Tracker::default();
    let mut fired: Vec<(Duration, String, bool)> = Vec::new();
    let mut cleared: Vec<(Duration, Cleared)> = Vec::new();
    let mut standing_at = Vec::new();
    let mut last_seen = String::new();

    // The app looks at the pane whenever its screen changes and once a second in between; here every 250 ms.
    for step in 1..=80u64 {
        let at = Duration::from_millis(step * 250);
        pane.run_to(at);
        let clock = CastClock { base: crate::handoff::reset::FixedClock::taipei_noon(), elapsed: at };
        let screen = pane.screen_text();
        let tail = crate::pane_activity::visible_tail(&screen);
        let hit = profile.detect_limit(&screen, &clock);
        if let Some(hit) = &hit {
            let seen = format!("sees {} {}", hit.rule_id, hit.reset_text.as_deref().unwrap_or("-"));
            if hit.kind == LimitKind::Usage && seen != last_seen {
                pane.note("limit", seen.clone());
                last_seen = seen;
            }
        } else {
            last_seen.clear();
        }
        let now = clock.now_ms() as u64;
        let (standing, mut events) = tracker.observe(now, "pane", hit, profile.busy_marker(&tail));
        events.extend(tracker.tick(now));
        standing_at.push((at, standing));
        for event in events {
            match event {
                Event::Fired { hit, .. } => {
                    pane.note("limit", format!("FIRED {} armed={} reset_at={:?}", hit.rule_id, hit.self_resume_armed, hit.reset_text));
                    fired.push((at, hit.rule_id.clone(), hit.self_resume_armed));
                }
                Event::Cleared { why, .. } => {
                    pane.note("limit", format!("cleared {why:?}"));
                    cleared.push((at, why));
                }
            }
        }
    }

    // Held still for 2 s (6.0 -> 8.0), then once. The own-wait line that arrives at 6.4 s is the SAME
    // limit (same window, same reset), so it does not restart the clock; by the time it fires the card
    // knows Claude will continue by itself.
    assert_eq!(fired.len(), 1, "{}", pane.timeline.render());
    let (at, rule, armed) = &fired[0];
    assert_eq!((rule.as_str(), *armed), ("self_resume_armed", true), "{}", pane.timeline.render());
    assert!(*at >= Duration::from_millis(8_000) && *at <= Duration::from_millis(8_250), "fired at {at:?}: {}", pane.timeline.render());
    // Nothing was acted on while it was still settling (6.0 s .. 8.0 s).
    assert!(standing_at.iter().filter(|(t, _)| *t >= Duration::from_millis(6_000) && *t < Duration::from_millis(8_000)).all(|(_, s)| *s == Standing::Pending));
    assert!(standing_at.iter().filter(|(t, _)| *t < Duration::from_millis(6_000)).all(|(_, s)| *s == Standing::Clear), "no limit before it is on screen");
    // Claude carried on by itself at 14 s: the limit stops counting at once, and only once.
    assert_eq!(cleared.len(), 1, "{}", pane.timeline.render());
    assert_eq!(cleared[0].1, Cleared::Resumed);
    assert!(cleared[0].0 >= Duration::from_secs(14) && cleared[0].0 <= Duration::from_millis(14_250));
    // The finished turn still shows the old notice above its answer: history, not a new limit.
    assert!(pane.screen_text().contains("You've hit your session limit"), "the premise: the old notice is still on screen");
    assert_eq!(standing_at.last().unwrap().1, Standing::Clear);

    // Detection never typed anything.
    assert!(pane.inputs.is_empty(), "{:?}", pane.inputs);
    snap("claude_usage_limit_mid_turn", &pane.timeline);

    // A task offered while the limit is up is refused: nothing is typed, no keys are pressed.
    let mut at_the_limit = ReplayPane::corpus("claude/2.1.300/usage_limit_mid_turn.cast");
    at_the_limit.run_to(Duration::from_secs(10));
    let mut trace = Vec::new();
    let outcome = deliver_prompt_traced(&mut at_the_limit, profile.clear_input_sequence(), "Carry on please", &mut trace);
    match outcome {
        Delivery::Blocked(gate) => {
            assert_eq!(gate.id, "usage_limit", "{trace:#?}");
            assert!(gate.dismiss.is_none());
        }
        other => panic!("expected the task to be refused at the limit, got {other:?}\n{trace:#?}"),
    }
    assert!(at_the_limit.inputs.is_empty(), "something was typed at the limit: {:?}", at_the_limit.inputs);
}
