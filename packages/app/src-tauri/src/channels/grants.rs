//! Text Your Agents: single-use, short-lived tokens behind the bot's buttons.
//!
//! The button data the chat sees is only a random token. It means nothing by itself: the action
//! (which card, which choice, the prompt it was shown for) stays here, in memory, and comes back
//! once from `redeem`. A forged or unknown token gets `GrantError::Unknown`.

use std::collections::VecDeque;

use base64::Engine;
use parking_lot::Mutex;
use rand::rngs::OsRng;
use rand::RngCore;

/// An answer button stays live this long (10 minutes).
pub const ANSWER_TTL_MS: u64 = 600_000;
/// The confirm step of "Allow once" stays live this long (60 seconds).
pub const CONFIRM_TTL_MS: u64 = 60_000;
/// At most this many grants are held. The oldest one is dropped first.
const MAX_GRANTS: usize = 256;

/// What a button does once its token is redeemed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantAction {
    Answer { item_id: String, choice: Option<String>, prompt_id: Option<String>, occurrence: Option<u32>, fingerprint: Option<String> },
    ConfirmAllow { item_id: String, prompt_id: String, occurrence: u32, fingerprint: String },
    Cancel { item_id: String },
}

/// Why a token was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantError {
    /// Never issued, already used, or dropped by the cap.
    Unknown,
    /// Real once, but its time is up.
    Expired,
}

struct Entry {
    token: String,
    action: GrantAction,
    expires_ms: u64,
}

/// The live grants. Shared by the engine; every method takes `&self`.
#[derive(Default)]
pub struct Grants {
    entries: Mutex<VecDeque<Entry>>,
}

impl Grants {
    /// Mint a token for `action`, valid for `ttl_ms` from `now_ms`. Expired grants are dropped
    /// first; if the list is still at the cap, the oldest grant goes.
    pub fn issue(&self, action: GrantAction, now_ms: u64, ttl_ms: u64) -> String {
        let token = new_token();
        let mut entries = self.entries.lock();
        entries.retain(|e| now_ms < e.expires_ms);
        while entries.len() >= MAX_GRANTS {
            entries.pop_front();
        }
        entries.push_back(Entry { token: token.clone(), action, expires_ms: now_ms.saturating_add(ttl_ms) });
        token
    }

    /// Use a token once. On success every other grant for the same item is dropped too, so a
    /// tap on "Allow once" also kills the "Deny" button of that card.
    pub fn redeem(&self, token: &str, now_ms: u64) -> Result<GrantAction, GrantError> {
        let mut entries = self.entries.lock();
        let pos = entries.iter().position(|e| e.token == token).ok_or(GrantError::Unknown)?;
        let entry = entries.remove(pos).ok_or(GrantError::Unknown)?;
        if now_ms >= entry.expires_ms {
            return Err(GrantError::Expired);
        }
        let item = item_of(&entry.action).to_string();
        entries.retain(|e| item_of(&e.action) != item);
        Ok(entry.action)
    }
}

fn item_of(action: &GrantAction) -> &str {
    match action {
        GrantAction::Answer { item_id, .. } | GrantAction::ConfirmAllow { item_id, .. } | GrantAction::Cancel { item_id } => item_id,
    }
}

