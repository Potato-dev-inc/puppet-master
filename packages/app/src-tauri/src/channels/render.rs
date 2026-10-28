//! Text Your Agents: the one cleaning step for everything sent to the chat.
//!
//! Telegram keeps what the bot sends, so every outgoing string goes through `clean`: control and
//! bidi characters out, known secrets and personal paths out (`redact::strict`), e-mail addresses
//! out (`redact::mask_emails`, which `strict` does not do), then one length cap. Plain text only.

use crate::push::triggers::{sanitize, truncate_chars};
use crate::redact::{self, Redacted};

/// The longest text the bot sends in one message.
pub const MAX_OUT_CHARS: usize = 1500;
/// The longest button label the chat shows.
pub const MAX_BUTTON_LABEL_CHARS: usize = 40;
/// Telegram's limit for `callback_data`.
pub const MAX_BUTTON_DATA_BYTES: usize = 64;

/// One inline button. `data` is an opaque token (see `grants.rs`), never a payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub data: String,
}

/// One message to the chat: plain text plus its buttons (one `Vec` per row).
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub text: Redacted<String>,
    pub keyboard: Vec<Vec<Button>>,
}

/// Clean `raw` for the chat: sanitize, redact secrets and personal paths, mask e-mail addresses,
/// then cut to `max_chars` (an ellipsis marks a cut). The last step is `redact::strict` again on the
/// cut text: it changes nothing that is already masked, and it keeps the type honest (only the
/// redact module makes a `Redacted`).
pub fn clean(raw: &str, max_chars: usize) -> Redacted<String> {
    let masked = masked_text(&sanitize(raw));
    redact::strict(&truncate_chars(&masked, max_chars))
}

/// True when cleaning changed the text in a way the person would not see as the same words:
/// a secret, a path, an e-mail address or a hidden character was removed. Approvals with buttons
/// are only offered when this is false.
pub fn changed_by_redaction(raw: &str) -> bool {
    masked_text(raw) != sanitize(raw)
}

/// A message with no buttons, cleaned with the default cap.
pub fn plain(raw: &str) -> Outgoing {
    Outgoing { text: clean(raw, MAX_OUT_CHARS), keyboard: Vec::new() }
}

/// A button with a cleaned label (at most 40 characters). `None` when `data` is over 64 bytes:
/// the caller must not send a button Telegram would refuse.
pub fn button(label: &str, data: &str) -> Option<Button> {
    if data.is_empty() || data.len() > MAX_BUTTON_DATA_BYTES {
        return None;
    }
    Some(Button { label: clean(label, MAX_BUTTON_LABEL_CHARS).into_inner(), data: data.to_string() })
}

/// Redact (`strict`) and mask e-mail addresses. No sanitizing and no length cap: the callers decide.
fn masked_text(text: &str) -> String {
    redact::mask_emails(redact::strict(text).as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "sk-ant-api03-TESTKEYTESTKEY123456";

    #[test]
    fn clean_removes_token_email_and_user_path() {
        let raw = format!("Key {TOKEN} for you@example.com in C:\\Users\\you\\secret\\notes.txt");
        let out = clean(&raw, MAX_OUT_CHARS);
        let text = out.as_str();
        assert!(!text.contains("sk-ant"), "token leaked: {text}");
        assert!(!text.contains("you@example.com"), "email leaked: {text}");
        assert!(!text.contains("\\you\\"), "user folder leaked: {text}");
        assert!(text.contains("<email>"), "email not masked: {text}");
    }

    #[test]
    fn clean_removes_bidi_override_and_ansi_escape() {
        let raw = "ok\u{202E}evil \u{1b}[31mred\u{1b}[0m done";
        let text = clean(raw, MAX_OUT_CHARS).into_inner();
        assert!(!text.contains('\u{202E}'), "bidi override kept: {text:?}");
        assert!(!text.contains('\u{1b}'), "escape kept: {text:?}");
        assert!(text.contains("done"));
    }

    #[test]
    fn clean_caps_length_with_ellipsis() {
        let raw = "word ".repeat(400);
        let text = clean(&raw, MAX_OUT_CHARS).into_inner();
        assert_eq!(text.chars().count(), MAX_OUT_CHARS);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn plain_uses_the_default_cap_and_no_buttons() {
        let out = plain(&"word ".repeat(400));
        assert!(out.keyboard.is_empty());
        assert_eq!(out.text.as_str().chars().count(), MAX_OUT_CHARS);
        assert_eq!(plain("Connected.").text.as_str(), "Connected.");
    }

    #[test]
    fn changed_by_redaction_is_true_when_something_was_removed() {
        assert!(changed_by_redaction(&format!("token {TOKEN}")));
        assert!(changed_by_redaction("mail you@example.com"));
        assert!(changed_by_redaction("path C:\\Users\\you\\x"));
        assert!(changed_by_redaction("bidi \u{202E}text"));
    }

    #[test]
    fn changed_by_redaction_is_false_for_plain_text() {
        assert!(!changed_by_redaction("Build is green, 42 passed."));
        assert!(!changed_by_redaction("Run npm test in the project"));
    }

    #[test]
    fn button_data_is_limited_to_64_bytes() {
        let ok = format!("g:{}", "a".repeat(22));
        assert!(ok.len() <= MAX_BUTTON_DATA_BYTES);
        let b = button("Allow once…", &ok).expect("24-byte data is fine");
        assert_eq!(b.data, ok);
        assert!(button("Allow once", &"x".repeat(65)).is_none());
        assert!(button("Allow once", &"x".repeat(64)).is_some());
        assert!(button("Allow once", "").is_none());
    }

    #[test]
    fn button_label_is_capped_at_40_chars() {
        let b = button(&"label ".repeat(20), "g:abc").unwrap();
        assert!(b.label.chars().count() <= MAX_BUTTON_LABEL_CHARS);
        assert!(b.label.ends_with('…'));
    }
}
