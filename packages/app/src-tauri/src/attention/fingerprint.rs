//! The screen fingerprint of a prompt: a hash of exactly what a card showed, re-computed from the
//! live screen at answer time. An answer that carries a fingerprint the live prompt no longer has is
//! refused as `changed`: what you saw is what you answer.
//!
//! It is NOT the raw screen text (spinners, timers and scrolling above the dialog would make every
//! answer stale). It covers the prompt id (for a command prompt that already hashes the agent, kind,
//! command and folder), the occurrence (the same command asked again is a new prompt), the kind, the
//! summary the card printed, the offered choices and the risk level (so a risk read that differs from
//! the one on the card changes it).

use sha2::{Digest as _, Sha256};

/// 16 hex characters of `sha256` over the fields above.
pub fn fingerprint<'a>(
    prompt_id: &str,
    occurrence: u32,
    kind: &str,
    summary: Option<&str>,
    choice_ids: impl IntoIterator<Item = &'a str>,
    risk_level: Option<&str>,
) -> String {
    let mut ids: Vec<&str> = choice_ids.into_iter().collect();
    ids.sort_unstable();
    let body = serde_json::json!({
        "prompt_id": prompt_id,
        "occurrence": occurrence,
        "kind": kind,
        "summary": summary.unwrap_or(""),
        "choices": ids,
        "risk": risk_level.unwrap_or(""),
    });
    let digest = Sha256::digest(body.to_string().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> String {
        fingerprint("p1", 3, "command", Some("Run command: npm test"), ["deny", "allow_once"], Some("low"))
    }

    #[test]
    fn it_is_sixteen_hex_characters_and_stable() {
        let a = base();
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, base());
    }

    #[test]
    fn the_order_of_the_choices_does_not_matter() {
        assert_eq!(base(), fingerprint("p1", 3, "command", Some("Run command: npm test"), ["allow_once", "deny"], Some("low")));
    }

    #[test]
    fn anything_the_card_showed_changes_it() {
        let other = [
            fingerprint("p2", 3, "command", Some("Run command: npm test"), ["deny", "allow_once"], Some("low")),
            fingerprint("p1", 4, "command", Some("Run command: npm test"), ["deny", "allow_once"], Some("low")),
            fingerprint("p1", 3, "edit", Some("Run command: npm test"), ["deny", "allow_once"], Some("low")),
            fingerprint("p1", 3, "command", Some("Run command: npm test --all"), ["deny", "allow_once"], Some("low")),
            fingerprint("p1", 3, "command", Some("Run command: npm test"), ["deny"], Some("low")),
            fingerprint("p1", 3, "command", Some("Run command: npm test"), ["deny", "allow_once"], Some("high")),
            fingerprint("p1", 3, "command", Some("Run command: npm test"), ["deny", "allow_once"], None),
        ];
        let base = base();
        for (i, f) in other.iter().enumerate() {
            assert_ne!(*f, base, "variant {i}");
        }
    }
}
