//! Telegram long-poll loop and send queue.
//!
//! The poll thread asks Telegram for new messages (a long poll), keeps the offset so nothing is read
//! twice, and hands each batch to the engine. What the engine wants sent goes into a queue; a second
//! thread sends it, at most one message a second. Both threads end when the generation moves on
//! (a restart bumps it), when the engine returns `Effect::Stop`, or when Telegram refuses the token,
//! the bot or the poll (401, 403, 409). 429 waits `retry_after`; other failures back off from 1 s to 60 s.
//! Nothing here reads or formats the bot token: the `Api` implementation never returns it.

use super::api::{Api, ApiError, POLL_TIMEOUT_S};
use crate::audit;
use crate::channels::engine::{to_wire, Effect, Engine, GroupJob};
use crate::channels::host::Host;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Where the link stands, for the status line. Written only by the poller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkInfo {
    pub state: LinkState,
    pub error: Option<LinkError>,
    /// When Telegram last answered a poll, in ms.
    pub last_poll_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    On,
    Error,
}

/// Why the poller stopped for good. Names match the status the UI shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    TokenRejected,
    Conflict,
    Blocked,
}

impl LinkInfo {
    pub fn connecting() -> Self {
        Self { state: LinkState::Connecting, error: None, last_poll_ms: None }
    }
}

/// The link status, shared between the poller and whoever reports it.
pub type SharedLink = Arc<Mutex<LinkInfo>>;

/// How the poller ends: on purpose (`Stopped`) or because Telegram refused (`Failed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Stopped,
    Failed(LinkError),
}

/// What `spawn` returns: the poller's join handle, and a queue for messages the notifier wants sent.
pub struct Running {
    pub join: JoinHandle<Outcome>,
    pub outbox: Sender<Effect>,
}

/// The waits. Live values come from `Timing::LIVE`; tests use short ones.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub poll_timeout_s: u32,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    /// Gap between two sent messages (at most one a second).
    pub send_gap: Duration,
    /// How often a wait checks whether it should stop.
    pub tick: Duration,
}

impl Timing {
    pub const LIVE: Timing = Timing {
        poll_timeout_s: POLL_TIMEOUT_S,
        backoff_min: Duration::from_secs(1),
        backoff_max: Duration::from_secs(60),
        send_gap: Duration::from_secs(1),
        tick: Duration::from_millis(200),
    };
}

/// While a group question is being answered, "typing…" is shown again this often (Telegram clears it after 5 s).
const TYPING_EVERY: Duration = Duration::from_secs(4);

/// Retries for a send that Telegram rate-limited, before the message is dropped.
const SEND_RETRIES: u32 = 2;

/// Starts the poller with the live timing. The poller runs while `generation` still holds the value it
/// had at this call: a bump stops it.
pub fn spawn(
    api: Arc<dyn Api>,
    engine: Arc<Engine>,
    host: Arc<dyn Host>,
    generation: Arc<AtomicU64>,
    link: SharedLink,
) -> Running {
    spawn_with(api, engine, host, generation, link, Timing::LIVE)
}

/// `spawn` with explicit timing (tests).
pub fn spawn_with(
    api: Arc<dyn Api>,
    engine: Arc<Engine>,
    host: Arc<dyn Host>,
    generation: Arc<AtomicU64>,
    link: SharedLink,
    timing: Timing,
) -> Running {
    let gen = generation.load(Ordering::SeqCst);
    *lock(&link) = LinkInfo::connecting();
    let ctx = Ctx {
        api,
        engine,
        host,
        generation,
        gen,
        link,
        halt: Arc::new(AtomicBool::new(false)),
        timing,
    };
    let (tx, rx) = mpsc::channel::<Effect>();
    let sender_ctx = ctx.clone();
    let sender = thread::Builder::new()
        .name("telegram-send".into())
        .spawn(move || run_sender(sender_ctx, rx))
        .expect("start the Telegram send queue");
    let poll_tx = tx.clone();
    let join = thread::Builder::new()
        .name("telegram-poll".into())
        .spawn(move || {
            run_poll(&ctx, &poll_tx);
            // The queue still sends what is already in it, then ends (see `run_sender`).
            drop(poll_tx);
            ctx.halt.store(true, Ordering::SeqCst);
            let _ = sender.join();
            match lock(&ctx.link).error {
                Some(err) => Outcome::Failed(err),
                None => Outcome::Stopped,
            }
        })
        .expect("start the Telegram poll loop");
    Running { join, outbox: tx }
}

#[derive(Clone)]
struct Ctx {
    api: Arc<dyn Api>,
    engine: Arc<Engine>,
    host: Arc<dyn Host>,
    generation: Arc<AtomicU64>,
    /// The generation this poller was started in.
    gen: u64,
    link: SharedLink,
    /// Set when this poller must stop polling: `Effect::Stop`, or a refusal from Telegram.
    halt: Arc<AtomicBool>,
    timing: Timing,
}

