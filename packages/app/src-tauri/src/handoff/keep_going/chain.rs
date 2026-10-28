//! Keep going: the hand-over chain helpers (SPEC n32 section 3.4). Pure: they read the records the
//! store holds (newest first) and a clock passed in by the caller. No I/O.
//!
//! A chain is one task moved from agent to agent: each hand-over has a `chain_id` and a `hop`.

use super::model::{Back, Handover, Side, State, Target};

/// A record whose reset time is unknown counts as limited for this long after it was written.
const UNKNOWN_RESET_MS: u64 = 60 * 60 * 1_000;

fn side_is(side: &Side, target: &Target) -> bool {
    side.agent_id == target.agent_id && side.account == target.account
}

/// Chain id and next hop for a hand-over whose old side is `source_pane`.
///
/// A pane that is the new side of a record continues that record's chain, one hop further. Any
/// other pane starts a new chain: the chain id is None and the caller uses the new record's id.
/// Failed records count too, so a retry or a failed start never lowers the hop.
pub fn hop_for(records: &[Handover], source_pane: &str) -> (Option<String>, u8) {
    if source_pane.is_empty() {
        return (None, 1);
    }
    match records.iter().find(|r| r.to.pane_id == source_pane) {
        Some(record) => (Some(record.chain_id.clone()), record.hop.saturating_add(1)),
        None => (None, 1),
    }
}

/// Whether `target` is still out of usage according to the records (the old side of a record).
///
/// - `Some(Some(reset))`: a record's reset time is still ahead. The latest one is returned.
/// - `Some(None)`: no reset time is known, and the record is less than an hour old.
/// - `None`: no record says the target is limited now.
pub fn limited_by_records(
    records: &[Handover],
    target: &Target,
    now_ms: u64,
) -> Option<Option<u64>> {
    let mut known: Option<u64> = None;
    let mut unknown = false;
    for record in records.iter().filter(|r| side_is(&r.from, target)) {
        match record.from_reset_at_ms {
            Some(reset) if reset > now_ms => {
                known = Some(known.map_or(reset, |latest| latest.max(reset)));
            }
            Some(_) => {}
            None if record.at_ms.saturating_add(UNKNOWN_RESET_MS) > now_ms => unknown = true,
            None => {}
        }
    }
    match (known, unknown) {
        (Some(reset), _) => Some(Some(reset)),
        (None, true) => Some(None),
        (None, false) => None,
    }
}

/// The newest record that moved a task off `pane_id` and did not fail. The pane is then busy
/// with a hand-over, so a second one is refused. A failed record is ignored (the person can retry).
pub fn already_from<'a>(records: &'a [Handover], pane_id: &str) -> Option<&'a Handover> {
    if pane_id.is_empty() {
        return None;
    }
    records
        .iter()
        .find(|r| r.from.pane_id == pane_id && !matches!(r.state, State::Failed { .. }))
}

