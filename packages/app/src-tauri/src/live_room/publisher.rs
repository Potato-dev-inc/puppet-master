//! The publisher of one Live Room (SPEC-n32 3.5, 3.6, 3.8). Every tick it reads the shared panes, turns the new chat
//! items into viewer rows (`view`), queues them, seals the frames (`seal`) and hands them to a `Sink` (the relay
//! client). It also decides when the room stops. It has no Tauri types: the app's `service` gives it a `Source`
//! (the watchers) and a `Sink` (the relay). Pause throws away what is read while paused. Screen-read chats are held
//! back around a card. Frame plaintext is the camelCase JSON of `Body`, sealed by `seal`.

use std::collections::VecDeque;
use std::time::Duration;

use pm_relay_protocol::room::{FrameKind, RoomFrame, RoomStatus};
use serde::Serialize;

use super::client::ClientError;
use super::pin::{note_for, ChatShare};
use super::seal::Sealer;
use super::view::{clean, pane_view, to_live, LiveItem, LivePane, LiveState};
use super::{CLOCK_JUMP, IDLE_STOP, SCREEN_HOLD_BACK, SNAPSHOT_EVERY};
use crate::pty::status::PaneStatus;
use crate::redact::RedactContext;
use crate::session_chat::model::ChatItem;
use crate::tui::Activity;

/// Chat rows kept per pane for a snapshot.
const RING_ROWS: usize = 60;
/// Most rows in one delta frame.
const DELTA_ROWS: usize = 40;
/// Most delta frames sent in one tick.
const DELTA_FRAMES_PER_TICK: usize = 2;
/// Plaintext cap of a delta or state frame.
const DELTA_BYTES: usize = 12 * 1024;
/// Plaintext cap of a snapshot frame. The oldest rows are dropped until it fits.
const SNAPSHOT_BYTES: usize = 60 * 1024;
/// At most one state frame per this long.
const STATE_EVERY_MS: u64 = 2_000;
/// This many failed calls in a row, spanning at least `RELAY_LOST_MS`, stop the room.
const RELAY_LOST_CALLS: u32 = 3;
const RELAY_LOST_MS: u64 = 30_000;
/// When no call to the relay was tried for this long, the room asks the relay for its status. In a quiet room no frame
/// goes out, so without it a lost relay would only be noticed at the next snapshot (up to 60 s) and then take another
/// `RELAY_LOST_MS`. With it the room stops about 30 to 40 s after the relay is lost.
const HEARTBEAT_MS: u64 = 10_000;
/// The room title when the cleaned title is empty.
const DEFAULT_TITLE: &str = "Live Room";
const TITLE_CHARS: usize = 60;
/// The fixed reason of a frame this PC could not build or seal. It counts as a failed call.
const LOCAL_FRAME_ERROR: &str = "The frame could not be built.";

/// What the person picked and when the room must end. `pane_ids` is the viewers' order: pane `p1` is the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomPlan {
    pub room_id: String,
    pub title: String,
    pub pane_ids: Vec<String>,
    pub ends_at_ms: u64,
}

/// One shared pane as the watchers read it now. `status` `None` means the pane is gone.
#[derive(Debug, Clone)]
pub struct PaneNow {
    pub pane_id: String,
    pub agent_label: String,
    pub title: Option<String>,
    pub status: Option<PaneStatus>,
    pub activity: Option<Activity>,
    pub pending_card: bool,
    /// The chat comes off the screen (not a session file), so it can hold a dialog.
    pub screen_source: bool,
    /// Whether the pane's chat may be shared (it is the pane's own session). Not shared: no rows, and one note.
    pub chat: ChatShare,
}

/// Reads the shared panes and their chat. The app implements it over `Watchers`.
pub trait Source {
    fn panes(&mut self) -> Vec<PaneNow>;
    /// The chat items of `pane_id` after revision `rev` (`None`: all of them). Returns the new revision, whether the
    /// source started the chat over (the publisher then sends a snapshot), and the items. `None`: no chat for it.
    fn items_since(&mut self, pane_id: &str, rev: Option<u64>) -> Option<(u64, bool, Vec<ChatItem>)>;
}

