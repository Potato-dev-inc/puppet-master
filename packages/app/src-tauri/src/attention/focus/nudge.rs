//! The one reminder for a quiet card still open after 20 minutes (SPEC n33 3.4 C). Pure: the caller passes the open
//! keys and the time in. The reminder is counts only: `1 agent has waited 20 minutes for you.` No names, no card
//! text and no key leave this module.

use super::model::{Tier, Waiter, NUDGE_AFTER_MS};
use std::collections::HashSet;

/// The keys of the waiters that are due now. A waiter is due when it is `Quiet` (a `Through` card is never nudged),
/// still open (`open_keys`), not nudged yet, and `first_seen + 20 min <= now_ms`.
pub fn due(waiters: &[Waiter], open_keys: &HashSet<String>, now_ms: u64) -> Vec<String> {
    waiters
        .iter()
        .filter(|waiter| {
            waiter.tier == Tier::Quiet
                && !waiter.nudged
                && open_keys.contains(&waiter.key)
                && waiter.first_seen_ms.saturating_add(NUDGE_AFTER_MS) <= now_ms
        })
        .map(|waiter| waiter.key.clone())
        .collect()
}

/// Marks the waiters with these keys as nudged, so each waiter gets one reminder. Other waiters are not touched.
pub fn mark(waiters: &mut [Waiter], keys: &[String]) {
    for waiter in waiters.iter_mut() {
        if keys.iter().any(|key| key == &waiter.key) {
            waiter.nudged = true;
        }
    }
}

/// The reminder sentence for `n` waiting cards. Counts only.
pub fn text(n: usize) -> String {
    if n == 1 {
        "1 agent has waited 20 minutes for you.".to_string()
    } else {
        format!("{n} agents have waited 20 minutes for you.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_791_979_200_000;
    const MIN: u64 = 60_000;

    fn waiter(key: &str, tier: Tier, first_seen_ms: u64) -> Waiter {
        Waiter { key: key.into(), tier, first_seen_ms, nudged: false }
    }

    fn open(keys: &[&str]) -> HashSet<String> {
        keys.iter().map(|key| key.to_string()).collect()
    }

    #[test]
    fn exactly_twenty_minutes_is_due_and_nineteen_fifty_nine_is_not() {
        let waiters = [waiter("question:a", Tier::Quiet, T0)];
        let still = open(&["question:a"]);
        assert!(due(&waiters, &still, T0 + 20 * MIN - 1).is_empty(), "19:59.999 is not due");
        assert!(due(&waiters, &still, T0 + 19 * MIN + 59_000).is_empty(), "19:59 is not due");
        assert_eq!(due(&waiters, &still, T0 + 20 * MIN), vec!["question:a".to_string()], "exactly 20:00 is due");
        assert_eq!(due(&waiters, &still, T0 + 90 * MIN), vec!["question:a".to_string()], "later is due too");
    }

    #[test]
    fn through_waiters_are_never_nudged() {
        let waiters = [waiter("consent:x", Tier::Through, T0), waiter("budget:y", Tier::Through, T0)];
        let both_open = open(&["consent:x", "budget:y"]);
        assert!(due(&waiters, &both_open, T0 + 60 * MIN).is_empty());
    }

    #[test]
    fn a_closed_card_is_not_nudged() {
        let waiters = [waiter("question:a", Tier::Quiet, T0), waiter("question:b", Tier::Quiet, T0)];
        let only_b = open(&["question:b"]);
        assert_eq!(due(&waiters, &only_b, T0 + 25 * MIN), vec!["question:b".to_string()]);
        assert!(due(&waiters, &open(&[]), T0 + 25 * MIN).is_empty(), "nothing open, nothing due");
    }

    #[test]
    fn one_nudge_per_waiter() {
        let mut waiters = [waiter("question:a", Tier::Quiet, T0), waiter("question:b", Tier::Quiet, T0 + MIN)];
        let both = open(&["question:a", "question:b"]);

        let first = due(&waiters, &both, T0 + 25 * MIN);
        assert_eq!(first.len(), 2);
        mark(&mut waiters, &first);
        assert!(waiters.iter().all(|w| w.nudged), "both are marked");
        assert!(due(&waiters, &both, T0 + 40 * MIN).is_empty(), "a marked waiter is never due again");
    }

    #[test]
    fn mark_touches_only_the_listed_keys() {
        let mut waiters = [waiter("question:a", Tier::Quiet, T0), waiter("question:b", Tier::Quiet, T0)];
        mark(&mut waiters, &["question:a".to_string()]);
        assert!(waiters[0].nudged);
        assert!(!waiters[1].nudged);
    }

    #[test]
    fn the_text_is_singular_for_one_and_plural_otherwise() {
        assert_eq!(text(1), "1 agent has waited 20 minutes for you.");
        assert_eq!(text(3), "3 agents have waited 20 minutes for you.");
        assert_eq!(text(0), "0 agents have waited 20 minutes for you.");
    }
}