/// Ids of the records whose come-back card is due (`Back::AtReset` with `due_ms` reached).
pub fn come_backs_due(records: &[Handover], now_ms: u64) -> Vec<String> {
    records
        .iter()
        .filter(|r| matches!(r.back, Back::AtReset { due_ms, .. } if due_ms <= now_ms))
        .map(|r| r.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::keep_going::model::{BriefMeta, Reason};

    fn side(pane: &str, agent: &str, account: Option<&str>) -> Side {
        Side {
            pane_id: pane.into(),
            agent_id: agent.into(),
            account: account.map(str::to_string),
            label: agent.into(),
            handle: None,
            operation_id: None,
        }
    }

    fn target(agent: &str, account: Option<&str>) -> Target {
        Target {
            agent_id: agent.into(),
            account: account.map(str::to_string),
        }
    }

    fn record(id: &str, chain: &str, hop: u8, at_ms: u64, from: Side, to: Side) -> Handover {
        Handover {
            id: id.into(),
            chain_id: chain.into(),
            hop,
            at_ms,
            reason: Reason::UsageLimit,
            from,
            to,
            project: "project-a".into(),
            folder: r"C:\Users\you\work\app".into(),
            worktree_handle: None,
            from_reset_at_ms: None,
            goal: "Fix the cart total".into(),
            back: Back::Off,
            brief: BriefMeta {
                file: String::new(),
                sha8: String::new(),
                bytes: 0,
                trimmed: vec![],
                turns: 0,
            },
            state: State::Running,
        }
    }

    const CHAIN: &str = "kg-000000000001";

    #[test]
    fn a_fresh_pane_starts_a_new_chain_at_hop_one() {
        assert_eq!(hop_for(&[], "pane-a"), (None, 1));
        assert_eq!(hop_for(&[], ""), (None, 1));
    }

    #[test]
    fn the_new_side_of_a_hand_over_continues_the_chain() {
        let first = record(
            "kg-000000000001",
            CHAIN,
            1,
            10,
            side("pane-a", "fake_a", None),
            side("pane-b", "fake_b", None),
        );
        let records = vec![first];
        assert_eq!(hop_for(&records, "pane-b"), (Some(CHAIN.to_string()), 2));
        assert_eq!(
            hop_for(&records, "pane-a"),
            (None, 1),
            "the old side starts nothing new"
        );
    }

    #[test]
    fn a_to_b_then_b_as_source_is_hop_two_of_the_same_chain() {
        let first = record(
            "kg-000000000001",
            CHAIN,
            1,
            10,
            side("pane-a", "fake_a", None),
            side("pane-b", "fake_b", None),
        );
        let (chain, hop) = hop_for(std::slice::from_ref(&first), "pane-b");
        assert_eq!(chain.as_deref(), Some(CHAIN));
        assert_eq!(hop, 2);
        // The second hand-over is stored as hop 2 of the same chain, then A is the new side again.
        let second = record(
            "kg-000000000002",
            CHAIN,
            hop,
            20,
            side("pane-b", "fake_b", None),
            side("pane-a", "fake_a", None),
        );
        let records = vec![second, first];
        assert_eq!(hop_for(&records, "pane-a"), (Some(CHAIN.to_string()), 3));
    }

    #[test]
    fn a_failed_record_still_counts_for_the_hop() {
        let mut failed = record(
            "kg-000000000001",
            CHAIN,
            2,
            10,
            side("pane-a", "fake_a", None),
            side("pane-b", "fake_b", None),
        );
        failed.state = State::Failed {
            code: "KEEP_GOING_START_FAILED".into(),
            message: "m".into(),
        };
        assert_eq!(hop_for(&[failed], "pane-b"), (Some(CHAIN.to_string()), 3));
    }

    #[test]
    fn limited_by_records_uses_a_known_future_reset() {
        let mut rec = record(
            "kg-000000000001",
            CHAIN,
            1,
            10,
            side("pane-a", "fake_a", None),
            side("pane-b", "fake_b", None),
        );
        rec.from_reset_at_ms = Some(5_000);
        let records = vec![rec];
        assert_eq!(
            limited_by_records(&records, &target("fake_a", None), 4_000),
            Some(Some(5_000))
        );
        assert_eq!(
            limited_by_records(&records, &target("fake_a", None), 5_000),
            None,
            "reset reached"
        );
        assert_eq!(
            limited_by_records(&records, &target("fake_b", None), 4_000),
            None,
            "other agent"
        );
        assert_eq!(
            limited_by_records(&records, &target("fake_a", Some("work")), 4_000),
            None,
            "other account"
        );
    }

    #[test]
    fn limited_by_records_keeps_the_latest_known_reset() {
        let mut early = record(
            "kg-000000000001",
            CHAIN,
            1,
            10,
            side("pane-a", "fake_a", None),
            side("pane-b", "fake_b", None),
        );
        early.from_reset_at_ms = Some(5_000);
        let mut late = record(
            "kg-000000000002",
            CHAIN,
            1,
            20,
            side("pane-a", "fake_a", None),
            side("pane-c", "fake_c", None),
        );
        late.from_reset_at_ms = Some(9_000);
        assert_eq!(
            limited_by_records(&[early, late], &target("fake_a", None), 4_000),
            Some(Some(9_000))
        );
    }

    #[test]
    fn unknown_reset_counts_one_hour() {
        let rec = record(
            "kg-000000000001",
            CHAIN,
            1,
            1_000,
            side("pane-a", "fake_a", None),
            side("pane-b", "fake_b", None),
        );
        let records = vec![rec];
        let hour = 60 * 60 * 1_000;
        assert_eq!(
            limited_by_records(&records, &target("fake_a", None), 1_000 + hour - 1),
            Some(None)
        );
        assert_eq!(
            limited_by_records(&records, &target("fake_a", None), 1_000 + hour),
            None
        );
    }

    #[test]
    fn already_from_finds_the_newest_live_hand_over() {
        let older = record(
            "kg-000000000001",
            CHAIN,
            1,
            10,
            side("pane-a", "fake_a", None),
            side("pane-b", "fake_b", None),
        );
        let newer = record(
            "kg-000000000002",
            "kg-000000000002",
            1,
            20,
            side("pane-a", "fake_a", None),
            side("pane-c", "fake_c", None),
        );
        let records = vec![newer, older];
        assert_eq!(
            already_from(&records, "pane-a").map(|r| r.id.as_str()),
            Some("kg-000000000002")
        );
        assert_eq!(already_from(&records, "pane-x"), None);
        assert_eq!(already_from(&records, ""), None);
    }

    #[test]
    fn already_from_ignores_failed_records() {
        let mut failed = record(
            "kg-000000000002",
            "kg-000000000002",
            1,
            20,
            side("pane-a", "fake_a", None),
            side("pane-c", "fake_c", None),
        );
        failed.state = State::Failed {
            code: "KEEP_GOING_START_FAILED".into(),
            message: "m".into(),
        };
        let running = record(
            "kg-000000000001",
            CHAIN,
            1,
            10,
            side("pane-a", "fake_a", None),
            side("pane-b", "fake_b", None),
        );
        let records = vec![failed.clone()];
        assert_eq!(
            already_from(&records, "pane-a"),
            None,
            "only a failed record: the pane is free"
        );
        let records = vec![failed, running];
        assert_eq!(
            already_from(&records, "pane-a").map(|r| r.id.as_str()),
            Some("kg-000000000001")
        );
    }

    #[test]
    fn come_backs_due_lists_only_reached_resets() {
        let mut due = record(
            "kg-000000000001",
            CHAIN,
            1,
            10,
            side("pane-a", "fake_a", None),
            side("pane-b", "fake_b", None),
        );
        due.back = Back::AtReset {
            due_ms: 100,
            tries: 0,
        };
        let mut later = record(
            "kg-000000000002",
            "kg-000000000002",
            1,
            20,
            side("pane-c", "fake_c", None),
            side("pane-d", "fake_d", None),
        );
        later.back = Back::AtReset {
            due_ms: 101,
            tries: 0,
        };
        let mut asked = record(
            "kg-000000000003",
            "kg-000000000003",
            1,
            30,
            side("pane-e", "fake_e", None),
            side("pane-f", "fake_f", None),
        );
        asked.back = Back::Asked;
        let off = record(
            "kg-000000000004",
            "kg-000000000004",
            1,
            40,
            side("pane-g", "fake_g", None),
            side("pane-h", "fake_h", None),
        );
        let records = vec![due, later, asked, off];
        assert_eq!(
            come_backs_due(&records, 100),
            vec!["kg-000000000001".to_string()]
        );
        assert!(come_backs_due(&records, 99).is_empty());
    }
}
