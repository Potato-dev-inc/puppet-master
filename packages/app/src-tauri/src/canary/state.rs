//! The canary's rules, with no I/O: when an agent is in strict mode, when it should be checked,
//! what a result changes, and the plain words the UI shows. Nothing here names an agent.

use super::model::*;
use super::version::{label as version_label, standing as version_standing, Standing};
use crate::tui::TestedRange;

/// Why `facts`' agent is in strict mode, if it is. `standing` is the installed version against the
/// profile's range; a failed check only counts for the version it ran on.
pub fn strict_reason(standing: Standing, record: Option<&AgentRecord>) -> Option<StrictReason> {
    if standing == Standing::Untested {
        return Some(StrictReason::UntestedVersion);
    }
    let record = record?;
    let result = record.last_result.as_ref()?;
    (result.outcome.changed_screens() && same_version(result.version.as_deref(), record.version.as_deref()))
        .then_some(StrictReason::ChangedScreens)
}

fn same_version(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => version_label(a) == version_label(b),
        (None, None) => true,
        _ => false,
    }
}

/// Standing of an agent given its range text and the record's version.
pub fn standing_of(facts: &Facts, record: Option<&AgentRecord>) -> Standing {
    let range = facts.tested.as_deref().and_then(|text| TestedRange::parse(text).ok());
    version_standing(range.as_ref(), record.and_then(|r| r.version.as_deref()))
}

/// Should a probe that read `new_version` start a check? A changed version does (`auto` permitting);
/// so does the first sighting of a version outside the tested range. The first sighting of a version
/// inside the range does not: nothing changed, so existing installs launch nothing at app start.
pub fn should_check(auto: bool, facts: &Facts, before: Option<&AgentRecord>, new_version: Option<&str>) -> bool {
    if !auto || !facts.installed {
        return false;
    }
    let Some(new_version) = new_version else { return false };
    let previous = before.and_then(|r| r.version.as_deref());
    let changed = previous.is_some_and(|old| version_label(old) != version_label(new_version));
    let first_untested = previous.is_none() && {
        let probe = AgentRecord { version: Some(new_version.to_string()), ..Default::default() };
        standing_of(facts, Some(&probe)) == Standing::Untested
    };
    let already_checked = before
        .and_then(|r| r.last_result.as_ref())
        .is_some_and(|result| same_version(result.version.as_deref(), Some(new_version)));
    (changed || first_untested) && !already_checked
}

/// The Inbox card text for a check that found changed screens.
pub fn notice_text(label: &str, version: Option<&str>) -> String {
    match version {
        Some(version) => format!("{label} {} changed its screens. Puppet Master will ask you before acting.", version_label(version)),
        None => format!("{label} changed its screens. Puppet Master will ask you before acting."),
    }
}

/// Fold a finished check into the record: remember it, raise the Inbox card when the screens
/// changed, and clear an old card when they now read fine.
pub fn apply_result(record: &mut AgentRecord, facts: &Facts, result: CanaryResult) {
    if result.outcome.changed_screens() {
        let id = format!("canary:{}:{}", facts.id, result.version.as_deref().map(version_label).unwrap_or_default());
        let keep_dismissed = record.notice.as_ref().is_some_and(|n| n.id == id && n.dismissed);
        record.notice = Some(Notice {
            id,
            agent: facts.id.clone(),
            text: notice_text(&facts.label, result.version.as_deref()),
            at_ms: result.at_ms,
            dismissed: keep_dismissed,
        });
    } else if result.outcome.passed() {
        record.notice = None;
    }
    record.last_result = Some(result);
}

/// The chip text. Both reasons say the same thing to the person: Puppet Master is being careful.
pub const CHIP: &str = "Untested version";

