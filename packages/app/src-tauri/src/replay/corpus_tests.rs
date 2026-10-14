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
