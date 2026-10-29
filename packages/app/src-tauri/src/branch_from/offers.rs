//! Which agents can be picked for a branch (SPEC n33 section 3.6). Pure: no I/O, no clock.
//!
//! The picker rows and their reason texts come from Keep going's `Target` and `Offer`. The ranking
//! rules are this module's own: the same agent is listed first (a "try again" is common), there is
//! no same-company rule and no hop cap. The caller builds the candidates from the agent registry
//! and the live limits. Generic code: this file never names an agent. Ids and registry data only.

use crate::handoff::keep_going::model::{Offer, Target};

/// One agent that could run the branch (input to `rank`). `installed` and `runnable` come from the
/// agent registry. `limited_until` is the live usage limit: `Some(Some(reset))` when the reset time
/// is known, `Some(None)` when it is not, `None` when the agent has no live limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub target: Target,
    pub label: String,
    pub installed: bool,
    pub runnable: Result<(), String>,
    pub limited_until: Option<Option<u64>>,
}

const NOT_INSTALLED: &str = "Not installed on this computer";
const OUT_OF_USAGE: &str = "Out of usage";

/// The reason one candidate cannot be picked, or None when it can. The first matching rule wins,
/// in this order: not installed, not runnable, out of usage.
fn why_not(candidate: &Candidate, when: &dyn Fn(u64) -> String) -> Option<String> {
    if !candidate.installed {
        return Some(NOT_INSTALLED.to_string());
    }
    if let Err(text) = &candidate.runnable {
        return Some(text.clone());
    }
    match candidate.limited_until {
        Some(Some(reset)) => Some(format!("Out of usage until {}", when(reset))),
        Some(None) => Some(OUT_OF_USAGE.to_string()),
        None => None,
    }
}