/// Receives sealed frames. The app implements it over the relay client.
pub trait Sink {
    fn snapshot(&mut self, frame: &RoomFrame) -> Result<RoomStatus, ClientError>;
    fn frame(&mut self, frame: &RoomFrame) -> Result<RoomStatus, ClientError>;
    /// Asks the relay for the room's status (a call that carries no frame). It tells the publisher the relay still
    /// answers when the room is quiet.
    fn ping(&mut self) -> Result<RoomStatus, ClientError>;
    /// Called once, when the room stops.
    fn end(&mut self);
}

/// Why a room stopped. `as_str` is the word the app shows in its status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Owner,
    Expired,
    Idle,
    AllPanesClosed,
    RelayLost,
    RelayEnded,
    ClockJump,
    AppQuit,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::Owner => "owner",
            StopReason::Expired => "expired",
            StopReason::Idle => "idle",
            StopReason::AllPanesClosed => "all_panes_closed",
            StopReason::RelayLost => "relay_lost",
            StopReason::RelayEnded => "relay_ended",
            StopReason::ClockJump => "clock_jump",
            StopReason::AppQuit => "app_quit",
        }
    }
}

/// The answer of one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    Continue,
    Stopped(StopReason),
}

/// The room as the app shows it (`live://state` and the bar).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub live: bool,
    pub paused: bool,
    pub viewers: u32,
    pub ends_at_ms: u64,
    pub pane_ids: Vec<String>,
    pub hidden: u32,
    pub stopped: Option<String>,
}

/// The plaintext of one frame (camelCase JSON, SPEC-n32 3.6).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Body<'a> {
    v: u8,
    kind: FrameKind,
    room: RoomInfo<'a>,
    panes: &'a [LivePane],
    items: &'a [LiveItem],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RoomInfo<'a> {
    title: &'a str,
    ends_at: u64,
    paused: bool,
    hidden: u32,
}

/// One planned pane: its viewer line, its chat revision, its last published rows and what the last tick saw.
struct Slot {
    pane_id: String,
    key: String,
    /// The chat revision read so far (`None`: not read yet, or no chat).
    rev: Option<u64>,
    /// The chat comes off the screen.
    screen: bool,
    /// The last rows published, oldest first (at most `RING_ROWS`).
    ring: VecDeque<LiveItem>,
    /// The pane as the last tick saw it.
    now: LivePane,
    /// The pane as viewers were last sent it (`None`: never sent).
    shown: Option<LivePane>,
    /// Screen-read rows are held back until this time (after a card).
    hold_until_ms: u64,
    /// Viewers were told once that the chat is not shared (cannot be tied to the pane / its session changed).
    told_unpinned: bool,
    told_changed: bool,
    /// The last turn row is an end: a second end with nothing between is the same turn's late length line.
    turn_ended: bool,
}

impl Slot {
    fn new(pane_id: &str, index: usize) -> Self {
        let key = pane_key(index);
        let now = LivePane { key: key.clone(), label: String::new(), title: String::new(), state: LiveState::Gone, line: None };
        Self {
            pane_id: pane_id.to_string(),
            key,
            rev: None,
            screen: false,
            ring: VecDeque::new(),
            now,
            shown: None,
            hold_until_ms: 0,
            told_unpinned: false,
            told_changed: false,
            turn_ended: false,
        }
    }
}

/// What `gather` reads for one planned pane.
struct Gathered {
    pane: LivePane,
    rev: Option<u64>,
    rows: VecDeque<LiveItem>,
    screen: bool,
    told_unpinned: bool,
    told_changed: bool,
    turn_ended: bool,
}

pub struct Publisher<S: Source, K: Sink> {
    plan: RoomPlan,
    sealer: Sealer,
    source: S,
    sink: K,
    ctx: RedactContext,
    slots: Vec<Slot>,
    /// Rows read and not sent yet, oldest first.
    outbox: VecDeque<LiveItem>,
    started: bool,
    stopped: Option<StopReason>,
    sink_ended: bool,
    paused: bool,
    /// The paused state frame was sent for this pause.
    pause_sent: bool,
    /// A snapshot is owed (first start, a failed snapshot, or a resume).
    need_snapshot: bool,
    /// The source started a chat over: a snapshot is owed this tick.
    reset_pending: bool,
    seq: u64,
    next_id: u64,
    /// Rows read while paused or held back, never shared. Counted from the start of the room.
    hidden: u32,
    viewers: u32,
    last_tick_ms: u64,
    last_activity_ms: u64,
    last_snapshot_ms: u64,
    last_state_ms: u64,
    /// The last time a call to the relay was tried (it worked or not).
    last_call_ms: u64,
    failed_calls: u32,
    first_failure_ms: u64,
}

