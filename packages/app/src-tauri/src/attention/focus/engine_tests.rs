//! Engine tests on a fake `Io` and a virtual clock (SPEC n33 section 5, FT-9).

use super::fake_io::{engine_over, flag_of, rig, MIN, T0};
use super::live::Note;
use super::model::{By, Class, Settings, MAX_MINUTES};
use super::policy::Gate;
use super::scan::ItemFact;
use super::summary::{EndedBy, TurnFact};
use crate::attention::model::{Kind, Level};
use crate::push::triggers::Kind as PushKind;
use std::sync::atomic::Ordering;

fn card(key: &str, kind: Kind, risk: Option<Level>, since: u64) -> ItemFact {
    ItemFact { key: key.into(), kind, risk, since_ms: Some(since), open: true }
}

fn turn(pane: &str, n: u32, at: u64, end: &str) -> TurnFact {
    TurnFact { pane_id: pane.into(), turn: n, ended_at_ms: at, end: end.into(), outcome: None }
}

// ---- start, extend, end ----

#[test]
fn start_extend_and_the_240_cap_are_exact() {
    let (engine, io) = rig();
    let view = engine.start(45, By::Desktop).unwrap();
    assert_eq!(view.desktop.ends_at_ms, Some(T0 + 45 * MIN));
    assert_eq!(view.desktop.started_at_ms, Some(T0));
    assert!(view.desktop.on && view.phone.on);
    assert!(flag_of(&engine).is_on_at(T0 + 45 * MIN - 1));
    assert!(!flag_of(&engine).is_on_at(T0 + 45 * MIN));

    let view = engine.extend(By::Phone).unwrap();
    assert_eq!(view.desktop.ends_at_ms, Some(T0 + 60 * MIN));
    assert_eq!(view.desktop.extended, 1);

    // A 240 minute Focus cannot grow: every +15 is clamped to the start plus 240.
    io.set_now(T0 + 100 * MIN);
    engine.end(By::Desktop);
    io.set_now(T0 + 101 * MIN);
    let begun = T0 + 101 * MIN;
    let view = engine.start(240, By::Desktop).unwrap();
    assert_eq!(view.desktop.ends_at_ms, Some(begun + 240 * MIN));
    let view = engine.extend(By::Desktop).unwrap();
    assert_eq!(view.desktop.ends_at_ms, Some(begun + 240 * MIN), "never longer than 240 minutes from the start");
    assert_eq!(u64::from(MAX_MINUTES) * MIN, 240 * MIN);
}

#[test]
fn bad_lengths_are_refused_with_the_stable_code() {
    let (engine, _io) = rig();
    for minutes in [0, 4, 241, 10_000] {
        let err = engine.start(minutes, By::Desktop).unwrap_err();
        assert_eq!(err.code, "FOCUS_BAD_MINUTES", "{minutes}");
    }
    assert!(!engine.view().desktop.on);
    assert!(engine.start(5, By::Desktop).is_ok());
    assert!(engine.start(240, By::Desktop).is_ok());
}

#[test]
fn a_plain_local_program_may_end_but_never_start_or_extend() {
    let (engine, _io) = rig();
    assert_eq!(engine.start(45, By::Local).unwrap_err().code, "FOCUS_PERSON_ONLY");
    assert!(!engine.view().desktop.on);
    engine.start(45, By::Desktop).unwrap();
    assert_eq!(engine.extend(By::Local).unwrap_err().code, "FOCUS_PERSON_ONLY");
    assert_eq!(engine.start(90, By::Local).unwrap_err().code, "FOCUS_PERSON_ONLY");
    assert!(!engine.end(By::Local).desktop.on, "ending is always safe");
}

#[test]
fn extend_while_off_says_focus_is_not_on() {
    let (engine, io) = rig();
    assert_eq!(engine.extend(By::Desktop).unwrap_err().code, "FOCUS_NOT_ON");
    assert!(io.emits.lock().is_empty());
}

#[test]
fn pressing_focus_while_on_keeps_the_start_and_the_held_list() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    assert!(engine.hold(Class::RunDone, "Codex", None));
    io.advance(10 * MIN);
    let view = engine.start(90, By::Tray).unwrap();
    assert_eq!(view.desktop.started_at_ms, Some(T0), "the start time is kept");
    assert_eq!(view.desktop.ends_at_ms, Some(T0 + 100 * MIN), "the new length counts from now");
    assert_eq!(view.desktop.minutes, 90);
    assert_eq!(view.desktop.held_total, 1, "the held list is kept");
    assert_eq!(view.desktop.session_id, engine.view().desktop.session_id, "still one session");
}

