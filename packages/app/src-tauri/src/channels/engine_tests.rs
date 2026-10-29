//! Tests of the engine against the fake host: every refusal path, and the happy paths next to them.
//! Every batch goes through `run`, which also checks that no outgoing text holds a secret, an
//! e-mail address or a home folder (the property over the whole suite).

use super::*;
use crate::attention::model::{Item, Kind as ItemKind, Level, State};
use crate::channels::grants::{GrantAction, ANSWER_TTL_MS, CONFIRM_TTL_MS};
use crate::channels::telegram::api::{InCallback, InMessage, Update, UpdateKind};
use crate::channels::test_host::{approval, card, pending_question, question_item, FakeHost, PAIRED_ID, STRANGER_ID, T0};
use crate::mobile_pairing::Caps;
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};

const SECRET: &str = "sk-ant-api03-TESTKEYTESTKEY123456";
const EMAIL: &str = "you@example.com";
const HOME: &str = "C:\\Users\\you\\secret";

static NEXT_ID: AtomicI64 = AtomicI64::new(100);

fn next_id() -> i64 {
    NEXT_ID.fetch_add(1, Ordering::SeqCst)
}

fn message(chat_id: i64, user: i64, text: &str, age_s: i64, reply_to: Option<i64>) -> InMessage {
    InMessage {
        message_id: next_id(),
        chat_id,
        chat_private: true,
        from_id: Some(user),
        from_is_bot: false,
        username: Some("anna_k".into()),
        first_name: Some("Anna".into()),
        date_s: (T0 / 1000) as i64 - age_s,
        text: Some(text.into()),
        reply_to,
    }
}

fn upd(m: InMessage) -> Update {
    Update { update_id: next_id(), kind: UpdateKind::Message(m) }
}

/// A fresh message from the paired person.
fn say_to_bot(text: &str) -> Update {
    upd(message(PAIRED_ID, PAIRED_ID, text, 5, None))
}

fn aged(text: &str, age_s: i64) -> Update {
    upd(message(PAIRED_ID, PAIRED_ID, text, age_s, None))
}

fn reply(text: &str, to: i64) -> Update {
    upd(message(PAIRED_ID, PAIRED_ID, text, 5, Some(to)))
}

fn tap(data: &str) -> Update {
    tap_as(PAIRED_ID, Some(PAIRED_ID), data)
}

fn tap_as(user: i64, chat: Option<i64>, data: &str) -> Update {
    Update {
        update_id: next_id(),
        kind: UpdateKind::Callback(InCallback { id: format!("cb{}", next_id()), from_id: user, chat_id: chat, message_id: Some(900), data: Some(data.into()) }),
    }
}

fn tap_token(token: &str) -> Update {
    tap(&format!("g:{token}"))
}

/// Every text an effect would put in front of the person.
fn texts(effects: &[Effect]) -> Vec<String> {
    let mut all = Vec::new();
    for effect in effects {
        match effect {
            Effect::Send { out, .. } | Effect::SendTo { out, .. } | Effect::Edit { out, .. } => {
                all.push(out.text.as_str().to_string());
                for button in out.keyboard.iter().flatten() {
                    all.push(button.label.clone());
                }
            }
            Effect::AnswerCallback { text: Some(t), .. } => all.push(t.as_str().to_string()),
            Effect::AnswerCallback { text: None, .. } | Effect::Stop => {}
        }
    }
    all
}

/// The property over the whole suite: nothing leaves with a secret, an e-mail address, a home
/// folder, or a hidden character.
fn assert_clean(effects: &[Effect]) {
    for text in texts(effects) {
        for bad in [SECRET, "sk-ant", EMAIL, "C:\\Users\\you", "\\you\\", "\u{202E}", "\u{1b}"] {
            assert!(!text.contains(bad), "an outgoing text holds {bad:?}: {text:?}");
        }
    }
}

fn run(engine: &Engine, host: &FakeHost, updates: Vec<Update>) -> Vec<Effect> {
    let effects = engine.on_batch(host, &updates);
    assert_clean(&effects);
    effects
}

/// Messages sent to the chat (not edits, not toasts).
fn sent(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send { out, .. } | Effect::SendTo { out, .. } => Some(out.text.as_str().to_string()),
            _ => None,
        })
        .collect()
}

fn edits(effects: &[Effect]) -> Vec<(i64, Outgoing)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Edit { message_id, out } => Some((*message_id, out.clone())),
            _ => None,
        })
        .collect()
}

fn toasts(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::AnswerCallback { text: Some(t), .. } => Some(t.as_str().to_string()),
            _ => None,
        })
        .collect()
}

fn engine() -> Engine {
    Engine::new()
}

/// A host with one pane (`pane-1`, "auth refactor") and `/panes` already run, so `/to 1` works.
fn with_pane_list() -> (Engine, FakeHost) {
    let (e, h) = (engine(), FakeHost::new());
    h.pane("pane-1", "auth refactor", "Codex", "working");
    run(&e, &h, vec![say_to_bot("/panes")]);
    (e, h)
}

fn caps_of(list: &[crate::mobile_pairing::Cap]) -> Caps {
    list.iter().fold(Caps::none(), |caps, cap| caps.with(*cap, true))
}

fn prompt_of(item: &Item) -> (String, u32, String) {
    let p = item.prompt.as_ref().expect("an approval has a prompt");
    (p.prompt_id.clone(), p.occurrence, p.fingerprint.clone())
}

fn confirm_grant(e: &Engine, item: &Item) -> String {
    let (prompt_id, occurrence, fingerprint) = prompt_of(item);
    e.grants().issue(GrantAction::ConfirmAllow { item_id: item.id.clone(), prompt_id, occurrence, fingerprint }, T0, ANSWER_TTL_MS)
}

fn answer_grant(e: &Engine, item: &Item, choice: &str) -> String {
    let (prompt_id, occurrence, fingerprint) = prompt_of(item);
    e.grants().issue(
        GrantAction::Answer {
            item_id: item.id.clone(),
            choice: Some(choice.into()),
            prompt_id: Some(prompt_id),
            occurrence: Some(occurrence),
            fingerprint: Some(fingerprint),
        },
        T0,
        ANSWER_TTL_MS,
    )
}

fn question_grant(e: &Engine, item: &Item, choice: &str) -> String {
    e.grants().issue(
        GrantAction::Answer { item_id: item.id.clone(), choice: Some(choice.into()), prompt_id: None, occurrence: None, fingerprint: None },
        T0,
        ANSWER_TTL_MS,
    )
}

/// First tap on "Allow once…", then the token behind "Yes, allow once".
fn confirm_and_get_yes(e: &Engine, h: &FakeHost, item: &Item) -> (Vec<Effect>, String) {
    let token = confirm_grant(e, item);
    let effects = run(e, h, vec![tap_token(&token)]);
    let yes = edits(&effects)
        .first()
        .and_then(|(_, out)| out.keyboard.first().and_then(|row| row.first()).map(|b| b.data.clone()))
        .and_then(|data| data.strip_prefix("g:").map(str::to_string))
        .unwrap_or_default();
    (effects, yes)
}

// ---------------------------------------------------------------------------------------------
// Who may talk to the bot

