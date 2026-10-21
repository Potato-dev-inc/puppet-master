//! The canary's rules, with no I/O: when an agent is in strict mode, when it should be checked,
//! what a result changes, and the plain words the UI shows. Nothing here names an agent.

use super::model::*;
use super::version::{label as version_label, standing as version_standing, Standing};
use crate::tui::TestedRange;

/// The exact version as a key: `0.171.0` from whatever the CLI printed, empty when unreadable.
pub fn key(version: Option<&str>) -> String {
    version.map(version_label).unwrap_or_default()
}

/// A check passed on exactly this version, on this computer. Only the exact version counts: the
/// next update is a new version and starts unverified.
pub fn is_verified(record: &AgentRecord, version: Option<&str>) -> bool {
    version.is_some() && record.verified.contains(&key(version))
}

/// The screens are known to have changed on the version the record holds now.
pub fn screens_changed(record: &AgentRecord) -> bool {
    let current = key(record.version.as_deref());
    if record.screens_changed.as_deref() == Some(current.as_str()) {
        return true;
    }
    // A file from before `screens_changed` existed: the last result says it.
    record
        .last_result
        .as_ref()
        .is_some_and(|result| result.outcome.changed_screens() && key(result.version.as_deref()) == current)
}

/// Why `facts`' agent is in strict mode, if it is. `standing` is the installed version against the
/// profile's range. Both reasons are about the exact version installed now: a failed check does not
/// follow an update, and a passing check on this computer clears the untested chip for that
/// version only.
pub fn strict_reason(standing: Standing, record: Option<&AgentRecord>) -> Option<StrictReason> {
    if let Some(record) = record {
        if screens_changed(record) {
            return Some(StrictReason::ChangedScreens);
        }
        if is_verified(record, record.version.as_deref()) {
            return None;
        }
    }
    (standing == Standing::Untested).then_some(StrictReason::UntestedVersion)
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

/// Remember a freshly read version. A different one keeps the old as `previous_version` and notes
/// when. An unreadable version (`None`) keeps the last good one: the CLI may just have been slow.
pub fn note_version(record: &mut AgentRecord, version: Option<String>, now_ms: i64) {
    let Some(version) = version else { return };
    if let Some(old) = record.version.as_deref() {
        if version_label(old) != version_label(&version) {
            record.previous_version = Some(version_label(old));
            record.changed_ms = now_ms;
        }
    }
    record.version = Some(version);
}

/// Should a probe that read `new_version` start a check? A changed version does (`auto` permitting);
/// so does the first sighting of a version outside the tested range. The first sighting of a version
/// inside the range does not: nothing changed, so existing installs launch nothing at app start.
/// A version already checked, verified or found changed is not checked again.
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
    let already_checked = before.is_some_and(|r| {
        r.last_result.as_ref().is_some_and(|result| same_version(result.version.as_deref(), Some(new_version)))
            || is_verified(r, Some(new_version))
            || r.screens_changed.as_deref() == Some(key(Some(new_version)).as_str())
    });
    (changed || first_untested) && !already_checked
}

/// Least time between two checks that runtime signals start for one agent.
pub const SIGNAL_GAP_MS: i64 = 10 * 60 * 1000;

/// Should a runtime signal (a screen the profile cannot read, a run that could not type) start a
/// check for this agent now? At most one per agent and version, never faster than
/// [`SIGNAL_GAP_MS`], never with the switch off, and not for a version already found changed (the
/// answer is known). Signals can come in dozens a minute; this is what keeps them from starting
/// a CLI each.
pub fn should_check_for_signal(
    auto: bool,
    facts: &Facts,
    record: Option<&AgentRecord>,
    now_ms: i64,
    last_signal_ms: Option<i64>,
) -> bool {
    if !auto || !facts.installed {
        return false;
    }
    if last_signal_ms.is_some_and(|last| now_ms.saturating_sub(last) < SIGNAL_GAP_MS) {
        return false;
    }
    let Some(record) = record else { return true };
    let current = key(record.version.as_deref());
    record.signal_checked.as_deref() != Some(current.as_str()) && !screens_changed(record)
}