// ---- the end ----

#[test]
fn a_timer_end_builds_one_summary_one_digest_one_push_note_and_one_chat_note() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    io.turns.lock().extend([turn("p1", 1, T0 + 5 * MIN, "idle"), turn("p1", 2, T0 + 9 * MIN, "idle"), turn("p2", 1, T0 + 12 * MIN, "error")]);
    engine.hold(Class::Companion, "Mochi", None);

    io.set_now(T0 + 25 * MIN - 1);
    engine.tick();
    assert!(io.digests.lock().is_empty(), "not over yet");

    io.set_now(T0 + 25 * MIN);
    engine.tick();
    let digests = io.digests.lock().clone();
    assert_eq!(digests.len(), 1);
    let summary = &digests[0];
    assert_eq!(summary.ended_by, EndedBy::Timer);
    assert_eq!(summary.ended_at_ms, T0 + 25 * MIN);
    assert_eq!(summary.counts.turns_done, 2);
    assert_eq!(summary.counts.failed, 1);
    assert!(summary.line.starts_with("While you focused (25 min): 2 turns done, 1 failed"), "{}", summary.line);

    let notes = io.notes.lock().clone();
    assert_eq!(notes.len(), 1);
    let Note::Summary { body, line } = &notes[0] else { panic!("a summary note") };
    assert!(line.starts_with("While you focused"));
    assert!(body.starts_with("2 turns done, 1 failed"), "{body}");

    // A second tick, an `end` and a late tick change nothing.
    io.advance(MIN);
    engine.tick();
    engine.end(By::Desktop);
    engine.end(By::Local);
    assert_eq!(io.digests.lock().len(), 1);
    assert_eq!(io.notes.lock().len(), 1);
    assert!(!flag_of(&engine).is_on_at(T0 + 26 * MIN));
    assert_eq!(engine.view().desktop.last_summary.as_ref().map(|s| s.session_id.clone()), Some(summary.session_id.clone()));
}

#[test]
fn an_early_end_says_after_how_long_and_audits_counts_only() {
    let (engine, io) = rig();
    engine.start(45, By::Desktop).unwrap();
    io.turns.lock().push(turn("p1", 1, T0 + 5 * MIN, "idle"));
    io.set_now(T0 + 18 * MIN);
    let view = engine.end(By::Tray);
    assert!(!view.desktop.on);
    let summary = io.digests.lock()[0].clone();
    assert_eq!(summary.ended_by, EndedBy::Early);
    assert!(summary.line.starts_with("Focus ended after 18 min: 1 turn done"), "{}", summary.line);

    let audits = io.audits.lock().clone();
    let (who, text, detail) = audits.last().unwrap();
    assert_eq!(*who, Some(By::Tray));
    assert_eq!(text, "Focus ended: 1 turn done");
    assert_eq!(detail["turns_done"], 1);
    for (_, line, detail) in &audits {
        let all = format!("{line} {detail}");
        assert!(!all.contains("p1"), "no pane id in the audit: {all}");
    }
}

#[test]
fn an_empty_session_shows_no_card_and_sends_nothing() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    io.set_now(T0 + 25 * MIN);
    engine.tick();
    assert!(!engine.view().desktop.on);
    assert!(io.digests.lock().is_empty(), "no card");
    assert!(io.notes.lock().is_empty(), "no notes");
    assert!(engine.view().desktop.last_summary.is_none(), "nothing to say Got it to");
    assert_eq!(io.audits.lock().last().unwrap().1, "Focus ended", "the audit line still records the end");
}

#[test]
fn turns_that_ended_after_the_end_are_not_part_of_the_session() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    io.turns.lock().extend([turn("p1", 1, T0 + 10 * MIN, "idle"), turn("p1", 2, T0 + 40 * MIN, "idle")]);
    io.set_now(T0 + 60 * MIN); // a late tick
    engine.tick();
    let summary = io.digests.lock()[0].clone();
    assert_eq!(summary.counts.turns_done, 1);
    assert_eq!(summary.ended_at_ms, T0 + 25 * MIN, "a timer end is at the deadline, not at the late tick");
}