#[test]
fn unknown_chat_and_user_is_ignored_and_nothing_is_sent() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![upd(message(STRANGER_ID, STRANGER_ID, "/to 1 rm everything", 5, None)), upd(message(STRANGER_ID, STRANGER_ID, "/panes", 5, None))]);
    assert!(effects.is_empty(), "a stranger got a reply: {:?}", texts(&effects));
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn stranger_is_counted_and_audited_once_an_hour() {
    let (e, h) = with_pane_list();
    for _ in 0..3 {
        run(&e, &h, vec![upd(message(STRANGER_ID, STRANGER_ID, "hi", 5, None))]);
    }
    let strangers = h.audit_kinds().into_iter().filter(|k| *k == AuditKind::ChannelStranger).count();
    assert_eq!(strangers, 1);
    assert_eq!(e.strangers_last_hour(T0), 3);
    // The audit line holds a count, never the text.
    let audits = h.audits.lock();
    let (_, summary, detail) = audits.iter().find(|(k, _, _)| *k == AuditKind::ChannelStranger).unwrap();
    assert_eq!(detail, &json!({"count": 1}));
    assert!(!summary.contains("hi"));
    drop(audits);
    // An hour later the count has dropped and a new line is allowed.
    h.set_now(T0 + 3_600_001);
    run(&e, &h, vec![upd(message(STRANGER_ID, STRANGER_ID, "hi", 5, None))]);
    assert_eq!(e.strangers_last_hour(T0 + 3_600_001), 1);
}

#[test]
fn right_user_in_the_wrong_chat_is_a_stranger() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![upd(message(7777, PAIRED_ID, "/panes", 5, None))]);
    assert!(effects.is_empty());
    assert_eq!(e.strangers_last_hour(T0), 1);
}

#[test]
fn right_chat_with_another_user_is_a_stranger() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![upd(message(PAIRED_ID, STRANGER_ID, "/panes", 5, None))]);
    assert!(effects.is_empty());
    assert_eq!(e.strangers_last_hour(T0), 1);
}

#[test]
fn group_chat_is_ignored_even_from_the_paired_person() {
    let (e, h) = with_pane_list();
    let mut m = message(-5000, PAIRED_ID, "/to 1 hi", 5, None);
    m.chat_private = false;
    let effects = run(&e, &h, vec![upd(m)]);
    assert!(effects.is_empty());
    assert!(h.pane_texts.lock().is_empty());
    assert_eq!(e.strangers_last_hour(T0), 0, "a group message is dropped before the stranger count");
}

#[test]
fn bot_sender_is_ignored() {
    let (e, h) = with_pane_list();
    let mut m = message(PAIRED_ID, PAIRED_ID, "/to 1 hi", 5, None);
    m.from_is_bot = true;
    assert!(run(&e, &h, vec![upd(m)]).is_empty());
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn message_without_a_sender_is_ignored() {
    let (e, h) = with_pane_list();
    let mut m = message(PAIRED_ID, PAIRED_ID, "/to 1 hi", 5, None);
    m.from_id = None;
    assert!(run(&e, &h, vec![upd(m)]).is_empty());
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn message_without_text_is_ignored() {
    let (e, h) = with_pane_list();
    let mut m = message(PAIRED_ID, PAIRED_ID, "x", 5, None);
    m.text = None;
    assert!(run(&e, &h, vec![upd(m)]).is_empty());
}

#[test]
fn switched_off_channel_drops_everything_from_the_paired_person() {
    let (e, h) = with_pane_list();
    h.edit_config(|c| c.enabled = false);
    let effects = run(&e, &h, vec![say_to_bot("/to 1 hi"), say_to_bot("/panes")]);
    assert!(effects.is_empty());
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn other_updates_are_acknowledged_and_ignored() {
    let (e, h) = with_pane_list();
    assert!(run(&e, &h, vec![Update { update_id: 1, kind: UpdateKind::Other }]).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Pairing

#[test]
fn not_paired_and_no_code_sends_nothing() {
    let (e, h) = (engine(), FakeHost::unpaired());
    let effects = run(&e, &h, vec![say_to_bot("hello"), say_to_bot("/start"), say_to_bot("/start ABCD1234"), say_to_bot("/panes")]);
    assert!(effects.is_empty(), "{:?}", texts(&effects));
    assert!(h.audits.lock().is_empty());
    assert_eq!(h.changed_times(), 0);
}

#[test]
fn pairing_happy_path_needs_the_desktop_yes() {
    let (e, h) = (engine(), FakeHost::unpaired());
    let (code, _) = e.new_pairing_code(T0);
    let effects = run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]);
    // The chat is told to wait, and the desktop is told to ask.
    assert_eq!(sent(&effects), vec!["Waiting for you to confirm on your PC."]);
    assert!(matches!(&effects[0], Effect::SendTo { chat_id, .. } if *chat_id == PAIRED_ID));
    assert!(h.changed_times() >= 1);
    assert_eq!(e.pairing_waiting().unwrap().first_name.as_deref(), Some("Anna"));
    // Not paired yet: nothing the chat sends is read.
    assert!(h.config().paired.is_none());
    assert!(run(&e, &h, vec![say_to_bot("/panes")]).is_empty());

    let effects = e.decide_pairing(&h, true);
    assert_clean(&effects);
    let paired = h.config().paired.expect("paired after the yes");
    assert_eq!((paired.chat_id, paired.user_id), (PAIRED_ID, PAIRED_ID));
    assert!(!h.config().enabled, "pairing does not turn the channel on (D-5)");
    assert!(h.audit_kinds().contains(&AuditKind::ChannelPaired));
    assert!(sent(&effects)[0].starts_with("Connected."));
    assert!(sent(&effects)[0].contains("Telegram can read"));
}

#[test]
fn pairing_with_the_right_code_but_no_yes_pairs_nobody() {
    let (e, h) = (engine(), FakeHost::unpaired());
    let (code, _) = e.new_pairing_code(T0);
    run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]);
    assert!(h.config().paired.is_none());
    assert!(h.saved.lock().is_empty());
}

#[test]
fn pairing_no_burns_the_code_and_says_nothing() {
    let (e, h) = (engine(), FakeHost::unpaired());
    let (code, _) = e.new_pairing_code(T0);
    run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]);
    assert!(e.decide_pairing(&h, false).is_empty());
    assert!(h.config().paired.is_none());
    assert!(h.audit_kinds().contains(&AuditKind::ChannelUnpaired));
    // The same code now says it is closed, once; then the bot is quiet again.
    let effects = run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]);
    assert_eq!(sent(&effects), vec!["This code does not work any more. Make a new code in Puppet Master."]);
    assert!(run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]).is_empty());
}

#[test]
fn wrong_codes_are_told_and_five_burn_the_code() {
    let (e, h) = (engine(), FakeHost::unpaired());
    let (code, _) = e.new_pairing_code(T0);
    for _ in 0..4 {
        let effects = run(&e, &h, vec![say_to_bot("/start ZZZZZZZZ")]);
        assert_eq!(sent(&effects), vec!["That code is not right."]);
    }
    let effects = run(&e, &h, vec![say_to_bot("/start ZZZZZZZZ")]);
    assert_eq!(sent(&effects).len(), 1, "the burn is told once");
    // Even the right code is dead now.
    let effects = run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]);
    assert!(effects.is_empty());
    assert!(e.pairing_waiting().is_none());
}

#[test]
fn a_second_chat_cannot_take_over_while_one_waits_for_the_desktop() {
    let (e, h) = (engine(), FakeHost::unpaired());
    let (code, _) = e.new_pairing_code(T0);
    run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]);
    let effects = run(&e, &h, vec![upd(message(STRANGER_ID, STRANGER_ID, &format!("/start {code}"), 5, None))]);
    assert!(effects.is_empty());
    assert_eq!(e.pairing_waiting().unwrap().user_id, PAIRED_ID);
    // The waiting person asking again is not told twice.
    assert!(run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]).is_empty());
}