impl<S: Source, K: Sink> Publisher<S, K> {
    pub fn new(mut plan: RoomPlan, sealer: Sealer, source: S, sink: K, ctx: RedactContext, now_ms: u64) -> Self {
        let title = clean(&plan.title, TITLE_CHARS, &ctx);
        plan.title = if title.trim().is_empty() { DEFAULT_TITLE.to_string() } else { title };
        let slots = plan.pane_ids.iter().enumerate().map(|(index, id)| Slot::new(id, index)).collect();
        Self {
            plan,
            sealer,
            source,
            sink,
            ctx,
            slots,
            outbox: VecDeque::new(),
            started: false,
            stopped: None,
            sink_ended: false,
            paused: false,
            pause_sent: false,
            need_snapshot: false,
            reset_pending: false,
            seq: 0,
            next_id: 1,
            hidden: 0,
            viewers: 0,
            last_tick_ms: now_ms,
            last_activity_ms: now_ms,
            last_snapshot_ms: now_ms,
            last_state_ms: now_ms.saturating_sub(STATE_EVERY_MS),
            last_call_ms: now_ms,
            failed_calls: 0,
            first_failure_ms: 0,
        }
    }

    /// Reads the panes and their chat, then sends the first snapshot. An error is returned, not counted: the
    /// service does not start a room the relay refused.
    pub fn start(&mut self, now_ms: u64) -> Result<(), ClientError> {
        if self.started {
            return Ok(());
        }
        let gathered = gather(&mut self.source, &self.plan.pane_ids, &self.ctx, &mut self.next_id);
        for (slot, got) in self.slots.iter_mut().zip(gathered) {
            slot.rev = got.rev;
            slot.screen = got.screen;
            slot.ring = got.rows;
            slot.now = got.pane;
            slot.told_unpinned = got.told_unpinned;
            slot.told_changed = got.told_changed;
            slot.turn_ended = got.turn_ended;
        }
        let (panes, rows) = self.snapshot_body();
        self.send_frame(FrameKind::Snap, &panes, &rows)?;
        self.started = true;
        self.after_snapshot(now_ms);
        Ok(())
    }

    /// One pass: read the panes and the chat, send what is due, check the stop rules.
    pub fn tick(&mut self, now_ms: u64) -> Tick {
        if let Some(why) = self.stopped {
            return Tick::Stopped(why);
        }
        if !self.started {
            return Tick::Continue;
        }
        if now_ms.saturating_sub(self.last_tick_ms) > ms(CLOCK_JUMP) {
            self.stop(StopReason::ClockJump);
            return self.verdict();
        }
        self.last_tick_ms = now_ms;
        if now_ms >= self.plan.ends_at_ms {
            self.stop(StopReason::Expired);
            return self.verdict();
        }
        self.read_panes(now_ms);
        self.read_items(now_ms);
        if self.slots.iter().all(|slot| slot.now.state == LiveState::Gone) {
            self.stop(StopReason::AllPanesClosed);
        }
        if now_ms.saturating_sub(self.last_activity_ms) >= ms(IDLE_STOP) {
            self.stop(StopReason::Idle);
        }
        if self.stopped.is_none() {
            self.send(now_ms);
        }
        if self.stopped.is_none() {
            self.heartbeat(now_ms);
        }
        self.verdict()
    }

    /// Pause: rows read from now on are thrown away and counted. The paused note is sent on the next tick.
    pub fn pause(&mut self, _now_ms: u64) {
        if !self.started || self.stopped.is_some() || self.paused {
            return;
        }
        self.paused = true;
        self.pause_sent = false;
    }

    /// Resume: a snapshot is sent next, so viewers get the current panes and the count of hidden rows.
    pub fn resume(&mut self, _now_ms: u64) {
        if !self.started || self.stopped.is_some() || !self.paused {
            return;
        }
        self.paused = false;
        self.pause_sent = false;
        self.need_snapshot = true;
    }

