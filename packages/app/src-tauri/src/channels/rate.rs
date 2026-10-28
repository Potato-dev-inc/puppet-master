//! Text Your Agents: message buckets (inbound, answers, outbound, strangers).
//!
//! A bucket holds `cap` tokens and gets them back evenly over `window_ms`: one token every
//! `window_ms / cap`. It starts full. Time is passed in by the caller, so tests need no clock.

const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 3_600_000;

/// A token bucket. Kept in whole units so there is no floating point: one token costs `window_ms`
/// units and the bucket gains `cap` units per millisecond. So one token comes back every
/// `window_ms / cap` milliseconds.
pub struct Bucket {
    cap: u64,
    window_ms: u64,
    level: u64,
    last_ms: Option<u64>,
}

impl Bucket {
    /// `cap` messages per `window_ms`, starting full.
    pub fn new(cap: u32, window_ms: u64) -> Self {
        let mut b = Self { cap: u64::from(cap), window_ms, level: 0, last_ms: None };
        b.level = b.full();
        b
    }

    /// Take one token. `false` means the caller is over the limit and must drop or hold the message.
    pub fn take(&mut self, now_ms: u64) -> bool {
        self.refill(now_ms);
        let cost = self.cost();
        if self.level < cost {
            return false;
        }
        self.level -= cost;
        true
    }

    fn cost(&self) -> u64 {
        self.window_ms.max(1)
    }

    fn full(&self) -> u64 {
        self.cap.saturating_mul(self.cost())
    }

    fn refill(&mut self, now_ms: u64) {
        let last = self.last_ms.unwrap_or(now_ms);
        let gained = now_ms.saturating_sub(last).saturating_mul(self.cap);
        self.level = self.level.saturating_add(gained).min(self.full());
        self.last_ms = Some(last.max(now_ms));
    }
}

/// The limits the chat channel keeps. Counts are per minute or per hour, as in the spec.
pub struct Limits {
    /// Messages from the paired chat: 20 a minute.
    pub inbound: Bucket,
    /// Button answers: 10 a minute.
    pub answers: Bucket,
    /// Messages sent out: 20 a minute.
    pub outbound: Bucket,
    /// Audit lines for strangers: 1 an hour.
    pub stranger_audit: Bucket,
    /// Strangers who wrote to the bot in the last hour (the engine keeps this number).
    pub strangers_last_hour: u32,
}

impl Limits {
    pub fn new() -> Self {
        Self {
            inbound: Bucket::new(20, MINUTE_MS),
            answers: Bucket::new(10, MINUTE_MS),
            outbound: Bucket::new(20, MINUTE_MS),
            stranger_audit: Bucket::new(1, HOUR_MS),
            strangers_last_hour: 0,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_starts_full_and_refuses_past_the_cap() {
        let mut b = Bucket::new(2, 1_000);
        assert!(b.take(0));
        assert!(b.take(0));
        assert!(!b.take(0));
    }

    #[test]
    fn bucket_refills_one_token_per_window_over_cap() {
        let mut b = Bucket::new(2, 1_000);
        assert!(b.take(0));
        assert!(b.take(0));
        assert!(!b.take(499));
        assert!(b.take(500));
        assert!(!b.take(500));
        assert!(b.take(1_000));
    }

    #[test]
    fn bucket_never_refills_past_its_cap() {
        let mut b = Bucket::new(2, 1_000);
        assert!(b.take(0));
        assert!(b.take(100_000));
        assert!(b.take(100_000));
        assert!(!b.take(100_000));
    }

    #[test]
    fn a_clock_going_backwards_does_not_refill() {
        let mut b = Bucket::new(1, 1_000);
        assert!(b.take(5_000));
        assert!(!b.take(4_000));
        assert!(!b.take(5_999));
        assert!(b.take(6_000));
    }

    #[test]
    fn limits_have_the_spec_caps() {
        let mut l = Limits::new();
        for _ in 0..20 {
            assert!(l.inbound.take(0));
        }
        assert!(!l.inbound.take(0));
        for _ in 0..10 {
            assert!(l.answers.take(0));
        }
        assert!(!l.answers.take(0));
        for _ in 0..20 {
            assert!(l.outbound.take(0));
        }
        assert!(!l.outbound.take(0));
        assert!(l.stranger_audit.take(0));
        assert!(!l.stranger_audit.take(HOUR_MS - 1));
        assert!(l.stranger_audit.take(HOUR_MS));
        assert_eq!(l.strangers_last_hour, 0);
    }

    #[test]
    fn inbound_refills_one_message_every_three_seconds() {
        let mut l = Limits::default();
        for _ in 0..20 {
            assert!(l.inbound.take(0));
        }
        assert!(!l.inbound.take(2_999));
        assert!(l.inbound.take(3_000));
    }
}