#[test]
fn pairing_is_only_read_in_a_private_chat_with_the_sender_and_when_fresh() {
    let (e, h) = (engine(), FakeHost::unpaired());
    let (code, _) = e.new_pairing_code(T0);
    let text = format!("/start {code}");
    let mut group = message(-5000, PAIRED_ID, &text, 5, None);
    group.chat_private = false;
    let mut bot = message(PAIRED_ID, PAIRED_ID, &text, 5, None);
    bot.from_is_bot = true;
    let effects = run(
        &e,
        &h,
        vec![upd(group), upd(bot), upd(message(9999, PAIRED_ID, &text, 5, None)), upd(message(PAIRED_ID, PAIRED_ID, &text, 11 * 60, None))],
    );
    assert!(effects.is_empty());
    assert!(e.pairing_waiting().is_none());
}

#[test]
fn a_new_code_after_a_burned_one_works_again() {
    let (e, h) = (engine(), FakeHost::unpaired());
    e.new_pairing_code(T0);
    for _ in 0..5 {
        run(&e, &h, vec![say_to_bot("/start ZZZZZZZZ")]);
    }
    let (code, _) = e.new_pairing_code(T0);
    let effects = run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]);
    assert_eq!(sent(&effects), vec!["Waiting for you to confirm on your PC."]);
}

#[test]
fn a_failed_save_does_not_say_connected() {
    let (e, h) = (engine(), FakeHost::unpaired());
    let (code, _) = e.new_pairing_code(T0);
    run(&e, &h, vec![say_to_bot(&format!("/start {code}"))]);
    *h.save_fails.lock() = true;
    assert!(e.decide_pairing(&h, true).is_empty());
    assert!(h.audit_kinds().contains(&AuditKind::ChannelError));
}

// ---------------------------------------------------------------------------------------------
// Stale messages

#[test]
fn message_sent_before_the_channel_was_turned_on_is_not_acted_on() {
    let (e, h) = with_pane_list();
    // Turned on 60 s ago; this one is 90 s old (so it was sent while off).
    let effects = run(&e, &h, vec![aged("/to 1 old news", 90)]);
    assert!(h.pane_texts.lock().is_empty());
    assert_eq!(sent(&effects).len(), 1);
    assert!(sent(&effects)[0].starts_with("1 message came while your PC was asleep"));
}

#[test]
fn message_eleven_minutes_old_is_not_acted_on() {
    let (e, h) = with_pane_list();
    h.edit_config(|c| c.enabled_at_ms = Some(T0 - 3_600_000));
    let effects = run(&e, &h, vec![aged("/to 1 old news", 11 * 60)]);
    assert!(h.pane_texts.lock().is_empty());
    assert_eq!(sent(&effects).len(), 1);
}

#[test]
fn ten_minutes_exactly_is_still_fresh_and_one_second_more_is_not() {
    let (e, h) = with_pane_list();
    h.edit_config(|c| c.enabled_at_ms = Some(T0 - 3_600_000));
    run(&e, &h, vec![aged("/to 1 on time", 600)]);
    assert_eq!(h.pane_texts.lock().len(), 1);
    run(&e, &h, vec![aged("/to 1 too late", 601)]);
    assert_eq!(h.pane_texts.lock().len(), 1);
}

#[test]
fn three_stale_messages_make_one_note_and_the_fresh_one_still_runs() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![aged("/to 1 a", 120), aged("/to 1 b", 130), aged("/to 1 c", 140), say_to_bot("/to 1 fresh")]);
    assert_eq!(h.pane_texts.lock().clone(), vec![("pane-1".to_string(), "fresh".to_string())]);
    let notes: Vec<_> = sent(&effects).into_iter().filter(|t| t.contains("came while your PC was asleep")).collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0],
        "3 messages came while your PC was asleep or Puppet Master was off. They were not sent. Send them again if you still want them."
    );
}

