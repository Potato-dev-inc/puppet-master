// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT (hop-net session.rs, heartbeat.rs).
//! The rules a pair of computers follow when they link, as plain state with no I/O. `desk/link` applies them.
//!
//! What DevHop got wrong, and what each rule below does about it (every item has a test in this file):
//! 1. Two links to one peer, and each side kept a different one: both sides then saw their link close and redialed
//!    forever. Here the survivor is a function of the two node ids and who dialed, so both sides always agree
//!    (`decide_install`). The link the smaller node id dialed wins; between two links of the same kind the first one
//!    installed wins.
//! 2. The same peer was dialed twice (once by id, once by address). `DialLedger` is keyed by peer id: one dial in
//!    flight per peer, with a growing wait after each failure.
//! 3. The close of an old link tore down the session that had replaced it. Every link has a number; the close of a
//!    number that is no longer current changes nothing (`Sessions::link_ended`).
//! 4. Silence. A heartbeat is 100 ms; 400 ms without a sign of life releases the keys (within 425 ms with the watchdog's
//!    25 ms look), 3 s closes the link. DevHop's 3 missed heartbeats were 300-400 ms; 250 ms sent the pointer home on
//!    ordinary Wi-Fi hiccups.

use std::collections::HashMap;

use super::endpoint::NodeId;

/// Milliseconds between heartbeats.
pub const HEARTBEAT_MS: u64 = 100;
/// This much silence and the keys are released. Long enough to ride out a Wi-Fi hiccup (the old app used 3 missed
/// 100 ms beats, 300-400 ms), short enough that a pulled cable still frees the keys well within half a second.
pub const STALL_MS: u64 = 400;
/// A session this old is settled: a link that shows up later is a stray duplicate, whatever its direction. The
/// tie-break is only for the race of the first moments, when both computers dial at once.
pub const SETTLE_MS: u64 = 2_000;
/// This much silence and the link is closed: the other computer is gone (cable pulled, asleep, crashed).
pub const DEAD_MS: u64 = 3_000;

/// Whether the link that `dialed` describes is the one both sides prefer: the one the smaller node id opened.
pub fn is_preferred(dialed: bool, local: &NodeId, remote: &NodeId) -> bool {
    dialed == (local < remote)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallAction {
    /// Nothing to compare with: this link becomes the session.
    Accept,
    /// Close the new link, keep what we have.
    RejectNew,
    /// Close the existing link, install the new one.
    ReplaceExisting,
}

/// An installed link, as far as the rule needs to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Installed {
    pub id: u64,
    pub dialed: bool,
    pub open: bool,
    /// When it became the session (service clock, ms).
    pub since_ms: u64,
}

/// Which of two links to one peer survives. Both sides call this with their own view of the same two
/// connections and get the same answer, whatever the order in which the handshakes finished.
pub fn decide_install(existing: Option<Installed>, new_dialed: bool, local: &NodeId, remote: &NodeId, now_ms: u64) -> InstallAction {
    let Some(old) = existing.filter(|e| e.open) else {
        return InstallAction::Accept;
    };
    // A settled session is never replaced: a late duplicate must not interrupt a pointer that is in use. (Both sides
    // agree: both have had the same session for the same two seconds.)
    if now_ms.saturating_sub(old.since_ms) >= SETTLE_MS {
        return InstallAction::RejectNew;
    }
    let new_pref = is_preferred(new_dialed, local, remote);
    let old_pref = is_preferred(old.dialed, local, remote);
    if new_pref && !old_pref {
        InstallAction::ReplaceExisting
    } else {
        // Same kind, or the old one is the preferred one: first installed wins.
        InstallAction::RejectNew
    }
}

/// "Dial each peer at most once": what is in flight, when a failed dial may be tried again.
#[derive(Debug, Default)]
pub struct DialLedger {
    in_flight: HashMap<NodeId, u64>,
    retry_after: HashMap<NodeId, u64>,
    fails: HashMap<NodeId, u32>,
}

/// First wait after a failed dial, and the longest wait.
pub const RETRY_FIRST_MS: u64 = 1_000;
pub const RETRY_MAX_MS: u64 = 10_000;

