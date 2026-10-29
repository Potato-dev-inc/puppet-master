//! Text Your Agents: the runtime. One `Runtime` owns the engine, the notifier and the poller thread,
//! and starts or stops the poller as the person's settings say. Everything it needs from the app comes
//! in through `Deps`, so the tests run it over a fake host and a fake bot API.
//!
//! Rules (spec 3.3, D-5):
//! - the poller runs only when the person asked for it: the channel is on and a chat is paired, or a
//!   pairing code is live and nobody is paired (pairing mode, which reads `/start` only);
//! - starting the app never turns anything on by itself: `apply` at start-up starts a poller only if
//!   the saved settings already say "on";
//! - a poller that Telegram refused (401, 403, 409) is NOT started again by itself. Only an action of
//!   the person (turn on, new token, make a code) tries again;
//! - stopping bumps the generation: the old poller drops whatever it fetched after that.

use super::engine::{Effect, Engine};
use super::host::Host;
use super::notify::Notifier;
use super::telegram::api::Api;
use super::telegram::poller::{self, LinkInfo, Running, SharedLink, Timing};
use crate::audit::{Kind as AuditKind, Who};
use crate::secrets::SecretBackend;
use crate::turn_summary::model::TurnSummary;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::JoinHandle;

/// How the runtime reaches the rest of the app. The live values are built in `channels::start`.
pub struct Deps {
    pub host: Arc<dyn Host>,
    pub secrets: Arc<dyn SecretBackend>,
    pub profile: Option<String>,
    /// Builds the bot client for a token. The client never shows the token.
    pub api_for: Arc<dyn Fn(&str) -> Arc<dyn Api> + Send + Sync>,
    pub timing: Timing,
    /// Audit lines written by the person on the PC or by a paired phone (the host audits as the chat).
    pub audit: Arc<dyn Fn(AuditKind, Who, &str, serde_json::Value) + Send + Sync>,
    /// Turns that ended at or after the given time (ms).
    pub turns: Arc<dyn Fn(u64) -> Vec<TurnSummary> + Send + Sync>,
}

/// The poller thread and the queue the notifier writes to.
pub(super) struct Live {
    pub join: JoinHandle<poller::Outcome>,
    pub outbox: Sender<Effect>,
}

/// A pairing code the Settings page is showing. In memory only.
pub(super) struct ShownCode {
    pub code: String,
    pub expires_ms: u64,
}

pub struct Runtime {
    pub(super) deps: Deps,
    pub(super) engine: Arc<Engine>,
    pub(super) notifier: Notifier,
    pub(super) generation: Arc<AtomicU64>,
    pub(super) link: SharedLink,
    pub(super) live: Mutex<Option<Live>>,
    pub(super) code: Mutex<Option<ShownCode>>,
    /// The channel is on and a chat is paired (so a poller that is alive may speak).
    pub(super) armed: AtomicBool,
    /// Which finished turns the 5 s tick already looked at (see `feed::TurnsSeen`).
    pub(super) turns_seen: Mutex<super::feed::TurnsSeen>,
}

impl Runtime {
    pub fn new(deps: Deps) -> Self {
        let start = deps.host.now_ms();
        Self {
            deps,
            engine: Arc::new(Engine::new()),
            notifier: Notifier::default(),
            generation: Arc::new(AtomicU64::new(0)),
            link: Arc::new(std::sync::Mutex::new(LinkInfo::connecting())),
            live: Mutex::new(None),
            code: Mutex::new(None),
            armed: AtomicBool::new(false),
            turns_seen: Mutex::new(super::feed::TurnsSeen::new(start)),
        }
    }

    /// True while a poller is running and the channel may speak: what `push` asks before it decides
    /// that nobody is listening to its notifications.
    pub fn listening(&self) -> bool {
        self.armed.load(Ordering::SeqCst) && self.alive()
    }

    /// A poller thread exists and has not ended.
    pub(super) fn alive(&self) -> bool {
        self.live.lock().as_ref().is_some_and(|l| !l.join.is_finished())
    }

    /// Whether a poller should be running with these settings.
    pub(super) fn wants_poller(&self) -> bool {
        let cfg = self.deps.host.config();
        match &cfg.paired {
            Some(_) => cfg.enabled,
            None => self.pairing_open(),
        }
    }

    /// A pairing code is live, or a chat sent the right one and waits for the PC's click.
    pub(super) fn pairing_open(&self) -> bool {
        let now = self.deps.host.now_ms();
        self.code.lock().as_ref().is_some_and(|c| now < c.expires_ms) || self.engine.pairing_waiting().is_some()
    }

    /// Brings the poller in line with the settings: starts it if it should run and does not, stops it
    /// if it runs and should not. Called after every action of the person, and once at start-up.
    pub fn apply(&self) {
        let cfg = self.deps.host.config();
        self.armed.store(cfg.enabled && cfg.paired.is_some(), Ordering::SeqCst);
        let wants = self.wants_poller();
        let alive = self.alive();
        if wants && !alive {
            self.spawn();
        } else if !wants && self.live.lock().is_some() {
            self.halt();
        }
    }

    /// Stops the poller and starts it again from the saved offset (when it should run).
    pub fn restart(&self) {
        self.halt();
        self.apply();
    }

    /// Stops the poller for good. The link status is reset, so an old error does not stay on screen.
    pub fn stop(&self) {
        self.halt();
        self.armed.store(false, Ordering::SeqCst);
        *self.link.lock().unwrap_or_else(|e| e.into_inner()) = LinkInfo::connecting();
    }

    /// Every 5 s: ends a poller that is no longer wanted (a pairing code that ran out) and forgets
    /// one that already ended by itself (`/stop`, a refusal from Telegram). Never starts one.
    pub fn watch(&self) {
        let cfg = self.deps.host.config();
        self.armed.store(cfg.enabled && cfg.paired.is_some(), Ordering::SeqCst);
        let finished = self.live.lock().as_ref().is_some_and(|l| l.join.is_finished());
        if finished {
            self.live.lock().take();
        } else if self.live.lock().is_some() && !self.wants_poller() {
            self.halt();
        }
    }

    fn spawn(&self) {
        let Some(token) = self.read_token() else { return };
        let api = (self.deps.api_for)(&token);
        let Running { join, outbox } = poller::spawn_with(
            api,
            self.engine.clone(),
            self.deps.host.clone(),
            self.generation.clone(),
            self.link.clone(),
            self.deps.timing,
        );
        *self.live.lock() = Some(Live { join, outbox });
        self.deps.host.changed();
    }

    /// Bumps the generation (the poller drops what it fetched after this) and lets go of the thread.
    /// It is not joined: a long poll can take 50 s to return, and nothing it fetches is acted on.
    pub(super) fn halt(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.live.lock().take();
    }

    /// The token from the credential store, or `None` when there is none. Masked from here on.
    pub(super) fn read_token(&self) -> Option<String> {
        super::token::read_with(&*self.deps.secrets, self.deps.profile.as_deref()).ok().flatten()
    }

    /// Queues a message for the paired chat. Nothing is sent when no poller is running.
    pub(super) fn queue(&self, effect: Effect) {
        if let Some(live) = self.live.lock().as_ref() {
            let _ = live.outbox.send(effect);
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