#[test]
fn no_enabled_time_means_nothing_is_fresh() {
    let (e, h) = with_pane_list();
    h.edit_config(|c| c.enabled_at_ms = None);
    run(&e, &h, vec![say_to_bot("/to 1 hi")]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn a_message_with_a_bad_date_is_stale() {
    let (e, h) = with_pane_list();
    let mut m = message(PAIRED_ID, PAIRED_ID, "/to 1 hi", 5, None);
    m.date_s = 0;
    run(&e, &h, vec![upd(m)]);
    let mut m = message(PAIRED_ID, PAIRED_ID, "/to 1 hi", 5, None);
    m.date_s = -4;
    run(&e, &h, vec![upd(m)]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn stale_messages_from_a_stranger_are_not_counted_in_the_note() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![upd(message(STRANGER_ID, STRANGER_ID, "hi", 3600, None))]);
    assert!(effects.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Rate limit and capabilities

#[test]
fn inbound_bucket_stops_the_21st_message_with_one_note_a_minute() {
    let (e, h) = with_pane_list(); // /panes already used one token
    for i in 0..19 {
        let effects = run(&e, &h, vec![say_to_bot("/help")]);
        assert!(!effects.is_empty(), "message {i} should pass");
    }
    let effects = run(&e, &h, vec![say_to_bot("/help")]);
    assert_eq!(sent(&effects), vec!["Too many messages. Wait a minute."]);
    assert!(run(&e, &h, vec![say_to_bot("/help")]).is_empty(), "the note is said once a minute");
    assert!(run(&e, &h, vec![say_to_bot("/to 1 hi")]).is_empty());
    assert!(h.pane_texts.lock().is_empty());
    // A minute later it works again.
    h.set_now(T0 + 61_000);
    let effects = run(&e, &h, vec![upd(message(PAIRED_ID, PAIRED_ID, "/help", -50, None))]);
    assert!(sent(&effects)[0].starts_with("What I can do"));
}

#[test]
fn stop_still_works_when_the_person_is_over_the_limit() {
    let (e, h) = with_pane_list();
    for _ in 0..25 {
        run(&e, &h, vec![say_to_bot("/help")]);
    }
    let effects = run(&e, &h, vec![say_to_bot("/stop")]);
    assert!(matches!(effects.last(), Some(Effect::Stop)));
    assert!(!h.config().enabled);
}

#[test]
fn without_type_the_chat_cannot_write_to_panes_or_companions() {
    let (e, h) = with_pane_list();
    h.companion("c1", "Mochi", true);
    h.edit_config(|c| c.caps = caps_of(&[crate::mobile_pairing::Cap::Watch, crate::mobile_pairing::Cap::Answer]));
    for text in ["/to 1 hi", "/c Mochi hi", "hello there"] {
        let effects = run(&e, &h, vec![say_to_bot(text)]);
        assert_eq!(sent(&effects), vec![NOT_ALLOWED], "{text}");
    }
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn without_watch_the_chat_cannot_list_panes() {
    let (e, h) = (engine(), FakeHost::new());
    h.pane("pane-1", "auth refactor", "Codex", "working");
    h.edit_config(|c| c.caps = caps_of(&[crate::mobile_pairing::Cap::Type]));
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/panes")])), vec![NOT_ALLOWED]);
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/status")])), vec![NOT_ALLOWED]);
}

#[test]
fn with_no_caps_only_help_and_stop_still_work() {
    let (e, h) = (engine(), FakeHost::new());
    h.edit_config(|c| c.caps = Caps::none());
    assert!(sent(&run(&e, &h, vec![say_to_bot("/help")]))[0].starts_with("What I can do"));
    let effects = run(&e, &h, vec![say_to_bot("/stop")]);
    assert!(matches!(effects.last(), Some(Effect::Stop)));
}

#[test]
fn status_needs_watch_or_answer() {
    let (e, h) = (engine(), FakeHost::new());
    h.edit_config(|c| c.caps = caps_of(&[crate::mobile_pairing::Cap::Answer]));
    assert!(sent(&run(&e, &h, vec![say_to_bot("/status")]))[0].contains("need you"));
}

// ---------------------------------------------------------------------------------------------
// Commands

#[test]
fn to_goes_through_send_to_pane_with_control_characters_stripped() {
    let (e, h) = with_pane_list();
    run(&e, &h, vec![say_to_bot("/to 1 run\u{7} the\u{1b}[0m tests\u{202E} now\r\nline two\tend")]);
    let texts = h.pane_texts.lock().clone();
    assert_eq!(texts, vec![("pane-1".to_string(), "run the[0m tests now\nline two\tend".to_string())]);
}

#[test]
fn to_replies_with_the_heading_and_remembers_the_message() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![say_to_bot("/to 1 hi")]);
    match &effects[0] {
        Effect::Send { out, remember, .. } => {
            assert_eq!(out.text.as_str(), "Sent to auth refactor. It goes in when the agent is ready.");
            assert_eq!(remember, &Some(Target::Pane("pane-1".into())));
        }
        other => panic!("{other:?}"),
    }
    assert!(e.refs().lock().in_window(&Target::Pane("pane-1".into()), T0), "a conversation window opens");
    assert!(h.audit_kinds().contains(&AuditKind::ChannelMessage));
    // The audit line holds a count, never the text.
    let audits = h.audits.lock();
    let (_, summary, detail) = audits.iter().find(|(k, _, _)| *k == AuditKind::ChannelMessage).unwrap();
    assert_eq!(detail, &json!({"target": "pane", "chars": 2}));
    assert!(!summary.contains("hi"));
}

#[test]
fn to_without_a_pane_list_or_with_a_wrong_number_sends_nothing() {
    let (e, h) = (engine(), FakeHost::new());
    h.pane("pane-1", "auth refactor", "Codex", "working");
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/to 1 hi")])), vec!["Send /panes first, then /to 1 your text."]);
    run(&e, &h, vec![say_to_bot("/panes")]);
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/to 2 hi")])), vec!["Send /panes first, then /to 1 your text."]);
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/to 0 hi")])), vec!["I don't know that. Send /help."]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn the_pane_list_expires_after_30_minutes() {
    let (e, h) = with_pane_list();
    h.edit_config(|c| c.enabled_at_ms = Some(T0 - 7_200_000));
    h.set_now(T0 + 31 * 60_000);
    let effects = run(&e, &h, vec![upd(message(PAIRED_ID, PAIRED_ID, "/to 1 hi", -31 * 60 + 5, None))]);
    assert_eq!(sent(&effects), vec!["Send /panes first, then /to 1 your text."]);
}

#[test]
fn to_a_pane_that_closed_since_the_list_is_refused() {
    let (e, h) = with_pane_list();
    h.panes.lock().clear();
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/to 1 hi")])), vec!["That agent is gone. Send /panes to see the list."]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn an_outbox_refusal_comes_back_as_a_redacted_sentence() {
    let (e, h) = with_pane_list();
    *h.send_result.lock() = Err(format!("The agent is busy {SECRET} {EMAIL} {HOME}"));
    let effects = run(&e, &h, vec![say_to_bot("/to 1 hi")]);
    assert!(sent(&effects)[0].starts_with("Not sent: The agent is busy"));
    assert!(e.refs().lock().in_window(&Target::Pane("pane-1".into()), T0) == false);
    assert!(!h.audit_kinds().contains(&AuditKind::ChannelMessage));
}

#[test]
fn text_over_a_thousand_characters_is_refused() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![say_to_bot(&format!("/to 1 {}", "a".repeat(1001)))]);
    assert_eq!(sent(&effects), vec!["Too long (1000 characters at most)."]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn text_that_is_only_control_characters_is_not_sent() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![say_to_bot("/to 1 \u{7}\u{1b}\u{202E}")]);
    assert_eq!(sent(&effects), vec!["There is no text to send."]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn reply_to_a_pane_message_goes_to_that_pane() {
    let (e, h) = with_pane_list();
    e.refs().lock().remember(55, Target::Pane("pane-1".into()), T0);
    run(&e, &h, vec![reply("and then deploy", 55)]);
    assert_eq!(h.pane_texts.lock().clone(), vec![("pane-1".to_string(), "and then deploy".to_string())]);
}

#[test]
fn reply_to_a_companion_message_goes_to_that_companion() {
    let (e, h) = (engine(), FakeHost::new());
    h.companion("c1", "Mochi", true);
    h.companion("c2", "Nori", true);
    e.refs().lock().remember(56, Target::Companion("c2".into()), T0);
    run(&e, &h, vec![reply("morning", 56)]);
    assert_eq!(h.pane_texts.lock().clone(), vec![("companion-c2".to_string(), "morning".to_string())]);
}

#[test]
fn reply_to_a_message_we_do_not_know_sends_nothing() {
    let (e, h) = with_pane_list();
    h.companion("c1", "Mochi", true);
    let effects = run(&e, &h, vec![reply("hello", 4040)]);
    assert_eq!(sent(&effects), vec!["I can't tell what that message was about. Use /panes and /to."]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn reply_to_a_pane_that_is_gone_is_refused() {
    let (e, h) = with_pane_list();
    e.refs().lock().remember(55, Target::Pane("pane-9".into()), T0);
    let effects = run(&e, &h, vec![reply("hi", 55)]);
    assert_eq!(sent(&effects), vec!["That agent is gone. Send /panes to see the list."]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn plain_text_with_no_companion_says_so() {
    let (e, h) = (engine(), FakeHost::new());
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("is the build green?")])), vec!["You have no companion. Use /panes and /to."]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn plain_text_with_one_companion_goes_to_it() {
    let (e, h) = (engine(), FakeHost::new());
    h.companion("c1", "Mochi", true);
    let effects = run(&e, &h, vec![say_to_bot("is the build green?")]);
    assert_eq!(h.pane_texts.lock().clone(), vec![("companion-c1".to_string(), "is the build green?".to_string())]);
    match &effects[0] {
        Effect::Send { out, remember, .. } => {
            assert_eq!(out.text.as_str(), "Sent to Mochi.");
            assert_eq!(remember, &Some(Target::Companion("c1".into())));
        }
        other => panic!("{other:?}"),
    }
    assert!(e.refs().lock().in_window(&Target::Companion("c1".into()), T0));
}

#[test]
fn plain_text_with_two_companions_asks_which() {
    let (e, h) = (engine(), FakeHost::new());
    h.companion("c1", "Mochi", true);
    h.companion("c2", "Nori", true);
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("hi")])), vec!["Which one? /c Mochi your message"]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn a_sleeping_companion_is_not_woken_from_the_chat() {
    let (e, h) = (engine(), FakeHost::new());
    h.companion("c1", "Mochi", false);
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("hi")])), vec!["Mochi is asleep. Wake it in Puppet Master."]);
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/c mochi hi")])), vec!["Mochi is asleep. Wake it in Puppet Master."]);
    assert!(h.pane_texts.lock().is_empty());
}

#[test]
fn c_command_picks_a_companion_by_name_ignoring_case() {
    let (e, h) = (engine(), FakeHost::new());
    h.companion("c1", "Mochi", true);
    h.companion("c2", "Nori", true);
    run(&e, &h, vec![say_to_bot("/c nORI status please")]);
    assert_eq!(h.pane_texts.lock().clone(), vec![("companion-c2".to_string(), "status please".to_string())]);
    let effects = run(&e, &h, vec![say_to_bot("/c Pip hi")]);
    assert!(sent(&effects)[0].starts_with("No companion has that name."));
    assert_eq!(h.pane_texts.lock().len(), 1);
}

#[test]
fn stop_saves_off_audits_and_ends_the_poller() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![say_to_bot("/stop")]);
    assert!(!h.config().enabled);
    assert!(!h.saved.lock().last().unwrap().enabled);
    assert_eq!(sent(&effects), vec!["Telegram is off. Turn it on again in Puppet Master on your PC."]);
    assert!(matches!(effects.last(), Some(Effect::Stop)));
    let audits = h.audits.lock();
    let (kind, _, detail) = audits.last().unwrap();
    assert_eq!(*kind, AuditKind::ChannelOff);
    assert_eq!(detail, &json!({"by": "telegram"}));
    drop(audits);
    assert!(h.changed_times() >= 1);
}