impl DialLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when the caller may dial now (and must call `succeeded` or `failed` afterwards).
    pub fn begin(&mut self, peer: &NodeId, now_ms: u64) -> bool {
        if self.in_flight.contains_key(peer) || self.waiting(peer, now_ms) {
            return false;
        }
        self.in_flight.insert(*peer, now_ms);
        true
    }

    pub fn in_flight(&self, peer: &NodeId) -> bool {
        self.in_flight.contains_key(peer)
    }

    pub fn waiting(&self, peer: &NodeId, now_ms: u64) -> bool {
        self.retry_after.get(peer).is_some_and(|t| now_ms < *t)
    }

    pub fn succeeded(&mut self, peer: &NodeId) {
        self.in_flight.remove(peer);
        self.retry_after.remove(peer);
        self.fails.remove(peer);
    }

    /// The wait before the next try doubles from 1 s to 10 s.
    pub fn failed(&mut self, peer: &NodeId, now_ms: u64) {
        self.in_flight.remove(peer);
        let n = self.fails.entry(*peer).or_insert(0);
        *n += 1;
        let wait = (RETRY_FIRST_MS << (*n - 1).min(4)).min(RETRY_MAX_MS);
        self.retry_after.insert(*peer, now_ms + wait);
    }

    /// The person turned the switch off or removed the computer: forget everything about it.
    pub fn forget(&mut self, peer: &NodeId) {
        self.in_flight.remove(peer);
        self.retry_after.remove(peer);
        self.fails.remove(peer);
    }
}

/// What the page shows for one computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    Offline,
    Connecting,
    Online,
}

/// One peer's link bookkeeping.
#[derive(Debug, Default)]
pub struct Sessions {
    next_id: u64,
    current: HashMap<NodeId, Installed>,
    handshaking: HashMap<NodeId, u32>,
}

impl Sessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// A number for a new link (dialed or accepted), before anything is known about it.
    pub fn new_link(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn handshake_started(&mut self, peer: &NodeId) {
        *self.handshaking.entry(*peer).or_insert(0) += 1;
    }

    pub fn handshake_ended(&mut self, peer: &NodeId) {
        if let Some(n) = self.handshaking.get_mut(peer) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.handshaking.remove(peer);
            }
        }
    }

    pub fn current(&self, peer: &NodeId) -> Option<Installed> {
        self.current.get(peer).copied()
    }

    /// A handshake finished. Returns what to do, and on `Accept`/`ReplaceExisting` the link is now the current one.
    pub fn install(&mut self, peer: &NodeId, id: u64, dialed: bool, local: &NodeId, now_ms: u64) -> (InstallAction, Option<u64>) {
        let existing = self.current.get(peer).copied();
        let action = decide_install(existing, dialed, local, peer, now_ms);
        let replaced = match action {
            InstallAction::Accept | InstallAction::ReplaceExisting => {
                self.current.insert(*peer, Installed { id, dialed, open: true, since_ms: now_ms });
                existing.map(|e| e.id)
            }
            InstallAction::RejectNew => None,
        };
        (action, replaced)
    }

    /// A link ended. Returns true only when it was the current link of the peer: the session is over. The end of a
    /// link that was replaced or rejected changes nothing.
    pub fn link_ended(&mut self, peer: &NodeId, id: u64) -> bool {
        if self.current.get(peer).is_some_and(|c| c.id == id) {
            self.current.remove(peer);
            true
        } else {
            false
        }
    }

    pub fn is_current(&self, peer: &NodeId, id: u64) -> bool {
        self.current.get(peer).is_some_and(|c| c.id == id)
    }

    pub fn reach(&self, peer: &NodeId) -> Reach {
        if self.current.contains_key(peer) {
            Reach::Online
        } else if self.handshaking.contains_key(peer) {
            Reach::Connecting
        } else {
            Reach::Offline
        }
    }
}

