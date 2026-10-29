//! Runtime tests: when the poller runs, what a refused poller does, and what the chat is told. A fake
//! bot and the fake host stand in for Telegram and the app; the poller and send threads are real.

use crate::channels::host::Host;
use crate::channels::fake_bot::{rig, text_from_paired, turn, wait_for, Rig, TOKEN_SECRET};
use crate::channels::refs::Target;
use crate::channels::telegram::api::ApiError;
use crate::channels::test_host::{FakeHost, T0};
use crate::push::hub::Candidate;
use crate::push::triggers::{Kind, PromptInfo};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Paired and on (the `FakeHost` default), with a token in the store.
fn on() -> Rig {
    rig(FakeHost::new()).with_token()
}

/// Paired but switched off.
fn paired_off() -> Rig {
    let host = FakeHost::new();
    host.edit_config(|c| {
        c.enabled = false;
        c.enabled_at_ms = None;
    });
    rig(host).with_token()
}

fn candidate(kind: Kind, key: &str, pane: &str) -> Candidate {
    Candidate {
        kind,
        key: key.into(),
        pane_id: Some(pane.into()),
        agent_label: "Codex".into(),
        heading: Some("auth refactor".into()),
        prompt: None,
        item: None,
        detail: None,
    }
}

fn polls(r: &Rig) -> usize {
    r.bot.polls.load(Ordering::SeqCst)
}

/// No new poll for a moment: the old poller is really gone.
fn polls_settled(r: &Rig) -> bool {
    let before = polls(r);
    std::thread::sleep(Duration::from_millis(40));
    polls(r) == before
}

#[test]
fn nothing_starts_when_the_person_has_not_turned_it_on() {
    let r = paired_off();
    r.rt.apply();
    std::thread::sleep(Duration::from_millis(30));
    assert!(!r.rt.alive());
    assert!(!r.rt.listening());
    assert_eq!(polls(&r), 0, "the bot must not be asked for messages while the channel is off");
}

#[test]
fn an_unpaired_install_with_a_token_does_not_start_either() {
    let r = rig(FakeHost::unpaired()).with_token();
    r.rt.apply();
    std::thread::sleep(Duration::from_millis(30));
    assert!(!r.rt.alive());
    assert_eq!(polls(&r), 0);
}

#[test]
fn on_without_a_token_does_not_start() {
    let r = rig(FakeHost::new());
    r.rt.apply();
    assert!(!r.rt.alive());
    assert_eq!(polls(&r), 0);
}

#[test]
fn a_saved_on_with_a_paired_chat_starts_at_apply_and_listens() {
    let r = on();
    assert!(!r.rt.listening());
    r.rt.apply();
    wait_for("the first poll", || polls(&r) > 0);
    assert!(r.rt.alive());
    assert!(r.rt.listening());
    r.rt.stop();
}

#[test]
fn stop_ends_the_poller_and_the_listening() {
    let r = on();
    r.rt.apply();
    wait_for("the first poll", || polls(&r) > 0);
    r.rt.stop();
    assert!(!r.rt.listening());
    wait_for("the poller thread to end", || polls_settled(&r));
}

#[test]
fn restart_starts_a_new_poller_from_the_saved_settings() {
    let r = on();
    r.rt.apply();
    wait_for("the first poll", || polls(&r) > 0);
    let before = r.rt.generation.load(Ordering::SeqCst);
    r.rt.restart();
    assert!(r.rt.generation.load(Ordering::SeqCst) > before, "the old poller must be told to stop");
    assert!(r.rt.alive());
    r.rt.stop();
}

#[test]
fn a_poller_telegram_refused_is_not_started_again_by_the_5_second_watch() {
    let r = on();
    *r.bot.poll_error.lock() = Some(ApiError::Unauthorized);
    r.rt.apply();
    wait_for("the poller to end", || !r.rt.alive());
    let status = r.rt.status();
    assert_eq!(status.state, "error");
    assert_eq!(status.error, Some("token_rejected"));
    let after_failure = polls(&r);
    for _ in 0..5 {
        r.rt.watch();
        r.rt.tick();
    }
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(polls(&r), after_failure, "no retry storm");
    assert!(!r.rt.alive());
    assert!(!r.rt.listening());
}