#[test]
fn a_held_run_end_is_counted_and_not_forwarded() {
    let (engine, _io) = rig();
    assert!(!engine.hold(Class::RunDone, "Codex", None), "off: the caller sends as today");
    engine.start(45, By::Desktop).unwrap();
    assert_eq!(engine.gate_push(PushKind::RunFinished, None), Gate::Hold);
    assert_eq!(engine.gate_push(PushKind::RunFailed, None), Gate::Hold);
    assert_eq!(engine.gate_push(PushKind::CompanionReply, None), Gate::Hold);
    assert_eq!(engine.gate_push(PushKind::NightShift, None), Gate::Hold);
    assert!(engine.hold(Class::RunDone, "Codex", Some("Fixed the tests")));
    assert!(engine.hold(Class::RunFailed, "Codex", None));
    assert!(engine.hold(Class::Review, "Codex", None));
    let view = engine.view().desktop;
    assert_eq!(view.held_total, 3);
    assert_eq!(view.held.run_done, 1);
    assert_eq!(view.held.review, 1);
}

#[test]
fn the_held_list_stops_at_the_cap_but_the_counts_stay_exact() {
    let (engine, io) = rig();
    engine.start(45, By::Desktop).unwrap();
    for _ in 0..150 {
        assert!(engine.hold(Class::RunDone, "Codex", None));
    }
    assert_eq!(engine.view().desktop.held_total, 150);
    assert_eq!(io.disk.lock().ledger.held.len(), 0, "hooks do not save; the tick does");
    engine.tick();
    assert_eq!(io.disk.lock().ledger.held.len(), 100);
    assert_eq!(io.disk.lock().ledger.counts.run_done, 150);
}

#[test]
fn held_sentences_are_redacted_and_cut_before_they_are_kept() {
    let (engine, io) = rig();
    engine.start(45, By::Desktop).unwrap();
    let secret_a = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let secret_b = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let raw = format!("Wrote {secret_a} and {secret_b} into C:\\Users\\you\\work\\app\\.env\n then   stopped");
    assert!(engine.hold(Class::RunDone, &format!("Codex {secret_a}"), Some(&raw)));
    engine.hold(Class::RunDone, "Codex", Some(&"long ".repeat(100)));
    engine.tick();
    let held = io.disk.lock().ledger.held.clone();
    let text = format!("{} | {} | {}", held[0].agent, held[0].sentence.clone().unwrap(), held[1].sentence.clone().unwrap());
    assert!(!text.contains("AAAAAAAAAAAAAAAA"), "{text}");
    assert!(!text.contains("abcdefghijklmnop"), "{text}");
    assert!(!text.contains("C:\\Users"), "{text}");
    assert!(!text.contains('\n'), "one line");
    assert!(held[0].sentence.as_ref().unwrap().contains(".env"), "paths keep only their last name");
    assert!(held[1].sentence.as_ref().unwrap().chars().count() <= 120);
}

// ---- the tiers at the gate ----

#[test]
fn medium_is_quiet_high_risk_budget_and_consent_go_through() {
    let (engine, _io) = rig();
    assert_eq!(engine.gate_push(PushKind::WaitingInput, Some(Level::Medium)), Gate::Send, "off: everything sends");
    engine.start(45, By::Desktop).unwrap();
    assert_eq!(engine.gate_push(PushKind::WaitingInput, Some(Level::Medium)), Gate::Quiet);
    assert_eq!(engine.gate_push(PushKind::WaitingInput, None), Gate::Quiet);
    assert_eq!(engine.gate_push(PushKind::WaitingInput, Some(Level::High)), Gate::Send);
    assert_eq!(engine.gate_push(PushKind::Budget, None), Gate::Send);
    assert_eq!(engine.gate_push(PushKind::Consent, None), Gate::Send);
    assert_eq!(engine.gate_push(PushKind::TrustGate, None), Gate::Quiet);
    assert_eq!(engine.gate_push(PushKind::Focus, None), Gate::Send, "Focus's own messages are never gated");
    assert!(engine.quiet_for(PushKind::Question, None));
    assert!(!engine.quiet_for(PushKind::Budget, None));
}

#[test]
fn quiet_blocking_off_sends_waiting_cards_at_full_strength() {
    let (engine, _io) = rig();
    engine.settings_set(Settings { default_minutes: 45, quiet_blocking: false }).unwrap();
    engine.start(45, By::Desktop).unwrap();
    assert_eq!(engine.gate_push(PushKind::WaitingInput, Some(Level::Medium)), Gate::Send);
    assert_eq!(engine.gate_push(PushKind::Question, None), Gate::Send);
    assert_eq!(engine.gate_push(PushKind::RunFinished, None), Gate::Hold, "hold is unchanged by the switch");
    assert!(!engine.quiet_for(PushKind::Question, None));
}