impl Ctx {
    /// True while nothing has told this poller to stop.
    fn alive(&self) -> bool {
        self.current() && !self.halt.load(Ordering::SeqCst)
    }

    /// True until a newer poller has been started.
    fn current(&self) -> bool {
        self.generation.load(Ordering::SeqCst) == self.gen
    }

    /// Waits `d`, checking `alive` every tick. False when the poller should stop instead.
    fn pause(&self, d: Duration) -> bool {
        self.wait(d, || self.alive())
    }

    /// Like `pause`, but only a generation bump stops it, so queued messages still go out after a halt.
    fn pace(&self, d: Duration) -> bool {
        self.wait(d, || self.current())
    }

    fn wait(&self, d: Duration, keep_going: impl Fn() -> bool) -> bool {
        let until = Instant::now() + d;
        loop {
            if !keep_going() {
                return false;
            }
            let now = Instant::now();
            if now >= until {
                return true;
            }
            thread::sleep((until - now).min(self.timing.tick));
        }
    }

    /// Updates the link. When the state or the error changed (not just the time of the last poll), the
    /// desktop is told, so the status line in Settings does not keep saying "Connecting…".
    fn set_link(&self, f: impl FnOnce(&mut LinkInfo)) {
        let moved = {
            let mut link = lock(&self.link);
            let before = (link.state, link.error);
            f(&mut link);
            before != (link.state, link.error)
        };
        if moved && self.current() {
            self.host.changed();
        }
    }

    /// Telegram refused for good: say why, audit it, and stop. A poller that was already replaced
    /// must not write over the newer one's status.
    fn fail(&self, err: LinkError) {
        if !self.current() {
            return;
        }
        {
            let mut link = lock(&self.link);
            link.state = LinkState::Error;
            link.error = Some(err);
        }
        self.halt.store(true, Ordering::SeqCst);
        self.host.audit(audit::Kind::ChannelError, "Telegram stopped", json!({ "error": error_code(err) }));
        self.host.changed();
    }
}

fn lock(link: &SharedLink) -> MutexGuard<'_, LinkInfo> {
    link.lock().unwrap_or_else(|e| e.into_inner())
}

fn error_code(err: LinkError) -> &'static str {
    match err {
        LinkError::TokenRejected => "token_rejected",
        LinkError::Conflict => "conflict",
        LinkError::Blocked => "blocked",
    }
}

/// The refusals that end the poller. Network trouble, 5xx and parse errors are not in this list.
fn fatal_for(err: ApiError) -> Option<LinkError> {
    match err {
        ApiError::Unauthorized => Some(LinkError::TokenRejected),
        ApiError::Conflict => Some(LinkError::Conflict),
        ApiError::Forbidden => Some(LinkError::Blocked),
        _ => None,
    }
}

fn run_poll(ctx: &Ctx, outbox: &Sender<Effect>) {
    let mut offset = ctx.host.config().last_update_id.map(|id| id + 1);
    let mut backoff = ctx.timing.backoff_min;
    while ctx.alive() {
        let polled = ctx.api.get_updates(offset, ctx.timing.poll_timeout_s);
        // Checked before anything is acted on: a batch fetched after a restart is dropped, and the new
        // poller asks again from the saved offset.
        if !ctx.alive() {
            break;
        }
        match polled {
            Ok(updates) => {
                let now = ctx.host.now_ms();
                ctx.set_link(|l| {
                    l.state = LinkState::On;
                    l.error = None;
                    l.last_poll_ms = Some(now);
                });
                backoff = ctx.timing.backoff_min;
                let Some(last) = updates.iter().map(|u| u.update_id).max() else { continue };
                // Saved before the engine runs, so a message is never read twice after a restart.
                offset = Some(last + 1);
                save_offset(ctx, last);
                for effect in ctx.engine.on_batch(&*ctx.host, &updates) {
                    match effect {
                        Effect::Stop => {
                            ctx.halt.store(true, Ordering::SeqCst);
                            break;
                        }
                        // A group question takes seconds to a minute: it never runs on the poll thread.
                        Effect::Ask(job) => spawn_ask(ctx, outbox, job),
                        other => {
                            let _ = outbox.send(other);
                        }
                    }
                }
            }
            Err(ApiError::RateLimited { retry_after_s }) => {
                if !ctx.pause(Duration::from_secs(retry_after_s)) {
                    break;
                }
            }
            Err(err) => {
                if let Some(fatal) = fatal_for(err) {
                    ctx.fail(fatal);
                    break;
                }
                ctx.set_link(|l| l.state = LinkState::Connecting);
                if !ctx.pause(backoff) {
                    break;
                }
                backoff = (backoff * 2).min(ctx.timing.backoff_max);
            }
        }
    }
}