/// 22 url-safe characters: 128 random bits, base64url without padding. Fits Telegram's 64-byte
/// button data limit with the `g:` prefix.
fn new_token() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(item: &str, choice: &str) -> GrantAction {
        GrantAction::Answer {
            item_id: item.to_string(),
            choice: Some(choice.to_string()),
            prompt_id: Some("p1".to_string()),
            occurrence: Some(1),
            fingerprint: Some("fp".to_string()),
        }
    }

    fn cancel(item: &str) -> GrantAction {
        GrantAction::Cancel { item_id: item.to_string() }
    }

    #[test]
    fn token_is_22_url_safe_chars_and_unique() {
        let g = Grants::default();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let t = g.issue(cancel("a"), 0, ANSWER_TTL_MS);
            assert_eq!(t.len(), 22);
            assert!(t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{t}");
            assert!(seen.insert(t));
        }
    }

    #[test]
    fn redeem_returns_the_action_once() {
        let g = Grants::default();
        let t = g.issue(answer("a", "yes"), 0, ANSWER_TTL_MS);
        assert_eq!(g.redeem(&t, 1), Ok(answer("a", "yes")));
        assert_eq!(g.redeem(&t, 2), Err(GrantError::Unknown));
    }

    #[test]
    fn unknown_token_is_refused() {
        let g = Grants::default();
        g.issue(answer("a", "yes"), 0, ANSWER_TTL_MS);
        assert_eq!(g.redeem("not-a-real-token-at-all", 1), Err(GrantError::Unknown));
    }

    #[test]
    fn grant_expires_at_its_ttl() {
        let g = Grants::default();
        let t = g.issue(answer("a", "yes"), 1_000, ANSWER_TTL_MS);
        assert_eq!(g.redeem(&t, 1_000 + ANSWER_TTL_MS - 1), Ok(answer("a", "yes")));
        let t2 = g.issue(answer("b", "yes"), 1_000, ANSWER_TTL_MS);
        assert_eq!(g.redeem(&t2, 1_000 + ANSWER_TTL_MS), Err(GrantError::Expired));
        // Once refused as expired, the token is gone.
        assert_eq!(g.redeem(&t2, 1_001), Err(GrantError::Unknown));
    }

    #[test]
    fn confirm_grant_lives_60_seconds() {
        let g = Grants::default();
        let confirm = GrantAction::ConfirmAllow {
            item_id: "a".to_string(),
            prompt_id: "p1".to_string(),
            occurrence: 1,
            fingerprint: "fp".to_string(),
        };
        let t = g.issue(confirm.clone(), 0, CONFIRM_TTL_MS);
        assert_eq!(g.redeem(&t, CONFIRM_TTL_MS), Err(GrantError::Expired));
        let t = g.issue(confirm.clone(), 0, CONFIRM_TTL_MS);
        assert_eq!(g.redeem(&t, CONFIRM_TTL_MS - 1), Ok(confirm));
    }

    #[test]
    fn redeeming_drops_sibling_grants_of_the_same_item_only() {
        let g = Grants::default();
        let allow = g.issue(answer("a", "yes"), 0, ANSWER_TTL_MS);
        let deny = g.issue(cancel("a"), 0, ANSWER_TTL_MS);
        let other = g.issue(cancel("b"), 0, ANSWER_TTL_MS);
        assert_eq!(g.redeem(&allow, 1), Ok(answer("a", "yes")));
        assert_eq!(g.redeem(&deny, 2), Err(GrantError::Unknown));
        assert_eq!(g.redeem(&other, 3), Ok(cancel("b")));
    }

    #[test]
    fn a_refused_expired_redeem_keeps_siblings() {
        let g = Grants::default();
        let short = g.issue(answer("a", "yes"), 0, 100);
        let sibling = g.issue(cancel("a"), 0, ANSWER_TTL_MS);
        assert_eq!(g.redeem(&short, 100), Err(GrantError::Expired));
        assert_eq!(g.redeem(&sibling, 101), Ok(cancel("a")));
    }

    #[test]
    fn cap_of_256_evicts_the_oldest() {
        let g = Grants::default();
        let mut tokens = Vec::new();
        for i in 0..257 {
            tokens.push(g.issue(cancel(&format!("item-{i}")), 0, ANSWER_TTL_MS));
        }
        assert_eq!(g.redeem(&tokens[0], 1), Err(GrantError::Unknown));
        assert_eq!(g.redeem(&tokens[256], 1), Ok(cancel("item-256")));
        assert_eq!(g.redeem(&tokens[1], 1), Ok(cancel("item-1")));
    }

    #[test]
    fn issue_drops_expired_grants_before_the_cap_applies() {
        let g = Grants::default();
        let mut tokens = Vec::new();
        for i in 0..256 {
            tokens.push(g.issue(cancel(&format!("old-{i}")), 0, 100));
        }
        // Everything above has expired by now, so the new grant does not push out a live one.
        let live = g.issue(cancel("fresh"), 200, ANSWER_TTL_MS);
        assert_eq!(g.redeem(&live, 201), Ok(cancel("fresh")));
    }
}
