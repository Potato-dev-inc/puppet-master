//! Poller tests with a fake `Api`, a fake engine and a fake host. No test talks to Telegram.

use super::*;
use crate::channels::config::PairedChat;
use crate::channels::render;
use crate::channels::telegram::api::{ApiError, BotInfo, UpdateKind};
use std::collections::VecDeque;

const CHAT: i64 = 42;

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
    /// A batch with these update ids.
    Batch(Vec<i64>),
    Fail(ApiError),
    /// Bumps the generation (a restart), then returns this batch.
    Bump(Vec<i64>),
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
            Some(Poll::Fail(err)) => Err(err),
            Some(Poll::Bump(ids)) => {
                self.generation.fetch_add(1, Ordering::SeqCst);
                Ok(updates(&ids))
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

/// Hands out one scripted list of effects per batch, and records what it was given.
struct FakeEngine {
    replies: Mutex<VecDeque<Vec<Effect>>>,
    /// The update ids of each batch the engine was asked about.
    seen: Mutex<Vec<Vec<i64>>>,
    remembered: Mutex<Vec<(i64, Target)>>,
}

impl Engine for FakeEngine {
    fn on_batch(&self, _host: &dyn Host, updates: &[Update]) -> Vec<Effect> {
        self.seen.lock().unwrap().push(updates.iter().map(|u| u.update_id).collect());
        self.replies.lock().unwrap().pop_front().unwrap_or_default()
    }

    fn remember(&self, message_id: i64, target: Target, _now_ms: u64) {
        self.remembered.lock().unwrap().push((message_id, target));
    }
}

struct FakeHost {
    config: Mutex<TelegramConfig>,
    saves: Mutex<usize>,
    audits: Mutex<Vec<&'static str>>,
}

impl Host for FakeHost {
    fn now_ms(&self) -> u64 {
        1_000
    }

    fn config(&self) -> TelegramConfig {
        self.config.lock().unwrap().clone()
    }

    fn save_config(&self, config: &TelegramConfig) -> Result<(), String> {
        *self.config.lock().unwrap() = config.clone();
        *self.saves.lock().unwrap() += 1;
        Ok(())
    }

    fn audit(&self, kind: audit::Kind, _summary: &str, _detail: Value) {
        self.audits.lock().unwrap().push(kind.as_str());
    }

    fn changed(&self) {}
}

/// Everything one test needs: the fakes, the shared generation and the shared link status.
struct Rig {
    api: Arc<FakeApi>,
    engine: Arc<FakeEngine>,
    host: Arc<FakeHost>,
    generation: Arc<AtomicU64>,
    link: SharedLink,
}

fn rig(script: Vec<Poll>, replies: Vec<Vec<Effect>>) -> Rig {
    let generation = Arc::new(AtomicU64::new(0));
    let config = TelegramConfig {
        enabled: true,
        paired: Some(PairedChat { chat_id: CHAT, user_id: 7, username: None, first_name: None, paired_at_ms: 0 }),
        ..TelegramConfig::default()
    };
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
        engine: Arc::new(FakeEngine {
            replies: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
            remembered: Mutex::new(Vec::new()),
        }),
        host: Arc::new(FakeHost {
            config: Mutex::new(config),
            saves: Mutex::new(0),
            audits: Mutex::new(Vec::new()),
        }),
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

fn plain(text: &str) -> Outgoing {
    render::plain(text)
}

#[test]
fn offset_advances_and_is_saved_before_the_batch_is_handled() {
    let r = rig(vec![Poll::Batch(vec![5]), Poll::Batch(vec![6])], vec![vec![], vec![Effect::Stop]]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(*r.api.offsets.lock().unwrap(), vec![None, Some(6)]);
    assert_eq!(r.host.config().last_update_id, Some(6));
    assert_eq!(*r.engine.seen.lock().unwrap(), vec![vec![5], vec![6]]);
}

#[test]
fn a_restart_resumes_from_the_saved_offset() {
    let r = rig(vec![Poll::Bump(vec![])], vec![]);
    r.host.config.lock().unwrap().last_update_id = Some(9);
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(*r.api.offsets.lock().unwrap(), vec![Some(10)]);
}

#[test]
fn conflict_stops_the_loop_and_sets_conflict() {
    let r = rig(vec![Poll::Fail(ApiError::Conflict)], vec![]);
    assert_eq!(run(&r), Outcome::Failed(LinkError::Conflict));
    let link = *r.link.lock().unwrap();
    assert_eq!(link.state, LinkState::Error);
    assert_eq!(link.error, Some(LinkError::Conflict));
    assert_eq!(r.api.offsets.lock().unwrap().len(), 1, "no retry after a conflict");
    assert_eq!(*r.host.audits.lock().unwrap(), vec!["channel.error"]);
}

#[test]
fn a_refused_token_or_bot_stops_with_its_own_state() {
    let cases = [
        (ApiError::Unauthorized, LinkError::TokenRejected),
        (ApiError::Forbidden, LinkError::Blocked),
    ];
    for (err, expected) in cases {
        let r = rig(vec![Poll::Fail(err)], vec![]);
        assert_eq!(run(&r), Outcome::Failed(expected));
        assert_eq!(r.link.lock().unwrap().error, Some(expected));
    }
}

#[test]
fn a_batch_fetched_after_a_restart_is_dropped() {
    let r = rig(vec![Poll::Bump(vec![7])], vec![vec![Effect::Stop]]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert!(r.engine.seen.lock().unwrap().is_empty(), "the engine must not see the stale batch");
    assert_eq!(r.host.config().last_update_id, None, "its offset is not saved, the new poller asks again");
}

#[test]
fn effect_stop_ends_the_loop() {
    let r = rig(vec![Poll::Batch(vec![1]), Poll::Batch(vec![2])], vec![vec![Effect::Stop]]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(r.api.offsets.lock().unwrap().len(), 1, "no poll after /stop");
}

#[test]
fn rate_limit_waits_and_keeps_polling() {
    let r = rig(vec![Poll::Fail(ApiError::RateLimited { retry_after_s: 0 }), Poll::Batch(vec![])], vec![]);
    assert_eq!(run(&r), Outcome::Stopped);
    assert_eq!(r.api.offsets.lock().unwrap().len(), 3);
    assert_eq!(r.link.lock().unwrap().error, None);
}

#[test]
fn network_trouble_backs_off_and_keeps_polling() {
    let r = rig(
        vec![Poll::Fail(ApiError::Network), Poll::Fail(ApiError::Bad { status: 500 }), Poll::Batch(vec![])],
        vec![],
    );
    assert_eq!(run(&r), Outcome::Stopped);
    assert!(r.api.offsets.lock().unwrap().len() >= 3);
    let link = *r.link.lock().unwrap();
    assert_eq!(link.state, LinkState::On);
    assert_eq!(link.error, None);
    assert_eq!(link.last_poll_ms, Some(1_000));
}

#[test]
fn queued_effects_go_out_in_order_and_sent_messages_are_remembered() {
    let r = rig(
        vec![Poll::Batch(vec![1])],
        vec![vec![
            Effect::Send { out: plain("hi"), reply_to: Some(9), remember: Some(Target::Pane("p1".into())) },
            Effect::Edit { message_id: 3, out: plain("edited") },
            Effect::AnswerCallback { id: "cb-1".into(), text: None },
            Effect::Stop,
        ]],
    );
    assert_eq!(run(&r), Outcome::Stopped);
    // The poller waits for the send queue, so everything above has happened by now.
    assert_eq!(*r.api.sent.lock().unwrap(), vec![(CHAT, "hi".to_string(), Some(9))]);
    assert_eq!(*r.api.edits.lock().unwrap(), vec![3]);
    assert_eq!(*r.api.answers.lock().unwrap(), vec!["cb-1".to_string()]);
    assert_eq!(*r.engine.remembered.lock().unwrap(), vec![(101, Target::Pane("p1".into()))]);
}

#[test]
fn a_refused_send_stops_the_poller_as_blocked() {
    let r = rig(
        vec![Poll::Batch(vec![1])],
        vec![vec![Effect::Send { out: plain("hi"), reply_to: None, remember: None }, Effect::Stop]],
    );
    r.api.send_fail.lock().unwrap().push_back(ApiError::Forbidden);
    assert_eq!(run(&r), Outcome::Failed(LinkError::Blocked));
    assert_eq!(r.link.lock().unwrap().error, Some(LinkError::Blocked));
    assert!(r.api.sent.lock().unwrap().is_empty());
}

#[test]
fn without_a_paired_chat_nothing_is_sent() {
    let r = rig(
        vec![Poll::Batch(vec![1])],
        vec![vec![Effect::Send { out: plain("hi"), reply_to: None, remember: None }, Effect::Stop]],
    );
    r.host.config.lock().unwrap().paired = None;
    assert_eq!(run(&r), Outcome::Stopped);
    assert!(r.api.sent.lock().unwrap().is_empty());
}
