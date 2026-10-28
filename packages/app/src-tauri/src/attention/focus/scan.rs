//! Finds the blocking cards that appeared during a Focus session (SPEC n33 3.4 B). Pure: the caller passes the
//! attention facts in, so every rule is tested with fixed times and no pane.
//!
//! A card becomes a waiter when it is open, in the blocking lane, raised at or after the session start, and its key
//! is not known yet. Its tier is the card's own tier (SPEC 3.2): `Quiet` or `Through`. Ready-lane kinds and the Hold
//! kinds are never waiters. A waiter keeps only the key (never shown), the tier and the time. No card text.

use super::model::{Settings, Tier, Waiter, WAITERS_CAP};
use crate::attention::model::{lane_of, Item, Kind, Lane, Level, Snapshot, State};
use std::collections::HashSet;

/// What `scan` needs from one attention item. Built from `attention::Item`, so tests need no pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFact {
    pub key: String,
    pub kind: Kind,
    pub risk: Option<Level>,
    pub since_ms: Option<u64>,
    pub open: bool,
}

/// The facts of every item in a snapshot, in snapshot order.
pub fn facts_from(snapshot: &Snapshot) -> Vec<ItemFact> {
    snapshot.items.iter().map(fact_of).collect()
}

fn fact_of(item: &Item) -> ItemFact {
    ItemFact {
        key: item.key.clone(),
        kind: item.kind,
        risk: item.risk.as_ref().map(|risk| risk.level),
        since_ms: item.since_ms,
        open: item.state == State::Open,
    }
}

/// The tier of a blocking card (SPEC 3.2, attention table). `None` for the Hold kinds (`Review`, `Done`).
/// This mirrors `policy::tier_of_attention` (FT-4); keep the two in step until FT-4 lands and this can call it.
fn blocking_tier(kind: Kind, risk: Option<Level>) -> Option<Tier> {
    match kind {
        Kind::Consent | Kind::Tamper | Kind::Budget | Kind::Stuck | Kind::Answer => Some(Tier::Through),
        Kind::Approval if risk == Some(Level::High) => Some(Tier::Through),
        Kind::Approval | Kind::TrustGate | Kind::Question | Kind::Limit | Kind::Setup | Kind::Error => {
            Some(Tier::Quiet)
        }
        Kind::Review | Kind::Done => None,
    }
}

