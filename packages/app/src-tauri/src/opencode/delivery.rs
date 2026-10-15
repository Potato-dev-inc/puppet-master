//! Delivery acknowledgement for native prompts.
//!
//! `prompt_async` answers 204 as soon as the request is queued, which does not prove the session
//! recorded the prompt. A follow-up can be accepted over HTTP yet never show up (stale busy view,
//! attach churn), leaving the turn waiting for a completion that will never come. After sending we
//! confirm a *new* user message carrying the text appears in the session; if not, resend once, then
//! fail with a recoverable `PROMPT_NOT_DELIVERED`.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::messages::{MessagePartView, MessageView};

pub const PROMPT_NOT_DELIVERED: &str = "PROMPT_NOT_DELIVERED";
pub const ACK_TIMEOUT: Duration = Duration::from_secs(10);
const ACK_POLL: Duration = Duration::from_millis(250);

fn norm(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True when `messages` holds a user message that is not in `before` and whose text carries `text`.
pub fn prompt_acked(before: &HashSet<String>, messages: &[MessageView], text: &str) -> bool {
    let want = norm(text);
    if want.is_empty() {
        return true;
    }
    messages.iter().any(|message| {
        message.role.eq_ignore_ascii_case("user")
            && message.id.as_ref().map_or(true, |id| !before.contains(id))
            && {
                let have: String = message
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        MessagePartView::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                norm(&have).contains(&want)
            }
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Acked after this many sends (1 or 2).
    Delivered(u8),
    NotDelivered,
}

/// Send, wait for the ack, resend once, then give up. `sleep` and `now` are injected for tests.
pub fn deliver_with_ack<S, L, Z, N>(
    before: &HashSet<String>,
    text: &str,
    ack_timeout: Duration,
    mut send: S,
    mut list: L,
    mut sleep: Z,
    mut now: N,
) -> Result<Delivery, String>
where
    S: FnMut() -> Result<(), String>,
    L: FnMut() -> Result<Vec<MessageView>, String>,
    Z: FnMut(Duration),
    N: FnMut() -> Instant,
{
    for attempt in 1..=2u8 {
        send()?;
        let deadline = now() + ack_timeout;
        loop {
            // A failed listing is not a failed delivery; keep polling until the deadline.
            if let Ok(messages) = list() {
                if prompt_acked(before, &messages, text) {
                    return Ok(Delivery::Delivered(attempt));
                }
            }
            if now() >= deadline {
                break;
            }
            sleep(ACK_POLL);
        }
    }
    Ok(Delivery::NotDelivered)
}

pub fn not_delivered_message() -> String {
    format!(
        "{PROMPT_NOT_DELIVERED}: the prompt was accepted by the OpenCode API but never appeared as a user message in the session after a retry; the turn was not started, retrying is safe"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn user(id: &str, text: &str) -> MessageView {
        MessageView {
            id: Some(id.into()),
            role: "user".into(),
            model: None,
            finish: None,
            error: None,
            parts: vec![MessagePartView::Text { text: text.into() }],
        }
    }

    fn run(visible_after_send: Option<u32>) -> (Delivery, u32) {
        let sends = Cell::new(0u32);
        let clock = RefCell::new(Instant::now());
        let before: HashSet<String> = ["m0".to_string()].into();
        let result = deliver_with_ack(
            &before,
            "do the thing",
            Duration::from_secs(10),
            || {
                sends.set(sends.get() + 1);
                Ok(())
            },
            || {
                if visible_after_send.is_some_and(|n| sends.get() >= n) {
                    Ok(vec![user("m0", "old"), user("m1", "do the  thing")])
                } else {
                    Ok(vec![user("m0", "old")])
                }
            },
            |d| *clock.borrow_mut() += d,
            || *clock.borrow(),
        )
        .unwrap();
        (result, sends.get())
    }

    #[test]
    fn acked_first_send() {
        assert_eq!(run(Some(1)), (Delivery::Delivered(1), 1));
    }

    #[test]
    fn retries_once_then_acks() {
        assert_eq!(run(Some(2)), (Delivery::Delivered(2), 2));
    }

    #[test]
    fn fails_after_one_retry() {
        assert_eq!(run(None), (Delivery::NotDelivered, 2));
    }

    #[test]
    fn old_user_message_with_same_text_is_not_an_ack() {
        let before: HashSet<String> = ["m1".to_string()].into();
        assert!(!prompt_acked(&before, &[user("m1", "hi")], "hi"));
        assert!(prompt_acked(&before, &[user("m2", "hi")], "hi"));
    }

    #[test]
    fn send_error_propagates_without_retry() {
        let before = HashSet::new();
        let r = deliver_with_ack(
            &before,
            "x",
            Duration::from_secs(1),
            || Err("boom".into()),
            || Ok(vec![]),
            |_| {},
            Instant::now,
        );
        assert_eq!(r, Err("boom".to_string()));
    }
}
