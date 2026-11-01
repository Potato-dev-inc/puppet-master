//! One live writer per conversation.
//!
//! A conversation is one object in the agent's own session files. The chat (over ACP) and a terminal pane
//! can both show it, but only one may add to it at a time: two writers would interleave turns and corrupt
//! the agent's file. This ledger is the single place that says who has it, in the vocabulary of the MCP
//! leases (`mcp_sessions/leases.rs`): a holder is claimed, released, or taken over on purpose, and a second
//! claim by a different holder is refused. The ledger is keyed by conversation (a chat conversation has no
//! pane to hang a lease on).
//!
//! Hand-over is an atomic swap from one holder to the other, so there is no moment with two or with none.
//! The session itself is also held shut while the terminal has it (`AcpSession::hold`), so a caller that
//! forgets to ask the ledger still cannot start an adapter.

use std::collections::BTreeMap;

use parking_lot::Mutex;
use serde::Serialize;

/// Who may add to a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "writer", rename_all = "snake_case")]
pub enum Writer {
    /// The chat, over an adapter.
    Chat,
    /// A terminal pane. `pane_id` is `None` while the pane is being started.
    Terminal { pane_id: Option<String> },
}

impl Writer {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Writer::Terminal { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Writer::Chat => "the chat".into(),
            Writer::Terminal { pane_id: Some(pane) } => format!("the terminal pane {pane}"),
            Writer::Terminal { pane_id: None } => "a terminal that is starting".into(),
        }
    }
}

/// A claim was refused because somebody else has the conversation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{} already has this conversation", .held_by.describe())]
pub struct WriterConflict {
    pub held_by: Writer,
}

#[derive(Default)]
pub struct WriterLedger {
    holders: Mutex<BTreeMap<String, Writer>>,
}

impl WriterLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the conversation for `who`. The same holder claiming again is fine; any other holder is a conflict.
    pub fn claim(&self, conversation: &str, who: Writer) -> Result<(), WriterConflict> {
        let mut holders = self.holders.lock();
        match holders.get(conversation) {
            Some(held) if *held != who => Err(WriterConflict { held_by: held.clone() }),
            Some(_) => Ok(()),
            None => {
                holders.insert(conversation.to_string(), who);
                Ok(())
            }
        }
    }

    /// Gives the conversation from `from` to `to` in one step. Refused unless `from` is the holder now.
    pub fn swap(&self, conversation: &str, from: &Writer, to: Writer) -> Result<(), WriterConflict> {
        let mut holders = self.holders.lock();
        match holders.get(conversation) {
            Some(held) if held == from => {
                holders.insert(conversation.to_string(), to);
                Ok(())
            }
            Some(held) => Err(WriterConflict { held_by: held.clone() }),
            None => {
                // Nobody holds it: a swap from nobody is a plain claim.
                holders.insert(conversation.to_string(), to);
                Ok(())
            }
        }
    }

    /// Lets go, but only if `who` is the holder (a stale release cannot drop somebody else's claim).
    pub fn release(&self, conversation: &str, who: &Writer) -> bool {
        let mut holders = self.holders.lock();
        if holders.get(conversation) == Some(who) {
            holders.remove(conversation);
            true
        } else {
            false
        }
    }

    pub fn forget(&self, conversation: &str) {
        self.holders.lock().remove(conversation);
    }

    pub fn holder(&self, conversation: &str) -> Option<Writer> {
        self.holders.lock().get(conversation).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str) -> Writer {
        Writer::Terminal { pane_id: Some(id.into()) }
    }

    #[test]
    fn two_writers_are_refused() {
        let ledger = WriterLedger::new();
        ledger.claim("c1", Writer::Chat).unwrap();
        let err = ledger.claim("c1", pane("p1")).unwrap_err();
        assert_eq!(err.held_by, Writer::Chat);
        assert!(err.to_string().contains("the chat"));
        assert_eq!(ledger.holder("c1"), Some(Writer::Chat), "the refused claim changed nothing");
        ledger.claim("c1", Writer::Chat).unwrap();
    }

    #[test]
    fn a_second_terminal_is_refused_too() {
        let ledger = WriterLedger::new();
        ledger.claim("c1", pane("p1")).unwrap();
        assert_eq!(ledger.claim("c1", pane("p2")).unwrap_err().held_by, pane("p1"));
        assert_eq!(ledger.claim("c1", Writer::Chat).unwrap_err().held_by, pane("p1"));
    }

    #[test]
    fn a_swap_moves_the_claim_in_one_step_and_only_from_the_holder() {
        let ledger = WriterLedger::new();
        ledger.claim("c1", Writer::Chat).unwrap();
        assert!(ledger.swap("c1", &pane("nope"), Writer::Chat).is_err());
        ledger.swap("c1", &Writer::Chat, Writer::Terminal { pane_id: None }).unwrap();
        assert!(ledger.holder("c1").unwrap().is_terminal());
        ledger.swap("c1", &Writer::Terminal { pane_id: None }, pane("p1")).unwrap();
        assert_eq!(ledger.holder("c1"), Some(pane("p1")));
    }

    #[test]
    fn a_stale_release_cannot_drop_somebody_elses_claim() {
        let ledger = WriterLedger::new();
        ledger.claim("c1", pane("p1")).unwrap();
        assert!(!ledger.release("c1", &Writer::Chat));
        assert!(!ledger.release("c1", &pane("old")));
        assert!(ledger.release("c1", &pane("p1")));
        assert_eq!(ledger.holder("c1"), None);
    }

    #[test]
    fn conversations_are_independent() {
        let ledger = WriterLedger::new();
        ledger.claim("c1", Writer::Chat).unwrap();
        ledger.claim("c2", pane("p1")).unwrap();
        assert_eq!(ledger.holder("c1"), Some(Writer::Chat));
        assert_eq!(ledger.holder("c2"), Some(pane("p1")));
    }
}
