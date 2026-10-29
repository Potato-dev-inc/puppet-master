//! Poller tests with a fake `Api` and the real engine on the fake host (`test_host`). No test talks
//! to Telegram. Effects the engine cannot be made to produce from a message (edits, a remembered
//! send, a pairing reply) go straight to `deliver`.

use super::*;
use crate::channels::refs::Target;
use crate::channels::render::{self, Outgoing};
use crate::channels::telegram::api::{ApiError, BotInfo, InMessage, Update, UpdateKind, WireMessage};
use crate::channels::test_host::{FakeHost, PAIRED_ID, T0};
use crate::redact::Redacted;
use std::collections::VecDeque;
use std::sync::Mutex;

const CHAT: i64 = PAIRED_ID;

/// Timing for tests: nothing waits for real.
const FAST: Timing = Timing {
    poll_timeout_s: 0,
    backoff_min: Duration::from_millis(1),
    backoff_max: Duration::from_millis(4),
    send_gap: Duration::ZERO,
    tick: Duration::from_millis(1),
};

/// One scripted answer to `get_updates`.
enum Poll {
    /// A batch of updates the engine ignores, with these ids.
    Batch(Vec<i64>),
    /// A batch of real updates, for the real engine to read.
    Real(Vec<Update>),
    Fail(ApiError),
    /// Bumps the generation (a restart), then returns a batch of ignored updates.
    Bump(Vec<i64>),
    /// Bumps the generation, then returns these real updates.
    BumpReal(Vec<Update>),
}

struct FakeApi {
    script: Mutex<VecDeque<Poll>>,
    /// The offset each `get_updates` call was given.
    offsets: Mutex<Vec<Option<i64>>>,
    generation: Arc<AtomicU64>,
    /// Errors for the next sends, one per call. Empty means every send works.
    send_fail: Mutex<VecDeque<ApiError>>,
    /// (chat id, text, reply-to) of every send that went out.
    sent: Mutex<Vec<(i64, String, Option<i64>)>>,
    edits: Mutex<Vec<i64>>,
    answers: Mutex<Vec<String>>,
    next_message_id: Mutex<i64>,
}

fn updates(ids: &[i64]) -> Vec<Update> {
    ids.iter().map(|&id| Update { update_id: id, kind: UpdateKind::Other }).collect()
}

impl Api for FakeApi {
    fn get_me(&self) -> Result<BotInfo, ApiError> {
        Ok(BotInfo { id: 1, username: "fake_bot".into() })
    }

    fn webhook_url(&self) -> Result<Option<String>, ApiError> {
        Ok(None)
    }

    fn get_updates(&self, offset: Option<i64>, _timeout_s: u32) -> Result<Vec<Update>, ApiError> {
        self.offsets.lock().unwrap().push(offset);
        let next = self.script.lock().unwrap().pop_front();
        match next {
            Some(Poll::Batch(ids)) => Ok(updates(&ids)),
            Some(Poll::Real(list)) => Ok(list),
            Some(Poll::Fail(err)) => Err(err),
            Some(Poll::Bump(ids)) => {
                self.generation.fetch_add(1, Ordering::SeqCst);
                Ok(updates(&ids))
            }
            Some(Poll::BumpReal(list)) => {
                self.generation.fetch_add(1, Ordering::SeqCst);
                Ok(list)
            }
            // Script used up: end the loop, so a test can never hang.
            None => {
                self.generation.fetch_add(1, Ordering::SeqCst);
                Ok(Vec::new())
            }
        }
    }

    fn send(&self, chat_id: i64, out: &WireMessage, reply_to: Option<i64>) -> Result<i64, ApiError> {
        if let Some(err) = self.send_fail.lock().unwrap().pop_front() {
            return Err(err);
        }
        self.sent.lock().unwrap().push((chat_id, out.text.as_str().to_string(), reply_to));
        let mut next = self.next_message_id.lock().unwrap();
        *next += 1;
        Ok(*next)
    }

    fn edit(&self, _chat_id: i64, message_id: i64, _out: &WireMessage) -> Result<(), ApiError> {
        self.edits.lock().unwrap().push(message_id);
        Ok(())
    }

    fn answer_callback(&self, id: &str, _text: Option<&Redacted<String>>) -> Result<(), ApiError> {
        self.answers.lock().unwrap().push(id.to_string());
        Ok(())
    }

    fn set_commands(&self, _list: &[(&str, &str)]) -> Result<(), ApiError> {
        Ok(())
    }
}

/// A fresh text from the paired person, as the engine reads it.
fn text_update(id: i64, text: &str) -> Update {
    Update {
        update_id: id,
        kind: UpdateKind::Message(InMessage {
            message_id: id,
            chat_id: PAIRED_ID,
            chat_private: true,
            from_id: Some(PAIRED_ID),
            from_is_bot: false,
            username: None,
            first_name: None,
            date_s: (T0 / 1000) as i64 - 5,
            text: Some(text.into()),
            reply_to: None,
        }),
    }
}

