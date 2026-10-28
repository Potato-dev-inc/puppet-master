//! The app's two ends of a Live Room (SPEC-n32 3.8): `WatchSource` reads the shared panes through the chat
//! watchers, `ClientSink` hands sealed frames to the relay client. The publisher knows neither type, only the
//! `Source` and `Sink` traits, so it is tested with fakes. Nothing here sends anything to a pane.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use pm_relay_protocol::room::{RoomFrame, RoomStatus};

use super::client::{ClientError, RoomClient};
use super::publisher::{PaneNow, Sink, Source};
use crate::pty::status::PaneStatus;
use crate::pty::PaneRegistry;
use crate::session_chat::model::ChatItem;
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
}

impl WatchSource {
    pub fn new(
        watchers: Arc<Watchers>,
        registry: Arc<Mutex<PaneRegistry>>,
        holder: &'static str,
        pane_ids: Vec<String>,
    ) -> Self {
        Self { watchers, registry, holder, pane_ids, openings: HashMap::new(), renewed: HashMap::new() }
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
            // A pane that closed, or whose process ended, is gone: it is left out and the publisher says so.
            let info = self.registry.lock().get(&id).filter(|pane| !*pane.exited.lock()).map(|pane| pane.info());
            let Some(info) = info else { continue };
            let screen_source = self
                .opening_of(&id)
                .is_some_and(|opening| opening.source_now(&self.watchers, &id) == "screen");
            out.push(PaneNow {
                agent_label: crate::ambient::agent_label(&info.agent_type),
                title: info.title.clone(),
                status: Some(status_of(&info.status)),
                activity: activity.get(&id).cloned(),
                pending_card: pending.iter().any(|q| q.pane_id.as_deref() == Some(id.as_str())),
                screen_source,
                pane_id: id,
            });
        }
        out
    }

    fn items_since(&mut self, pane_id: &str, rev: Option<u64>) -> Option<(u64, bool, Vec<ChatItem>)> {
        self.keep_open(pane_id)?;
        let cap = if rev.is_some() { usize::MAX } else { FIRST_READ };
        let delta = self.watchers.since(pane_id, rev, cap)?;
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

    /// The relay wipes the room's frames and keeps a tombstone. A failed call is not retried here: the room's
    /// time limit on the relay ends it anyway.
    fn end(&mut self) {
        let _ = self.client.delete();
    }
}

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
