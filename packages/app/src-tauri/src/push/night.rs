//! The one morning push of a Night Shift run: "4 of 5 done, tests green. 1 needs you." (SPEC n32 3.11).
//!
//! Counts only. The line comes from `night_shift::report::line`, which never carries task text, a
//! project name or a path. `PushHub::night_deliveries` builds one message per phone that may watch
//! and is not looking at the PWA, and `push::notify_night` sends it. A phone with "Hide details" on
//! gets the fixed sentence instead of the line. Not urgent: the phone shows it when it next wakes.

use super::triggers::{truncate_chars, MAX_BODY_CHARS};
use crate::night_shift::report::{self, Summary, HIDDEN_PUSH};

/// The notification title.
pub const TITLE: &str = "Night Shift";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The summary line, counts only: "4 of 5 done, tests green. 1 needs you."
    pub line: String,
}

impl Message {
    /// The message for a finished run's counts.
    pub fn from_summary(summary: &Summary) -> Self {
        Self { line: report::line(summary) }
    }

    /// The notification title. Always the same words, never the task text.
    pub fn title(&self) -> String {
        TITLE.to_string()
    }

    /// The notification body. `hidden` is a phone with "Hide details on lock screen" on: the fixed
    /// sentence, no counts. Otherwise the line, one line, cut to `MAX_BODY_CHARS`.
    pub fn body(&self, hidden: bool) -> String {
        if hidden {
            HIDDEN_PUSH.to_string()
        } else {
            truncate_chars(&self.line, MAX_BODY_CHARS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::hub::testing::{hub_with, FakeDirectory};
    use crate::push::store::test_subscription;
    use std::sync::Arc;

    fn summary(total: u32, done: u32, green: u32, needs_you: u32, didnt_run: u32, no_changes: u32) -> Summary {
        Summary { total, done, green, needs_you, didnt_run, no_changes }
    }

    /// Phones: "dev-a" (watch), "dev-w" (watch), "dev-h" (watch, hide details), "dev-n" (no watch).
    fn night_hub(name: &str) -> crate::push::hub::PushHub {
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

    fn payload(deliveries: &[crate::push::hub::Delivery], device: &str) -> serde_json::Value {
        let delivery = deliveries.iter().find(|d| d.device_id == device).unwrap_or_else(|| panic!("no delivery for {device}"));
        serde_json::from_slice(&delivery.payload).unwrap()
    }

    #[test]
    fn the_body_is_the_counts_line_for_each_summary_shape() {
        let shapes = [
            (summary(5, 4, 4, 1, 0, 0), "4 of 5 done, tests green. 1 needs you."),
            (summary(5, 0, 0, 3, 2, 0), "0 of 5 done. 3 need you. 2 didn't run."),
            (summary(3, 2, 1, 0, 0, 0), "2 of 3 done, tests green on 1"),
            (summary(2, 2, 0, 0, 0, 0), "2 of 2 done, no tests ran"),
            (summary(4, 3, 3, 0, 0, 1), "3 of 4 done, tests green. 1 changed nothing."),
            (summary(1, 1, 1, 0, 0, 0), "1 of 1 done, tests green"),
        ];
        for (s, expected) in shapes {
            assert_eq!(Message::from_summary(&s).body(false), expected);
        }
    }

    #[test]
    fn a_hidden_phone_gets_only_the_fixed_sentence_and_no_counts() {
        let message = Message::from_summary(&summary(5, 4, 4, 1, 0, 0));
        assert_eq!(message.body(true), "Night Shift is ready");
        assert!(!message.body(true).contains('4'), "no counts for a hidden phone");
    }

    #[test]
    fn the_title_is_always_night_shift() {
        assert_eq!(Message::from_summary(&summary(1, 0, 0, 0, 1, 0)).title(), "Night Shift");
    }

    #[test]
    fn the_body_is_cut_to_the_body_limit() {
        let message = Message { line: "word ".repeat(60) };
        let body = message.body(false);
        assert!(body.chars().count() <= MAX_BODY_CHARS, "{}", body.chars().count());
    }

    #[test]
    fn the_night_push_goes_to_the_phones_that_may_watch_and_nowhere_else() {
        let hub = night_hub("night_push_targets");
        let message = Message::from_summary(&summary(5, 4, 4, 1, 0, 0));
        let deliveries = hub.night_deliveries(&message, 1_000);
        let mut devices: Vec<&str> = deliveries.iter().map(|d| d.device_id.as_str()).collect();
        devices.sort_unstable();
        assert_eq!(devices, vec!["dev-a", "dev-h", "dev-w"], "dev-n has no watch ability");
        assert!(deliveries.iter().all(|d| !d.urgent && d.pane_id.is_none() && d.kind == crate::push::triggers::Kind::NightShift));
    }

    #[test]
    fn a_phone_without_watch_gets_nothing() {
        let hub = night_hub("night_push_no_watch");
        let devices: Vec<String> = hub.night_deliveries(&Message::from_summary(&summary(1, 1, 1, 0, 0, 0)), 1_000).into_iter().map(|d| d.device_id).collect();
        assert!(!devices.contains(&"dev-n".to_string()), "{devices:?}");
    }

    #[test]
    fn a_phone_on_screen_gets_no_night_push() {
        let hub = night_hub("night_push_visible");
        hub.set_visible("dev-a", true, 1_000);
        let devices: Vec<String> = hub.night_deliveries(&Message::from_summary(&summary(1, 1, 1, 0, 0, 0)), 1_500).into_iter().map(|d| d.device_id).collect();
        assert!(!devices.contains(&"dev-a".to_string()), "{devices:?}");
        assert!(devices.contains(&"dev-w".to_string()));
    }

    #[test]
    fn a_hidden_phone_gets_the_fixed_body_and_the_night_routing_keys() {
        let hub = night_hub("night_push_keys");
        let deliveries = hub.night_deliveries(&Message::from_summary(&summary(5, 4, 4, 1, 0, 0)), 1_000);
        let hidden = payload(&deliveries, "dev-h");
        assert_eq!(hidden["body"], "Night Shift is ready");
        assert_eq!(hidden["title"], "Night Shift");
        assert_eq!(hidden["kind"], "night_shift");
        assert_eq!(hidden["tag"], "pm-night");
        assert_eq!(hidden["v"], 2);
        assert!(!hidden.to_string().contains("tests green"));
        assert_eq!(payload(&deliveries, "dev-a")["body"], "4 of 5 done, tests green. 1 needs you.");
    }

    #[test]
    fn the_title_and_body_pass_through_redaction() {
        // The counts line never holds a secret by construction; this proves the push still redacts.
        let secret = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let hub = night_hub("night_push_redact");
        let message = Message { line: format!("Key {secret} done") };
        let deliveries = hub.night_deliveries(&message, 1_000);
        let body = payload(&deliveries, "dev-a")["body"].as_str().unwrap().to_string();
        assert!(!body.contains("AAAAAAAAAAAAAAAA"), "{body}");
    }
}