#[test]
fn nothing_after_stop_in_the_same_batch_is_read() {
    let (e, h) = with_pane_list();
    let effects = run(&e, &h, vec![say_to_bot("/stop"), say_to_bot("/to 1 too late"), aged("/to 1 old", 300)]);
    assert!(h.pane_texts.lock().is_empty());
    assert_eq!(sent(&effects).len(), 1, "no stale note after stop");
}

#[test]
fn stop_keeps_the_rest_of_the_config() {
    let (e, h) = with_pane_list();
    h.edit_config(|c| c.notify_runs = true);
    run(&e, &h, vec![say_to_bot("/stop")]);
    let saved = h.saved.lock().last().unwrap().clone();
    assert!(saved.notify_runs);
    assert!(saved.paired.is_some());
}

#[test]
fn status_counts_and_lists_what_waits() {
    let (e, h) = (engine(), FakeHost::new());
    h.pane("pane-1", "auth refactor", "Codex", "working");
    h.pane("pane-2", "Claude Code 2", "Claude Code", "needs you");
    h.add_item(approval("pane-2", "npm test", Some(Level::Low)));
    let mut done = card(ItemKind::Done, "Finished");
    done.lane = crate::attention::model::Lane::Ready;
    done.id = "item-done".into();
    h.add_item(done);
    let text = sent(&run(&e, &h, vec![say_to_bot("/status")])).remove(0);
    assert!(text.starts_with("1 needs you · 1 ready · 1 agent running"), "{text}");
    assert!(text.contains("- auth refactor: Run tests"), "{text}");
}

#[test]
fn status_in_short_mode_or_without_watch_shows_counts_only() {
    let (e, h) = (engine(), FakeHost::new());
    h.add_item(approval("pane-2", "npm test", Some(Level::Low)));
    h.edit_config(|c| c.short_mode = true);
    let text = sent(&run(&e, &h, vec![say_to_bot("/status")])).remove(0);
    assert_eq!(text, "1 needs you · 0 ready · 0 agents running");
    h.edit_config(|c| {
        c.short_mode = false;
        c.caps = caps_of(&[crate::mobile_pairing::Cap::Answer]);
    });
    let text = sent(&run(&e, &h, vec![say_to_bot("/status")])).remove(0);
    assert_eq!(text, "1 needs you · 0 ready · 0 agents running");
}

#[test]
fn panes_are_numbered_capped_at_twenty_and_remembered() {
    let (e, h) = (engine(), FakeHost::new());
    for i in 1..=25 {
        h.pane(&format!("pane-{i}"), &format!("agent {i}"), "Codex", "working");
    }
    let text = sent(&run(&e, &h, vec![say_to_bot("/panes")])).remove(0);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 20);
    assert_eq!(lines[0], "1 agent 1 · Codex · working");
    assert_eq!(e.refs().lock().pane_at(20, T0).as_deref(), Some("pane-20"));
    assert_eq!(e.refs().lock().pane_at(21, T0), None);
}

#[test]
fn panes_with_no_agents_says_so() {
    let (e, h) = (engine(), FakeHost::new());
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/panes")])), vec!["No agents are open."]);
}

#[test]
fn help_says_the_bot_is_silent_when_the_pc_sleeps() {
    let (e, h) = (engine(), FakeHost::new());
    let text = sent(&run(&e, &h, vec![say_to_bot("/help")])).remove(0);
    assert!(text.contains("When your PC sleeps or Puppet Master is closed, I can't answer."));
    assert!(text.lines().count() > 5, "help is a list, one line each");
}

#[test]
fn unknown_commands_and_a_repeated_start_get_a_short_answer() {
    let (e, h) = (engine(), FakeHost::new());
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/nope")])), vec!["I don't know that. Send /help."]);
    assert_eq!(sent(&run(&e, &h, vec![say_to_bot("/start ABC")])), vec!["You are already connected. Send /help."]);
}

#[test]
fn what_agents_say_in_listings_is_cleaned() {
    let (e, h) = (engine(), FakeHost::new());
    h.pane("pane-1", &format!("key {SECRET} for {EMAIL} in {HOME}"), "Codex", "working");
    let text = sent(&run(&e, &h, vec![say_to_bot("/panes")])).remove(0);
    assert!(text.contains("<email>"), "{text}");
    let mut item = card(ItemKind::Error, &format!("failed {SECRET} {EMAIL}"));
    item.pane.as_mut().unwrap().heading = format!("in {HOME}");
    h.add_item(item);
    let text = sent(&run(&e, &h, vec![say_to_bot("/status")])).remove(0);
    assert!(text.contains("<email>"), "{text}");
}

const PEM_BODY: [&str; 2] = ["MIIEvgIBADANBgkqhkiG9w0BAQEFAASC", "Zq9kLm2pRt8wXyVbN3cHdEaFgJi4kOl0"];

fn pem_key() -> String {
    format!("-----BEGIN PRIVATE KEY-----\n{}\n{}\n-----END PRIVATE KEY-----", PEM_BODY[0], PEM_BODY[1])
}

/// No line of the key's body reaches any effect: not in a text, a button or a toast.
fn assert_no_key_body(effects: &[Effect]) {
    for text in texts(effects) {
        for body in PEM_BODY {
            assert!(!text.contains(body), "a key body line was sent: {text:?}");
        }
    }
}

#[test]
fn a_multi_line_key_in_a_pane_name_is_masked_as_one_block() {
    let (e, h) = (engine(), FakeHost::new());
    h.pane("pane-1", &format!("deploy\n{}\nnotes", pem_key()), "Codex", "working");
    let effects = run(&e, &h, vec![say_to_bot("/panes")]);
    assert_no_key_body(&effects);
    let text = sent(&effects).remove(0);
    assert!(text.contains("Codex"), "the rest of the reply is still there: {text}");
}

#[test]
fn a_multi_line_key_in_a_card_title_is_masked_as_one_block() {
    let (e, h) = (engine(), FakeHost::new());
    let mut item = card(ItemKind::Error, &format!("failed:\n{}", pem_key()));
    item.pane.as_mut().unwrap().heading = "auth refactor".into();
    h.add_item(item);
    let effects = run(&e, &h, vec![say_to_bot("/status")]);
    assert_no_key_body(&effects);
    assert!(sent(&effects).remove(0).starts_with("1 needs you"));
}

#[test]
fn lines_are_cut_after_the_cleaning_and_the_whole_stays_under_the_cap() {
    let lines: Vec<String> = (0..30).map(|i| format!("{i} {}", "long name ".repeat(40))).collect();
    let out = lines_out(&lines);
    let text = out.text.as_str();
    assert!(text.chars().count() <= MAX_OUT_CHARS);
    assert!(text.lines().all(|l| l.chars().count() <= LINE_CHARS), "{text}");
    assert!(text.lines().count() > 1);
}

#[test]
fn a_long_listing_stays_under_the_message_cap() {
    let (e, h) = (engine(), FakeHost::new());
    for i in 1..=20 {
        h.pane(&format!("pane-{i}"), &"long name ".repeat(40), "Codex", "working");
        let _ = i;
    }
    let text = sent(&run(&e, &h, vec![say_to_bot("/panes")])).remove(0);
    assert!(text.chars().count() <= MAX_OUT_CHARS);
}

