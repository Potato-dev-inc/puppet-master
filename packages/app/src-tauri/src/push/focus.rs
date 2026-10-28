//! The end-of-Focus message and its one reminder, for the phones (SPEC n33 3.4 E, FT-7).
//!
//! Counts only. The line comes from `attention::focus::summary` (the counts part) or from `nudge::text`,
//! and never names an agent, a pane, a task or a sentence. `PushHub::focus_deliveries` builds one message per
//! phone that may watch and is not looking at the PWA, and `push::notify_focus` sends it. A phone with "Hide
//! details" on gets the fixed sentence instead of the counts. Not urgent: the phone shows it when it next wakes.

use super::triggers::{truncate_chars, MAX_BODY_CHARS};
use crate::attention::focus::nudge;

/// The title of the end-of-Focus message.
pub const SUMMARY_TITLE: &str = "Focus is over";
/// The title of the one reminder (a quiet card still open 20 minutes after Focus saw it).
pub const REMINDER_TITLE: &str = "Waiting for you";
/// What a phone with "Hide details on lock screen" reads instead of the counts.
pub const HIDDEN_BODY: &str = "Open Puppet Master to see what happened.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The notification title: always `SUMMARY_TITLE` or `REMINDER_TITLE`.
    pub title: String,
    /// Counts only, no header: "4 turns done, 1 failed, 2 asked for you. 1 still waits."
    pub line: String,
}

impl Message {
    /// The end-of-Focus message. `line` is the counts part of the summary (`summary::body`).
    pub fn summary(line: &str) -> Self {
        Self { title: SUMMARY_TITLE.to_string(), line: line.to_string() }
    }

    /// The one reminder for `waiting` quiet cards that stayed open for 20 minutes.
    pub fn reminder(waiting: u32) -> Self {
        Self { title: REMINDER_TITLE.to_string(), line: nudge::text(waiting as usize) }
    }