/// Everything one test needs: the fakes, the shared generation and the shared link status.
struct Rig {
    api: Arc<FakeApi>,
    engine: Arc<Engine>,
    host: Arc<FakeHost>,
    generation: Arc<AtomicU64>,
    link: SharedLink,
}

/// `script` is what Telegram answers, in order. The host is paired with `CHAT` and turned on.
fn rig(script: Vec<Poll>) -> Rig {
    let generation = Arc::new(AtomicU64::new(0));
    Rig {
        api: Arc::new(FakeApi {
            script: Mutex::new(script.into()),
            offsets: Mutex::new(Vec::new()),
            generation: generation.clone(),
            send_fail: Mutex::new(VecDeque::new()),
            sent: Mutex::new(Vec::new()),
            edits: Mutex::new(Vec::new()),
            answers: Mutex::new(Vec::new()),
            next_message_id: Mutex::new(100),
        }),
        engine: Arc::new(Engine::new()),
        host: Arc::new(FakeHost::new()),
        generation,
        link: Arc::new(Mutex::new(LinkInfo::connecting())),
    }
}

/// Runs the poller to its end and returns how it ended.
fn run(r: &Rig) -> Outcome {
    let running = spawn_with(
        r.api.clone(),
        r.engine.clone(),
        r.host.clone(),
        r.generation.clone(),
        r.link.clone(),
        FAST,
    );
    running.join.join().unwrap()
}

/// A context for calling `deliver` directly.
fn ctx(r: &Rig) -> Ctx {
    Ctx {
        api: r.api.clone(),
        engine: r.engine.clone(),
        host: r.host.clone(),
        generation: r.generation.clone(),
        gen: r.generation.load(Ordering::SeqCst),
        link: r.link.clone(),
        halt: Arc::new(AtomicBool::new(false)),
        timing: FAST,
    }
}

fn plain(text: &str) -> Outgoing {
    render::plain(text)
}

