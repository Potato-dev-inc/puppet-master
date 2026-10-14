//! Long-poll fallback for the bridge event stream. Cloudflare quick tunnels (trycloudflare.com)
//! do not pass server-sent events through, so a phone on a tunnel polls `GET /events/poll`
//! instead: every `push_sse` payload is also kept here under a sequence number, and a poll returns
//! the payloads after the caller's cursor as soon as there are any (or nothing after `wait_ms`).

use once_cell::sync::Lazy;
use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const MAX_ITEMS: usize = 4096;
const MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_WAIT_MS: u64 = 25_000;

#[derive(Debug, Serialize, PartialEq)]
pub struct Poll {
    /// Pass back as `cursor` on the next poll.
    pub cursor: u64,
    /// The SSE payloads (`event: ...\ndata: ...\n\n` blocks) after the given cursor, in order.
    pub data: String,
    /// Some events after the cursor were already dropped (phone asleep, app restarted): the
    /// client must re-read state (pane list, terminal scrollback) instead of trusting the stream.
    pub gap: bool,
}

#[derive(Debug, Default)]
pub struct EventLog {
    next: u64,
    items: VecDeque<(u64, String)>,
    bytes: usize,
}

impl EventLog {
    pub fn push(&mut self, payload: String) {
        let seq = self.next;
        self.next += 1;
        self.bytes += payload.len();
        self.items.push_back((seq, payload));
        while self.items.len() > MAX_ITEMS || self.bytes > MAX_BYTES {
            match self.items.pop_front() {
                Some((_, dropped)) => self.bytes -= dropped.len(),
                None => break,
            }
        }
    }

    /// Payloads with a sequence number at or after `cursor`.
    pub fn since(&self, cursor: u64) -> Poll {
        self.since_for(cursor, &|_| true)
    }

    /// [`since`](Self::since), keeping only the payloads this viewer may receive (a paired device
    /// without `watch` gets no terminal bytes). The cursor still moves past what was left out.
    pub fn since_for(&self, cursor: u64, keep: &dyn Fn(&str) -> bool) -> Poll {
        let first = self.items.front().map(|(seq, _)| *seq).unwrap_or(self.next);
        let data = self
            .items
            .iter()
            .filter(|(seq, payload)| *seq >= cursor && keep(payload))
            .map(|(_, payload)| payload.as_str())
            .collect();
        Poll { cursor: self.next, data, gap: cursor < first || cursor > self.next }
    }

    fn has_after(&self, cursor: u64) -> bool {
        self.next > cursor
    }
}

static LOG: Lazy<(Mutex<EventLog>, Condvar)> = Lazy::new(|| (Mutex::new(EventLog::default()), Condvar::new()));

/// Called for every payload `push_sse` broadcasts.
pub fn record(payload: &str) {
    let (log, wake) = &*LOG;
    log.lock().push(payload.to_string());
    wake.notify_all();
}

/// `GET /events/poll`. Without a cursor: the current cursor, at once (subscribe from now). With
/// one: the payloads after it as soon as there are any, or an empty answer after `wait`.
#[cfg(test)]
pub fn poll(cursor: Option<u64>, wait: Duration) -> Poll {
    poll_for(cursor, wait, &|_| true)
}

/// [`poll`] for one viewer: payloads `keep` refuses are left out (see `EventLog::since_for`).
pub fn poll_for(cursor: Option<u64>, wait: Duration, keep: &dyn Fn(&str) -> bool) -> Poll {
    let (log, wake) = &*LOG;
    let mut guard = log.lock();
    let Some(cursor) = cursor else {
        return Poll { cursor: guard.next, data: String::new(), gap: false };
    };
    if cursor <= guard.next {
        let deadline = Instant::now() + wait.min(Duration::from_millis(MAX_WAIT_MS));
        while !guard.has_after(cursor) {
            if wake.wait_until(&mut guard, deadline).timed_out() {
                break;
            }
        }
    }
    guard.since_for(cursor, keep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attention_frame_is_carried_by_the_poll_log_like_any_other_event() {
        // GET /events/poll replays the very strings pushed to SSE and the WebSocket.
        let body = r#"{"v":1,"rev":7,"counts":{"blocking":2,"ready":0},"items":[]}"#;
        let mut log = EventLog::default();
        log.push(crate::attention::frame(body));
        let poll = log.since(0);
        assert!(poll.data.starts_with("event: attention
data: {\"v\":1,\"rev\":7"), "{}", poll.data);
        assert!(!poll.gap);
    }

    #[test]
    fn returns_payloads_after_the_cursor_in_order() {
        let mut log = EventLog::default();
        log.push("a\n\n".into());
        log.push("b\n\n".into());
        log.push("c\n\n".into());
        assert_eq!(log.since(1), Poll { cursor: 3, data: "b\n\nc\n\n".into(), gap: false });
        assert_eq!(log.since(3), Poll { cursor: 3, data: String::new(), gap: false });
    }

    #[test]
    fn reports_a_gap_when_events_were_dropped_or_the_cursor_is_from_before_a_restart() {
        let mut log = EventLog::default();
        for i in 0..(MAX_ITEMS + 10) {
            log.push(format!("{i}\n\n"));
        }
        assert!(log.since(0).gap, "the oldest events are gone");
        assert!(!log.since(20).gap);
        assert!(log.since(log.next + 5).gap, "cursor from an earlier app run");
    }

    #[test]
    fn caps_memory_by_bytes() {
        let mut log = EventLog::default();
        let big = "x".repeat(1024 * 1024);
        for _ in 0..20 {
            log.push(big.clone());
        }
        assert!(log.bytes <= MAX_BYTES);
        assert!(log.items.len() <= 8);
    }

    #[test]
    fn a_poll_wakes_as_soon_as_an_event_arrives() {
        let start = poll(None, Duration::ZERO).cursor;
        let waiter = std::thread::spawn(move || poll(Some(start), Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        record("event: test\ndata: {}\n\n");
        let got = waiter.join().unwrap();
        assert!(got.data.contains("event: test"));
        assert!(got.cursor > start);
    }
}