    /// The notification body. `hidden` is a phone with "Hide details on lock screen" on: the fixed
    /// sentence, no counts. Otherwise the line, cut to `MAX_BODY_CHARS`.
    pub fn body(&self, hidden: bool) -> String {
        if hidden {
            HIDDEN_BODY.to_string()
        } else {
            truncate_chars(&self.line, MAX_BODY_CHARS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::hub::testing::{hub_with, FakeDirectory};
    use crate::push::hub::{Delivery, PushHub};
    use crate::push::store::test_subscription;
    use crate::push::triggers::Kind;
    use serde_json::Value;
    use std::sync::Arc;

    const COUNTS: &str = "4 turns done, 1 failed, 2 asked for you. 1 still waits.";

    /// Phones: "dev-a" (watch), "dev-w" (watch), "dev-h" (watch, hide details), "dev-n" (no watch).
    fn focus_hub(name: &str) -> PushHub {
        let dir = Arc::new(FakeDirectory::new(&[("tok-a", "dev-a"), ("tok-w", "dev-w"), ("tok-h", "dev-h"), ("tok-n", "dev-n")]));
        dir.cannot_watch.lock().push("dev-n".into());
        let hub = hub_with(name, dir);
        for (device, endpoint) in [
            ("dev-a", "https://fcm.googleapis.com/fcm/send/a"),
            ("dev-w", "https://fcm.googleapis.com/fcm/send/w"),
            ("dev-h", "https://fcm.googleapis.com/fcm/send/h"),
            ("dev-n", "https://fcm.googleapis.com/fcm/send/n"),
        ] {
            let (sub, _, _) = test_subscription(endpoint);
            hub.store().lock().upsert(device, sub).unwrap();
        }
        hub.store().lock().set_hide_details("dev-h", true).unwrap();
        hub
    }

    fn payload(deliveries: &[Delivery], device: &str) -> Value {
        let delivery = deliveries.iter().find(|d| d.device_id == device).unwrap_or_else(|| panic!("no delivery for {device}"));
        serde_json::from_slice(&delivery.payload).unwrap()
    }

    #[test]
    fn the_summary_body_is_the_counts_line_without_a_header() {
        let message = Message::summary(COUNTS);
        assert_eq!(message.title, "Focus is over");
        assert_eq!(message.body(false), COUNTS);
    }

    #[test]
    fn a_hidden_phone_gets_the_fixed_sentence_and_no_counts() {
        let body = Message::summary(COUNTS).body(true);
        assert_eq!(body, "Open Puppet Master to see what happened.");
        assert!(!body.chars().any(|c| c.is_ascii_digit()), "no counts for a hidden phone");
    }

    #[test]
    fn the_body_is_cut_to_the_body_limit() {
        let body = Message::summary(&"word ".repeat(60)).body(false);
        assert!(body.chars().count() <= MAX_BODY_CHARS, "{}", body.chars().count());
    }

    #[test]
    fn the_reminder_reads_one_agent_or_many() {
        assert_eq!(Message::reminder(1).line, "1 agent has waited 20 minutes for you.");
        assert_eq!(Message::reminder(3).line, "3 agents have waited 20 minutes for you.");
        let message = Message::reminder(3);
        assert_eq!(message.title, "Waiting for you");
        assert_eq!(message.body(false), "3 agents have waited 20 minutes for you.");
        assert_eq!(message.body(true), HIDDEN_BODY);
    }

    #[test]
    fn the_focus_kind_has_its_own_name_and_is_not_urgent() {
        assert_eq!(Kind::Focus.as_str(), "focus");
        assert!(!Kind::Focus.is_urgent(), "a Focus message waits for the phone to wake");
        assert!(!Kind::Focus.is_pane_prompt());
    }

    #[test]
    fn the_focus_message_goes_to_the_phones_that_may_watch_and_nowhere_else() {
        let hub = focus_hub("focus_push_targets");
        let deliveries = hub.focus_deliveries(&Message::summary(COUNTS), 1_000);
        let mut devices: Vec<&str> = deliveries.iter().map(|d| d.device_id.as_str()).collect();
        devices.sort_unstable();
        assert_eq!(devices, vec!["dev-a", "dev-h", "dev-w"], "dev-n has no watch ability");
        assert!(deliveries.iter().all(|d| !d.urgent && d.pane_id.is_none() && d.kind == Kind::Focus));
    }

    #[test]
    fn a_phone_on_screen_gets_no_focus_message_and_the_others_do() {
        let hub = focus_hub("focus_push_visible");
        hub.set_visible("dev-a", true, 1_000);
        let devices: Vec<String> = hub.focus_deliveries(&Message::summary(COUNTS), 1_500).into_iter().map(|d| d.device_id).collect();
        assert!(!devices.contains(&"dev-a".to_string()), "{devices:?}");
        assert!(devices.contains(&"dev-w".to_string()));
    }

    #[test]
    fn the_payload_carries_only_the_focus_routing_keys_and_never_a_pane() {
        let hub = focus_hub("focus_push_keys");
        let deliveries = hub.focus_deliveries(&Message::summary(COUNTS), 1_000);
        let body = payload(&deliveries, "dev-a");
        assert_eq!(body["kind"], "focus");
        assert_eq!(body["tag"], "pm-focus");
        assert_eq!(body["v"], 2);
        assert_eq!(body["title"], "Focus is over");
        for absent in ["pane_id", "act", "prompt_id", "item_id", "companion_id"] {
            assert!(body.get(absent).is_none(), "{absent} must not be in a Focus push");
        }
    }

    #[test]
    fn a_hidden_phone_gets_the_fixed_body_and_the_others_the_counts() {
        let hub = focus_hub("focus_push_hidden");
        let deliveries = hub.focus_deliveries(&Message::summary(COUNTS), 1_000);
        assert_eq!(payload(&deliveries, "dev-h")["body"], "Open Puppet Master to see what happened.");
        assert_eq!(payload(&deliveries, "dev-a")["body"], COUNTS);
    }

    #[test]
    fn the_title_and_body_pass_through_redaction() {
        let secret = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let hub = focus_hub("focus_push_redact");
        let deliveries = hub.focus_deliveries(&Message::summary(&format!("Key {secret} done")), 1_000);
        let body = payload(&deliveries, "dev-a")["body"].as_str().unwrap().to_string();
        assert!(!body.contains("AAAAAAAAAAAAAAAA"), "{body}");
    }
}
