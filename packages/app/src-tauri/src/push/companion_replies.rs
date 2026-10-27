//! Push when a companion replies: at most one message per companion per minute. The first reply in a
//! minute goes out at once. Replies after it are counted, and when the minute ends one message says
//! how many came ("3 new replies"), or the text of the only one. Pure state and words: the hub builds
//! the messages per phone (`PushHub::companion_deliveries`) and `push/mod.rs` sends them.

use super::triggers::{truncate_chars, MAX_BODY_CHARS, MAX_TITLE_CHARS};
use std::collections::HashMap;

/// At most one message per companion in this window.
pub const WINDOW_MS: u64 = 60_000;

/// What one message says: one reply's text, or how many replies waited for the window to end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Summary {
    /// One reply: redacted, one line, cut to `MAX_BODY_CHARS`.
    Text(String),
    /// Several replies held in one window.
    Count(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub companion_id: String,
    /// The companion's name (the notification title).
    pub name: String,
    pub summary: Summary,
}

impl Message {
    /// The notification title: the companion's name, cut to `MAX_TITLE_CHARS`.
    pub fn title(&self) -> String {
        truncate_chars(&self.name, MAX_TITLE_CHARS)
    }

    /// The notification body. `hidden` is a phone with "Hide details on lock screen" on: no words of
    /// the reply leave the computer, only that there is one (or how many).
    pub fn body(&self, hidden: bool) -> String {
        match (&self.summary, hidden) {
            (Summary::Count(n), _) => format!("{n} new replies"),
            (Summary::Text(_), true) => "New reply".to_string(),
            (Summary::Text(text), false) => text.clone(),
        }
    }
}

/// What `Gate::reply` decided.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing to say (the reply was empty).
    Skip,
    /// Goes out now.
    Send(Message),
    /// Held until the window ends. `arm` is true when no timer waits for this companion yet: the
    /// caller starts one, which calls `Gate::release` until it returns `Send` or `Done`.
    Held { arm: bool },
}

/// What `Gate::release` decided for a waiting timer.
#[derive(Debug, PartialEq, Eq)]
pub enum Release {
    Send(Message),
    /// The window is still open: wake at this time and look again.
    Wait(u64),
    /// Nothing is held. The timer stops.
    Done,
}

#[derive(Default)]
struct Slot {
    name: String,
    /// When the last message went out (`None`: none yet).
    last_sent_ms: Option<u64>,
    /// Replies held back in the current window.
    held: u32,
    /// The newest held reply's text (sent alone when it is the only one).
    latest: String,
    /// A timer is waiting to release this companion's held replies.
    armed: bool,
}

#[derive(Default)]
pub struct Gate {
    slots: HashMap<String, Slot>,
}

/// One line of a reply, redacted before it is cut (a cut can leave half a secret no rule recognises).
fn line_of(text: &str) -> String {
    truncate_chars(crate::redact::text(text).as_str(), MAX_BODY_CHARS)
}

impl Gate {
    /// A reply went into the feed. Called only for a companion whose toggle is on.
    pub fn reply(&mut self, companion_id: &str, name: &str, text: &str, now_ms: u64) -> Step {
        let line = line_of(text);
        if line.is_empty() {
            return Step::Skip;
        }
        let slot = self.slots.entry(companion_id.to_string()).or_default();
        slot.name = name.to_string();
        match slot.last_sent_ms {
            Some(last) if now_ms.saturating_sub(last) < WINDOW_MS => {
                slot.held += 1;
                slot.latest = line;
                let arm = !slot.armed;
                slot.armed = true;
                Step::Held { arm }
            }
            _ => {
                slot.last_sent_ms = Some(now_ms);
                slot.held = 0;
                slot.latest.clear();
                Step::Send(Message {
                    companion_id: companion_id.to_string(),
                    name: name.to_string(),
                    summary: Summary::Text(line),
                })
            }
        }
    }