#[test]
fn outgoing_converts_to_the_wire_shape() {
    let out = Outgoing {
        text: render::clean("hi", 100),
        keyboard: vec![vec![render::button("Deny", "g:abc").unwrap(), render::button("Cancel", "g:def").unwrap()]],
        silent: false,
    };
    let wire = to_wire(&out);
    assert_eq!(wire.text.as_str(), "hi");
    assert_eq!(wire.keyboard.len(), 1);
    assert_eq!(wire.keyboard[0][1].data, "g:def");
    assert_eq!(wire.keyboard[0][0].label, "Deny");
    assert!(!wire.disable_notification, "a loud message keeps the sound");
    let quiet = to_wire(&Outgoing { silent: true, ..out });
    assert!(quiet.disable_notification, "the silent flag reaches the wire");
}

// ---------------------------------------------------------------------------------------------
// Buttons: refusals

#[test]
fn forged_or_unknown_button_data_expires_without_an_answer() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item);
    for data in ["", "x", "g:", "g:AAAAAAAAAAAAAAAAAAAAAA", "allow_once", "answer:item-pane-1-1:allow_once", "g:../../etc"] {
        let effects = run(&e, &h, vec![tap(data)]);
        assert_eq!(toasts(&effects), vec!["This button expired."], "{data:?}");
        assert!(edits(&effects).is_empty(), "an unknown token must not rewrite a message");
    }
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_tap_with_no_data_expires() {
    let (e, h) = with_pane_list();
    let mut cb = InCallback { id: "cb".into(), from_id: PAIRED_ID, chat_id: Some(PAIRED_ID), message_id: Some(1), data: None };
    let effects = run(&e, &h, vec![Update { update_id: 1, kind: UpdateKind::Callback(cb.clone()) }]);
    assert_eq!(toasts(&effects), vec!["This button expired."]);
    cb.data = Some("g:".into());
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_tap_from_another_user_is_ignored_and_the_grant_survives() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let token = answer_grant(&e, &item, "deny");
    let effects = run(&e, &h, vec![tap_as(STRANGER_ID, Some(PAIRED_ID), &format!("g:{token}"))]);
    assert!(effects.is_empty(), "{:?}", texts(&effects));
    assert!(h.answers.lock().is_empty());
    // The right person can still use it.
    let effects = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&effects), vec!["Allowed once"]);
    assert_eq!(e.strangers_last_hour(T0), 1);
}

#[test]
fn a_tap_in_another_chat_or_without_a_chat_is_ignored() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let token = answer_grant(&e, &item, "deny");
    assert!(run(&e, &h, vec![tap_as(PAIRED_ID, Some(8888), &format!("g:{token}"))]).is_empty());
    assert!(run(&e, &h, vec![tap_as(PAIRED_ID, None, &format!("g:{token}"))]).is_empty());
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_tap_while_the_channel_is_off_or_unpaired_is_ignored() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let token = answer_grant(&e, &item, "deny");
    h.edit_config(|c| c.enabled = false);
    assert!(run(&e, &h, vec![tap_token(&token)]).is_empty());
    h.edit_config(|c| {
        c.enabled = true;
        c.paired = None;
    });
    assert!(run(&e, &h, vec![tap_token(&token)]).is_empty());
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_replayed_button_is_refused_the_second_time() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let token = answer_grant(&e, &item, "deny");
    let first = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&first), vec!["Allowed once"]);
    let second = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&second), vec!["This button expired."]);
    assert_eq!(h.answers.lock().len(), 1, "one tap, one answer");
}

#[test]
fn tapping_one_button_kills_the_others_of_the_same_card() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let allow = confirm_grant(&e, &item);
    let deny = answer_grant(&e, &item, "deny");
    run(&e, &h, vec![tap_token(&deny)]);
    let effects = run(&e, &h, vec![tap_token(&allow)]);
    assert_eq!(toasts(&effects), vec!["This button expired."]);
    assert_eq!(h.answers.lock().len(), 1);
}

#[test]
fn an_expired_grant_says_so_and_does_not_answer() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let token = answer_grant(&e, &item, "deny");
    h.set_now(T0 + ANSWER_TTL_MS);
    let effects = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&effects), vec!["This button expired."]);
    assert_eq!(edits(&effects)[0].1.text.as_str(), "This button expired.");
    assert!(edits(&effects)[0].1.keyboard.is_empty());
    assert!(h.answers.lock().is_empty());
}

#[test]
fn answers_are_limited_to_ten_a_minute_and_the_grant_is_kept() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    for _ in 0..10 {
        let t = answer_grant(&e, &item, "deny");
        let effects = run(&e, &h, vec![tap_token(&t)]);
        assert_eq!(toasts(&effects), vec!["Allowed once"]);
    }
    let held = answer_grant(&e, &item, "deny");
    let effects = run(&e, &h, vec![tap_token(&held)]);
    assert_eq!(toasts(&effects), vec!["Slow down."]);
    assert_eq!(h.answers.lock().len(), 10);
    // A minute later the same button works: it was not used up by the refusal.
    h.set_now(T0 + 61_000);
    let held = answer_grant(&e, &item, "deny");
    assert_eq!(toasts(&run(&e, &h, vec![tap_token(&held)])), vec!["Allowed once"]);
}

#[test]
fn without_the_answer_cap_a_tap_is_refused() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    h.edit_config(|c| c.caps = caps_of(&[crate::mobile_pairing::Cap::Watch, crate::mobile_pairing::Cap::Type]));
    let token = answer_grant(&e, &item, "deny");
    let effects = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&effects), vec![NOT_ALLOWED]);
    assert!(h.answers.lock().is_empty());
}

fn assert_refused_with(sentence: &str, item: Item, choice: &str) {
    let (e, h) = with_pane_list();
    h.add_item(item.clone());
    let token = if item.prompt.is_some() { answer_grant(&e, &item, choice) } else { question_grant(&e, &item, choice) };
    let effects = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&effects), vec![sentence], "{:?}", item.kind);
    assert_eq!(edits(&effects)[0].1.text.as_str(), sentence);
    assert!(edits(&effects)[0].1.keyboard.is_empty());
    assert!(h.answers.lock().is_empty(), "{:?} must never reach the answer path", item.kind);
}

#[test]
fn a_high_risk_approval_is_never_answered_from_the_chat() {
    assert_refused_with("Open Puppet Master to answer.", approval("pane-1", "rm -rf /", Some(Level::High)), "allow_once");
    assert_refused_with("Open Puppet Master to answer.", approval("pane-1", "rm -rf /", Some(Level::High)), "deny");
}

#[test]
fn a_medium_risk_approval_is_never_answered_from_the_chat() {
    assert_refused_with("Open Puppet Master to answer.", approval("pane-1", "git push", Some(Level::Medium)), "allow_once");
    assert_refused_with("Open Puppet Master to answer.", approval("pane-1", "git push", Some(Level::Medium)), "deny");
}

#[test]
fn an_approval_with_no_risk_read_is_never_answered_from_the_chat() {
    assert_refused_with("Open Puppet Master to answer.", approval("pane-1", "mystery", None), "allow_once");
}

#[test]
fn a_trust_gate_is_never_answered_from_the_chat() {
    let mut gate = card(ItemKind::TrustGate, "Trust this folder?");
    gate.prompt = approval("pane-1", "x", Some(Level::Low)).prompt;
    assert_refused_with("Only you can pass this, on your PC.", gate, "allow_once");
}

#[test]
fn a_consent_card_is_never_answered_from_the_chat() {
    assert_refused_with("An agent wants more control. Decide on your PC.", card(ItemKind::Consent, "More control?"), "allow");
}

#[test]
fn budget_and_limit_cards_are_never_answered_from_the_chat() {
    assert_refused_with("Open Puppet Master to decide.", card(ItemKind::Budget, "Budget used"), "continue");
    assert_refused_with("Open Puppet Master to decide.", card(ItemKind::Limit, "Limit reached"), "continue");
}

