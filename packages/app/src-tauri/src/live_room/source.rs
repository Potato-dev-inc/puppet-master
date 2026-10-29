//! The app's two ends of a Live Room (SPEC-n32 3.8): `WatchSource` reads the shared panes through the chat
//! watchers, `ClientSink` hands sealed frames to the relay client. The publisher knows neither type, only the
//! `Source` and `Sink` traits, so it is tested with fakes. Nothing here sends anything to a pane.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use pm_relay_protocol::room::{RoomFrame, RoomStatus};

use super::client::{ClientError, RoomClient};
use super::pin::{ChatShare, Pins};
use super::publisher::{PaneNow, Sink, Source};
use crate::pty::status::PaneStatus;
use crate::pty::PaneRegistry;
use crate::session_chat::model::ChatItem;
use crate::session_chat::store::Delta;
use crate::session_chat::{Hold, Opening, Watchers};

/// How long a pane's reader stays open after the last renewal. The room renews well before this.
const LEASE: Duration = Duration::from_secs(90);
/// The room renews each lease this often (a renewal is cheap, but it copies the pane's items).
const RENEW_EVERY: Duration = Duration::from_secs(15);
/// Items read when a pane is read for the first time (the newest ones).
const FIRST_READ: usize = 400;

/// Reads the planned panes: their line (state, title, activity, pending card) and their chat items. Holds each
/// pane's reader with a lease while it lives and lets go in `Drop`, whatever way the room ends.
pub struct WatchSource {
    watchers: Arc<Watchers>,
    registry: Arc<Mutex<PaneRegistry>>,
    /// The holder name on the watchers: `HOLDER` for a room, another one for a preview.
    holder: &'static str,
    pane_ids: Vec<String>,
    /// How each pane's chat is read, found once.
    openings: HashMap<String, Opening>,
    /// When each pane's lease was last renewed.
    renewed: HashMap<String, Instant>,
    /// The agent session each pane was tied to when the room (or preview) first read it. A pane's chat is shared
    /// only while it is that session's (see `pin`).
    pins: Pins,
    /// The agent session id of a pane now: the hook's, else the one it was launched with.
    session_of: Arc<dyn Fn(&str) -> Option<String> + Send + Sync>,
    /// Each pane's last known label and title, kept for a pane that has left the registry.
    known: HashMap<String, (String, Option<String>)>,
}

impl WatchSource {
    pub fn new(
        watchers: Arc<Watchers>,
        registry: Arc<Mutex<PaneRegistry>>,
        holder: &'static str,
        pane_ids: Vec<String>,
    ) -> Self {
        Self::with_sessions(watchers, registry, holder, pane_ids, Arc::new(pane_session))
    }

    /// As `new`, with the way a pane's session id is found given (tests give their own).
    pub fn with_sessions(
        watchers: Arc<Watchers>,
        registry: Arc<Mutex<PaneRegistry>>,
        holder: &'static str,
        pane_ids: Vec<String>,
        session_of: Arc<dyn Fn(&str) -> Option<String> + Send + Sync>,
    ) -> Self {
        Self {
            watchers,
            registry,
            holder,
            pane_ids,
            openings: HashMap::new(),
            renewed: HashMap::new(),
            pins: Pins::default(),
            session_of,
            known: HashMap::new(),
        }
    }

    /// Whether the pane's chat may be shared now (see `pin`).
    fn share_of(&mut self, pane_id: &str) -> ChatShare {
        let Some(opening) = self.opening_of(pane_id) else { return ChatShare::Shared };
        let session = (self.session_of)(pane_id);
        read_chat(&mut self.pins, &self.watchers, pane_id, session.as_deref(), opening.is_file_only(), None, 0).0
    }

    /// How the pane's chat is read (found once). `None`: the pane is not open, or has no chat.
    fn opening_of(&mut self, pane_id: &str) -> Option<Opening> {
        if !self.openings.contains_key(pane_id) {
            let agent_type = self.registry.lock().get(pane_id).map(|pane| pane.info.agent_type.clone())?;
            self.openings.insert(pane_id.to_string(), Opening::for_agent(&agent_type)?);
        }
        self.openings.get(pane_id).cloned()
    }

    /// Keeps the pane's reader open, renewing the lease when it is due. Returns how the chat is read.
    fn keep_open(&mut self, pane_id: &str) -> Option<Opening> {
        let opening = self.opening_of(pane_id)?;
        let due = self.renewed.get(pane_id).map_or(true, |at| at.elapsed() >= RENEW_EVERY);
        if due {
            opening.open(&self.watchers, pane_id, Hold::lease(self.holder, LEASE));
            self.renewed.insert(pane_id.to_string(), Instant::now());
        }
        Some(opening)
    }
}