/// Watches the other side's beats on a clock the caller supplies (so tests run on simulated time).
#[derive(Clone, Debug)]
pub struct Silence {
    last_ms: u64,
    stalled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SilenceVerdict {
    Fine,
    /// First time past `STALL_MS` since the last beat: release held keys now.
    Stalled,
    /// Past `DEAD_MS`: close the link.
    Dead,
}

impl Silence {
    pub fn new(now_ms: u64) -> Self {
        Self { last_ms: now_ms, stalled: false }
    }

    /// Anything received counts: a beat, a key, a motion.
    pub fn heard(&mut self, now_ms: u64) {
        self.last_ms = now_ms;
        self.stalled = false;
    }

    pub fn check(&mut self, now_ms: u64) -> SilenceVerdict {
        let quiet = now_ms.saturating_sub(self.last_ms);
        if quiet >= DEAD_MS {
            SilenceVerdict::Dead
        } else if quiet >= STALL_MS && !self.stalled {
            self.stalled = true;
            SilenceVerdict::Stalled
        } else {
            SilenceVerdict::Fine
        }
    }

    pub fn is_stalled(&self) -> bool {
        self.stalled
    }

    /// Milliseconds since anything was heard (for the log line that says how long the silence lasted).
    pub fn quiet_for(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.last_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(b: u8) -> NodeId {
        NodeId([b; 32])
    }
    const A: u8 = 1;
    const B: u8 = 2;

    fn inst(id: u64, dialed: bool) -> Option<Installed> {
        Some(Installed { id, dialed, open: true, since_ms: 0 })
    }

    // Known bug 1: each side kept a different one of two links. Both sides must agree for every arrival order.
    #[test]
    fn both_sides_keep_the_same_connection_for_every_arrival_order() {
        let (a, b) = (id(A), id(B));
        // Connection X: dialed by A (A: dialed=true, B: dialed=false). Connection Y: dialed by B.
        for first_is_x_at_a in [true, false] {
            for first_is_x_at_b in [true, false] {
                let survivor = |local: &NodeId, remote: &NodeId, x_first: bool| {
                    let (x_dialed, y_dialed) = (local == &a, local == &b);
                    let (first, second) = if x_first { ((1u64, x_dialed), (2u64, y_dialed)) } else { ((2u64, y_dialed), (1u64, x_dialed)) };
                    let mut s = Sessions::new();
                    s.install(remote, first.0, first.1, local, 0);
                    s.install(remote, second.0, second.1, local, 5);
                    s.current(remote).unwrap().id
                };
                assert_eq!(survivor(&a, &b, first_is_x_at_a), survivor(&b, &a, first_is_x_at_b), "order {first_is_x_at_a}/{first_is_x_at_b}");
            }
        }
    }

    #[test]
    fn the_link_the_smaller_id_dialed_wins() {
        let (a, b) = (id(A), id(B));
        assert!(is_preferred(true, &a, &b));
        assert!(!is_preferred(false, &a, &b));
        assert!(is_preferred(false, &b, &a));
        assert_eq!(decide_install(inst(1, false), true, &a, &b, 10), InstallAction::ReplaceExisting);
        assert_eq!(decide_install(inst(1, true), false, &a, &b, 10), InstallAction::RejectNew);
        assert_eq!(decide_install(inst(1, true), false, &b, &a, 10), InstallAction::ReplaceExisting);
    }

    // A late duplicate (a restarted dial, a stray link) must not interrupt a session that is in use, whichever
    // direction it was dialed. Both sides see the same two seconds, so they still agree.
    #[test]
    fn a_settled_session_is_never_replaced() {
        let (a, b) = (id(A), id(B));
        let settled = Some(Installed { id: 1, dialed: false, open: true, since_ms: 1_000 });
        assert_eq!(decide_install(settled, true, &a, &b, 1_000 + SETTLE_MS), InstallAction::RejectNew, "even the preferred link");
        assert_eq!(decide_install(settled, true, &a, &b, 1_000 + SETTLE_MS - 1), InstallAction::ReplaceExisting, "inside the race window it can");
    }

    #[test]
    fn the_same_kind_of_link_keeps_the_first() {
        let (a, b) = (id(A), id(B));
        assert_eq!(decide_install(inst(1, true), true, &a, &b, 10), InstallAction::RejectNew);
        assert_eq!(decide_install(inst(1, false), false, &a, &b, 10), InstallAction::RejectNew);
    }

    #[test]
    fn a_closed_existing_link_is_replaced_by_anything() {
        let (a, b) = (id(A), id(B));
        let dead = Some(Installed { id: 1, dialed: true, open: false, since_ms: 0 });
        assert_eq!(decide_install(dead, false, &a, &b, 10), InstallAction::Accept);
        assert_eq!(decide_install(None, false, &a, &b, 10), InstallAction::Accept);
    }

    // Known bug 3: the close of the replaced link ended the session that replaced it.
    #[test]
    fn the_end_of_a_replaced_link_does_not_end_the_session() {
        let (a, b) = (id(A), id(B));
        let mut s = Sessions::new();
        let old = s.new_link();
        s.install(&b, old, false, &a, 0); // inbound, not preferred for A (A < B)
        let new = s.new_link();
        let (action, replaced) = s.install(&b, new, true, &a, 100);
        assert_eq!(action, InstallAction::ReplaceExisting);
        assert_eq!(replaced, Some(old));
        assert!(!s.link_ended(&b, old), "the old link closing must not end the session");
        assert_eq!(s.reach(&b), Reach::Online);
        assert!(s.link_ended(&b, new));
        assert_eq!(s.reach(&b), Reach::Offline);
    }

    #[test]
    fn a_rejected_link_ending_changes_nothing() {
        let (a, b) = (id(A), id(B));
        let mut s = Sessions::new();
        let first = s.new_link();
        s.install(&b, first, true, &a, 0);
        let dup = s.new_link();
        assert_eq!(s.install(&b, dup, true, &a, 1).0, InstallAction::RejectNew);
        assert!(!s.link_ended(&b, dup));
        assert!(s.is_current(&b, first));
    }

    // Known bug 2: a second dial for a peer that is already being dialed.
    #[test]
    fn one_dial_per_peer_and_a_growing_wait() {
        let b = id(B);
        let mut l = DialLedger::new();
        assert!(l.begin(&b, 0));
        assert!(!l.begin(&b, 1), "second dial must be a no-op");
        l.failed(&b, 10);
        assert!(!l.begin(&b, 500), "waiting after a failure");
        assert!(l.begin(&b, 1_010));
        l.failed(&b, 1_010);
        assert!(!l.begin(&b, 2_900));
        assert!(l.begin(&b, 3_010), "second wait is 2 s");
        for _ in 0..10 {
            l.failed(&b, 0);
        }
        assert!(l.waiting(&b, 9_999) && !l.waiting(&b, 10_000), "never longer than 10 s");
        l.succeeded(&b);
        assert!(l.begin(&b, 0));
    }

    #[test]
    fn reach_goes_connecting_online_offline() {
        let (a, b) = (id(A), id(B));
        let mut s = Sessions::new();
        assert_eq!(s.reach(&b), Reach::Offline);
        s.handshake_started(&b);
        assert_eq!(s.reach(&b), Reach::Connecting);
        let n = s.new_link();
        s.install(&b, n, true, &a, 0);
        s.handshake_ended(&b);
        assert_eq!(s.reach(&b), Reach::Online);
        s.link_ended(&b, n);
        assert_eq!(s.reach(&b), Reach::Offline);
    }

    #[test]
    fn a_wifi_hiccup_is_not_a_stall_but_400_ms_of_silence_is() {
        let mut w = Silence::new(0);
        assert_eq!(w.check(350), SilenceVerdict::Fine, "350 ms of silence is a hiccup, not a stall");
        assert_eq!(w.check(399), SilenceVerdict::Fine);
        assert_eq!(w.check(450), SilenceVerdict::Stalled, "450 ms of silence stalls");
        assert_eq!(w.check(470), SilenceVerdict::Fine, "stall is reported once");
        assert_eq!(w.quiet_for(470), 470);
        assert_eq!(w.check(3_000), SilenceVerdict::Dead);
        w.heard(3_100);
        assert!(!w.is_stalled());
        assert_eq!(w.check(3_499), SilenceVerdict::Fine);
        assert_eq!(w.check(3_500), SilenceVerdict::Stalled);
    }
}
