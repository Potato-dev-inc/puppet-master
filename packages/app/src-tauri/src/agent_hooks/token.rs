//! Per-pane hook tokens (§4.1): minted at spawn, held in memory as a SHA-256 only, revoked when the
//! pane exits or hooks turn off. A token can do exactly one thing: append an event to its own
//! pane's ledger. The raw token lives only in the child's environment.

use std::collections::{HashMap, VecDeque};

use base64::Engine;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Per pane: at most this many events in any second ...
pub const BURST_PER_SEC: usize = 20;
/// ... and this many in any minute.
pub const PER_MINUTE: usize = 600;

struct Entry {
    sha: [u8; 32],
    agent: String,
    cwd: String,
    recent: VecDeque<u64>,
}

static TOKENS: Lazy<Mutex<HashMap<String, Entry>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// A fresh token for `pane_id` (43 chars, base64url of 32 random bytes). Replaces any older one.
pub fn mint(pane_id: &str, agent: &str, cwd: &str) -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    TOKENS.lock().insert(
        pane_id.to_string(),
        Entry { sha: digest(&token), agent: agent.to_string(), cwd: cwd.to_string(), recent: VecDeque::new() },
    );
    token
}

pub fn revoke(pane_id: &str) -> bool {
    TOKENS.lock().remove(pane_id).is_some()
}

pub fn revoke_all() {
    TOKENS.lock().clear();
}

/// Revoke every token minted for an agent `off` says yes to.
pub fn revoke_where(off: impl Fn(&str) -> bool) {
    TOKENS.lock().retain(|_, entry| !off(&entry.agent));
}

pub fn is_live(pane_id: &str) -> bool {
    TOKENS.lock().contains_key(pane_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The token belongs to this pane and agent; the pane's project folder for path projection.
    Valid { cwd: String },
    /// Unknown pane, revoked token or wrong token: one answer, no hint which.
    Invalid,
    /// A real token, but minted for another agent (a nested CLI inherited the pane's env).
    AgentMismatch,
    RateLimited,
}

/// Verify `token` for `pane_id` and `agent` (constant time on the SHA-256) and count it against
/// the pane's rate limit.
pub fn check(pane_id: &str, token: &str, agent: &str, now_ms: u64) -> Check {
    let mut tokens = TOKENS.lock();
    let Some(entry) = tokens.get_mut(pane_id) else {
        return Check::Invalid;
    };
    if !bool::from(entry.sha.ct_eq(&digest(token))) {
        return Check::Invalid;
    }
    if entry.agent != agent {
        return Check::AgentMismatch;
    }
    while entry.recent.front().is_some_and(|t| now_ms.saturating_sub(*t) >= 60_000) {
        entry.recent.pop_front();
    }
    let last_second = entry.recent.iter().filter(|t| now_ms.saturating_sub(**t) < 1_000).count();
    if last_second >= BURST_PER_SEC || entry.recent.len() >= PER_MINUTE {
        return Check::RateLimited;
    }
    entry.recent.push_back(now_ms);
    Check::Valid { cwd: entry.cwd.clone() }
}
