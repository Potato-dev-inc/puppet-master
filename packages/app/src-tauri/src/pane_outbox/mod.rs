//! The pane outbox: messages a person wrote in the Chat tab (or on a phone) for the agent in a pane, sent
//! when the agent can take them. Nothing else queues text for a pane a person started by hand.
//!
//! Each pane has a short first-in-first-out list. One worker thread per pane that has messages looks at the
//! pane about every 400 ms (`agent_runs::readiness::assess`) and types the first waiting message only when the
//! agent is ready, its input box is empty and its status reads idle. It never presses a key on a dialog or a
//! question, never clears text the person typed in the terminal box, and never types into a shell.
//!
//! - `model`: the message, its states and the sentences shown to the person.
//! - `host`: what the outbox needs from the outside (`Host`), so the state machine runs without a terminal.
//! - `worker`: `decide` (one look -> deliver, wait, or fail) and the loop.
//! - `draft_guard`: a `DeliveryIo` wrapper that refuses to type over someone's text.
//! - `live`: `Host` over the live pane registry, the Tauri event and the SSE event `pane-outbox`.
//!
//! Generic: nothing here names an agent; the profile of the pane decides what ready means.

mod draft_guard;
mod host;
mod live;
mod model;
mod worker;

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;

pub use host::{Attempt, Host, Look, PaneKind};
pub use live::{global, redacted, LiveHost};
pub use model::{OutboxMessage, State, MAX_CHARS, MAX_WAITING};
/// The one decision per look, for the live checks that ask it of a real screen.
#[cfg(test)]
pub(crate) use worker::{decide, Step};

use model::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How often a worker looks, how long a message may wait, how long a finished one stays listed.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub poll: Duration,
    pub give_up: Duration,
    pub keep: Duration,
    /// After a message went in, the next one waits this long even if the pane still reads ready (the agent
    /// may not have drawn the start of its turn yet).
    pub after_send: Duration,
    /// After a startup notice ignored its own dismiss keys, the next try to close it waits this long (the keys are
    /// not pressed again every poll).
    pub gate_retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(400),
            give_up: Duration::from_secs(2 * 60 * 60),
            keep: Duration::from_secs(120),
            after_send: Duration::from_millis(1500),
            gate_retry: Duration::from_secs(20),
        }
    }
}

#[derive(Default)]
struct PaneQueue {
    entries: Vec<model::Entry>,
    worker: bool,
    last_sent: Option<Instant>,
    /// When delivery last came back blocked by a startup screen (`Attempt::Gate`).
    gate_tried: Option<Instant>,
}

struct Inner {
    host: Arc<dyn Host>,
    timing: Timing,
    panes: Mutex<HashMap<String, PaneQueue>>,
    next_id: AtomicU64,
    closed: AtomicBool,
}

pub struct Outbox {
    inner: Arc<Inner>,
}

impl Drop for Outbox {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::SeqCst);
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Outbox {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self::with_timing(host, Timing::default())
    }

    pub fn with_timing(host: Arc<dyn Host>, timing: Timing) -> Self {
        Self {
            inner: Arc::new(Inner {
                host,
                timing,
                panes: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
                closed: AtomicBool::new(false),
            }),
        }
    }

    /// Adds a message to the pane's list. The error is one plain sentence for the person.
    pub fn send(&self, pane_id: &str, text: &str) -> Result<OutboxMessage, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err(EMPTY.into());
        }
        if text.chars().count() > MAX_CHARS {
            return Err(TOO_LONG.into());
        }
        match self.inner.host.pane_kind(pane_id) {
            None => return Err(NO_SUCH_PANE.into()),
            Some(PaneKind::Shell) => return Err(NOT_AN_AGENT.into()),
            Some(PaneKind::Protocol) => return Err(NOT_SUPPORTED.into()),
            Some(PaneKind::Agent) => {}
        }
        if self.inner.host.run_controls(pane_id) {
            return Err(RUN_CONTROLS.into());
        }
        let (message, list, start_worker) = {
            let mut panes = self.inner.panes.lock();
            let queue = panes.entry(pane_id.to_string()).or_default();
            worker::prune(queue, &self.inner.timing);
            if queue.entries.iter().filter(|e| e.message.state.active()).count() >= MAX_WAITING {
                return Err(TOO_MANY.into());
            }
            let id = format!("m{}-{:x}", self.inner.next_id.fetch_add(1, Ordering::SeqCst), now_ms());
            let message = OutboxMessage {
                id,
                text: text.to_string(),
                created_at: now_ms(),
                state: State::Queued,
                waiting_for: None,
                error: None,
            };
            queue.entries.push(Entry { message: message.clone(), created: Instant::now(), finished: None });
            let start_worker = !queue.worker;
            queue.worker = true;
            (message, worker::snapshot(queue), start_worker)
        };
        self.inner.host.publish(pane_id, &list);
        if start_worker {
            worker::spawn(self.inner.clone(), pane_id);
        }
        Ok(message)
    }

    /// Cancels a message that has not started to be typed.
    pub fn cancel(&self, pane_id: &str, message_id: &str) -> Result<OutboxMessage, String> {
        let (message, list) = {
            let mut panes = self.inner.panes.lock();
            let queue = panes.get_mut(pane_id).ok_or_else(|| NO_SUCH_MESSAGE.to_string())?;
            let entry = queue
                .entries
                .iter_mut()
                .find(|e| e.message.id == message_id)
                .ok_or_else(|| NO_SUCH_MESSAGE.to_string())?;
            match entry.message.state {
                State::Queued => entry.finish(State::Cancelled, None),
                State::Sending => return Err(ALREADY_SENDING.into()),
                State::Sent | State::Failed | State::Cancelled => return Err(NO_SUCH_MESSAGE.into()),
            }
            let message = entry.message.clone();
            (message, worker::snapshot(queue))
        };
        self.inner.host.publish(pane_id, &list);
        Ok(message)
    }

    /// The pane's messages, oldest first: the waiting ones and those that finished in the last two minutes.
    pub fn list(&self, pane_id: &str) -> Vec<OutboxMessage> {
        let mut panes = self.inner.panes.lock();
        match panes.get_mut(pane_id) {
            Some(queue) => {
                worker::prune(queue, &self.inner.timing);
                worker::snapshot(queue)
            }
            None => Vec::new(),
        }
    }
}