    /// A timer wakes for a companion: send what is held once the window has ended.
    pub fn release(&mut self, companion_id: &str, now_ms: u64) -> Release {
        let Some(slot) = self.slots.get_mut(companion_id) else {
            return Release::Done;
        };
        if slot.held == 0 {
            slot.armed = false;
            return Release::Done;
        }
        let due = slot.last_sent_ms.map_or(now_ms, |last| last + WINDOW_MS);
        if due > now_ms {
            return Release::Wait(due);
        }
        let summary = if slot.held == 1 {
            Summary::Text(std::mem::take(&mut slot.latest))
        } else {
            Summary::Count(slot.held)
        };
        slot.held = 0;
        slot.latest.clear();
        slot.last_sent_ms = Some(now_ms);
        slot.armed = false;
        Release::Send(Message {
            companion_id: companion_id.to_string(),
            name: slot.name.clone(),
            summary,
        })
    }

    /// The timer could not start: nothing waits for this companion any more, so the next hold may arm one.
    pub fn disarm(&mut self, companion_id: &str) {
        if let Some(slot) = self.slots.get_mut(companion_id) {
            slot.armed = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_000_000;

    fn text_of(message: &Message) -> &str {
        match &message.summary {
            Summary::Text(text) => text,
            Summary::Count(_) => panic!("expected one reply's text"),
        }
    }

    #[test]
    fn the_first_reply_goes_out_at_once_and_later_ones_wait_for_the_window() {
        let mut gate = Gate::default();
        let Step::Send(first) = gate.reply("mochi", "Mochi", "Good morning.", T0) else {
            panic!("first reply")
        };
        assert_eq!(first.title(), "Mochi");
        assert_eq!(first.body(false), "Good morning.");
        assert_eq!(
            gate.reply("mochi", "Mochi", "Second.", T0 + 10_000),
            Step::Held { arm: true }
        );
        assert_eq!(
            gate.reply("mochi", "Mochi", "Third.", T0 + 20_000),
            Step::Held { arm: false },
            "one timer per companion"
        );
        assert_eq!(
            gate.release("mochi", T0 + 30_000),
            Release::Wait(T0 + WINDOW_MS)
        );
        let Release::Send(batch) = gate.release("mochi", T0 + WINDOW_MS) else {
            panic!("the window ended")
        };
        assert_eq!(batch.body(false), "2 new replies");
        assert_eq!(
            gate.release("mochi", T0 + WINDOW_MS + 1),
            Release::Done,
            "nothing held any more"
        );
    }

    #[test]
    fn a_single_held_reply_is_sent_as_its_own_text_not_a_count() {
        let mut gate = Gate::default();
        let _ = gate.reply("mochi", "Mochi", "One.", T0);
        let _ = gate.reply("mochi", "Mochi", "Only the newest one counts.", T0 + 5_000);
        let Release::Send(message) = gate.release("mochi", T0 + WINDOW_MS) else {
            panic!("held")
        };
        assert_eq!(text_of(&message), "Only the newest one counts.");
    }

    #[test]
    fn each_companion_has_its_own_window() {
        let mut gate = Gate::default();
        assert!(matches!(
            gate.reply("mochi", "Mochi", "a", T0),
            Step::Send(_)
        ));
        assert!(matches!(
            gate.reply("bit", "Bit", "b", T0 + 1),
            Step::Send(_)
        ));
    }

    #[test]
    fn the_text_is_one_line_redacted_and_cut() {
        let mut gate = Gate::default();
        let raw = format!(
            "Key sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n\nthen {}",
            "word ".repeat(60)
        );
        let Step::Send(message) = gate.reply("mochi", "Mochi", &raw, T0) else {
            panic!("send")
        };
        let body = message.body(false);
        assert!(!body.contains("AAAAAAAAAAAAAAAA"), "{body}");
        assert!(!body.contains('\n'), "one line: {body:?}");
        assert!(
            body.chars().count() <= MAX_BODY_CHARS,
            "{}",
            body.chars().count()
        );
        assert!(body.ends_with('…'));
    }

    #[test]
    fn a_phone_with_hidden_details_gets_no_words() {
        let single = Message {
            companion_id: "mochi".into(),
            name: "Mochi".into(),
            summary: Summary::Text("Secret plan".into()),
        };
        assert_eq!(single.body(true), "New reply");
        let many = Message {
            summary: Summary::Count(3),
            ..single
        };
        assert_eq!(many.body(true), "3 new replies");
    }

    #[test]
    fn an_empty_reply_is_skipped() {
        assert_eq!(
            Gate::default().reply("mochi", "Mochi", "   \n ", T0),
            Step::Skip
        );
    }
}
