//! Text Your Agents: the one-time pairing code and the desktop "Is this you?" step.
//!
//! Pairing needs both: the right code sent to the bot, and a click on the PC. A code is 8 characters
//! (40 random bits, Crockford base32), lasts 10 minutes, and is burned after 5 wrong tries. The code
//! is kept in memory only: a restart forgets it.

use super::config::PairedChat;
use rand::rngs::OsRng;
use rand::RngCore;
use subtle::ConstantTimeEq;

/// Crockford base32 (no I, L, O or U), so the code is easy to read and type.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CODE_LEN: usize = 8;
/// A code works for 10 minutes after it is made.
pub const CODE_TTL_MS: u64 = 600_000;
/// The fifth wrong code burns the code for everyone.
pub const MAX_WRONG: u8 = 5;

/// The Telegram chat that sent the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub chat_id: i64,
    pub user_id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
}

/// What one offered code did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStep {
    /// No code was ever made, or the last one was used up.
    NoCode,
    /// Wrong code. `left` tries remain before the code burns.
    Wrong { left: u8 },
    /// The code was burned (5 wrong tries, or "Not me"). Make a new one on the PC.
    Burned,
    /// The code is older than 10 minutes.
    Expired,
    /// The code is right (or another chat is already waiting). The PC must say yes or no.
    AwaitingDesktop,
}

/// The code slot. Not `Debug`, so the code never ends up in a log line.
#[derive(Default)]
enum Slot {
    #[default]
    None,
    Live {
        code: String,
        expires_ms: u64,
        wrong: u8,
    },
    Burned,
    Expired,
}

#[derive(Default)]
pub struct Pairing {
    slot: Slot,
    pending: Option<Who>,
}

impl Pairing {
    /// Makes a fresh code. It replaces any old code and drops a chat that was waiting for the PC's
    /// click. Returns the code and the time (ms) when it stops working.
    pub fn new_code(&mut self, now_ms: u64) -> (String, u64) {
        let code = random_code();
        let expires_ms = now_ms.saturating_add(CODE_TTL_MS);
        self.slot = Slot::Live {
            code: code.clone(),
            expires_ms,
            wrong: 0,
        };
        self.pending = None;
        (code, expires_ms)
    }

    /// Checks a code the person sent with `/start`. The compare is constant time, and the code is
    /// read without regard to case, spaces and dashes. While a chat is waiting for the PC, nothing
    /// is replaced and no try is counted.
    pub fn offer(&mut self, offered: &str, who: Who, now_ms: u64) -> PairStep {
        if self.pending.is_some() {
            return PairStep::AwaitingDesktop;
        }
        let expired = match &self.slot {
            Slot::None => return PairStep::NoCode,
            Slot::Burned => return PairStep::Burned,
            Slot::Expired => return PairStep::Expired,
            Slot::Live { expires_ms, .. } => now_ms >= *expires_ms,
        };
        if expired {
            self.slot = Slot::Expired;
            return PairStep::Expired;
        }
        let (code, wrong) = match &mut self.slot {
            Slot::Live { code, wrong, .. } => (code, wrong),
            _ => return PairStep::NoCode,
        };
        if same_code(&normalize(offered), code.as_str()) {
            self.pending = Some(who);
            return PairStep::AwaitingDesktop;
        }
        *wrong += 1;
        if *wrong >= MAX_WRONG {
            self.slot = Slot::Burned;
            return PairStep::Burned;
        }
        PairStep::Wrong {
            left: MAX_WRONG - *wrong,
        }
    }

    /// The chat waiting for the PC's answer, if any.
    pub fn pending(&self) -> Option<&Who> {
        self.pending.as_ref()
    }

    /// The PC's answer. `true` pairs the waiting chat and uses the code up. `false` (or "Not me")
    /// burns the code. Returns `None` when nothing was waiting, or when the answer was no.
    pub fn decide(&mut self, accept: bool, now_ms: u64) -> Option<PairedChat> {
        let who = self.pending.take()?;
        if !accept {
            self.slot = Slot::Burned;
            return None;
        }
        self.slot = Slot::None;
        Some(PairedChat {
            chat_id: who.chat_id,
            user_id: who.user_id,
            username: who.username,
            first_name: who.first_name,
            paired_at_ms: now_ms,
        })
    }
}

/// Upper case, with spaces and dashes taken out.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn same_code(a: &str, b: &str) -> bool {
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