/// The new waiters: open blocking cards raised at or after `started_ms` whose key is not in `known`.
/// A key counts once, even when it shows up twice in one batch. The list stops at `WAITERS_CAP` in total, so
/// `known.len()` counts against it. A card with no raise time is skipped: it cannot be placed in the session.
///
/// `settings` does not change who is a waiter. The gate (`policy::decide`) decides what is sent; this list decides
/// what is counted and reminded. The argument stays so the engine call matches SPEC 3.4 B.
pub fn scan(items: &[ItemFact], started_ms: u64, known: &[Waiter], _settings: &Settings) -> Vec<Waiter> {
    let room = WAITERS_CAP.saturating_sub(known.len());
    let mut seen: HashSet<&str> = known.iter().map(|waiter| waiter.key.as_str()).collect();
    let mut found = Vec::new();
    for item in items {
        if found.len() >= room {
            break;
        }
        if !item.open || lane_of(item.kind) != Lane::Blocking {
            continue;
        }
        let Some(tier) = blocking_tier(item.kind, item.risk) else { continue };
        let Some(since) = item.since_ms else { continue };
        if since < started_ms || !seen.insert(item.key.as_str()) {
            continue;
        }
        found.push(Waiter { key: item.key.clone(), tier, first_seen_ms: since, nudged: false });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::model::Counts;

    const T0: u64 = 1_791_979_200_000;
    const MIN: u64 = 60_000;

    fn fact(key: &str, kind: Kind, risk: Option<Level>, since: Option<u64>) -> ItemFact {
        ItemFact { key: key.into(), kind, risk, since_ms: since, open: true }
    }

    fn waiter(key: &str) -> Waiter {
        Waiter { key: key.into(), tier: Tier::Quiet, first_seen_ms: T0, nudged: false }
    }

    fn keys(found: &[Waiter]) -> Vec<&str> {
        found.iter().map(|w| w.key.as_str()).collect()
    }

    fn settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn a_card_from_before_the_start_is_not_counted() {
        let items = [
            fact("approval:old", Kind::Approval, None, Some(T0 - 1)),
            fact("approval:at-start", Kind::Approval, None, Some(T0)),
            fact("approval:later", Kind::Approval, None, Some(T0 + MIN)),
        ];
        let found = scan(&items, T0, &[], &settings());
        assert_eq!(keys(&found), vec!["approval:at-start", "approval:later"]);
        assert_eq!(found[0].first_seen_ms, T0, "a card raised exactly at the start counts");
    }

    #[test]
    fn a_card_with_no_raise_time_is_skipped() {
        let items = [fact("question:undated", Kind::Question, None, None)];
        assert!(scan(&items, T0, &[], &settings()).is_empty());
    }

    #[test]
    fn a_re_raised_same_key_counts_once() {
        let items = [fact("approval:x", Kind::Approval, None, Some(T0 + MIN))];
        let known = [waiter("approval:x")];
        assert!(scan(&items, T0, &known, &settings()).is_empty(), "a key already known is not added again");

        let twice = [
            fact("question:y", Kind::Question, None, Some(T0 + MIN)),
            fact("question:y", Kind::Question, None, Some(T0 + 2 * MIN)),
        ];
        let found = scan(&twice, T0, &[], &settings());
        assert_eq!(keys(&found), vec!["question:y"], "the same key twice in one batch counts once");
        assert_eq!(found[0].first_seen_ms, T0 + MIN, "the first raise is kept");
    }

    #[test]
    fn tiers_follow_the_attention_table() {
        let items = [
            fact("consent", Kind::Consent, None, Some(T0)),
            fact("tamper", Kind::Tamper, None, Some(T0)),
            fact("budget", Kind::Budget, None, Some(T0)),
            fact("stuck", Kind::Stuck, None, Some(T0)),
            fact("approval-high", Kind::Approval, Some(Level::High), Some(T0)),
            fact("approval-medium", Kind::Approval, Some(Level::Medium), Some(T0)),
            fact("approval-low", Kind::Approval, Some(Level::Low), Some(T0)),
            fact("approval-unrated", Kind::Approval, None, Some(T0)),
            fact("trust", Kind::TrustGate, None, Some(T0)),
            fact("question", Kind::Question, None, Some(T0)),
            fact("limit", Kind::Limit, None, Some(T0)),
            fact("setup", Kind::Setup, None, Some(T0)),
            fact("error", Kind::Error, None, Some(T0)),
        ];
        let found = scan(&items, T0, &[], &settings());
        let tier_of = |key: &str| found.iter().find(|w| w.key == key).map(|w| w.tier);
        assert_eq!(tier_of("consent"), Some(Tier::Through));
        assert_eq!(tier_of("tamper"), Some(Tier::Through));
        assert_eq!(tier_of("budget"), Some(Tier::Through));
        assert_eq!(tier_of("stuck"), Some(Tier::Through));
        assert_eq!(tier_of("approval-high"), Some(Tier::Through));
        assert_eq!(tier_of("approval-medium"), Some(Tier::Quiet));
        assert_eq!(tier_of("approval-low"), Some(Tier::Quiet));
        assert_eq!(tier_of("approval-unrated"), Some(Tier::Quiet));
        assert_eq!(tier_of("trust"), Some(Tier::Quiet));
        assert_eq!(tier_of("question"), Some(Tier::Quiet));
        assert_eq!(tier_of("limit"), Some(Tier::Quiet));
        assert_eq!(tier_of("setup"), Some(Tier::Quiet));
        assert_eq!(tier_of("error"), Some(Tier::Quiet));
        assert_eq!(found.len(), items.len(), "every blocking card above is a waiter");
    }

    #[test]
    fn ready_lane_and_hold_kinds_are_never_waiters() {
        let items = [
            fact("review", Kind::Review, None, Some(T0)),
            fact("done", Kind::Done, None, Some(T0)),
            fact("answer", Kind::Answer, None, Some(T0)),
        ];
        assert!(scan(&items, T0, &[], &settings()).is_empty());
    }

    #[test]
    fn a_closed_card_is_not_counted() {
        let mut closed = fact("approval:gone", Kind::Approval, None, Some(T0));
        closed.open = false;
        assert!(scan(&[closed], T0, &[], &settings()).is_empty());
    }

    #[test]
    fn the_list_stops_at_the_cap_and_known_waiters_count_against_it() {
        let items: Vec<ItemFact> =
            (0..250).map(|i| fact(&format!("question:{i}"), Kind::Question, None, Some(T0))).collect();
        assert_eq!(scan(&items, T0, &[], &settings()).len(), WAITERS_CAP);

        let known: Vec<Waiter> = (0..150).map(|i| waiter(&format!("old:{i}"))).collect();
        assert_eq!(scan(&items, T0, &known, &settings()).len(), WAITERS_CAP - 150);

        let full: Vec<Waiter> = (0..WAITERS_CAP).map(|i| waiter(&format!("old:{i}"))).collect();
        assert!(scan(&items, T0, &full, &settings()).is_empty());
    }

    #[test]
    fn settings_do_not_change_who_is_a_waiter() {
        let items = [
            fact("approval-high", Kind::Approval, Some(Level::High), Some(T0)),
            fact("question", Kind::Question, None, Some(T0)),
        ];
        let on = scan(&items, T0, &[], &Settings { default_minutes: 45, quiet_blocking: true });
        let off = scan(&items, T0, &[], &Settings { default_minutes: 45, quiet_blocking: false });
        assert_eq!(on, off);
    }

    #[test]
    fn facts_come_from_a_snapshot_with_open_state_and_risk_level() {
        let item = Item {
            id: "id-1".into(),
            key: "approval:pane-a".into(),
            kind: Kind::Approval,
            lane: Lane::Blocking,
            rank: 0,
            state: State::Open,
            since_ms: Some(T0),
            expires_at_ms: None,
            pane: None,
            title: "Approval".into(),
            sentence: None,
            risk: Some(crate::attention::model::RiskView {
                level: Level::High,
                word: Level::High.word(),
                headline: None,
                reasons: Vec::new(),
            }),
            prompt: None,
            actions: Vec::new(),
            resolution: None,
        };
        let snapshot = Snapshot { v: 1, rev: 1, counts: Counts::default(), items: vec![item] };
        let facts = facts_from(&snapshot);
        assert_eq!(
            facts,
            vec![ItemFact {
                key: "approval:pane-a".into(),
                kind: Kind::Approval,
                risk: Some(Level::High),
                since_ms: Some(T0),
                open: true,
            }]
        );
    }
}