    /// Stops the room once. The sink's `end` runs once, whatever the reason.
    pub fn stop(&mut self, why: StopReason) {
        if self.stopped.is_some() {
            return;
        }
        self.stopped = Some(why);
        if !self.sink_ended {
            self.sink_ended = true;
            self.sink.end();
        }
    }

    pub fn status(&self) -> LiveStatus {
        LiveStatus {
            live: self.started && self.stopped.is_none(),
            paused: self.paused,
            viewers: self.viewers,
            ends_at_ms: self.plan.ends_at_ms,
            pane_ids: self.plan.pane_ids.clone(),
            hidden: self.hidden,
            stopped: self.stopped.map(|why| why.as_str().to_string()),
        }
    }

    /// What viewers would see first: the same gathering and cleaning as the first snapshot (`start`).
    /// Ids count from 1, so the rows match the first snapshot of a new room.
    pub fn preview(source: &mut S, pane_ids: &[String], ctx: &RedactContext) -> (Vec<LivePane>, Vec<LiveItem>) {
        let mut next_id = 1;
        let gathered = gather(source, pane_ids, ctx, &mut next_id);
        let panes = gathered.iter().map(|got| got.pane.clone()).collect();
        let mut items: Vec<LiveItem> = gathered.into_iter().flat_map(|got| got.rows).collect();
        items.sort_by_key(item_id);
        (panes, items)
    }

    fn verdict(&self) -> Tick {
        match self.stopped {
            Some(why) => Tick::Stopped(why),
            None => Tick::Continue,
        }
    }

    /// Asks the relay for the room's status when no call was tried for `HEARTBEAT_MS`, so a quiet room still notices
    /// a relay that is gone. A failure counts like a failed frame; an answer carries the viewer count.
    fn heartbeat(&mut self, now_ms: u64) {
        if now_ms.saturating_sub(self.last_call_ms) < HEARTBEAT_MS {
            return;
        }
        match self.sink.ping() {
            Ok(status) => {
                self.viewers = status.viewers;
                self.settle(Ok(()), now_ms);
            }
            Err(error) => {
                self.settle(Err(error), now_ms);
            }
        }
    }

