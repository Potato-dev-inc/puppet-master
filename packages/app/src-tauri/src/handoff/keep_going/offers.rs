//! Keep going: which agents may take over a task, and in what order (SPEC n32 section 3.4).
//!
//! Pure: no I/O, no clock. The caller passes the registry data (`limits.vendor`, `agent_ok`) and
//! the limit facts as plain values. Generic code: this file never names an agent. Ranking uses the
//! billing vendor slug, the runnable check and the person's settings order only.

use super::model::{Candidate, Offer, Source, Target, HOP_CAP};

/// A vendor slug that really names one billing company. `None`, empty and `varies` do not, so an
/// agent without a known company is never treated as the same company as another.
fn known_vendor(vendor: Option<&str>) -> Option<&str> {
    vendor.filter(|v| !v.is_empty() && *v != "varies")
}

/// True when both agents bill through the same known company.
fn same_company<'a>(source: &'a Source, candidate: &Candidate) -> Option<&'a str> {
    let from = known_vendor(source.vendor.as_deref())?;
    let to = known_vendor(candidate.vendor.as_deref())?;
    (from == to).then_some(from)
}

/// The reason one candidate cannot take over, or None when it can. The first matching rule wins,
/// in this order: hop cap, same company, not runnable, out of usage.
fn why_not(
    source: &Source,
    candidate: &Candidate,
    hop_next: u8,
    when: &dyn Fn(u64) -> String,
) -> Option<String> {
    if hop_next > HOP_CAP {
        return Some(format!("This task has moved {HOP_CAP} times"));
    }
    if let Some(company) = same_company(source, candidate) {
        return Some(format!("Same company as {company}"));
    }
    if let Err(text) = &candidate.runnable {
        return Some(text.clone());
    }
    match candidate.limited_until {
        Some(Some(reset)) => Some(format!("Out of usage until {}", when(reset))),
        Some(None) => Some("Out of usage".to_string()),
        None => None,
    }
}

/// The picker rows for one task that ran out of usage.
///
/// - The same agent (any account) is not listed.
/// - Each target is listed once. Entries of `order` come first, in that order, when they are
///   candidates. The rest follow in candidate (registry) order.
/// - A row is `ok` only when `why_not` finds no reason. A hop cap disables every row.
/// - `when` formats a reset time (in milliseconds) for the text, such as "17:00".
/// - `_now_ms` is the caller's clock. The limit facts are already in each candidate, so the rules
///   above do not read it.
pub fn rank(
    source: &Source,
    order: &[Target],
    candidates: &[Candidate],
    hop_next: u8,
    _now_ms: u64,
    when: &dyn Fn(u64) -> String,
) -> Vec<Offer> {
    let listed: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.target.agent_id != source.agent_id)
        .collect();

    let mut picked: Vec<&Candidate> = Vec::new();
    for wanted in order {
        if let Some(found) = listed.iter().find(|c| c.target == *wanted).copied() {
            if !picked.iter().any(|p| p.target == found.target) {
                picked.push(found);
            }
        }
    }
    for &candidate in &listed {
        if !picked.iter().any(|p| p.target == candidate.target) {
            picked.push(candidate);
        }
    }

    picked
        .into_iter()
        .map(|candidate| {
            let reason = why_not(source, candidate, hop_next, when);
            Offer {
                target: candidate.target.clone(),
                label: candidate.label.clone(),
                ok: reason.is_none(),
                why_not: reason,
            }
        })
        .collect()
}