// ---- scan and the reminder ----

#[test]
fn a_quiet_card_gets_one_reminder_at_twenty_minutes() {
    let (engine, io) = rig();
    engine.start(90, By::Desktop).unwrap();
    // A card from before Focus is not a waiter; the new one is.
    io.items.lock().extend([
        card("question:old", Kind::Question, None, T0 - 5 * MIN),
        card("question:a", Kind::Question, None, T0 + MIN),
        card("budget:b", Kind::Budget, None, T0 + MIN),
    ]);
    io.set_now(T0 + 2 * MIN);
    engine.tick();
    let view = engine.view().desktop;
    assert_eq!((view.waiting_quiet, view.waiting_urgent), (1, 1));

    io.set_now(T0 + 21 * MIN - 1);
    engine.tick();
    assert!(io.notes.lock().is_empty(), "19:59 after the card appeared: no reminder yet");

    io.set_now(T0 + 21 * MIN);
    engine.tick();
    assert_eq!(io.notes.lock().clone(), vec![Note::Reminder { waiting: 1 }], "exactly one, quiet cards only");

    io.set_now(T0 + 60 * MIN);
    engine.tick();
    assert_eq!(io.notes.lock().len(), 1, "once per card");
}

#[test]
fn a_card_that_closed_is_not_reminded_and_the_switch_off_sends_no_reminder() {
    let (engine, io) = rig();
    engine.start(90, By::Desktop).unwrap();
    io.items.lock().push(card("question:a", Kind::Question, None, T0 + MIN));
    io.set_now(T0 + 2 * MIN);
    engine.tick();
    io.items.lock().clear();
    io.set_now(T0 + 40 * MIN);
    engine.tick();
    assert!(io.notes.lock().is_empty(), "the card was answered");

    let (engine, io) = rig();
    engine.settings_set(Settings { default_minutes: 45, quiet_blocking: false }).unwrap();
    engine.start(90, By::Desktop).unwrap();
    io.items.lock().push(card("question:a", Kind::Question, None, T0 + MIN));
    io.set_now(T0 + 2 * MIN);
    engine.tick();
    io.set_now(T0 + 40 * MIN);
    engine.tick();
    assert!(io.notes.lock().is_empty(), "those cards went out at normal strength already");
}

#[test]
fn the_end_summary_counts_what_asked_and_what_still_waits() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    io.items.lock().extend([
        card("question:a", Kind::Question, None, T0 + MIN),
        card("approval:b", Kind::Approval, Some(Level::High), T0 + 2 * MIN),
    ]);
    io.open.store(2, Ordering::SeqCst);
    io.set_now(T0 + 3 * MIN);
    engine.tick();
    io.set_now(T0 + 25 * MIN);
    engine.tick();
    let summary = io.digests.lock()[0].clone();
    assert_eq!((summary.counts.asked, summary.counts.urgent, summary.counts.still_waiting), (1, 1, 2));
    let Note::Summary { body, .. } = io.notes.lock()[0].clone() else { panic!("summary") };
    assert_eq!(body, "1 asked for you, 1 urgent. 2 still wait.");
}

// ---- restart ----

#[test]
fn a_restart_inside_the_window_resumes_it() {
    let (engine, io) = rig();
    engine.start(45, By::Desktop).unwrap();
    engine.hold(Class::RunDone, "Codex", None);
    io.advance(MIN);
    engine.tick(); // saves the ledger

    io.advance(10 * MIN);
    let again = engine_over(&io);
    assert!(!again.view().desktop.on, "a new engine knows nothing until it loads");
    again.load_and_resume();
    let view = again.view().desktop;
    assert!(view.on);
    assert_eq!(view.ends_at_ms, Some(T0 + 45 * MIN));
    assert_eq!(view.held_total, 1, "the ledger came back");
    assert!(flag_of(&again).is_on_at(T0 + 20 * MIN));
    assert!(io.digests.lock().is_empty());
}

#[test]
fn a_restart_after_the_end_ends_it_once_with_the_summary_pending() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    io.turns.lock().push(turn("p1", 1, T0 + 5 * MIN, "idle"));
    io.advance(MIN);
    engine.tick();

    io.set_now(T0 + 3 * 60 * MIN);
    let again = engine_over(&io);
    again.load_and_resume();
    assert!(!again.view().desktop.on);
    assert_eq!(io.digests.lock().len(), 1);
    assert_eq!(io.notes.lock().len(), 1);
    assert_eq!(io.digests.lock()[0].ended_at_ms, T0 + 25 * MIN);
    assert!(again.view().desktop.last_summary.is_some(), "waiting for Got it");

    // A second restart does not end it again.
    let third = engine_over(&io);
    third.load_and_resume();
    assert_eq!(io.digests.lock().len(), 1);
    assert_eq!(io.notes.lock().len(), 1);
}

