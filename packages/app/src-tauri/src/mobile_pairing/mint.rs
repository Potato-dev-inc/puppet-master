//! Making a pairing code, and writing it down once.
//!
//! A pairing code is made from two places: Settings > Phone (`create_mobile_pairing_session`) and a
//! program on this computer (`POST /pair/invite`, used by `puppet-master pair-link`). There is one
//! pending code at a time, so a new one replaces the one a person may still be showing as a QR code.
//! Both places go through [`mint`], which makes the code and records exactly one line in the app's
//! log, saying so when it replaced a code that was still good.

use super::{Caps, MobilePairingStore, PairingSession};
use crate::audit::{self, Kind, Who};
use parking_lot::Mutex;
use serde_json::json;

/// Who asked for the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maker {
    /// The person, in Settings.
    Settings,
    /// A program on this computer, through the bridge (`pair-link`).
    Program,
}

/// Make a code for a phone that gets `caps`, and record it once. The code itself is never logged.
pub fn mint(store: &Mutex<MobilePairingStore>, bridge_url: String, caps: Caps, maker: Maker) -> PairingSession {
    let (session, replaced) = {
        let mut guard = store.lock();
        let replaced = guard.has_live_pending_code();
        (guard.create_pairing_session_with_caps(bridge_url, caps), replaced)
    };
    let (who, said, via) = match maker {
        Maker::Settings => (Who::Desktop, "You made a pairing code for a phone", "settings"),
        Maker::Program => (Who::Local, "A pairing link was made from a program on this computer", "pair-link"),
    };
    let summary = if replaced { format!("{said} (replaced the previous code)") } else { said.to_string() };
    audit::record(Kind::Pairing, who, &summary, json!({ "via": via, "caps": caps.list(), "replaced": replaced }));
    session
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The log is one file for the whole test process: these tests take turns reading it.
    static TURN: Mutex<()> = Mutex::new(());

    fn store() -> Mutex<MobilePairingStore> {
        let dir = std::env::temp_dir().join(format!("pm-mint-{}-{}", std::process::id(), super::super::random_pairing_code()));
        std::fs::create_dir_all(&dir).unwrap();
        Mutex::new(MobilePairingStore::load_or_create(dir.join("pairing.json")).unwrap())
    }

    /// The log lines this test wrote (the log is shared by the whole test process; other tests do
    /// not go through `mint`, so every `pairing` line that says `via` pair-link / settings is ours).
    fn lines_since(seq: u64) -> Vec<(String, serde_json::Value)> {
        let mut lines: Vec<_> = audit::global()
            .recent(200)
            .into_iter()
            .filter(|e| e.seq > seq && e.kind == "pairing" && e.detail.get("replaced").is_some())
            .map(|e| (e.summary, e.detail))
            .collect();
        lines.reverse();
        lines
    }

    fn last_seq() -> u64 {
        audit::global().recent(1).first().map_or(0, |e| e.seq)
    }

    /// Round 13 #4: the verifier's `/pair/invite` calls replaced the pending code the desktop was
    /// showing, and the log never said so.
    #[test]
    fn making_a_code_is_logged_once_and_says_when_it_replaced_a_good_one() {
        let _turn = TURN.lock();
        let store = store();
        let start = last_seq();

        let first = mint(&store, "https://pm.example/bridge".into(), Caps::all(), Maker::Settings);
        let second = mint(&store, "https://pm.example/bridge".into(), Caps::all(), Maker::Program);
        assert_ne!(first.pairing_code, second.pairing_code, "a new code");

        let lines = lines_since(start);
        assert_eq!(lines.len(), 2, "one line per code made, not more: {lines:#?}");
        assert_eq!(lines[0].0, "You made a pairing code for a phone", "the first replaced nothing");
        assert_eq!(lines[0].1["replaced"], false);
        assert_eq!(lines[1].0, "A pairing link was made from a program on this computer (replaced the previous code)");
        assert_eq!(lines[1].1["replaced"], true);
        assert_eq!(lines[1].1["via"], "pair-link");
        // the old code no longer works, which is what the line tells the person
        assert!(store.lock().lookup_pairing_session(&first.pairing_code).is_err());
        // the codes themselves are never in the log
        let text = serde_json::to_string(&lines).unwrap();
        assert!(!text.contains(&first.pairing_code) && !text.contains(&second.pairing_code));
    }

    #[test]
    fn a_used_code_is_not_called_replaced() {
        let _turn = TURN.lock();
        let store = store();
        let start = last_seq();
        let session = mint(&store, "https://pm.example/bridge".into(), Caps::all(), Maker::Settings);
        store
            .lock()
            .pair_device(super::super::PairRequestBody {
                pairing_code: session.pairing_code,
                device_name: "phone".into(),
                device_public_key: session.server_public_key,
            })
            .unwrap();
        mint(&store, "https://pm.example/bridge".into(), Caps::all(), Maker::Settings);
        let lines = lines_since(start);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].0, "You made a pairing code for a phone", "the first code was used up: nothing to replace");
    }
}