/// The first row the person can tap, if any.
pub fn first_ok(offers: &[Offer]) -> Option<&Offer> {
    offers.iter().find(|offer| offer.ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(agent: &str, vendor: Option<&str>) -> Source {
        Source {
            agent_id: agent.into(),
            account: None,
            vendor: vendor.map(str::to_string),
        }
    }

    fn candidate(agent: &str, account: Option<&str>, vendor: Option<&str>) -> Candidate {
        let target = Target {
            agent_id: agent.into(),
            account: account.map(str::to_string),
        };
        Candidate {
            label: target.key(),
            target,
            vendor: vendor.map(str::to_string),
            runnable: Ok(()),
            limited_until: None,
        }
    }

    fn clock(ms: u64) -> String {
        format!("at {ms}")
    }

    fn fixed_time(_ms: u64) -> String {
        "17:00".to_string()
    }

    fn target(agent: &str, account: Option<&str>) -> Target {
        Target {
            agent_id: agent.into(),
            account: account.map(str::to_string),
        }
    }

    fn keys(offers: &[Offer]) -> Vec<String> {
        offers.iter().map(|o| o.target.key()).collect()
    }

    #[test]
    fn same_agent_is_not_listed_for_any_account() {
        let src = source("fake_a", Some("vendor_a"));
        let all = vec![
            candidate("fake_a", None, Some("vendor_a")),
            candidate("fake_a", Some("work"), Some("vendor_a")),
            candidate("fake_b", None, Some("vendor_b")),
        ];
        let offers = rank(&src, &[], &all, 1, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_b"]);
        assert!(offers[0].ok);
    }

    #[test]
    fn same_company_is_listed_but_disabled() {
        let src = source("fake_a", Some("vendor_x"));
        let all = vec![
            candidate("fake_c", None, Some("vendor_x")),
            candidate("fake_d", None, Some("vendor_y")),
        ];
        let offers = rank(&src, &[], &all, 1, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_c", "fake_d"]);
        assert!(!offers[0].ok);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Same company as vendor_x")
        );
        assert!(offers[1].ok);
    }

    #[test]
    fn varies_and_missing_vendor_are_allowed() {
        // Unknown company on either side never counts as the same company.
        let cases = [
            (Some("varies"), Some("varies")),
            (Some("varies"), None),
            (None, Some("vendor_x")),
            (None, None),
            (Some("vendor_x"), Some("varies")),
            (Some(""), Some("")),
        ];
        for (source_vendor, candidate_vendor) in cases {
            let src = source("fake_a", source_vendor);
            let all = vec![candidate("fake_b", None, candidate_vendor)];
            let offers = rank(&src, &[], &all, 1, 0, &clock);
            assert!(
                offers[0].ok,
                "{source_vendor:?} -> {candidate_vendor:?} must be allowed"
            );
        }
    }

    #[test]
    fn without_settings_the_order_is_candidate_order() {
        let src = source("fake_a", None);
        let all = vec![
            candidate("fake_b", None, None),
            candidate("fake_c", None, None),
            candidate("fake_d", None, None),
        ];
        let offers = rank(&src, &[], &all, 1, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_b", "fake_c", "fake_d"]);
    }

    #[test]
    fn settings_order_comes_first_then_registry_order() {
        let src = source("fake_a", None);
        let all = vec![
            candidate("fake_b", None, None),
            candidate("fake_c", None, None),
            candidate("fake_d", None, None),
        ];
        let order = vec![target("fake_d", None), target("fake_b", None)];
        let offers = rank(&src, &order, &all, 1, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_d", "fake_b", "fake_c"]);
    }

    #[test]
    fn settings_entries_that_are_not_candidates_are_skipped() {
        let src = source("fake_a", None);
        let all = vec![
            candidate("fake_b", None, None),
            candidate("fake_c", None, None),
        ];
        // Not installed, unknown, or the source's own agent: none of them is listed.
        let order = vec![
            target("fake_z", None),
            target("fake_a", None),
            target("fake_c", Some("work")),
        ];
        let offers = rank(&src, &order, &all, 1, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_b", "fake_c"]);
    }

    #[test]
    fn an_account_entry_orders_only_that_account() {
        let src = source("fake_a", None);
        let all = vec![
            candidate("fake_b", None, None),
            candidate("fake_b", Some("work"), None),
        ];
        let order = vec![target("fake_b", Some("work"))];
        let offers = rank(&src, &order, &all, 1, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_b@work", "fake_b"]);
    }

    #[test]
    fn a_candidate_listed_twice_appears_once() {
        let src = source("fake_a", None);
        let all = vec![
            candidate("fake_b", None, None),
            candidate("fake_b", None, None),
        ];
        let order = vec![target("fake_b", None)];
        assert_eq!(rank(&src, &order, &all, 1, 0, &clock).len(), 1);
    }

    #[test]
    fn not_runnable_says_why() {
        let src = source("fake_a", None);
        let mut missing = candidate("fake_b", None, None);
        missing.runnable = Err("Not installed on this PC".to_string());
        let offers = rank(&src, &[], &[missing], 1, 0, &clock);
        assert!(!offers[0].ok);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Not installed on this PC")
        );
    }

    #[test]
    fn limited_target_is_disabled_with_the_reset_time() {
        let src = source("fake_a", None);
        let mut limited = candidate("fake_b", None, None);
        limited.limited_until = Some(Some(1_000));
        let mut unknown = candidate("fake_c", None, None);
        unknown.limited_until = Some(None);
        let offers = rank(&src, &[], &[limited, unknown], 1, 0, &fixed_time);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Out of usage until 17:00")
        );
        assert_eq!(offers[1].why_not.as_deref(), Some("Out of usage"));
        assert!(!offers[0].ok && !offers[1].ok);
    }

    #[test]
    fn the_reset_time_text_comes_from_the_caller() {
        let src = source("fake_a", None);
        let mut limited = candidate("fake_b", None, None);
        limited.limited_until = Some(Some(42));
        let offers = rank(&src, &[], &[limited], 1, 0, &clock);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Out of usage until at 42")
        );
    }

    #[test]
    fn same_company_wins_over_the_other_reasons() {
        let src = source("fake_a", Some("vendor_x"));
        let mut both = candidate("fake_c", None, Some("vendor_x"));
        both.limited_until = Some(None);
        both.runnable = Err("Not installed".to_string());
        let offers = rank(&src, &[], &[both], 1, 0, &clock);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Same company as vendor_x")
        );
    }

    #[test]
    fn hop_cap_disables_every_row() {
        let src = source("fake_a", None);
        let all = vec![
            candidate("fake_b", None, None),
            candidate("fake_c", None, None),
        ];
        let capped = rank(&src, &[], &all, HOP_CAP + 1, 0, &clock);
        assert!(capped.iter().all(|o| !o.ok));
        assert!(capped
            .iter()
            .all(|o| o.why_not.as_deref() == Some("This task has moved 3 times")));
        let last_allowed = rank(&src, &[], &all, HOP_CAP, 0, &clock);
        assert!(
            last_allowed.iter().all(|o| o.ok),
            "hop {HOP_CAP} is still allowed"
        );
    }

    #[test]
    fn first_ok_skips_disabled_rows() {
        let src = source("fake_a", Some("vendor_x"));
        let mut limited = candidate("fake_b", None, None);
        limited.limited_until = Some(None);
        let all = vec![
            candidate("fake_c", None, Some("vendor_x")),
            limited,
            candidate("fake_d", None, None),
        ];
        let offers = rank(&src, &[], &all, 1, 0, &clock);
        assert_eq!(
            first_ok(&offers).map(|o| o.target.key()),
            Some("fake_d".to_string())
        );
    }

    #[test]
    fn first_ok_is_none_when_nobody_can_take_over() {
        let src = source("fake_a", None);
        let offers = rank(&src, &[], &[], 1, 0, &clock);
        assert!(offers.is_empty());
        assert!(first_ok(&offers).is_none());
        let capped = rank(
            &src,
            &[],
            &[candidate("fake_b", None, None)],
            HOP_CAP + 1,
            0,
            &clock,
        );
        assert!(first_ok(&capped).is_none());
    }

    #[test]
    fn generic_code_never_names_an_agent() {
        // S13: the keep_going code outside its test modules names no agent.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("handoff")
            .join("keep_going");
        let banned = ["claude", "codex", "cursor", "opencode", "agy"];
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("keep_going folder") {
            let path = entry.expect("dir entry").path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if path.extension().and_then(|e| e.to_str()) != Some("rs")
                || name.ends_with("_tests.rs")
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read source");
            let code = text
                .split("#[cfg(test)]")
                .next()
                .unwrap_or("")
                .to_lowercase();
            for word in banned {
                assert!(
                    !code.contains(word),
                    "{name} names an agent ({word}) outside its tests"
                );
            }
            checked += 1;
        }
        assert!(
            checked >= 5,
            "the scan must see the keep_going files, saw {checked}"
        );
    }
}