/// The Inbox card text for a check that found changed screens, when the old version is not known.
pub fn notice_text(label: &str, version: Option<&str>) -> String {
    match version {
        Some(version) => format!("{label} {} changed its screens. Puppet Master will ask you before acting.", version_label(version)),
        None => format!("{label} changed its screens. Puppet Master will ask you before acting."),
    }
}

/// "Claude Code 2.3.0 -> 2.4.0", or "Claude Code 2.4.0" when the old version is not known.
fn named(label: &str, from: Option<&str>, to: Option<&str>) -> String {
    match (from, to) {
        (Some(from), Some(to)) => format!("{label} {from} \u{2192} {to}"),
        (None, Some(to)) => format!("{label} {to}"),
        _ => label.to_string(),
    }
}

/// The short result sentence for an update: "Claude Code 2.3.0 -> 2.4.0: checked, all good."
pub fn change_text(label: &str, from: Option<&str>, to: Option<&str>, verdict: Verdict) -> String {
    match verdict {
        Verdict::Good => format!("{}: checked, all good.", named(label, from, to)),
        Verdict::Changed if from.is_some() => {
            format!("{}: screens changed. Puppet Master will ask you before acting.", named(label, from, to))
        }
        Verdict::Changed => notice_text(label, to),
        Verdict::Unchecked => format!("{}: not checked yet.", named(label, from, to)),
    }
}

fn mark_verified(record: &mut AgentRecord, key: &str) {
    const KEEP: usize = 24;
    record.verified.retain(|v| v != key);
    record.verified.push(key.to_string());
    if record.verified.len() > KEEP {
        let drop = record.verified.len() - KEEP;
        record.verified.drain(..drop);
    }
}