    /// Reads every planned pane: its line now, and activity when its state changed (not while paused).
    fn read_panes(&mut self, now_ms: u64) {
        let panes = self.source.panes();
        let mut shares = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let found = panes.iter().find(|pane| pane.pane_id == slot.pane_id);
            let mut next = match found {
                Some(pane) => {
                    slot.screen = pane.screen_source;
                    pane_view(
                        &slot.key,
                        &pane.agent_label,
                        pane.title.as_deref(),
                        pane.status,
                        pane.activity.as_ref(),
                        pane.pending_card,
                        &self.ctx,
                    )
                }
                None => LivePane {
                    key: slot.key.clone(),
                    label: slot.now.label.clone(),
                    title: slot.now.title.clone(),
                    state: LiveState::Gone,
                    line: None,
                },
            };
            if next.state != slot.now.state && !self.paused {
                self.last_activity_ms = now_ms;
            }
            // A pane that closed keeps the name it had: an empty one never replaces a known one.
            if next.label.is_empty() {
                next.label = slot.now.label.clone();
            }
            if next.title.is_empty() && next.state == LiveState::Gone {
                next.title = slot.now.title.clone();
            }
            slot.now = next;
            if let Some(pane) = found {
                shares.push((index, pane.chat));
            }
        }
        for (index, share) in shares {
            self.tell_chat_share(index, share);
        }
    }

    /// A pane whose chat is not shared gets one note for viewers (per reason), as a row of its own.
    fn tell_chat_share(&mut self, index: usize, share: ChatShare) {
        let slot = &mut self.slots[index];
        let told = match share {
            ChatShare::Shared => return,
            ChatShare::NotPinned => &mut slot.told_unpinned,
            ChatShare::SessionChanged => &mut slot.told_changed,
        };
        if std::mem::replace(told, true) {
            return;
        }
        let Some(text) = note_for(share) else { return };
        let row = LiveItem::Note { id: self.next_id, pane: slot.key.clone(), text: text.to_string() };
        self.next_id += 1;
        if self.paused {
            self.hidden += 1;
            return;
        }
        push_ring(&mut slot.ring, row.clone());
        self.outbox.push_back(row);
    }

    /// Reads the new chat of every pane that is not gone. Rows are queued, held back or thrown away.
    fn read_items(&mut self, now_ms: u64) {
        for index in 0..self.slots.len() {
            if self.slots[index].now.state == LiveState::Gone {
                continue;
            }
            let waiting = self.slots[index].now.state == LiveState::Waiting;
            if waiting {
                self.slots[index].hold_until_ms = now_ms + ms(SCREEN_HOLD_BACK);
            }
            let held = self.slots[index].screen && (waiting || now_ms < self.slots[index].hold_until_ms);
            let pane_id = self.slots[index].pane_id.clone();
            let key = self.slots[index].key.clone();
            let rev = self.slots[index].rev;
            let Some((next_rev, reset, items)) = self.source.items_since(&pane_id, rev) else {
                continue;
            };
            self.slots[index].rev = Some(next_rev);
            if reset {
                self.slots[index].turn_ended = false;
                if !self.paused {
                    self.slots[index].ring.clear();
                    self.reset_pending = true;
                }
            }
            let drop_all = self.paused || held;
            for item in &items {
                if !turn_gate(&mut self.slots[index].turn_ended, item) {
                    continue;
                }
                let Some(row) = to_live(item, &key, &mut self.next_id, &self.ctx) else {
                    continue;
                };
                if drop_all {
                    self.hidden += 1;
                    continue;
                }
                push_ring(&mut self.slots[index].ring, row.clone());
                self.outbox.push_back(row);
                self.last_activity_ms = now_ms;
            }
        }
    }

    /// Sends what is due this tick: a snapshot, or up to two delta frames and one state frame.
    fn send(&mut self, now_ms: u64) {
        if self.paused {
            self.send_paused(now_ms);
            return;
        }
        let snapshot_due = self.need_snapshot
            || self.reset_pending
            || now_ms.saturating_sub(self.last_snapshot_ms) >= ms(SNAPSHOT_EVERY);
        if snapshot_due {
            self.push_snapshot(now_ms);
            return;
        }
        let mut frames = 0;
        while frames < DELTA_FRAMES_PER_TICK && !self.outbox.is_empty() && self.stopped.is_none() {
            if !self.push_delta(now_ms) {
                return;
            }
            frames += 1;
        }
        if self.outbox.is_empty()
            && self.stopped.is_none()
            && !self.changed().is_empty()
            && now_ms.saturating_sub(self.last_state_ms) >= STATE_EVERY_MS
        {
            self.push_state(now_ms);
        }
    }

    /// While paused: send any rows read before the pause, then the paused note once. Nothing else goes out.
    fn send_paused(&mut self, now_ms: u64) {
        if self.pause_sent {
            return;
        }
        if self.need_snapshot && !self.push_snapshot(now_ms) {
            return;
        }
        let mut frames = 0;
        while !self.outbox.is_empty() {
            if frames == DELTA_FRAMES_PER_TICK || !self.push_delta(now_ms) {
                return;
            }
            frames += 1;
        }
        let result = self.send_frame(FrameKind::State, &[], &[]);
        if self.settle(result, now_ms) {
            self.pause_sent = true;
            self.last_state_ms = now_ms;
        }
    }

    /// Sends a snapshot of every pane. Returns false when the relay did not take it (retried next tick).
    fn push_snapshot(&mut self, now_ms: u64) -> bool {
        let (panes, rows) = self.snapshot_body();
        let result = self.send_frame(FrameKind::Snap, &panes, &rows);
        if !self.settle(result, now_ms) {
            self.need_snapshot = true;
            return false;
        }
        self.after_snapshot(now_ms);
        true
    }

    /// Sends the first delta batch: up to `DELTA_ROWS` rows in `DELTA_BYTES`, with the panes that changed.
    /// Returns false when the relay did not take it.
    fn push_delta(&mut self, now_ms: u64) -> bool {
        let changed = self.changed();
        let panes = self.panes_of(&changed);
        let mut take = self.outbox.len().min(DELTA_ROWS);
        let rows: Vec<LiveItem> = loop {
            let rows: Vec<LiveItem> = self.outbox.iter().take(take).cloned().collect();
            if self.encode(FrameKind::Delta, &panes, &rows).len() <= DELTA_BYTES {
                break rows;
            }
            if take <= 1 {
                // One row too big to share: it is dropped and counted.
                self.outbox.pop_front();
                self.hidden += 1;
                return true;
            }
            take -= 1;
        };
        let result = self.send_frame(FrameKind::Delta, &panes, &rows);
        if !self.settle(result, now_ms) {
            return false;
        }
        self.outbox.drain(..rows.len());
        self.mark_shown(&changed);
        true
    }

    /// Sends the panes that changed, with no rows.
    fn push_state(&mut self, now_ms: u64) -> bool {
        let changed = self.changed();
        let panes = self.panes_of(&changed);
        let result = self.send_frame(FrameKind::State, &panes, &[]);
        if !self.settle(result, now_ms) {
            return false;
        }
        self.mark_shown(&changed);
        self.last_state_ms = now_ms;
        true
    }

    /// Seals one frame and gives it to the sink. Snapshots go to `snapshot`, the rest to `frame`.
    fn send_frame(&mut self, kind: FrameKind, panes: &[LivePane], rows: &[LiveItem]) -> Result<(), ClientError> {
        let bytes = self.encode(kind, panes, rows);
        self.seq += 1;
        let frame = self
            .sealer
            .seal(&self.plan.room_id, self.seq, kind, &bytes)
            .map_err(|_| ClientError::Refused(LOCAL_FRAME_ERROR.to_string()))?;
        let status = match kind {
            FrameKind::Snap => self.sink.snapshot(&frame)?,
            FrameKind::Delta | FrameKind::State => self.sink.frame(&frame)?,
        };
        self.viewers = status.viewers;
        Ok(())
    }

    /// Books the outcome of one call: a success clears the failure run, an error may stop the room.
    fn settle(&mut self, result: Result<(), ClientError>, now_ms: u64) -> bool {
        self.last_call_ms = now_ms;
        match result {
            Ok(()) => {
                self.failed_calls = 0;
                true
            }
            Err(error) => {
                self.failed(error, now_ms);
                false
            }
        }
    }

    fn failed(&mut self, error: ClientError, now_ms: u64) {
        match error {
            ClientError::Gone(_) => self.stop(StopReason::RelayEnded),
            ClientError::Network(_) | ClientError::Refused(_) => {
                if self.failed_calls == 0 {
                    self.first_failure_ms = now_ms;
                }
                self.failed_calls += 1;
                if self.failed_calls >= RELAY_LOST_CALLS
                    && now_ms.saturating_sub(self.first_failure_ms) >= RELAY_LOST_MS
                {
                    self.stop(StopReason::RelayLost);
                }
            }
        }
    }

    /// Bookkeeping after a snapshot the relay took: the viewers now hold exactly the rings.
    fn after_snapshot(&mut self, now_ms: u64) {
        self.outbox.clear();
        let all: Vec<usize> = (0..self.slots.len()).collect();
        self.mark_shown(&all);
        self.need_snapshot = false;
        self.reset_pending = false;
        self.last_snapshot_ms = now_ms;
        self.last_call_ms = now_ms;
    }

    /// The panes and rows of a snapshot: every pane, and the rings' rows (oldest dropped until it fits).
    fn snapshot_body(&self) -> (Vec<LivePane>, Vec<LiveItem>) {
        let all: Vec<usize> = (0..self.slots.len()).collect();
        let panes = self.panes_of(&all);
        let mut rows: Vec<LiveItem> = self.slots.iter().flat_map(|slot| slot.ring.iter().cloned()).collect();
        rows.sort_by_key(item_id);
        let base = self.encode(FrameKind::Snap, &panes, &[]).len();
        let sizes: Vec<usize> = rows.iter().map(|row| serde_json::to_vec(row).map_or(0, |v| v.len() + 1)).collect();
        let mut total = base + sizes.iter().sum::<usize>();
        let mut drop = 0;
        while total > SNAPSHOT_BYTES && drop < rows.len() {
            total -= sizes[drop];
            drop += 1;
        }
        rows.drain(..drop);
        (panes, rows)
    }

    /// Indexes of the panes whose viewer line differs from what viewers were last sent.
    fn changed(&self) -> Vec<usize> {
        (0..self.slots.len()).filter(|&i| self.slots[i].shown.as_ref() != Some(&self.slots[i].now)).collect()
    }

    fn panes_of(&self, indexes: &[usize]) -> Vec<LivePane> {
        indexes.iter().map(|&i| self.slots[i].now.clone()).collect()
    }

    fn mark_shown(&mut self, indexes: &[usize]) {
        for &i in indexes {
            self.slots[i].shown = Some(self.slots[i].now.clone());
        }
    }

    /// The plaintext of a frame. Room state comes from the publisher, so every frame carries it.
    fn encode(&self, kind: FrameKind, panes: &[LivePane], rows: &[LiveItem]) -> Vec<u8> {
        let body = Body {
            v: 1,
            kind,
            room: RoomInfo {
                title: &self.plan.title,
                ends_at: self.plan.ends_at_ms,
                paused: self.paused,
                hidden: self.hidden,
            },
            panes,
            items: rows,
        };
        serde_json::to_vec(&body).unwrap_or_default()
    }
}