/// 8 random bytes, each reduced to 5 bits. 256 is a multiple of 32, so every letter is equally likely.
fn random_code() -> String {
    let mut bytes = [0u8; CODE_LEN];
    OsRng.fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|b| ALPHABET[(b & 31) as usize] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;

    fn who(user: i64) -> Who {
        Who {
            chat_id: user,
            user_id: user,
            username: Some("anna_k".to_string()),
            first_name: Some("Anna".to_string()),
        }
    }

    /// A code that is different from `code` in its first character.
    fn other(code: &str) -> String {
        let first = if code.starts_with('0') { "1" } else { "0" };
        format!("{first}{}", &code[1..])
    }

    #[test]
    fn code_is_eight_characters_from_the_alphabet() {
        let mut p = Pairing::default();
        for _ in 0..50 {
            let (code, _) = p.new_code(NOW);
            assert_eq!(code.len(), 8);
            assert!(code.bytes().all(|b| ALPHABET.contains(&b)), "bad code {code}");
            assert!(!code.contains(['I', 'L', 'O', 'U']), "bad code {code}");
        }
    }

    #[test]
    fn codes_differ_between_makes() {
        let mut p = Pairing::default();
        let (a, _) = p.new_code(NOW);
        let (b, _) = p.new_code(NOW);
        assert_ne!(a, b);
    }

    #[test]
    fn code_expires_ten_minutes_after_it_is_made() {
        let mut p = Pairing::default();
        let (_, expires) = p.new_code(NOW);
        assert_eq!(expires, NOW + 600_000);
    }

    #[test]
    fn no_code_made_means_no_code() {
        let mut p = Pairing::default();
        assert_eq!(p.offer("PM7K2QX9", who(1), NOW), PairStep::NoCode);
        assert!(p.pending().is_none());
    }

    #[test]
    fn right_code_waits_for_the_desktop_and_ignores_case_spaces_and_dashes() {
        let mut p = Pairing::default();
        let (code, _) = p.new_code(NOW);
        let typed = format!("{} {}-{}", &code[..2], &code[2..4].to_lowercase(), &code[4..]);
        assert_eq!(p.offer(&typed.to_lowercase(), who(7), NOW), PairStep::AwaitingDesktop);
        assert_eq!(p.pending().map(|w| w.user_id), Some(7));
    }

    #[test]
    fn wrong_codes_count_down_and_the_fifth_burns_the_code() {
        let mut p = Pairing::default();
        let (code, _) = p.new_code(NOW);
        let wrong = other(&code);
        assert_eq!(p.offer(&wrong, who(1), NOW), PairStep::Wrong { left: 4 });
        assert_eq!(p.offer(&wrong, who(1), NOW), PairStep::Wrong { left: 3 });
        assert_eq!(p.offer(&wrong, who(1), NOW), PairStep::Wrong { left: 2 });
        assert_eq!(p.offer(&wrong, who(1), NOW), PairStep::Wrong { left: 1 });
        assert_eq!(p.offer(&wrong, who(1), NOW), PairStep::Burned);
        // Burned for everyone: even the right code no longer works.
        assert_eq!(p.offer(&code, who(2), NOW), PairStep::Burned);
        assert!(p.pending().is_none());
    }

    #[test]
    fn expired_code_is_refused() {
        let mut p = Pairing::default();
        let (code, expires) = p.new_code(NOW);
        assert_eq!(p.offer(&code, who(1), expires), PairStep::Expired);
        assert_eq!(p.offer(&code, who(1), NOW), PairStep::Expired);
        assert!(p.pending().is_none());
    }

    #[test]
    fn code_still_works_one_ms_before_expiry() {
        let mut p = Pairing::default();
        let (code, expires) = p.new_code(NOW);
        assert_eq!(p.offer(&code, who(1), expires - 1), PairStep::AwaitingDesktop);
    }

    #[test]
    fn second_chat_while_one_waits_does_not_replace_the_first() {
        let mut p = Pairing::default();
        let (code, _) = p.new_code(NOW);
        assert_eq!(p.offer(&code, who(1), NOW), PairStep::AwaitingDesktop);
        assert_eq!(p.offer(&code, who(2), NOW), PairStep::AwaitingDesktop);
        assert_eq!(p.offer(&other(&code), who(3), NOW), PairStep::AwaitingDesktop);
        assert_eq!(p.pending().map(|w| w.user_id), Some(1));
    }

    #[test]
    fn decide_no_burns_the_code() {
        let mut p = Pairing::default();
        let (code, _) = p.new_code(NOW);
        assert_eq!(p.offer(&code, who(1), NOW), PairStep::AwaitingDesktop);
        assert!(p.decide(false, NOW).is_none());
        assert!(p.pending().is_none());
        assert_eq!(p.offer(&code, who(1), NOW), PairStep::Burned);
    }

    #[test]
    fn decide_yes_returns_the_paired_chat_and_uses_the_code_up() {
        let mut p = Pairing::default();
        let (code, _) = p.new_code(NOW);
        assert_eq!(p.offer(&code, who(42), NOW), PairStep::AwaitingDesktop);
        let paired = p.decide(true, NOW + 5).expect("paired");
        assert_eq!(paired.chat_id, 42);
        assert_eq!(paired.user_id, 42);
        assert_eq!(paired.username.as_deref(), Some("anna_k"));
        assert_eq!(paired.first_name.as_deref(), Some("Anna"));
        assert_eq!(paired.paired_at_ms, NOW + 5);
        // Single use: the same code means nothing now.
        assert_eq!(p.offer(&code, who(42), NOW), PairStep::NoCode);
    }

    #[test]
    fn decide_without_a_waiting_chat_does_nothing() {
        let mut p = Pairing::default();
        assert!(p.decide(true, NOW).is_none());
        assert!(p.decide(false, NOW).is_none());
        let (code, _) = p.new_code(NOW);
        assert_eq!(p.offer(&code, who(1), NOW), PairStep::AwaitingDesktop);
    }

    #[test]
    fn new_code_drops_the_waiting_chat() {
        let mut p = Pairing::default();
        let (code, _) = p.new_code(NOW);
        assert_eq!(p.offer(&code, who(1), NOW), PairStep::AwaitingDesktop);
        let (fresh, _) = p.new_code(NOW + 1);
        assert!(p.pending().is_none());
        assert_eq!(p.offer(&fresh, who(2), NOW + 1), PairStep::AwaitingDesktop);
    }
}