#[test]
fn info_cards_have_nothing_to_answer() {
    for kind in [ItemKind::Error, ItemKind::Stuck, ItemKind::Done, ItemKind::Review] {
        assert_refused_with("Open Puppet Master to act on this.", card(kind, "Something"), "ok");
    }
}

#[test]
fn a_confirm_grant_for_a_risky_or_other_card_does_not_show_a_confirm() {
    let (e, h) = with_pane_list();
    let high = approval("pane-1", "rm -rf /", Some(Level::High));
    h.add_item(high.clone());
    let effects = run(&e, &h, vec![tap_token(&confirm_grant(&e, &high))]);
    assert_eq!(toasts(&effects), vec!["Open Puppet Master to answer."]);
    assert!(edits(&effects)[0].1.keyboard.is_empty(), "no buttons on a high-risk card");
    let gate = card(ItemKind::TrustGate, "Trust?");
    h.add_item(gate.clone());
    let token = e.grants().issue(
        GrantAction::ConfirmAllow { item_id: gate.id.clone(), prompt_id: "p1".into(), occurrence: 1, fingerprint: "x".into() },
        T0,
        ANSWER_TTL_MS,
    );
    let effects = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&effects), vec!["Only you can pass this, on your PC."]);
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_card_that_got_riskier_since_the_message_is_refused_at_the_tap() {
    let (e, h) = with_pane_list();
    let low = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(low.clone());
    let token = answer_grant(&e, &low, "deny");
    // The same card is now high risk on the PC.
    h.items.lock()[0].risk = approval("pane-1", "npm test", Some(Level::High)).risk;
    let effects = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&effects), vec!["Open Puppet Master to answer."]);
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_card_that_left_the_list_is_expired_not_answered() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    let token = answer_grant(&e, &item, "deny");
    let effects = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&effects), vec!["This button expired."]);
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_broad_choice_is_never_sent_even_with_a_valid_grant() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let token = answer_grant(&e, &item, "allow_all");
    assert_eq!(toasts(&run(&e, &h, vec![tap_token(&token)])), vec!["This button expired."]);
    assert!(h.answers.lock().is_empty());
}

#[test]
fn an_approval_grant_without_the_card_proof_is_refused() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let token = e.grants().issue(
        GrantAction::Answer { item_id: item.id.clone(), choice: Some("deny".into()), prompt_id: Some("p1".into()), occurrence: None, fingerprint: None },
        T0,
        ANSWER_TTL_MS,
    );
    assert_eq!(toasts(&run(&e, &h, vec![tap_token(&token)])), vec!["This button expired."]);
    assert!(h.answers.lock().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Buttons: Allow once (two taps), Deny, receipts

#[test]
fn allow_once_needs_the_confirm_tap_and_a_direct_grant_is_refused() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    // A grant that says allow_once but never went through the confirm step does nothing.
    let direct = answer_grant(&e, &item, "allow_once");
    let effects = run(&e, &h, vec![tap_token(&direct)]);
    assert_eq!(toasts(&effects), vec!["This button expired."]);
    assert!(h.answers.lock().is_empty());

    // First tap: the message becomes a question, nothing is allowed yet.
    let (effects, yes) = confirm_and_get_yes(&e, &h, &item);
    assert!(h.answers.lock().is_empty());
    let (message_id, out) = edits(&effects).remove(0);
    assert_eq!(message_id, 900);
    assert_eq!(out.text.as_str(), "Allow once? npm test · shop");
    let labels: Vec<_> = out.keyboard.iter().flatten().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, vec!["Yes, allow once", "Cancel"]);
    assert!(out.keyboard.iter().flatten().all(|b| b.data.starts_with("g:") && b.data.len() <= 64));
    assert_eq!(toasts(&effects), Vec::<String>::new());

    // Second tap: the answer goes through, with the proof from the card.
    let effects = run(&e, &h, vec![tap_token(&yes)]);
    let answers = h.answers.lock();
    assert_eq!(answers.len(), 1);
    let (item_id, request, caps) = &answers[0];
    assert_eq!(item_id, &item.id);
    assert_eq!(request.choice.as_deref(), Some("allow_once"));
    assert_eq!(request.text, None);
    let (prompt_id, occurrence, fingerprint) = prompt_of(&item);
    assert_eq!(request.prompt_id.as_deref(), Some(prompt_id.as_str()));
    assert_eq!(request.occurrence, Some(occurrence));
    assert_eq!(request.fingerprint.as_deref(), Some(fingerprint.as_str()));
    assert!(request.answer_key.as_deref().unwrap().starts_with("tg-"));
    assert!(caps.can_answer && !caps.can_control);
    drop(answers);
    assert_eq!(edits(&effects)[0].1.text.as_str(), "Allowed once");
    assert!(edits(&effects)[0].1.keyboard.is_empty());
    assert_eq!(toasts(&effects), vec!["Allowed once"]);
}

#[test]
fn the_confirm_tap_works_once_only() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let (_, yes) = confirm_and_get_yes(&e, &h, &item);
    run(&e, &h, vec![tap_token(&yes)]);
    let again = run(&e, &h, vec![tap_token(&yes)]);
    assert_eq!(toasts(&again), vec!["This button expired."]);
    assert_eq!(h.answers.lock().len(), 1);
}

#[test]
fn the_confirm_expires_after_60_seconds() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let (_, yes) = confirm_and_get_yes(&e, &h, &item);
    h.set_now(T0 + CONFIRM_TTL_MS);
    let effects = run(&e, &h, vec![tap_token(&yes)]);
    assert_eq!(toasts(&effects), vec!["This button expired."]);
    assert_eq!(edits(&effects)[0].1.text.as_str(), "This button expired.");
    assert!(h.answers.lock().is_empty());
}

#[test]
fn the_confirm_still_works_just_before_60_seconds() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let (_, yes) = confirm_and_get_yes(&e, &h, &item);
    h.set_now(T0 + CONFIRM_TTL_MS - 1);
    assert_eq!(toasts(&run(&e, &h, vec![tap_token(&yes)])), vec!["Allowed once"]);
}

#[test]
fn cancel_allows_nothing_and_kills_the_yes_button() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let (effects, yes) = confirm_and_get_yes(&e, &h, &item);
    let cancel = edits(&effects)[0].1.keyboard[0][1].data.clone();
    let effects = run(&e, &h, vec![tap(&cancel)]);
    assert_eq!(edits(&effects)[0].1.text.as_str(), "Cancelled. Nothing was allowed.");
    let effects = run(&e, &h, vec![tap_token(&yes)]);
    assert_eq!(toasts(&effects), vec!["This button expired."]);
    assert!(h.answers.lock().is_empty());
}

#[test]
fn deny_is_one_tap() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    *h.answer_reply.lock() = (200, json!({"outcome": "answered", "choice_label": "Denied"}));
    let token = answer_grant(&e, &item, "deny");
    let effects = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(h.answers.lock()[0].1.choice.as_deref(), Some("deny"));
    assert_eq!(edits(&effects)[0].1.text.as_str(), "Denied");
}

#[test]
fn the_confirm_step_refuses_a_card_whose_prompt_changed() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let token = e.grants().issue(
        GrantAction::ConfirmAllow { item_id: item.id.clone(), prompt_id: "p1".into(), occurrence: 1, fingerprint: "an older fingerprint".into() },
        T0,
        ANSWER_TTL_MS,
    );
    let effects = run(&e, &h, vec![tap_token(&token)]);
    assert_eq!(toasts(&effects), vec!["This changed. Open Puppet Master."]);
    assert!(edits(&effects)[0].1.keyboard.is_empty());
    assert!(h.answers.lock().is_empty());
}

