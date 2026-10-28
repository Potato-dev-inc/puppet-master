//! Live Room rooms on the axum relay (SPEC-n32 §3.7, §4 #5-#7, #9, #11).
//!
//! A room is an in-memory entry, never written to disk: a restart ends every room, and the owner's
//! app then sees `RelayEnded`. The relay keeps the room's registration (its key id, the hash of its
//! read token, expiry and viewer limit), the newest snapshot, up to 300 deltas or 2 MiB of them (per room),
//! and one seat per viewer page load. All rooms together hold at most `global_room_bytes`: at that cap the
//! busiest room drops its oldest deltas first. Frames are ciphertext; the read token is only ever compared
//! by its hash. Nothing here reads or logs what a viewer sees.
//!
//! Time comes from the caller (`Relay::room_now`), so tests can move the clock. The lock is never held
//! across an await. The HTTP routes live in `rooms/routes.rs`.

mod routes;

use crate::Limits;
use pm_relay_protocol::room::{
    is_room_id, FrameKind, RoomEnd, RoomFrame, RoomRegistration, RoomStatus, ROOM_TOMBSTONE_MS, SEAT_ACTIVE_MS,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub use routes::routes;

/// Deltas kept per room (oldest dropped first).
pub const RING_FRAMES: usize = 300;
/// Frames in one `GET …/frames` answer at most; the viewer asks again for the rest.
pub const BATCH_FRAMES: usize = 100;
/// Owner frames a room may send in a burst; refilled at `room_frames_per_second`.
pub const FRAME_BURST: f64 = 10.0;
/// Largest registration body.
pub const REGISTRATION_BYTES: usize = 8 * 1024;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Why a room call is refused. The routes turn each into a status code and a short body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomError {
    /// Unknown room, a read token that does not open it, or a tombstone past its keep time (404).
    Missing,
    /// The room has ended (410 with `{"ended": …}`).
    Ended(RoomEnd),
    /// The viewer limit is reached (429 with `{"full": true}`).
    Full,
    /// Too many calls, or too many frames (429).
    SlowDown,
    /// A registration for a used id, or a frame whose `seq` does not rise (409).
    Conflict(&'static str),
    /// Too many open rooms on this relay (503).
    Busy,
    /// The relay's room memory is full and nothing can be dropped to make room (507).
    Storage,
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    at_ms: u64,
}

struct Seat {
    last_ms: u64,
    addr: String,
}

pub(crate) struct Room {
    /// The owner's room key (base64url). Owner calls must be signed by it.
    key: String,
    /// SHA-256 of the read token, base64url. The token itself is never stored.
    read_hash: String,
    expires_at_ms: u64,
    max_viewers: u32,
    /// Set once the room stopped or ran out of time; the room then keeps only its tombstone.
    ended: Option<RoomEnd>,
    snapshot: Option<RoomFrame>,
    snapshot_bytes: usize,
    deltas: VecDeque<(RoomFrame, usize)>,
    delta_bytes: usize,
    /// The newest `seq` the owner sent (snapshots and deltas share one counter).
    last_seq: u64,
    bucket: Bucket,
    day_start_ms: u64,
    day_count: u32,
    seats: HashMap<String, Seat>,
    /// Woken when a frame arrives or the room ends (long polls).
    notify: Arc<Notify>,
}

impl Room {
    fn new(reg: &RoomRegistration, now: u64) -> Self {
        Self {
            key: reg.key.clone(),
            read_hash: reg.read_hash.clone(),
            expires_at_ms: reg.expires_at_ms,
            max_viewers: reg.max_viewers,
            ended: None,
            snapshot: None,
            snapshot_bytes: 0,
            deltas: VecDeque::new(),
            delta_bytes: 0,
            last_seq: 0,
            bucket: Bucket { tokens: FRAME_BURST, at_ms: now },
            day_start_ms: now,
            day_count: 0,
            seats: HashMap::new(),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Ends the room when its time is up.
    fn settle(&mut self, now: u64) {
        if self.ended.is_none() && now >= self.expires_at_ms {
            self.stop(RoomEnd::Expired);
        }
    }

    /// Ends the room now: frames, snapshot and seats are dropped at once; the tombstone stays.
    fn stop(&mut self, end: RoomEnd) {
        self.ended = Some(end);
        self.snapshot = None;
        self.snapshot_bytes = 0;
        self.deltas.clear();
        self.delta_bytes = 0;
        self.seats.clear();
        self.notify.notify_waiters();
    }

    /// Takes one owner frame from the room's budget: a token bucket (burst 10, refilled per second)
    /// and a daily count. False when either is spent.
    fn spend(&mut self, now: u64, limits: &Limits) -> bool {
        let elapsed = now.saturating_sub(self.bucket.at_ms) as f64 / 1000.0;
        self.bucket.tokens = (self.bucket.tokens + elapsed * f64::from(limits.room_frames_per_second)).min(FRAME_BURST);
        self.bucket.at_ms = now;
        if now.saturating_sub(self.day_start_ms) >= DAY_MS {
            self.day_start_ms = now;
            self.day_count = 0;
        }
        if self.bucket.tokens < 1.0 || self.day_count >= limits.room_frames_per_day {
            return false;
        }
        self.bucket.tokens -= 1.0;
        self.day_count += 1;
        true
    }

    fn snapshot_frame(&self) -> Option<RoomFrame> {
        self.snapshot.clone()
    }

    fn viewers(&self, now: u64) -> u32 {
        self.seats.values().filter(|s| now.saturating_sub(s.last_ms) < SEAT_ACTIVE_MS).count() as u32
    }

    /// A viewer call from `seat`: refreshes a seat that is still active, or takes a new one when the
    /// room and the client's address have room for it. A seat that went quiet frees its place.
    fn seat(&mut self, seat: &str, addr: &str, now: u64, limits: &Limits) -> Result<(), RoomError> {
        if let Some(s) = self.seats.get_mut(seat) {
            if now.saturating_sub(s.last_ms) < SEAT_ACTIVE_MS {
                s.last_ms = now;
                return Ok(());
            }
        }
        self.seats.retain(|_, s| now.saturating_sub(s.last_ms) < SEAT_ACTIVE_MS);
        if self.seats.len() as u32 >= self.max_viewers {
            return Err(RoomError::Full);
        }
        if self.seats.values().filter(|s| s.addr == addr).count() as u32 >= limits.seats_per_address {
            return Err(RoomError::Full);
        }
        self.seats.insert(seat.to_string(), Seat { last_ms: now, addr: addr.to_string() });
        Ok(())
    }

    /// Bytes this room holds: its snapshot and its deltas.
    fn bytes(&self) -> usize {
        self.snapshot_bytes + self.delta_bytes
    }

    /// Keeps a delta or state frame; the oldest go when the ring is over its frame or byte cap.
    fn keep_delta(&mut self, frame: RoomFrame, size: usize, ring_bytes: usize) {
        self.delta_bytes += size;
        self.deltas.push_back((frame, size));
        while self.deltas.len() > RING_FRAMES || self.delta_bytes > ring_bytes {
            self.drop_oldest_delta();
        }
    }

    /// Drops the oldest delta (a no-op when there is none).
    fn drop_oldest_delta(&mut self) {
        if let Some((_, gone)) = self.deltas.pop_front() {
            self.delta_bytes -= gone;
        }
    }

    /// What a viewer that has seen up to `after` still needs, oldest first: the snapshot if it is
    /// newer than `after` (a viewer behind the snapshot starts from it), then the deltas after `after`.
    fn frames_after(&self, after: u64) -> Vec<RoomFrame> {
        let mut out = Vec::new();
        if let Some(snap) = self.snapshot.as_ref().filter(|s| s.seq > after) {
            out.push(snap.clone());
        }
        let room_left = BATCH_FRAMES.saturating_sub(out.len());
        out.extend(self.deltas.iter().filter(|(f, _)| f.seq > after).take(room_left).map(|(f, _)| f.clone()));
        out
    }

    fn status(&self, now: u64) -> RoomStatus {
        RoomStatus { viewers: self.viewers(now), expires_at_ms: self.expires_at_ms }
    }
}

/// Constant-time equality of two base64url hashes (the token check must not leak a prefix).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A room that exists and is not past its keep time, after its expiry is applied.
fn find<'a>(rooms: &'a mut HashMap<String, Room>, id: &str, now: u64) -> Result<&'a mut Room, RoomError> {
    if !is_room_id(id) {
        return Err(RoomError::Missing);
    }
    let room = rooms.get_mut(id).ok_or(RoomError::Missing)?;
    if now >= room.expires_at_ms.saturating_add(ROOM_TOMBSTONE_MS) {
        return Err(RoomError::Missing);
    }
    room.settle(now);
    Ok(room)
}

fn open_rooms(rooms: &mut HashMap<String, Room>, now: u64) -> usize {
    rooms
        .values_mut()
        .map(|r| {
            r.settle(now);
            r.ended.is_none()
        })
        .filter(|open| *open)
        .count()
}

/// Snapshot bytes of every room except `skip` (snapshots are never dropped to make room, so they must fit).
fn snapshot_bytes(rooms: &HashMap<String, Room>, skip: &str) -> usize {
    rooms.iter().filter(|(id, _)| id.as_str() != skip).map(|(_, r)| r.snapshot_bytes).sum()
}

/// Drops deltas until the relay holds at most `cap` bytes. Each one comes from the busiest room that still
/// has deltas (most delta bytes), oldest first. Callers check first that the snapshots fit, so this always
/// ends at or under the cap.
fn enforce(rooms: &mut HashMap<String, Room>, cap: usize) {
    loop {
        let total: usize = rooms.values().map(Room::bytes).sum();
        if total <= cap {
            return;
        }
        let busiest =
            rooms.iter().filter(|(_, r)| !r.deltas.is_empty()).max_by_key(|(_, r)| r.delta_bytes).map(|(id, _)| id.clone());
        match busiest.and_then(|id| rooms.get_mut(&id)) {
            Some(room) => room.drop_oldest_delta(),
            None => return,
        }
    }
}

/// Checks a frame against its room before any budget is spent: the room exists, is live, and the
/// `seq` rises.
fn check_frame(rooms: &mut HashMap<String, Room>, id: &str, seq: u64, now: u64) -> Result<(), RoomError> {
    let room = find(rooms, id, now)?;
    if let Some(end) = room.ended {
        return Err(RoomError::Ended(end));
    }
    if seq <= room.last_seq {
        return Err(RoomError::Conflict("frame sequence number does not rise"));
    }
    Ok(())
}

/// The relay's rooms. Lives inside `Relay`; every method takes the time and the limits from the caller.
#[derive(Default)]
pub struct Rooms {
    rooms: Mutex<HashMap<String, Room>>,
}

impl Rooms {
    /// Registers a room the owner signed. A used id is refused, tombstones included, so a stopped room's
    /// id, token and key are never reused.
    pub fn register(&self, reg: &RoomRegistration, now: u64, limits: &Limits) -> Result<(), RoomError> {
        let mut rooms = self.rooms.lock().unwrap();
        if rooms.contains_key(&reg.room) {
            return Err(RoomError::Conflict("room already exists"));
        }
        if open_rooms(&mut rooms, now) >= limits.max_open_rooms as usize {
            return Err(RoomError::Busy);
        }
        rooms.insert(reg.room.clone(), Room::new(reg, now));
        Ok(())
    }

    /// The room's key, for the owner check before a signed call is remembered.
    pub fn owner_key(&self, id: &str, now: u64) -> Result<String, RoomError> {
        let mut rooms = self.rooms.lock().unwrap();
        Ok(find(&mut rooms, id, now)?.key.clone())
    }

    /// Replaces the snapshot of `size` bytes and drops every delta (all of them are older: `seq` rises).
    /// The other rooms' deltas go first if the relay is at its byte cap.
    pub fn put_snapshot(
        &self,
        id: &str,
        frame: RoomFrame,
        size: usize,
        now: u64,
        limits: &Limits,
    ) -> Result<RoomStatus, RoomError> {
        let mut rooms = self.rooms.lock().unwrap();
        check_frame(&mut rooms, id, frame.seq, now)?;
        let cap = limits.global_room_bytes as usize;
        // The room's own old snapshot and deltas are replaced, so only the other rooms' snapshots count.
        if snapshot_bytes(&rooms, id).saturating_add(size) > cap {
            return Err(RoomError::Storage);
        }
        let room = find(&mut rooms, id, now)?;
        if !room.spend(now, limits) {
            return Err(RoomError::SlowDown);
        }
        room.last_seq = frame.seq;
        room.snapshot = Some(frame);
        room.snapshot_bytes = size;
        room.deltas.clear();
        room.delta_bytes = 0;
        room.notify.notify_waiters();
        let status = room.status(now);
        enforce(&mut rooms, cap);
        Ok(status)
    }

    /// Keeps one delta or state frame of `size` bytes. At the relay's byte cap, the busiest room's oldest
    /// deltas go first; refused with `Storage` only when the snapshots alone leave no room.
    pub fn push(&self, id: &str, frame: RoomFrame, size: usize, now: u64, limits: &Limits) -> Result<RoomStatus, RoomError> {
        let mut rooms = self.rooms.lock().unwrap();
        check_frame(&mut rooms, id, frame.seq, now)?;
        let cap = limits.global_room_bytes as usize;
        if snapshot_bytes(&rooms, "").saturating_add(size) > cap {
            return Err(RoomError::Storage);
        }
        let room = find(&mut rooms, id, now)?;
        if !room.spend(now, limits) {
            return Err(RoomError::SlowDown);
        }
        room.last_seq = frame.seq;
        room.keep_delta(frame, size, limits.room_ring_bytes as usize);
        room.notify.notify_waiters();
        let status = room.status(now);
        enforce(&mut rooms, cap);
        Ok(status)
    }

    /// The owner stopped the room: wiped at once, tombstone kept.
    pub fn end(&self, id: &str, now: u64) -> Result<(), RoomError> {
        let mut rooms = self.rooms.lock().unwrap();
        let room = find(&mut rooms, id, now)?;
        if let Some(end) = room.ended {
            return Err(RoomError::Ended(end));
        }
        room.stop(RoomEnd::Stopped);
        Ok(())
    }

    pub fn status(&self, id: &str, now: u64) -> Result<RoomStatus, RoomError> {
        let mut rooms = self.rooms.lock().unwrap();
        let room = find(&mut rooms, id, now)?;
        if let Some(end) = room.ended {
            return Err(RoomError::Ended(end));
        }
        Ok(room.status(now))
    }

    /// A viewer's call: the room must exist, `token_hash` must open it, the room must be live, and the
    /// seat must have room. Then `read` looks at the room.
    #[allow(clippy::too_many_arguments)]
    pub fn view<T>(
        &self,
        id: &str,
        token_hash: &str,
        seat: &str,
        addr: &str,
        now: u64,
        limits: &Limits,
        read: impl FnOnce(&Room) -> T,
    ) -> Result<T, RoomError> {
        let mut rooms = self.rooms.lock().unwrap();
        let room = find(&mut rooms, id, now)?;
        if !same(&room.read_hash, token_hash) {
            return Err(RoomError::Missing);
        }
        if let Some(end) = room.ended {
            return Err(RoomError::Ended(end));
        }
        room.seat(seat, addr, now, limits)?;
        Ok(read(room))
    }

    /// Wakes when this room gets a frame or ends. Waiters should `enable()` before they check.
    pub fn notifier(&self, id: &str) -> Option<Arc<Notify>> {
        self.rooms.lock().unwrap().get(id).map(|r| r.notify.clone())
    }

    /// Drops tombstones past their keep time and settles expiries. Returns how many rooms went.
    pub fn sweep(&self, now: u64) -> usize {
        let mut rooms = self.rooms.lock().unwrap();
        let before = rooms.len();
        rooms.retain(|_, r| now < r.expires_at_ms.saturating_add(ROOM_TOMBSTONE_MS));
        for room in rooms.values_mut() {
            room.settle(now);
            room.seats.retain(|_, s| now.saturating_sub(s.last_ms) < SEAT_ACTIVE_MS);
        }
        before - rooms.len()
    }

    /// Test hook: ids and sizes only (never a key, a token hash or ciphertext).
    pub fn dump(&self, now: u64) -> String {
        let mut rooms = self.rooms.lock().unwrap();
        let mut ids: Vec<String> = rooms.keys().cloned().collect();
        ids.sort();
        let mut out = String::new();
        for id in ids {
            let room = rooms.get_mut(&id).expect("listed room");
            room.settle(now);
            let ended = room.ended.map(|e| format!("{e:?}")).unwrap_or_else(|| "live".into());
            out.push_str(&format!(
                "room|{id}|{ended}|seq={}|deltas={}|bytes={}|snapshot={}|viewers={}\n",
                room.last_seq,
                room.deltas.len(),
                room.delta_bytes,
                room.snapshot_bytes,
                room.viewers(now),
            ));
        }
        out
    }
}

/// The frame kinds an owner may post on the frames route.
pub fn is_delta_kind(kind: FrameKind) -> bool {
    matches!(kind, FrameKind::Delta | FrameKind::State)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_compares_every_byte() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }

    #[test]
    fn a_full_bucket_spends_its_burst_then_refills_per_second() {
        let limits = Limits::default();
        let reg = RoomRegistration {
            v: 1,
            room: String::new(),
            key: String::new(),
            expires_at_ms: u64::MAX,
            max_viewers: 1,
            read_hash: String::new(),
            sig: String::new(),
        };
        let mut room = Room::new(&reg, 0);
        for _ in 0..10 {
            assert!(room.spend(0, &limits));
        }
        assert!(!room.spend(0, &limits), "the burst is spent");
        assert!(!room.spend(499, &limits), "half a token is not enough");
        assert!(room.spend(500, &limits), "2 per second: one token after 500 ms");
    }
}