#[test]
fn a_session_that_ends_further_away_than_the_cap_is_ended_on_load() {
    let (engine, io) = rig();
    engine.start(45, By::Desktop).unwrap();
    // The clock was set far back: the end is now more than 240 minutes away.
    io.set_now(T0 - 500 * MIN);
    let again = engine_over(&io);
    again.load_and_resume();
    assert!(!again.view().desktop.on);
    assert!(!flag_of(&again).is_on_at(T0 - 500 * MIN));
}

#[test]
fn a_damaged_or_missing_file_means_focus_off() {
    let (engine, _io) = rig();
    engine.load_and_resume();
    assert!(!engine.view().desktop.on);
    assert!(!flag_of(&engine).is_on_at(T0));
}

// ---- fail open ----

#[test]
fn if_the_tick_never_runs_the_hooks_still_stop_holding_at_the_deadline() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    let deadline = T0 + 25 * MIN;
    assert_eq!(engine.gate_push(PushKind::RunFinished, None), Gate::Hold);

    io.set_now(deadline + 1); // no tick ran
    assert!(!flag_of(&engine).is_on_at(deadline + 1));
    assert_eq!(engine.gate_push(PushKind::RunFinished, None), Gate::Send, "pings flow again");
    assert!(!engine.hold(Class::RunDone, "Codex", None), "nothing is held after the deadline");
    assert!(!engine.view().desktop.on, "the view reads off too");
    assert!(io.digests.lock().is_empty(), "the summary waits for the tick or the next start");

    // The next start ends the old one first, so its summary is not lost.
    io.turns.lock().push(turn("p1", 1, T0 + MIN, "idle"));
    engine.start(25, By::Desktop).unwrap();
    assert_eq!(io.digests.lock().len(), 1);
}

// ---- settings and dismiss ----

#[test]
fn settings_are_validated_saved_and_audited() {
    let (engine, io) = rig();
    assert_eq!(engine.settings_set(Settings { default_minutes: 3, quiet_blocking: true }).unwrap_err().code, "FOCUS_BAD_MINUTES");
    assert_eq!(engine.settings_set(Settings { default_minutes: 300, quiet_blocking: true }).unwrap_err().code, "FOCUS_BAD_MINUTES");
    let view = engine.settings_set(Settings { default_minutes: 90, quiet_blocking: false }).unwrap();
    assert_eq!(view.desktop.settings, Settings { default_minutes: 90, quiet_blocking: false });
    assert_eq!(view.desktop.minutes, 90, "off: the view shows the default length");
    assert_eq!(io.disk.lock().settings.default_minutes, 90);
    assert_eq!(io.audits.lock().last().unwrap().1, "Focus settings changed");
}

#[test]
fn got_it_clears_the_last_summary_and_the_pending_digest() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    io.turns.lock().push(turn("p1", 1, T0 + MIN, "idle"));
    io.advance(5 * MIN);
    engine.end(By::Desktop);
    assert!(engine.view().desktop.last_summary.is_some());
    engine.summary_dismiss();
    assert!(engine.view().desktop.last_summary.is_none());
    assert_eq!(io.dismissed.load(Ordering::SeqCst), 1);
    assert!(io.disk.lock().last_summary.is_none(), "saved too");
}

#[test]
fn the_next_start_clears_the_old_summary() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    io.turns.lock().push(turn("p1", 1, T0 + MIN, "idle"));
    io.advance(5 * MIN);
    engine.end(By::Desktop);
    assert!(engine.view().desktop.last_summary.is_some());
    engine.start(25, By::Desktop).unwrap();
    assert!(engine.view().desktop.last_summary.is_none());
}

#[test]
fn every_change_emits_a_view_and_the_phone_view_has_no_names() {
    let (engine, io) = rig();
    engine.start(25, By::Desktop).unwrap();
    engine.extend(By::Desktop).unwrap();
    engine.end(By::Desktop);
    assert_eq!(io.emits.lock().len(), 3);
    let phone = serde_json::to_string(&engine.view().phone).unwrap();
    assert!(!phone.contains("pane") && !phone.contains("Codex"), "{phone}");
}