/// Reads the planned panes and their last rows. Shared by `start` and `preview`, so both see the same thing.
fn gather<S: Source>(source: &mut S, pane_ids: &[String], ctx: &RedactContext, next_id: &mut u64) -> Vec<Gathered> {
    let panes = source.panes();
    let mut out = Vec::with_capacity(pane_ids.len());
    for (index, pane_id) in pane_ids.iter().enumerate() {
        let key = pane_key(index);
        let found = panes.iter().find(|pane| pane.pane_id == *pane_id);
        let pane = match found {
            Some(pane) => pane_view(
                &key,
                &pane.agent_label,
                pane.title.as_deref(),
                pane.status,
                pane.activity.as_ref(),
                pane.pending_card,
                ctx,
            ),
            None => pane_view(&key, "", None, None, None, false, ctx),
        };
        let screen = found.is_some_and(|pane| pane.screen_source);
        let mut rows = VecDeque::new();
        let mut rev = None;
        let mut turn_ended = false;
        let (mut told_unpinned, mut told_changed) = (false, false);
        if let Some(share) = found.map(|pane| pane.chat).filter(|share| *share != ChatShare::Shared) {
            if let Some(text) = note_for(share) {
                push_ring(&mut rows, LiveItem::Note { id: *next_id, pane: key.clone(), text: text.to_string() });
                *next_id += 1;
            }
            told_unpinned = share == ChatShare::NotPinned;
            told_changed = share == ChatShare::SessionChanged;
        }
        if found.is_some_and(|pane| pane.status.is_some()) {
            if let Some((next, _, items)) = source.items_since(pane_id, None) {
                rev = Some(next);
                for item in &items {
                    if !turn_gate(&mut turn_ended, item) {
                        continue;
                    }
                    if let Some(row) = to_live(item, &key, next_id, ctx) {
                        push_ring(&mut rows, row);
                    }
                }
            }
        }
        out.push(Gathered { pane, rev, rows, screen, told_unpinned, told_changed, turn_ended });
    }
    out
}