#[test]
fn offset_advances_and_is_saved_before_the_batch_is_handled() {
    let r = rig(vec![Poll::Batch(vec![5]), Poll::Real(vec![text_update(6, "/stop")])]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(*r.api.offsets.lock().unwrap(), vec![None, Some(6)]);
    assert_eq!(r.host.config().last_update_id, Some(6));
    // The engine read the /stop of batch two: it switched Telegram off (keeping the saved offset) and said so.
    assert!(!r.host.config().enabled);
    assert_eq!(r.api.sent.lock().unwrap().len(), 1);
}

#[test]
fn a_restart_resumes_from_the_saved_offset() {
    let r = rig(vec![Poll::Bump(vec![])]);
    r.host.edit_config(|c| c.last_update_id = Some(9));
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(*r.api.offsets.lock().unwrap(), vec![Some(10)]);
}

#[test]
fn conflict_stops_the_loop_and_sets_conflict() {
    let r = rig(vec![Poll::Fail(ApiError::Conflict)]);
    assert_eq!(run(&r), Outcome::Failed(LinkError::Conflict));
    let link = *r.link.lock().unwrap();
    assert_eq!(link.state, LinkState::Error);
    assert_eq!(link.error, Some(LinkError::Conflict));
    assert_eq!(r.api.offsets.lock().unwrap().len(), 1, "no retry after a conflict");
    assert_eq!(r.host.audit_kinds(), vec![audit::Kind::ChannelError]);
}

#[test]
fn a_refused_token_or_bot_stops_with_its_own_state() {
    let cases = [
        (ApiError::Unauthorized, LinkError::TokenRejected),
        (ApiError::Forbidden, LinkError::Blocked),
    ];
    for (err, expected) in cases {
        let r = rig(vec![Poll::Fail(err)]);
        assert_eq!(run(&r), Outcome::Failed(expected));
        assert_eq!(r.link.lock().unwrap().error, Some(expected));
    }
}

#[test]
fn the_desktop_hears_when_the_link_goes_on_but_not_for_every_poll() {
    let r = rig(vec![Poll::Batch(vec![]), Poll::Batch(vec![]), Poll::Batch(vec![])]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(r.link.lock().unwrap().state, LinkState::On);
    assert_eq!(r.host.changed_times(), 1, "connecting to on once; the later polls only move the clock");
}

#[test]
fn the_desktop_hears_when_the_link_drops_and_comes_back() {
    let r = rig(vec![Poll::Batch(vec![]), Poll::Fail(ApiError::Network), Poll::Batch(vec![])]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(r.host.changed_times(), 3, "on, connecting, on");
}

#[test]
fn a_conflict_is_announced_once() {
    let r = rig(vec![Poll::Fail(ApiError::Conflict)]);
    assert_eq!(run(&r), Outcome::Failed(LinkError::Conflict));
    assert_eq!(r.host.changed_times(), 1);
}

#[test]
fn a_batch_fetched_after_a_restart_is_dropped() {
    let r = rig(vec![Poll::BumpReal(vec![text_update(7, "/stop")])]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert!(r.host.config().enabled, "the engine must not read the stale /stop");
    assert!(r.api.sent.lock().unwrap().is_empty());
    assert_eq!(r.host.config().last_update_id, None, "its offset is not saved, the new poller asks again");
}

#[test]
fn effect_stop_ends_the_loop() {
    let r = rig(vec![Poll::Real(vec![text_update(1, "/stop")]), Poll::Batch(vec![2])]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(r.api.offsets.lock().unwrap().len(), 1, "no poll after /stop");
}

#[test]
fn rate_limit_waits_and_keeps_polling() {
    let r = rig(vec![Poll::Fail(ApiError::RateLimited { retry_after_s: 0 }), Poll::Batch(vec![])]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(r.api.offsets.lock().unwrap().len(), 3);
    assert_eq!(r.link.lock().unwrap().error, None);
}

#[test]
fn network_trouble_backs_off_and_keeps_polling() {
    let r = rig(vec![Poll::Fail(ApiError::Network), Poll::Fail(ApiError::Bad { status: 500 }), Poll::Batch(vec![])]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert!(r.api.offsets.lock().unwrap().len() >= 3);
    let link = *r.link.lock().unwrap();
    assert_eq!(link.state, LinkState::On);
    assert_eq!(link.error, None);
    assert_eq!(link.last_poll_ms, Some(T0));
}

#[test]
fn the_engines_replies_go_out_in_order_to_the_paired_chat() {
    let r = rig(vec![Poll::Real(vec![text_update(1, "/help"), text_update(2, "/stop")])]);
    assert_eq!(run(&r), Outcome::Stopped);
    // The poller waits for the send queue, so everything has happened by now.
    let sent = r.api.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|(chat, _, reply_to)| *chat == CHAT && reply_to.is_none()));
    assert!(sent[0].1.starts_with("What I can do:"), "{:?}", sent[0].1);
    assert!(sent[1].1.contains("Telegram is off"), "{:?}", sent[1].1);
}

#[test]
fn a_sent_message_is_remembered_so_a_reply_to_it_is_routed() {
    let r = rig(vec![]);
    let c = ctx(&r);
    deliver(&c, Effect::Send { out: plain("hi"), reply_to: Some(9), remember: Some(Target::Pane("p1".into())) });
    assert_eq!(*r.api.sent.lock().unwrap(), vec![(CHAT, "hi".to_string(), Some(9))]);
    assert_eq!(r.engine.refs().lock().target_of(101, T0), Some(Target::Pane("p1".into())));
}

#[test]
fn edits_and_button_answers_are_delivered() {
    let r = rig(vec![]);
    let c = ctx(&r);
    deliver(&c, Effect::Edit { message_id: 3, out: plain("edited") });
    deliver(&c, Effect::AnswerCallback { id: "cb-1".into(), text: None });
    assert_eq!(*r.api.edits.lock().unwrap(), vec![3]);
    assert_eq!(*r.api.answers.lock().unwrap(), vec!["cb-1".to_string()]);
}

#[test]
fn send_to_goes_to_the_chat_it_names_even_before_pairing() {
    let r = rig(vec![]);
    r.host.edit_config(|c| c.paired = None);
    let c = ctx(&r);
    deliver(&c, Effect::SendTo { chat_id: 555, out: plain("Connected.") });
    assert_eq!(*r.api.sent.lock().unwrap(), vec![(555, "Connected.".to_string(), None)]);
    assert!(r.engine.refs().lock().target_of(101, T0).is_none(), "a pairing reply is never linked to a pane");
}

#[test]
fn a_refused_send_stops_the_poller_as_blocked() {
    let r = rig(vec![Poll::Real(vec![text_update(1, "/stop")])]);
    r.api.send_fail.lock().unwrap().push_back(ApiError::Forbidden);
    assert_eq!(run(&r), Outcome::Failed(LinkError::Blocked));
    assert_eq!(r.link.lock().unwrap().error, Some(LinkError::Blocked));
    assert!(r.api.sent.lock().unwrap().is_empty());
}

#[test]
fn without_a_paired_chat_nothing_else_is_sent() {
    let r = rig(vec![]);
    r.host.edit_config(|c| c.paired = None);
    let c = ctx(&r);
    deliver(&c, Effect::Send { out: plain("hi"), reply_to: None, remember: None });
    deliver(&c, Effect::Edit { message_id: 3, out: plain("edited") });
    deliver(&c, Effect::AnswerCallback { id: "cb-1".into(), text: None });
    assert!(r.api.sent.lock().unwrap().is_empty());
    assert!(r.api.edits.lock().unwrap().is_empty());
    assert!(r.api.answers.lock().unwrap().is_empty());
}