/// Fold a finished check into the record: remember it; mark the exact version verified when it
/// passed (and not verified when it failed); mark the screens changed on a failure that proves it;
/// raise the Inbox card, "all good" for a newly verified version, "screens changed" for a failure.
pub fn apply_result(record: &mut AgentRecord, facts: &Facts, result: CanaryResult) {
    let version_key = key(result.version.as_deref());
    let readable = result.version.is_some();
    let was_verified = readable && record.verified.contains(&version_key);
    // The old version this one replaced, when the result is about the version installed now.
    let from = (version_key == key(record.version.as_deref())).then(|| record.previous_version.clone()).flatten();

    if result.outcome.passed() {
        if readable {
            mark_verified(record, &version_key);
        }
        if record.screens_changed.as_deref() == Some(version_key.as_str()) {
            record.screens_changed = None;
        }
        record.failing = None;
    } else {
        record.verified.retain(|v| *v != version_key);
        if result.outcome.changed_screens() {
            record.screens_changed = Some(version_key.clone());
        }
        if result.outcome.counts_when_repeated() {
            let count = match &record.failing {
                Some(f) if f.version == version_key && f.outcome == result.outcome => f.count + 1,
                _ => 1,
            };
            record.failing = Some(Failing { version: version_key.clone(), outcome: result.outcome, count });
            if count >= 2 {
                record.screens_changed = Some(version_key.clone());
            }
        }
    }

    let changed_now = !result.outcome.passed() && record.screens_changed.as_deref() == Some(version_key.as_str());
    if changed_now {
        let id = format!("canary:{}:{}", facts.id, version_key);
        let keep_dismissed = record.notice.as_ref().is_some_and(|n| n.id == id && n.dismissed);
        let to = result.version.as_deref().map(version_label);
        record.notice = Some(Notice {
            id,
            agent: facts.id.clone(),
            text: change_text(&facts.label, from.as_deref(), to.as_deref(), Verdict::Changed),
            at_ms: result.at_ms,
            dismissed: keep_dismissed,
            kind: NoticeKind::Changed,
        });
    } else if result.outcome.passed() {
        if readable && !was_verified {
            record.notice = Some(Notice {
                id: format!("canary:{}:{}:ok", facts.id, version_key),
                agent: facts.id.clone(),
                text: change_text(&facts.label, from.as_deref(), Some(&version_key), Verdict::Good),
                at_ms: result.at_ms,
                dismissed: false,
                kind: NoticeKind::Good,
            });
        } else if record.notice.as_ref().is_some_and(|n| n.kind == NoticeKind::Changed) {
            record.notice = None;
        }
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
    let verified = record.is_some_and(|r| is_verified(r, r.version.as_deref()));
    match reason {
        Some(StrictReason::ChangedScreens) => {
            format!("{name} changed its screens. Puppet Master will ask you before acting.")
        }
        Some(StrictReason::UntestedVersion) => format!(
            "Puppet Master has not been tested with {name} yet. It will ask you before closing a start-up notice, and it will not keep one ready in advance."
        ),
        None if !facts.installed => "Not installed on this computer.".into(),
        None if verified && standing != Standing::Tested => {
            format!("{name} was checked on this computer. Its screens read fine.")
        }
        None => match standing {
            Standing::Tested => format!("{name} is a version Puppet Master has been tested with."),
            Standing::Unknown => format!("Could not read the version of {}.", facts.label),
            _ => format!("{name} has no version limit set."),
        },
    }
}

/// The last version change and how the new version got on, when the record has seen one.
fn change_view(label: &str, record: &AgentRecord) -> Option<ChangeView> {
    let to = record.version.as_deref().map(version_label)?;
    let from = record.previous_version.clone().filter(|from| *from != to)?;
    let verdict = if screens_changed(record) {
        Verdict::Changed
    } else if is_verified(record, Some(&to)) {
        Verdict::Good
    } else {
        Verdict::Unchecked
    };
    let cannot_check = verdict == Verdict::Unchecked
        && record.last_result.as_ref().is_some_and(|r| r.outcome == Outcome::Inconclusive && key(r.version.as_deref()) == to);
    let text = if cannot_check {
        format!("{}: updated. Puppet Master cannot check this agent's screens.", named(label, Some(&from), Some(&to)))
    } else {
        change_text(label, Some(&from), Some(&to), verdict)
    };
    Some(ChangeView { from, to, at_ms: record.changed_ms, verdict, text })
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
        verified: record.is_some_and(|r| is_verified(r, r.version.as_deref())),
        change: record.and_then(|r| change_view(&facts.label, r)),
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
            // A pass says "all good"; every other outcome that proves nothing raises no card.
            let expected = if outcome.passed() { Some(NoticeKind::Good) } else { None };
            assert_eq!(r.notice.as_ref().map(|n| n.kind), expected, "{outcome:?}");
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
        // The failure card is gone; the pass leaves its own "all good" card in its place.
        let good = r.notice.clone().expect("good card");
        assert_eq!(good.kind, NoticeKind::Good);
        assert_eq!(good.text, "Codex 0.162.1: checked, all good.");
        assert_ne!(good.id, notice.id);
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

    // ---- results, verified versions, signals ---------------------------------------------------

    #[test]
    fn a_new_version_keeps_the_old_one_and_when_it_changed() {
        let mut r = AgentRecord::default();
        note_version(&mut r, Some("codex-cli 0.159.3".into()), 10);
        assert_eq!((r.previous_version.as_deref(), r.changed_ms), (None, 0), "the first sighting is not a change");
        note_version(&mut r, Some("codex-cli 0.159.3".into()), 20);
        assert_eq!(r.changed_ms, 0, "the same version again changes nothing");
        note_version(&mut r, Some("codex-cli 0.160.0".into()), 30);
        assert_eq!((r.previous_version.as_deref(), r.changed_ms), (Some("0.159.3"), 30));
        note_version(&mut r, None, 40);
        assert_eq!(r.version.as_deref(), Some("codex-cli 0.160.0"), "an unreadable probe keeps the last version");
        assert_eq!(r.changed_ms, 30);
    }

    #[test]
    fn a_pass_verifies_the_exact_version_and_clears_the_chip_for_it_only() {
        let f = facts();
        let mut r = record("0.171.0");
        assert_eq!(view_of(&f, Some(&r), false, 0).strict_reason, Some(StrictReason::UntestedVersion));
        apply_result(&mut r, &f, result("0.171.0", Outcome::Ready));
        let view = view_of(&f, Some(&r), false, 0);
        assert!(view.verified && !view.strict && view.chip.is_none(), "{view:?}");
        assert_eq!(view.plain, "Codex 0.171.0 was checked on this computer. Its screens read fine.");
        // The next update is a different version: unverified again.
        r.version = Some("0.171.1".into());
        let view = view_of(&f, Some(&r), false, 0);
        assert!(!view.verified && view.strict, "{view:?}");
        assert_eq!(view.chip.as_deref(), Some("Untested version"));
    }

    #[test]
    fn a_failed_check_takes_the_version_off_the_verified_list() {
        let f = facts();
        let mut r = record("0.171.0");
        apply_result(&mut r, &f, result("0.171.0", Outcome::Ready));
        assert!(is_verified(&r, Some("0.171.0")));
        apply_result(&mut r, &f, result("0.171.0", Outcome::Unrecognised));
        assert!(!is_verified(&r, Some("0.171.0")));
        assert_eq!(view_of(&f, Some(&r), false, 0).strict_reason, Some(StrictReason::ChangedScreens));
    }

    #[test]
    fn asking_you_counts_as_a_pass_and_an_unreadable_version_is_never_verified() {
        let f = facts();
        let mut r = record("0.171.0");
        apply_result(&mut r, &f, result("0.171.0", Outcome::AsksYou));
        assert!(is_verified(&r, Some("0.171.0")));
        let mut blind = AgentRecord::default();
        let mut unreadable = result("0.171.0", Outcome::Ready);
        unreadable.version = None;
        apply_result(&mut blind, &f, unreadable);
        assert!(blind.verified.is_empty());
        assert!(blind.notice.is_none(), "nothing to say about a version nobody could read");
    }

    #[test]
    fn the_verified_list_is_capped_and_keeps_the_newest() {
        let f = facts();
        let mut r = record("0.200.0");
        for n in 0..40 {
            let version = format!("0.{n}.0");
            apply_result(&mut r, &f, result(&version, Outcome::Ready));
        }
        assert_eq!(r.verified.len(), 24);
        assert_eq!(r.verified.last().map(String::as_str), Some("0.39.0"));
        assert!(!r.verified.contains(&"0.0.0".to_string()));
    }

    #[test]
    fn a_version_that_was_verified_or_found_changed_is_not_checked_again() {
        let f = facts();
        let mut r = record("0.171.0");
        apply_result(&mut r, &f, result("0.171.0", Outcome::Ready));
        r.version = Some("0.171.1".into());
        assert!(should_check(true, &f, Some(&r), Some("0.171.2")), "a new version is checked");
        // Back to the version verified earlier: nothing to learn.
        assert!(!should_check(true, &f, Some(&r), Some("0.171.0")));
        let mut broken = record("0.162.0");
        broken.screens_changed = Some("0.162.1".into());
        assert!(!should_check(true, &f, Some(&broken), Some("0.162.1")));
    }

    #[test]
    fn the_same_failure_twice_on_one_version_marks_the_screens_changed() {
        let f = facts();
        for outcome in [Outcome::NeverDrew, Outcome::Exited] {
            let mut r = record("0.162.1");
            apply_result(&mut r, &f, result("0.162.1", outcome));
            assert_eq!(strict_reason(Standing::Tested, Some(&r)), None, "{outcome:?} once proves nothing");
            assert!(r.notice.is_none());
            apply_result(&mut r, &f, result("0.162.1", outcome));
            assert_eq!(strict_reason(Standing::Tested, Some(&r)), Some(StrictReason::ChangedScreens), "{outcome:?} twice");
            assert_eq!(r.notice.as_ref().map(|n| n.kind), Some(NoticeKind::Changed));
        }
    }

    #[test]
    fn different_failures_or_different_versions_do_not_add_up() {
        let f = facts();
        let mut r = record("0.162.1");
        apply_result(&mut r, &f, result("0.162.1", Outcome::NeverDrew));
        apply_result(&mut r, &f, result("0.162.1", Outcome::Exited));
        assert_eq!(strict_reason(Standing::Tested, Some(&r)), None, "two different failures");
        let mut r = record("0.162.1");
        apply_result(&mut r, &f, result("0.162.1", Outcome::NeverDrew));
        r.version = Some("0.162.2".into());
        apply_result(&mut r, &f, result("0.162.2", Outcome::NeverDrew));
        assert_eq!(strict_reason(Standing::Tested, Some(&r)), None, "two versions");
        // A start that could not happen says nothing about screens, however often it repeats.
        let mut r = record("0.162.1");
        for _ in 0..3 {
            apply_result(&mut r, &f, result("0.162.1", Outcome::StartFailed));
            apply_result(&mut r, &f, result("0.162.1", Outcome::Inconclusive));
        }
        assert_eq!(strict_reason(Standing::Tested, Some(&r)), None);
        // A pass in between starts the count again.
        let mut r = record("0.162.1");
        apply_result(&mut r, &f, result("0.162.1", Outcome::NeverDrew));
        apply_result(&mut r, &f, result("0.162.1", Outcome::Ready));
        apply_result(&mut r, &f, result("0.162.1", Outcome::NeverDrew));
        assert_eq!(strict_reason(Standing::Tested, Some(&r)), None);
    }

    #[test]
    fn the_update_sentences_are_short_and_say_both_versions() {
        assert_eq!(change_text("Claude Code", Some("2.3.0"), Some("2.4.0"), Verdict::Good), "Claude Code 2.3.0 \u{2192} 2.4.0: checked, all good.");
        assert_eq!(
            change_text("Claude Code", Some("2.3.0"), Some("2.4.0"), Verdict::Changed),
            "Claude Code 2.3.0 \u{2192} 2.4.0: screens changed. Puppet Master will ask you before acting."
        );
        assert_eq!(change_text("Codex", None, Some("0.162.1"), Verdict::Good), "Codex 0.162.1: checked, all good.");
        assert_eq!(change_text("Codex", Some("0.1.0"), Some("0.2.0"), Verdict::Unchecked), "Codex 0.1.0 \u{2192} 0.2.0: not checked yet.");
    }

    #[test]
    fn the_view_carries_the_last_change_and_what_became_of_it() {
        let f = facts();
        let mut r = record("0.159.3");
        assert!(view_of(&f, Some(&r), false, 0).change.is_none(), "no change yet");
        note_version(&mut r, Some("0.160.0".into()), 500);
        let change = view_of(&f, Some(&r), false, 0).change.expect("a change");
        assert_eq!((change.from.as_str(), change.to.as_str(), change.at_ms, change.verdict), ("0.159.3", "0.160.0", 500, Verdict::Unchecked));
        assert_eq!(change.text, "Codex 0.159.3 \u{2192} 0.160.0: not checked yet.");
        apply_result(&mut r, &f, result("0.160.0", Outcome::Ready));
        let change = view_of(&f, Some(&r), false, 0).change.unwrap();
        assert_eq!((change.verdict, change.text.as_str()), (Verdict::Good, "Codex 0.159.3 \u{2192} 0.160.0: checked, all good."));
        // The notice carries the same sentence.
        assert_eq!(r.notice.as_ref().unwrap().text, change.text);
        apply_result(&mut r, &f, result("0.160.0", Outcome::Unrecognised));
        let change = view_of(&f, Some(&r), false, 0).change.unwrap();
        assert_eq!(change.verdict, Verdict::Changed);
        assert!(change.text.starts_with("Codex 0.159.3 \u{2192} 0.160.0: screens changed."), "{}", change.text);
        assert_eq!(r.notice.as_ref().unwrap().text, change.text);
    }

    #[test]
    fn an_agent_that_cannot_be_checked_says_so_after_an_update() {
        let f = facts();
        let mut r = record("0.159.3");
        note_version(&mut r, Some("0.160.0".into()), 5);
        apply_result(&mut r, &f, result("0.160.0", Outcome::Inconclusive));
        let change = view_of(&f, Some(&r), false, 0).change.unwrap();
        assert_eq!(change.verdict, Verdict::Unchecked);
        assert!(change.text.ends_with("updated. Puppet Master cannot check this agent's screens."), "{}", change.text);
    }

    #[test]
    fn re_checking_a_verified_version_raises_no_new_card() {
        let f = facts();
        let mut r = record("0.162.1");
        apply_result(&mut r, &f, result("0.162.1", Outcome::Ready));
        r.notice.as_mut().unwrap().dismissed = true;
        apply_result(&mut r, &f, result("0.162.1", Outcome::Ready));
        assert!(r.notice.as_ref().unwrap().dismissed, "the person's dismissal stands");
    }

    #[test]
    fn a_failure_card_after_a_dismissed_good_card_is_not_hidden() {
        let f = facts();
        let mut r = record("0.162.1");
        apply_result(&mut r, &f, result("0.162.1", Outcome::Ready));
        r.notice.as_mut().unwrap().dismissed = true;
        apply_result(&mut r, &f, result("0.162.1", Outcome::Unrecognised));
        let notice = r.notice.as_ref().unwrap();
        assert_eq!(notice.kind, NoticeKind::Changed);
        assert!(!notice.dismissed);
    }

    #[test]
    fn a_signal_starts_one_check_per_version_and_waits_between() {
        let f = facts();
        let mut r = record("0.162.1");
        assert!(should_check_for_signal(true, &f, Some(&r), 1_000, None));
        assert!(should_check_for_signal(true, &f, None, 1_000, None), "an agent never probed still gets one");
        // Too soon after the last one.
        assert!(!should_check_for_signal(true, &f, Some(&r), 1_000 + SIGNAL_GAP_MS - 1, Some(1_000)));
        assert!(should_check_for_signal(true, &f, Some(&r), 1_000 + SIGNAL_GAP_MS, Some(1_000)));
        // Once per version.
        r.signal_checked = Some("0.162.1".into());
        assert!(!should_check_for_signal(true, &f, Some(&r), 9_999_999, Some(1_000)));
        r.version = Some("0.162.2".into());
        assert!(should_check_for_signal(true, &f, Some(&r), 9_999_999, Some(1_000)), "a new version is a new chance");
        // Switch off, not installed, answer already known.
        assert!(!should_check_for_signal(false, &f, Some(&r), 9_999_999, None));
        let mut gone = facts();
        gone.installed = false;
        assert!(!should_check_for_signal(true, &gone, Some(&r), 9_999_999, None));
        r.screens_changed = Some("0.162.2".into());
        assert!(!should_check_for_signal(true, &f, Some(&r), 9_999_999, None));
    }

    #[test]
    fn a_file_from_before_these_fields_still_loads_and_keeps_its_meaning() {
        let old = r#"{
            "auto_check": true,
            "agents": {
              "codex": {
                "version": "codex-cli 0.162.1",
                "probed_ms": 5,
                "last_result": {
                  "agent": "codex", "version": "codex-cli 0.162.1", "at_ms": 9, "duration_ms": 4000,
                  "outcome": "unrecognised", "passed": false, "summary": "Codex showed a screen Puppet Master does not recognise.",
                  "gate": null, "dismissed": [], "keys_sent": [], "prompts_sent": 0, "screens": []
                },
                "notice": { "id": "canary:codex:0.162.1", "agent": "codex", "text": "Codex 0.162.1 changed its screens. Puppet Master will ask you before acting.", "at_ms": 9, "dismissed": true }
              }
            }
        }"#;
        let loaded: Persisted = serde_json::from_str(old).expect("an old canary.json loads");
        let r = &loaded.agents["codex"];
        assert!(r.verified.is_empty() && r.previous_version.is_none() && r.changed_ms == 0 && r.binary.is_none());
        assert_eq!(r.notice.as_ref().map(|n| (n.kind, n.dismissed)), Some((NoticeKind::Changed, true)));
        // The failure still holds the agent in strict mode, for that version.
        assert_eq!(strict_reason(Standing::Tested, Some(r)), Some(StrictReason::ChangedScreens));
        let view = view_of(&facts(), Some(r), false, 0);
        assert!(view.strict && !view.verified && view.change.is_none());
        // And a written file reads back the same.
        let again: Persisted = serde_json::from_str(&serde_json::to_string(&loaded).unwrap()).unwrap();
        assert_eq!(again.agents["codex"].version, r.version);
    }
}