/// Whether a chat item is shown. A turn end that follows another turn end with nothing between is the same turn's
/// late length line (an agent writes the end, then the real length): the first one is shown, the second is dropped.
fn turn_gate(ended: &mut bool, item: &ChatItem) -> bool {
    match item {
        ChatItem::TurnEnd { .. } => !std::mem::replace(ended, true),
        ChatItem::User { .. } | ChatItem::Assistant { .. } | ChatItem::ToolCall { .. } => {
            *ended = false;
            true
        }
        _ => true,
    }
}

fn push_ring(ring: &mut VecDeque<LiveItem>, row: LiveItem) {
    ring.push_back(row);
    if ring.len() > RING_ROWS {
        ring.pop_front();
    }
}

fn item_id(item: &LiveItem) -> u64 {
    match item {
        LiveItem::User { id, .. }
        | LiveItem::Agent { id, .. }
        | LiveItem::Tool { id, .. }
        | LiveItem::Result { id, .. }
        | LiveItem::End { id, .. }
        | LiveItem::Plan { id, .. }
        | LiveItem::Note { id, .. } => *id,
    }
}

/// The viewer key of the pane at `index`: `p1` for the first.
fn pane_key(index: usize) -> String {
    format!("p{}", index + 1)
}

fn ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

#[cfg(test)]
#[path = "publisher_tests.rs"]
mod publisher_tests;