#[test]
fn the_persons_next_action_tries_a_refused_poller_again() {
    let r = on();
    *r.bot.poll_error.lock() = Some(ApiError::Conflict);
    r.rt.apply();
    wait_for("the poller to end", || !r.rt.alive());
    assert_eq!(r.rt.status().error, Some("conflict"));
    r.rt.restart();
    wait_for("a poll after the restart", || polls(&r) > 1);
    assert!(r.rt.alive());
    r.rt.stop();
}

#[test]
fn slash_stop_from_the_chat_ends_the_poller_and_switches_it_off() {
    let r = on();
    r.bot.push_updates(vec![text_from_paired(10, "/stop")]);
    r.rt.apply();
    wait_for("the poller to end", || !r.rt.alive());
    assert!(!r.host.config().enabled, "/stop saves enabled=false");
    r.rt.tick();
    assert!(r.rt.live.lock().is_none(), "the finished poller is forgotten");
    assert!(!r.rt.listening());
    assert!(r.bot.texts().iter().any(|t| t.contains("Telegram is off")), "the chat is told");
}

#[test]
fn the_watch_ends_a_pairing_poller_when_the_code_runs_out() {
    let r = rig(FakeHost::unpaired()).with_token();
    r.rt.pair_start().unwrap();
    assert!(r.rt.alive(), "pairing mode reads /start");
    assert!(!r.rt.listening(), "but it never speaks to a chat: nothing is paired");
    r.host.set_now(T0 + crate::channels::pairing::CODE_TTL_MS + 1);
    r.rt.watch();
    assert!(!r.rt.alive());
    assert!(r.rt.status().pairing.is_none());
}

// ---- what the chat is told -------------------------------------------------------------------

#[test]
fn a_candidate_is_ignored_while_the_chat_is_not_listening() {
    let r = paired_off();
    r.rt.offer(&candidate(Kind::Error, "p1", "p1"));
    std::thread::sleep(Duration::from_millis(30));
    assert!(r.bot.texts().is_empty());
}

#[test]
fn a_needs_you_candidate_reaches_the_chat_when_listening() {
    let r = on();
    r.rt.apply();
    wait_for("listening", || r.rt.listening());
    r.rt.offer(&candidate(Kind::Error, "p1", "p1"));
    wait_for("the message", || !r.bot.texts().is_empty());
    let text = r.bot.texts().remove(0);
    assert!(text.contains("auth refactor"), "{text}");
    r.rt.stop();
}

#[test]
fn what_the_chat_is_told_has_its_secrets_masked() {
    let r = on();
    let mut asked = candidate(Kind::Question, "q-q1", "p1");
    asked.pane_id = None;
    asked.prompt = Some(PromptInfo {
        prompt_id: String::new(),
        text: "Use key sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWx and mail you@example.com from C:\\Users\\you\\secret?".into(),
        actionable: false,
        risk: None,
    });
    r.rt.apply();
    wait_for("listening", || r.rt.listening());
    r.rt.offer(&asked);
    wait_for("the message", || !r.bot.texts().is_empty());
    let text = r.bot.texts().remove(0);
    for secret in ["sk-ant-api03", "you@example.com", "C:\\Users\\you", TOKEN_SECRET] {
        assert!(!text.contains(secret), "leaked {secret}: {text}");
    }
    r.rt.stop();
}

#[test]
fn a_companion_reply_comes_only_inside_a_conversation_the_chat_started() {
    let r = on();
    r.rt.apply();
    wait_for("listening", || r.rt.listening());
    r.rt.companion_reply("mochi", "Mochi", "The build is green.");
    std::thread::sleep(Duration::from_millis(40));
    assert!(r.bot.texts().is_empty(), "no window, no message (D-8)");
    r.rt.engine.refs().lock().open_window(&Target::Companion("mochi".into()), T0);
    r.rt.companion_reply("mochi", "Mochi", "The build is green.");
    wait_for("the reply", || !r.bot.texts().is_empty());
    assert!(r.bot.texts()[0].contains("The build is green."));
    r.rt.stop();
}

#[test]
fn held_urgent_messages_become_one_note_on_the_tick() {
    let r = on();
    r.rt.apply();
    wait_for("listening", || r.rt.listening());
    r.rt.offer(&candidate(Kind::Error, "p1", "p1"));
    wait_for("the first message", || r.bot.texts().len() == 1);
    // Same pane again inside the 30 s gate: held, not dropped.
    r.rt.offer(&candidate(Kind::Error, "p1", "p1"));
    r.host.set_now(T0 + 31_000);
    r.rt.tick();
    wait_for("the summary note", || r.bot.texts().len() == 2);
    assert!(r.bot.texts()[1].contains("more thing"), "{:?}", r.bot.texts());
    r.rt.stop();
}