impl Source for WatchSource {
    fn panes(&mut self) -> Vec<PaneNow> {
        let ids = self.pane_ids.clone();
        let activity = crate::pane_activity::snapshot();
        let pending = crate::questions::store().list_pending(crate::questions::now_ms());
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            // A pane that closed, or whose process ended, is gone (`status` None). It keeps the name it had.
            let seen = self.registry.lock().get(&id).map(|pane| (pane.info(), *pane.exited.lock()));
            let Some((info, exited)) = seen else {
                if let Some((label, title)) = self.known.get(&id) {
                    out.push(PaneNow {
                        pane_id: id,
                        agent_label: label.clone(),
                        title: title.clone(),
                        status: None,
                        activity: None,
                        pending_card: false,
                        screen_source: false,
                        chat: ChatShare::Shared,
                    });
                }
                continue;
            };
            let label = crate::ambient::agent_label(&info.agent_type);
            self.known.insert(id.clone(), (label.clone(), info.title.clone()));
            if exited {
                out.push(PaneNow {
                    pane_id: id,
                    agent_label: label,
                    title: info.title.clone(),
                    status: None,
                    activity: None,
                    pending_card: false,
                    screen_source: false,
                    chat: ChatShare::Shared,
                });
                continue;
            }
            let chat = self.share_of(&id);
            let screen_source = self
                .opening_of(&id)
                .is_some_and(|opening| opening.source_now(&self.watchers, &id) == "screen");
            out.push(PaneNow {
                agent_label: label,
                title: info.title.clone(),
                status: Some(status_of(&info.status)),
                activity: activity.get(&id).cloned(),
                pending_card: pending.iter().any(|q| q.pane_id.as_deref() == Some(id.as_str())),
                screen_source,
                chat,
                pane_id: id,
            });
        }
        out
    }

    fn items_since(&mut self, pane_id: &str, rev: Option<u64>) -> Option<(u64, bool, Vec<ChatItem>)> {
        let opening = self.keep_open(pane_id)?;
        let cap = if rev.is_some() { usize::MAX } else { FIRST_READ };
        let session = (self.session_of)(pane_id);
        let (_, delta) =
            read_chat(&mut self.pins, &self.watchers, pane_id, session.as_deref(), opening.is_file_only(), rev, cap);
        let delta = delta?;
        Some((delta.rev, delta.reset, delta.items))
    }
}

impl Drop for WatchSource {
    fn drop(&mut self) {
        for id in &self.pane_ids {
            self.watchers.close(id, self.holder);
        }
    }
}

/// The agent session id of a pane now: the one its hooks reported (it follows `/clear` and resume), else the one the
/// pane was launched with.
fn pane_session(pane_id: &str) -> Option<String> {
    crate::agent_hooks::ledger::pinned_session(pane_id).or_else(|| crate::pty::launch_session::get(pane_id))
}

/// What the pane's reader holds after `rev`, when this room may share it. The reader is shared with the Chat tab, and
/// a pane without a known session follows the newest file of its folder, which can be another session's. So the
/// delta is used only when the file it came from is the pinned session's (`pin`): the file is noted with the items,
/// under one lock, so a switch between the check and the read cannot leak. `file_only`: the chat is read from a
/// file and from nothing else (a chat read off the screen is the pane's own and needs no pin).
fn read_chat(
    pins: &mut Pins,
    watchers: &Watchers,
    pane_id: &str,
    session: Option<&str>,
    file_only: bool,
    rev: Option<u64>,
    cap: usize,
) -> (ChatShare, Option<Delta>) {
    let delta = watchers.since(pane_id, rev, cap);
    let file = delta.as_ref().and_then(|delta| delta.file.as_deref());
    let share = pins.check(pane_id, session, file, file_only || file.is_some());
    (share, delta.filter(|_| share == ChatShare::Shared))
}

/// The pane's status word as the registry keeps it. A word this code does not know counts as idle.
fn status_of(word: &str) -> PaneStatus {
    match word {
        "running" => PaneStatus::Running,
        "waiting_input" => PaneStatus::WaitingInput,
        "error" => PaneStatus::Error,
        _ => PaneStatus::Idle,
    }
}

/// Sends sealed frames to the relay with the room key. `end` deletes the room.
pub struct ClientSink {
    client: RoomClient,
}

impl ClientSink {
    pub fn new(client: RoomClient) -> Self {
        Self { client }
    }
}

impl Sink for ClientSink {
    fn snapshot(&mut self, frame: &RoomFrame) -> Result<RoomStatus, ClientError> {
        self.client.put_snapshot(frame)
    }

    fn frame(&mut self, frame: &RoomFrame) -> Result<RoomStatus, ClientError> {
        self.client.post_frame(frame)
    }

    fn ping(&mut self) -> Result<RoomStatus, ClientError> {
        self.client.status()
    }

    /// The relay wipes the room's frames and keeps a tombstone. A failed call is not retried here: the room's
    /// time limit on the relay ends it anyway.
    fn end(&mut self) {
        let _ = self.client.delete();
    }
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod chat_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_words_map_and_an_unknown_one_is_idle() {
        assert_eq!(status_of("running"), PaneStatus::Running);
        assert_eq!(status_of("waiting_input"), PaneStatus::WaitingInput);
        assert_eq!(status_of("error"), PaneStatus::Error);
        assert_eq!(status_of("idle"), PaneStatus::Idle);
        assert_eq!(status_of("something_new"), PaneStatus::Idle);
    }
}