/// The picker rows for a branch from a pane that runs `source`.
///
/// - The source's own target (same agent, same login) comes first, when it is a candidate. The
///   other candidates follow in candidate (registry) order.
/// - Each target is listed once, even when a candidate repeats.
/// - A row is `ok` only when `why_not` finds no reason.
/// - `when` formats a reset time (in milliseconds) for the text, such as "17:00".
/// - `_now_ms` is the caller's clock. The limit facts are already in each candidate, so the rules
///   above do not read it.
pub fn rank(
    source: &Target,
    candidates: &[Candidate],
    _now_ms: u64,
    when: &dyn Fn(u64) -> String,
) -> Vec<Offer> {
    let mut picked: Vec<&Candidate> = Vec::new();
    if let Some(same) = candidates.iter().find(|c| c.target == *source) {
        picked.push(same);
    }
    for candidate in candidates {
        if !picked.iter().any(|p| p.target == candidate.target) {
            picked.push(candidate);
        }
    }

    picked
        .into_iter()
        .map(|candidate| {
            let reason = why_not(candidate, when);
            Offer {
                target: candidate.target.clone(),
                label: candidate.label.clone(),
                ok: reason.is_none(),
                why_not: reason,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(agent: &str, account: Option<&str>) -> Target {
        Target {
            agent_id: agent.into(),
            account: account.map(str::to_string),
        }
    }

    fn candidate(agent: &str, account: Option<&str>) -> Candidate {
        let target = target(agent, account);
        Candidate {
            label: target.key(),
            target,
            installed: true,
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

    fn keys(offers: &[Offer]) -> Vec<String> {
        offers.iter().map(|o| o.target.key()).collect()
    }

    #[test]
    fn source_target_comes_first_then_registry_order() {
        let all = vec![
            candidate("fake_b", None),
            candidate("fake_a", None),
            candidate("fake_c", None),
        ];
        let offers = rank(&target("fake_a", None), &all, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_a", "fake_b", "fake_c"]);
    }

    #[test]
    fn same_agent_is_allowed_and_ok_when_runnable_and_not_limited() {
        let all = vec![candidate("fake_a", None), candidate("fake_b", None)];
        let offers = rank(&target("fake_a", None), &all, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_a", "fake_b"]);
        assert!(offers[0].ok);
        assert_eq!(offers[0].why_not, None);
    }

    #[test]
    fn source_login_comes_first_and_other_logins_keep_registry_order() {
        let all = vec![
            candidate("fake_a", None),
            candidate("fake_a", Some("work")),
            candidate("fake_b", None),
        ];
        let offers = rank(&target("fake_a", Some("work")), &all, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_a@work", "fake_a", "fake_b"]);
    }

    #[test]
    fn source_not_among_candidates_is_not_listed() {
        let all = vec![candidate("fake_b", None), candidate("fake_c", None)];
        let offers = rank(&target("fake_a", None), &all, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_b", "fake_c"]);
    }

    #[test]
    fn each_target_is_listed_once() {
        let all = vec![
            candidate("fake_a", None),
            candidate("fake_b", None),
            candidate("fake_b", None),
            candidate("fake_a", None),
        ];
        let offers = rank(&target("fake_a", None), &all, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_a", "fake_b"]);
    }

    #[test]
    fn registry_order_is_kept_without_a_source_match() {
        let all = vec![
            candidate("fake_c", None),
            candidate("fake_b", None),
            candidate("fake_d", None),
        ];
        let offers = rank(&target("fake_a", None), &all, 0, &clock);
        assert_eq!(keys(&offers), vec!["fake_c", "fake_b", "fake_d"]);
    }

    #[test]
    fn not_installed_is_disabled_with_its_reason() {
        let mut missing = candidate("fake_b", None);
        missing.installed = false;
        let offers = rank(&target("fake_a", None), &[missing], 0, &clock);
        assert!(!offers[0].ok);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Not installed on this computer")
        );
    }

    #[test]
    fn not_runnable_is_disabled_with_its_text() {
        let mut blocked = candidate("fake_b", None);
        blocked.runnable = Err("Sign in to this agent first".to_string());
        let offers = rank(&target("fake_a", None), &[blocked], 0, &clock);
        assert!(!offers[0].ok);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Sign in to this agent first")
        );
    }

    #[test]
    fn limit_with_a_reset_time_is_disabled_with_the_time() {
        let mut limited = candidate("fake_b", None);
        limited.limited_until = Some(Some(1_000));
        let offers = rank(&target("fake_a", None), &[limited], 0, &fixed_time);
        assert!(!offers[0].ok);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Out of usage until 17:00")
        );
    }

    #[test]
    fn limit_without_a_reset_time_is_disabled_without_one() {
        let mut limited = candidate("fake_b", None);
        limited.limited_until = Some(None);
        let offers = rank(&target("fake_a", None), &[limited], 0, &fixed_time);
        assert!(!offers[0].ok);
        assert_eq!(offers[0].why_not.as_deref(), Some("Out of usage"));
    }

    #[test]
    fn the_reset_time_is_formatted_by_the_caller() {
        let mut limited = candidate("fake_b", None);
        limited.limited_until = Some(Some(42));
        let offers = rank(&target("fake_a", None), &[limited], 0, &clock);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Out of usage until at 42")
        );
    }

    #[test]
    fn the_source_row_can_be_disabled_too() {
        let mut same = candidate("fake_a", None);
        same.limited_until = Some(None);
        let offers = rank(&target("fake_a", None), &[same], 0, &clock);
        assert!(!offers[0].ok);
        assert_eq!(offers[0].why_not.as_deref(), Some("Out of usage"));
    }

    #[test]
    fn not_installed_wins_over_the_other_reasons() {
        let mut both = candidate("fake_b", None);
        both.installed = false;
        both.runnable = Err("Sign in first".to_string());
        both.limited_until = Some(None);
        let offers = rank(&target("fake_a", None), &[both], 0, &clock);
        assert_eq!(
            offers[0].why_not.as_deref(),
            Some("Not installed on this computer")
        );
    }

    #[test]
    fn not_runnable_wins_over_out_of_usage() {
        let mut both = candidate("fake_b", None);
        both.runnable = Err("Sign in first".to_string());
        both.limited_until = Some(Some(1_000));
        let offers = rank(&target("fake_a", None), &[both], 0, &fixed_time);
        assert_eq!(offers[0].why_not.as_deref(), Some("Sign in first"));
    }

    #[test]
    fn ok_rows_and_disabled_rows_mix_in_registry_order() {
        let mut limited = candidate("fake_b", None);
        limited.limited_until = Some(None);
        let all = vec![candidate("fake_c", None), limited, candidate("fake_d", None)];
        let offers = rank(&target("fake_a", None), &all, 0, &clock);
        let ok: Vec<bool> = offers.iter().map(|o| o.ok).collect();
        assert_eq!(ok, vec![true, false, true]);
        assert_eq!(keys(&offers), vec!["fake_c", "fake_b", "fake_d"]);
    }

    #[test]
    fn no_candidates_means_no_rows() {
        assert!(rank(&target("fake_a", None), &[], 0, &clock).is_empty());
    }

    #[test]
    fn generic_code_never_names_an_agent() {
        // S12: the branch_from files and checkpoints/export.rs name no agent outside their tests.
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        for entry in std::fs::read_dir(src.join("branch_from")).expect("branch_from folder") {
            let path = entry.expect("dir entry").path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") && !name.ends_with("_tests.rs") {
                files.push((name, path));
            }
        }
        files.push((
            "checkpoints/export.rs".to_string(),
            src.join("checkpoints").join("export.rs"),
        ));
        assert!(files.len() >= 6, "the scan must see the branch_from files, saw {}", files.len());

        let banned = ["claude", "codex", "cursor", "opencode", "agy", "kilo"];
        for (name, path) in &files {
            let text = std::fs::read_to_string(path).expect("read source");
            let code = text.split("#[cfg(test)]").next().unwrap_or("").to_lowercase();
            for word in banned {
                assert!(!code.contains(word), "{name} names an agent ({word}) outside its tests");
            }
        }
    }
}