#[test]
fn a_turn_that_ends_in_a_pane_the_chat_texted_is_reported_with_its_sentence() {
    let r = on();
    r.host.pane("p1", "auth refactor", "Codex", "idle");
    r.rt.apply();
    wait_for("listening", || r.rt.listening());
    r.rt.engine.refs().lock().open_window(&Target::Pane("p1".into()), T0);
    r.turns.lock().push(turn("p1", T0 + 2_000, "Ran the tests, 42 passed."));
    r.host.set_now(T0 + 5_000);
    r.rt.tick();
    wait_for("the done line", || !r.bot.texts().is_empty());
    assert_eq!(r.bot.texts()[0], "Done · auth refactor: Ran the tests, 42 passed.");
    r.rt.stop();
}

#[test]
fn a_summary_that_shows_up_after_the_tick_that_could_have_seen_the_turn_is_still_reported_once() {
    let r = on();
    r.host.pane("p1", "auth refactor", "Codex", "idle");
    r.rt.apply();
    wait_for("listening", || r.rt.listening());
    r.rt.engine.refs().lock().open_window(&Target::Pane("p1".into()), T0);
    // The turn ended at T0+1 s, but its summary is only published at T0+10 s (settle step).
    r.host.set_now(T0 + 5_000);
    r.rt.tick();
    std::thread::sleep(Duration::from_millis(40));
    assert!(r.bot.texts().is_empty(), "no summary yet");
    r.host.set_now(T0 + 7_000);
    r.rt.tick();
    r.turns.lock().push(turn("p1", T0 + 1_000, "Ran the tests, 42 passed."));
    r.host.set_now(T0 + 10_000);
    r.rt.tick();
    wait_for("the done line", || !r.bot.texts().is_empty());
    assert_eq!(r.bot.texts()[0], "Done · auth refactor: Ran the tests, 42 passed.");
    // The same turn is not reported again by later ticks.
    r.host.set_now(T0 + 15_000);
    r.rt.tick();
    r.host.set_now(T0 + 20_000);
    r.rt.tick();
    std::thread::sleep(Duration::from_millis(40));
    assert_eq!(r.bot.texts().len(), 1, "{:?}", r.bot.texts());
    r.rt.stop();
}

#[test]
fn a_turn_older_than_the_lookback_is_never_reported() {
    let r = on();
    r.host.pane("p1", "auth refactor", "Codex", "idle");
    r.rt.apply();
    wait_for("listening", || r.rt.listening());
    r.rt.engine.refs().lock().open_window(&Target::Pane("p1".into()), T0);
    r.turns.lock().push(turn("p1", T0 + 1_000, "Ran the tests, 42 passed."));
    r.host.set_now(T0 + 300_000);
    r.rt.tick();
    std::thread::sleep(Duration::from_millis(40));
    assert!(r.bot.texts().is_empty());
    r.rt.stop();
}

#[test]
fn a_turn_in_a_pane_the_chat_never_texted_is_not_reported() {
    let r = on();
    r.host.pane("p1", "auth refactor", "Codex", "idle");
    r.rt.apply();
    wait_for("listening", || r.rt.listening());
    r.turns.lock().push(turn("p1", T0 + 2_000, "Ran the tests, 42 passed."));
    r.host.set_now(T0 + 5_000);
    r.rt.tick();
    std::thread::sleep(Duration::from_millis(40));
    assert!(r.bot.texts().is_empty());
    r.rt.stop();
}

#[test]
fn turns_that_ended_while_the_chat_was_off_are_not_reported_after_it_is_turned_on() {
    let r = paired_off();
    r.host.pane("p1", "auth refactor", "Codex", "idle");
    r.turns.lock().push(turn("p1", T0 + 2_000, "Ran the tests, 42 passed."));
    r.host.set_now(T0 + 10_000);
    r.rt.tick(); // off: the clock for turns moves on
    r.host.edit_config(|c| {
        c.enabled = true;
        c.enabled_at_ms = Some(T0 + 10_000);
    });
    r.rt.apply();
    wait_for("listening", || r.rt.listening());
    r.rt.engine.refs().lock().open_window(&Target::Pane("p1".into()), T0 + 10_000);
    r.host.set_now(T0 + 15_000);
    r.rt.tick();
    std::thread::sleep(Duration::from_millis(40));
    assert!(r.bot.texts().is_empty());
    r.rt.stop();
}