/// Starts the thread that answers one group question. If the thread cannot start, the question's place
/// is freed and nothing is said to the group.
fn spawn_ask(ctx: &Ctx, outbox: &Sender<Effect>, job: GroupJob) {
    let engine = ctx.engine.clone();
    let (ctx, outbox) = (ctx.clone(), outbox.clone());
    let started = thread::Builder::new().name("telegram-ask".into()).spawn(move || run_ask(ctx, outbox, job));
    if started.is_err() {
        engine.abandon_job();
    }
}

/// Asks on a helper thread and keeps "typing…" showing in the group until the reply is queued. A poller
/// that was replaced in the meantime lets the answer go.
fn run_ask(ctx: Ctx, outbox: Sender<Effect>, job: GroupJob) {
    if !ctx.current() {
        ctx.engine.abandon_job();
        return;
    }
    let chat_id = job.chat_id;
    let (done, result) = mpsc::channel::<Vec<Effect>>();
    let (engine, host) = (ctx.engine.clone(), ctx.host.clone());
    let worker = thread::Builder::new().name("telegram-ask-run".into()).spawn(move || {
        let _ = done.send(engine.answer_job(&*host, job));
    });
    if worker.is_err() {
        ctx.engine.abandon_job();
        return;
    }
    loop {
        let _ = ctx.api.typing(chat_id);
        match result.recv_timeout(TYPING_EVERY) {
            Ok(effects) => {
                if ctx.current() {
                    for effect in effects {
                        let _ = outbox.send(effect);
                    }
                }
                return;
            }
            Err(RecvTimeoutError::Timeout) if ctx.current() => {}
            Err(_) => return,
        }
    }
}

/// Writes the offset into the config. The config is read again right before the write, so a change
/// the engine just saved (for example `/stop`) is kept. A failed write is ignored: the offset still
/// moves on in memory, and the next batch saves it again.
fn save_offset(ctx: &Ctx, last_update_id: i64) {
    let mut config = ctx.host.config();
    config.last_update_id = Some(last_update_id);
    let _ = ctx.host.save_config(&config);
}

fn run_sender(ctx: Ctx, rx: Receiver<Effect>) {
    loop {
        if !ctx.current() {
            return;
        }
        match rx.recv_timeout(ctx.timing.tick) {
            Ok(effect) => {
                deliver(&ctx, effect);
                if !ctx.pace(ctx.timing.send_gap) {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // The poller has stopped and the queue is empty.
                if ctx.halt.load(Ordering::SeqCst) {
                    return;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Sends one queued effect to the paired chat (`SendTo`: to the chat it names). Messages wait while Telegram rate-limits us, and are
/// dropped if it still refuses. Without a paired chat nothing is sent.
fn deliver(ctx: &Ctx, effect: Effect) {
    let paired = ctx.host.config().paired.map(|p| p.chat_id);
    // Everything but `SendTo` goes to the paired chat only; `SendTo` names its own chat (pairing replies).
    if paired.is_none() && !matches!(effect, Effect::SendTo { .. } | Effect::Reply { .. }) {
        return;
    }
    let chat_id = paired.unwrap_or_default();
    let mut retries = 0;
    loop {
        let result: Result<Option<i64>, ApiError> = match &effect {
            Effect::Send { out, reply_to, .. } => ctx.api.send(chat_id, &to_wire(out), *reply_to).map(Some),
            Effect::SendTo { chat_id: to, out } => ctx.api.send(*to, &to_wire(out), None).map(Some),
            Effect::Reply { chat_id: to, reply_to, out } => ctx.api.send(*to, &to_wire(out), Some(*reply_to)).map(Some),
            Effect::Edit { message_id, out } => ctx.api.edit(chat_id, *message_id, &to_wire(out)).map(|()| None),
            Effect::AnswerCallback { id, text } => ctx.api.answer_callback(id, text.as_ref()).map(|()| None),
            // Never queued: the poller handles them.
            Effect::Stop | Effect::Ask(_) => return,
        };
        match result {
            Ok(sent_id) => {
                if let (Effect::Send { remember: Some(target), .. }, Some(message_id)) = (&effect, sent_id) {
                    ctx.engine.refs().lock().remember(message_id, target.clone(), ctx.host.now_ms());
                }
                return;
            }
            Err(ApiError::RateLimited { retry_after_s }) if retries < SEND_RETRIES => {
                retries += 1;
                if !ctx.pace(Duration::from_secs(retry_after_s)) {
                    return;
                }
            }
            Err(err) => {
                // A refusal for one group (the bot was removed, or may not write there) says nothing about
                // the paired chat, so it never stops the poller.
                if !matches!(effect, Effect::Reply { .. }) {
                    if let Some(fatal) = fatal_for(err) {
                        ctx.fail(fatal);
                    }
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