/// Plain sentence for the Settings row.
pub fn plain(facts: &Facts, reason: Option<StrictReason>, standing: Standing, record: Option<&AgentRecord>) -> String {
    let version = record.and_then(|r| r.version.as_deref()).map(version_label);
    let name = match &version {
        Some(version) => format!("{} {version}", facts.label),
        None => facts.label.clone(),
    };
    match reason {
        Some(StrictReason::ChangedScreens) => {
            format!("{name} changed its screens. Puppet Master will ask you before acting.")
        }
        Some(StrictReason::UntestedVersion) => format!(
            "Puppet Master has not been tested with {name} yet. It will ask you before closing a start-up notice, and it will not keep one ready in advance."
        ),
        None if !facts.installed => "Not installed on this computer.".into(),
        None => match standing {
            Standing::Tested => format!("{name} is a version Puppet Master has been tested with."),
            Standing::Unknown => format!("Could not read the version of {}.", facts.label),
            _ => format!("{name} has no version limit set."),
        },
    }
}

pub fn view_of(facts: &Facts, record: Option<&AgentRecord>, checking: bool, captures: usize) -> AgentView {
    let standing = standing_of(facts, record);
    let reason = strict_reason(standing, record);
    AgentView {
        agent: facts.id.clone(),
        label: facts.label.clone(),
        installed: facts.installed,
        version: record.and_then(|r| r.version.as_deref()).map(version_label),
        tested: facts.tested.clone(),
        standing,
        strict: reason.is_some(),
        strict_reason: reason,
        chip: reason.map(|_| CHIP.to_string()),
        plain: plain(facts, reason, standing, record),
        checking,
        last: record.and_then(|r| r.last_result.as_ref()).map(|r| ResultView {
            outcome: r.outcome,
            passed: r.passed,
            summary: r.summary.clone(),
            at_ms: r.at_ms,
            version: r.version.clone(),
            screens: r.screens.clone(),
        }),
        captures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts { id: "codex".into(), label: "Codex".into(), installed: true, tested: Some(">=0.159, <0.170".into()) }
    }

    fn record(version: &str) -> AgentRecord {
        AgentRecord { version: Some(version.into()), ..Default::default() }
    }

    fn result(version: &str, outcome: Outcome) -> CanaryResult {
        CanaryResult {
            agent: "codex".into(),
            version: Some(version.into()),
            at_ms: 10,
            duration_ms: 5,
            outcome,
            passed: outcome.passed(),
            summary: String::new(),
            gate: None,
            dismissed: vec![],
            keys_sent: vec![],
            prompts_sent: 0,
            screens: vec![],
        }
    }

    #[test]
    fn a_version_outside_the_range_is_strict_and_a_tested_one_is_not() {
        let tested = record("codex-cli 0.159.3");
        let untested = record("codex-cli 0.171.0");
        assert_eq!(strict_reason(standing_of(&facts(), Some(&tested)), Some(&tested)), None);
        assert_eq!(
            strict_reason(standing_of(&facts(), Some(&untested)), Some(&untested)),
            Some(StrictReason::UntestedVersion)
        );
    }

    #[test]
    fn an_agent_without_a_range_is_never_strict_for_its_version() {
        let mut f = facts();
        f.tested = None;
        let r = record("9.9.9");
        assert_eq!(strict_reason(standing_of(&f, Some(&r)), Some(&r)), None);
        assert_eq!(strict_reason(standing_of(&f, None), None), None);
    }

    #[test]
    fn a_failed_check_makes_the_agent_strict_for_that_version_only() {
        let mut r = record("0.162.1");
        apply_result(&mut r, &facts(), result("0.162.1", Outcome::Unrecognised));
        assert_eq!(strict_reason(standing_of(&facts(), Some(&r)), Some(&r)), Some(StrictReason::ChangedScreens));
        // The CLI updates again: the old failure says nothing about the new version.
        r.version = Some("0.163.0".into());
        assert_eq!(strict_reason(standing_of(&facts(), Some(&r)), Some(&r)), None);
    }

    #[test]
    fn only_unrecognised_screens_count_as_changed() {
        for outcome in [Outcome::NeverDrew, Outcome::Exited, Outcome::StartFailed, Outcome::Inconclusive, Outcome::Ready, Outcome::AsksYou] {
            let mut r = record("0.162.1");
            apply_result(&mut r, &facts(), result("0.162.1", outcome));
            assert_eq!(strict_reason(Standing::Tested, Some(&r)), None, "{outcome:?}");
            assert!(r.notice.is_none(), "{outcome:?} raises no card");
        }
    }

    #[test]
    fn a_failed_check_raises_the_inbox_card_and_a_pass_clears_it() {
        let mut r = record("0.162.1");
        apply_result(&mut r, &facts(), result("0.162.1", Outcome::Unrecognised));
        let notice = r.notice.clone().expect("card");
        assert_eq!(notice.text, "Codex 0.162.1 changed its screens. Puppet Master will ask you before acting.");
        assert!(!notice.dismissed);
        apply_result(&mut r, &facts(), result("0.162.1", Outcome::Ready));
        assert!(r.notice.is_none());
    }

    #[test]
    fn a_dismissed_card_stays_dismissed_when_the_same_failure_repeats() {
        let mut r = record("0.162.1");
        apply_result(&mut r, &facts(), result("0.162.1", Outcome::Unrecognised));
        r.notice.as_mut().unwrap().dismissed = true;
        apply_result(&mut r, &facts(), result("0.162.1", Outcome::Unrecognised));
        assert!(r.notice.as_ref().unwrap().dismissed);
    }

    #[test]
    fn a_changed_version_starts_a_check_but_a_first_sighting_in_range_does_not() {
        let before = record("0.159.3");
        assert!(should_check(true, &facts(), Some(&before), Some("codex-cli 0.160.0")));
        assert!(!should_check(true, &facts(), Some(&before), Some("codex-cli 0.159.3")));
        // First sighting, inside the range: nothing changed, nothing launches.
        assert!(!should_check(true, &facts(), None, Some("codex-cli 0.159.3")));
        // First sighting outside the range: worth knowing.
        assert!(should_check(true, &facts(), None, Some("codex-cli 0.171.0")));
    }

    #[test]
    fn auto_check_off_or_not_installed_or_no_version_checks_nothing() {
        let before = record("0.159.3");
        assert!(!should_check(false, &facts(), Some(&before), Some("0.160.0")));
        assert!(!should_check(true, &facts(), Some(&before), None));
        let mut f = facts();
        f.installed = false;
        assert!(!should_check(true, &f, Some(&before), Some("0.160.0")));
    }

    #[test]
    fn a_version_already_checked_is_not_checked_again() {
        let mut before = record("0.160.0");
        apply_result(&mut before, &facts(), result("0.161.0", Outcome::Ready));
        assert!(!should_check(true, &facts(), Some(&before), Some("0.161.0")));
    }

    #[test]
    fn the_view_carries_the_chip_and_plain_words_for_each_reason() {
        let untested = view_of(&facts(), Some(&record("0.171.0")), false, 0);
        assert!(untested.strict);
        assert_eq!(untested.chip.as_deref(), Some("Untested version"));
        assert!(untested.plain.contains("ask you before closing a start-up notice"), "{}", untested.plain);
        assert!(!untested.plain.to_lowercase().contains("strict"));

        let mut changed = record("0.162.1");
        apply_result(&mut changed, &facts(), result("0.162.1", Outcome::Unrecognised));
        let view = view_of(&facts(), Some(&changed), false, 2);
        assert_eq!(view.strict_reason, Some(StrictReason::ChangedScreens));
        assert_eq!(view.plain, "Codex 0.162.1 changed its screens. Puppet Master will ask you before acting.");
        assert_eq!(view.captures, 2);

        let fine = view_of(&facts(), Some(&record("0.162.1")), false, 0);
        assert!(!fine.strict && fine.chip.is_none());
    }
}