#[test]
fn the_confirm_step_needs_the_card_to_be_open() {
    let (e, h) = with_pane_list();
    let mut item = approval("pane-1", "npm test", Some(Level::Low));
    item.state = State::Resolved;
    h.add_item(item.clone());
    let effects = run(&e, &h, vec![tap_token(&confirm_grant(&e, &item))]);
    assert_eq!(toasts(&effects), vec!["This changed. Open Puppet Master."]);
}

#[test]
fn no_confirm_is_offered_when_redaction_changes_the_command() {
    let (e, h) = with_pane_list();
    for command in [format!("curl -H key={SECRET}"), format!("mail {EMAIL}"), format!("cat {HOME}\\notes.txt"), "x".repeat(121)] {
        let item = approval("pane-1", &command, Some(Level::Low));
        h.items.lock().clear();
        h.add_item(item.clone());
        let effects = run(&e, &h, vec![tap_token(&confirm_grant(&e, &item))]);
        assert_eq!(toasts(&effects), vec!["Open Puppet Master to answer."], "{command}");
        assert!(edits(&effects)[0].1.keyboard.is_empty());
    }
    assert!(h.answers.lock().is_empty());
}

#[test]
fn the_confirm_shows_only_the_last_folder_name() {
    let (e, h) = with_pane_list();
    let mut item = approval("pane-1", "npm test", Some(Level::Low));
    item.pane.as_mut().unwrap().project = Some("D:\\work\\shop".into());
    h.add_item(item.clone());
    let (effects, _) = confirm_and_get_yes(&e, &h, &item);
    assert_eq!(edits(&effects)[0].1.text.as_str(), "Allow once? npm test · shop");
}

#[test]
fn answer_outcomes_map_to_the_receipts() {
    let cases: Vec<((u16, serde_json::Value), &str)> = vec![
        ((200, json!({"outcome": "answered", "choice_label": "Denied"})), "Denied"),
        ((200, json!({"outcome": "answered", "choice_label": "Allowed once"})), "Allowed once"),
        ((200, json!({"outcome": "already_answered", "by": "pc"})), "Already answered on your PC"),
        ((200, json!({"outcome": "already_answered", "by": "phone"})), "Already answered on a phone"),
        ((200, json!({"outcome": "already_answered", "by": "you", "choice_label": "Denied"})), "Denied"),
        ((200, json!({"outcome": "changed"})), "This changed. Open Puppet Master."),
        ((200, json!({"outcome": "gone", "reason": "moved_on"})), "This button expired."),
        ((200, json!({"outcome": "failed", "code": "X", "detail": "pane closed"})), "Could not send that. Open Puppet Master."),
        ((403, json!({"error": "not_from_chat", "hint": "x"})), "Open Puppet Master to answer."),
        ((403, json!({"error": "not_allowed", "code": "DEVICE_CAP_REQUIRED"})), NOT_ALLOWED),
        ((400, json!({"error": "bad"})), "That did not work. Open Puppet Master."),
        ((200, json!({"nonsense": true})), "This changed. Open Puppet Master."),
    ];
    for (reply, expected) in cases {
        let (e, h) = with_pane_list();
        let item = approval("pane-1", "npm test", Some(Level::Low));
        h.add_item(item.clone());
        *h.answer_reply.lock() = reply.clone();
        let effects = run(&e, &h, vec![tap_token(&answer_grant(&e, &item, "deny"))]);
        assert_eq!(toasts(&effects), vec![expected], "{reply:?}");
        assert_eq!(edits(&effects)[0].1.text.as_str(), expected);
        assert!(edits(&effects)[0].1.keyboard.is_empty(), "a receipt has no buttons");
    }
}

#[test]
fn a_receipt_label_from_the_answer_path_is_cleaned() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    *h.answer_reply.lock() = (200, json!({"outcome": "answered", "choice_label": format!("Yes {SECRET} {EMAIL}")}));
    let effects = run(&e, &h, vec![tap_token(&answer_grant(&e, &item, "deny"))]);
    assert!(toasts(&effects)[0].contains("<email>"));
}

#[test]
fn the_caps_given_to_the_answer_path_never_include_control() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    h.edit_config(|c| c.caps = Caps::all());
    run(&e, &h, vec![tap_token(&answer_grant(&e, &item, "deny"))]);
    let caps = h.answers.lock()[0].2;
    assert!(caps.can_answer);
    assert!(!caps.can_control, "a chat is never given control");
}

#[test]
fn a_tap_without_a_message_id_still_stops_the_spinner() {
    let (e, h) = with_pane_list();
    let item = approval("pane-1", "npm test", Some(Level::Low));
    h.add_item(item.clone());
    let token = answer_grant(&e, &item, "deny");
    let update = Update {
        update_id: 1,
        kind: UpdateKind::Callback(InCallback { id: "cbx".into(), from_id: PAIRED_ID, chat_id: Some(PAIRED_ID), message_id: None, data: Some(format!("g:{token}")) }),
    };
    let effects = run(&e, &h, vec![update]);
    assert!(edits(&effects).is_empty());
    assert_eq!(toasts(&effects), vec!["Allowed once"]);
}

// ---------------------------------------------------------------------------------------------
// Buttons: questions

fn question_world() -> (Engine, FakeHost, Item) {
    let (e, h) = with_pane_list();
    let q = pending_question("Which branch?", &[("main", "main"), ("dev", "dev")]);
    let item = question_item(&q.id, "Which branch?");
    h.questions.lock().push(q);
    h.add_item(item.clone());
    (e, h, item)
}

#[test]
fn a_question_choice_is_answered_through_the_shared_path() {
    let (e, h, item) = question_world();
    *h.answer_reply.lock() = (200, json!({"outcome": "answered", "choice_label": "dev"}));
    let effects = run(&e, &h, vec![tap_token(&question_grant(&e, &item, "dev"))]);
    let answers = h.answers.lock();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].1.choice.as_deref(), Some("dev"));
    assert_eq!(answers[0].1.text, None, "free text is never sent from here");
    drop(answers);
    assert_eq!(edits(&effects)[0].1.text.as_str(), "dev");
}

#[test]
fn a_choice_the_question_does_not_offer_is_refused() {
    let (e, h, item) = question_world();
    let effects = run(&e, &h, vec![tap_token(&question_grant(&e, &item, "prod"))]);
    assert_eq!(toasts(&effects), vec!["This button expired."]);
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_question_that_is_no_longer_pending_is_not_answered() {
    let (e, h, item) = question_world();
    h.questions.lock().clear();
    assert_eq!(toasts(&run(&e, &h, vec![tap_token(&question_grant(&e, &item, "dev"))])), vec!["This button expired."]);
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_question_that_is_really_a_consent_or_budget_card_is_not_answered() {
    let (e, h, item) = question_world();
    h.questions.lock()[0].kind = Some("budget".into());
    assert_eq!(toasts(&run(&e, &h, vec![tap_token(&question_grant(&e, &item, "dev"))])), vec!["This button expired."]);
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_question_grant_without_a_choice_is_refused() {
    let (e, h, item) = question_world();
    let token = e.grants().issue(
        GrantAction::Answer { item_id: item.id.clone(), choice: None, prompt_id: None, occurrence: None, fingerprint: None },
        T0,
        ANSWER_TTL_MS,
    );
    assert_eq!(toasts(&run(&e, &h, vec![tap_token(&token)])), vec!["This button expired."]);
    assert!(h.answers.lock().is_empty());
}

#[test]
fn a_question_already_answered_on_the_pc_says_so() {
    let (e, h, item) = question_world();
    *h.answer_reply.lock() = (200, json!({"outcome": "already_answered", "by": "pc", "choice_label": "main"}));
    let effects = run(&e, &h, vec![tap_token(&question_grant(&e, &item, "dev"))]);
    assert_eq!(toasts(&effects), vec!["Already answered on your PC"]);
}
