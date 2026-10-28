//! Issue intake: filter (SPEC n32 section 3.3). Who may start a run, which label events are new,
//! what the person must still be asked about, and which queued run may start now. Pure: the
//! caller passes the GitHub facts (`github`) and the saved settings and runs (`model`) and gets a
//! decision back.
//!
//! Fail closed everywhere. An unknown labeler, a missing permission, an unlisted bot or a repo that
//! is not on the list all mean no. The labeler comes from GitHub's events, never from the text.

use super::github::{ApiIssue, LabelEvent};
use super::model::{Config, IssueRun, IssueSnapshot, RepoWatch, RunState, Trigger, PARALLEL_DEFAULT, PARALLEL_MAX};

/// The 24 hour window for the runs-per-day cap.
pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Who may trigger a run: your own login, anyone on your list, a bot only when listed exactly, and
/// (when `write_access_counts` is on) someone with admin, maintain or write permission. Anything
/// else is refused. A permission that could not be read is `None`, and that is refused too.
pub fn trigger(me: &str, cfg: &Config, ev: &LabelEvent, permission: Option<&str>) -> Trigger {
    let listed = cfg.allowed.iter().any(|login| login.eq_ignore_ascii_case(&ev.actor));
    if ev.actor_is_bot {
        return if listed {
            Trigger::Allowed { why: "listed bot" }
        } else {
            Trigger::Refused { why: format!("@{} is a bot that is not on your list", ev.actor) }
        };
    }
    if same_login(me, &ev.actor) {
        return Trigger::Allowed { why: "you" };
    }
    if listed {
        return Trigger::Allowed { why: "on your list" };
    }
    let role = permission.map(str::to_ascii_lowercase);
    if cfg.write_access_counts && matches!(role.as_deref(), Some("admin" | "maintain" | "write")) {
        return Trigger::Allowed { why: "write access" };
    }
    Trigger::Refused { why: format!("@{} is not you or on your list", ev.actor) }
}

/// Is this label event a new candidate for this repo? `None` means no: the number is ignored, or
/// this label event already has a record. `Some(Err(reason))` is a skip (the label came before the
/// repo was added). `Some(Ok(()))` means a start card may be built. Who labeled is `trigger`'s job,
/// so the caller must call this only for an `Allowed` labeler.
pub fn candidate(
    repo: &RepoWatch,
    cfg: &Config,
    issue: &ApiIssue,
    ev: &LabelEvent,
    runs: &[IssueRun],
) -> Option<Result<(), String>> {
    let ignored = cfg
        .ignored
        .iter()
        .any(|(name, n)| *n == issue.number && name.eq_ignore_ascii_case(&repo.owner_repo));
    let known = runs
        .iter()
        .any(|run| run.repo_id == repo.id && run.issue.number == issue.number && run.issue.label_event_id == ev.id);
    if ignored || known {
        return None;
    }
    if ev.at_ms < repo.added_at_ms {
        return Some(Err("Labeled before you added this repo".to_string()));
    }
    Some(Ok(()))
}

/// Why the start card cannot be skipped, in simple English. Empty means the card may follow the
/// repo's own "ask me before each run" switch. These reasons force a card even when that switch
/// is off: the author is not you, the labeler or on your list; hidden text was removed; the text
/// was cut; or this is the first run in the repo.
pub fn forced_reasons(repo: &RepoWatch, me: &str, cfg: &Config, snap: &IssueSnapshot, runs: &[IssueRun]) -> Vec<String> {
    let mut out = Vec::new();
    let author_ok = same_login(me, &snap.author)
        || same_login(&snap.labeled_by, &snap.author)
        || cfg.allowed.iter().any(|login| same_login(login, &snap.author));
    if !author_ok {
        let who = if snap.author.is_empty() { "an unknown person".to_string() } else { format!("@{}", snap.author) };
        out.push(format!("Written by {who}, who is not on your list"));
    }
    if snap.notes.html_comments > 0 || snap.notes.invisible_chars > 0 {
        out.push("Hidden text was removed from the issue".to_string());
    }
    if snap.notes.cut {
        out.push("The issue text was too long and was cut".to_string());
    }
    if !runs.iter().any(|run| run.repo_id == repo.id && run.started_at_ms.is_some()) {
        out.push("This is the first run in this repo".to_string());
    }
    out
}

/// Runs started in this repo in the last 24 hours. A run counts from its start, not from its card.
pub fn day_count(runs: &[IssueRun], repo_id: &str, now_ms: u64) -> u32 {
    let count = runs
        .iter()
        .filter(|run| run.repo_id == repo_id)
        .filter(|run| matches!(run.started_at_ms, Some(at) if at.saturating_add(DAY_MS) > now_ms))
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The next approved run that may start now, oldest first. Nothing starts when the number of busy
/// runs (all repos) has reached the parallel cap (1 to 3). A run also waits when its repo is
/// paused, is no longer on the list, or has used its runs-per-day cap.
pub fn next_queued(runs: &[IssueRun], cfg: &Config, now_ms: u64) -> Option<String> {
    let limit = cfg.parallel.clamp(PARALLEL_DEFAULT, PARALLEL_MAX) as usize;
    let busy = runs.iter().filter(|run| is_busy(&run.state)).count();
    if busy >= limit {
        return None;
    }
    let mut queued: Vec<&IssueRun> = runs.iter().filter(|run| matches!(run.state, RunState::Queued { .. })).collect();
    queued.sort_by_key(|run| run.at_ms);
    queued
        .into_iter()
        .find(|run| {
            cfg.repos.iter().find(|repo| repo.id == run.repo_id).is_some_and(|repo| {
                !repo.paused && day_count(runs, &repo.id, now_ms) < repo.runs_per_day
            })
        })
        .map(|run| run.id.clone())
}

/// A run that holds a slot: its agent is starting, running, being checked, or being pushed.
/// A run waiting on a card, or finished, holds no slot.
fn is_busy(state: &RunState) -> bool {
    matches!(state, RunState::Starting | RunState::Running { .. } | RunState::Checking | RunState::Pushing)
}

fn same_login(a: &str, b: &str) -> bool {
    !a.is_empty() && a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issue_intake::github;
    use crate::issue_intake::model::{AgentPick, CleanNotes};

    fn ms(at: &str) -> u64 {
        chrono::DateTime::parse_from_rfc3339(at).unwrap().timestamp_millis() as u64
    }

    fn repo(id: &str) -> RepoWatch {
        RepoWatch {
            id: id.to_string(),
            owner_repo: "octo-org/billing".to_string(),
            folder: "C:/Users/you/code/billing".to_string(),
            agent: AgentPick { agent_type: "fake-agent".to_string(), account: None },
            added_at_ms: ms("2026-10-26T00:00:00Z"),
            ..RepoWatch::default()
        }
    }

    fn cfg(repos: Vec<RepoWatch>) -> Config {
        Config { enabled: true, repos, ..Config::default() }
    }

    fn event(id: u64, actor: &str, bot: bool, at: &str) -> LabelEvent {
        LabelEvent { id, actor: actor.to_string(), actor_is_bot: bot, at_ms: ms(at) }
    }

    fn api_issue(number: u64) -> ApiIssue {
        ApiIssue {
            number,
            title: "Totals are wrong".to_string(),
            body: String::new(),
            author: "you".to_string(),
            author_association: "OWNER".to_string(),
            url: format!("https://github.com/octo-org/billing/issues/{number}"),
            updated_at: "2026-10-27T09:00:00Z".to_string(),
            locked: false,
        }
    }

    fn snapshot(number: u64, author: &str, labeled_by: &str) -> IssueSnapshot {
        IssueSnapshot {
            owner_repo: "octo-org/billing".to_string(),
            number,
            url: format!("https://github.com/octo-org/billing/issues/{number}"),
            title: "Totals are wrong".to_string(),
            body: "The total ignores the discount line.".to_string(),
            author: author.to_string(),
            author_association: "NONE".to_string(),
            labeled_by: labeled_by.to_string(),
            labeled_by_bot: false,
            label_event_id: 1001,
            labeled_at_ms: ms("2026-10-27T10:00:00Z"),
            notes: CleanNotes::default(),
            sha8: "0a1b2c3d".to_string(),
        }
    }

    /// A run for one issue. `at` orders the queue; `started` is when it started, if it did.
    fn run_for(id: &str, repo_id: &str, number: u64, event_id: u64, at: &str, started: Option<&str>, state: RunState) -> IssueRun {
        let mut issue = snapshot(number, "you", "you");
        issue.label_event_id = event_id;
        IssueRun {
            id: id.to_string(),
            repo_id: repo_id.to_string(),
            issue,
            agent: AgentPick { agent_type: "fake-agent".to_string(), account: None },
            agent_label: "Fake agent".to_string(),
            at_ms: ms(at),
            started_at_ms: started.map(ms),
            forced_ask: Vec::new(),
            approved_by: None,
            handle: None,
            branch: None,
            base: None,
            git_fingerprint: None,
            state,
        }
    }

    fn pr_open() -> RunState {
        RunState::PrOpen { number: 57, url: "https://github.com/octo-org/billing/pull/57".to_string() }
    }

    #[test]
    fn you_and_your_list_are_allowed() {
        let mut c = cfg(vec![]);
        c.allowed = vec!["helper".to_string()];
        let mine = event(1, "you", false, "2026-10-27T10:00:00Z");
        let listed = event(2, "Helper", false, "2026-10-27T10:00:00Z");
        assert_eq!(trigger("you", &c, &mine, None), Trigger::Allowed { why: "you" });
        assert_eq!(trigger("you", &c, &listed, None), Trigger::Allowed { why: "on your list" });
    }

    #[test]
    fn non_collaborator_labeler_is_refused_even_with_write_access_on() {
        let mut c = cfg(vec![]);
        c.write_access_counts = true;
        let stranger = event(3, "stranger", false, "2026-10-27T10:00:00Z");
        assert!(matches!(trigger("you", &c, &stranger, None), Trigger::Refused { .. }));
        assert!(matches!(trigger("you", &c, &stranger, Some("read")), Trigger::Refused { .. }));
    }

    #[test]
    fn write_access_counts_only_for_admin_maintain_write_when_the_switch_is_on() {
        let triage = github::parse_permission(include_str!("fixtures/gh/permission_triage.json"));
        let write = github::parse_permission(include_str!("fixtures/gh/permission_write.json"));
        assert_eq!(triage.as_deref(), Some("triage"));
        assert_eq!(write.as_deref(), Some("write"));
        let ev = event(4, "someone", false, "2026-10-27T10:00:00Z");
        let mut c = cfg(vec![]);
        c.write_access_counts = true;
        assert!(matches!(trigger("you", &c, &ev, triage.as_deref()), Trigger::Refused { .. }));
        assert!(matches!(trigger("you", &c, &ev, Some("read")), Trigger::Refused { .. }));
        assert_eq!(trigger("you", &c, &ev, Some("maintain")), Trigger::Allowed { why: "write access" });
        assert_eq!(trigger("you", &c, &ev, Some("admin")), Trigger::Allowed { why: "write access" });
        assert_eq!(trigger("you", &c, &ev, write.as_deref()), Trigger::Allowed { why: "write access" });
        // A permission that could not be read is refused.
        assert!(matches!(trigger("you", &c, &ev, None), Trigger::Refused { .. }));
        // With the switch off, write access is not enough.
        c.write_access_counts = false;
        assert!(matches!(trigger("you", &c, &ev, write.as_deref()), Trigger::Refused { .. }));
    }

    #[test]
    fn bots_only_when_listed_exactly() {
        let ev = github::latest_labeled(include_str!("fixtures/gh/events_bot.json"), "puppet").unwrap();
        assert!(ev.actor_is_bot);
        let mut c = cfg(vec![]);
        c.write_access_counts = true;
        assert!(matches!(trigger("you", &c, &ev, Some("admin")), Trigger::Refused { .. }));
        c.allowed = vec!["octo-org-ci".to_string()];
        assert!(matches!(trigger("you", &c, &ev, None), Trigger::Refused { .. }));
        c.allowed = vec!["octo-org-ci[bot]".to_string()];
        assert_eq!(trigger("you", &c, &ev, None), Trigger::Allowed { why: "listed bot" });
    }

    #[test]
    fn the_latest_label_event_decides_the_labeler() {
        let ev = github::latest_labeled(include_str!("fixtures/gh/events_relabeled.json"), "puppet").unwrap();
        assert_eq!(ev.id, 2003);
        assert!(matches!(trigger("you", &cfg(vec![]), &ev, None), Trigger::Refused { .. }));
    }

    #[test]
    fn the_same_label_event_is_not_new_twice() {
        let r = repo("rw-0123abcd");
        let c = cfg(vec![r.clone()]);
        let known = vec![run_for("ir-000000000001", "rw-0123abcd", 42, 1001, "2026-10-27T10:00:00Z", None, RunState::Declined)];
        let same = event(1001, "you", false, "2026-10-27T10:00:00Z");
        let relabel = event(1002, "you", false, "2026-10-27T12:00:00Z");
        assert_eq!(candidate(&r, &c, &api_issue(42), &same, &known), None);
        assert_eq!(candidate(&r, &c, &api_issue(42), &relabel, &known), Some(Ok(())));
    }

    #[test]
    fn an_ignored_number_is_never_asked_again() {
        let r = repo("rw-0123abcd");
        let mut c = cfg(vec![r.clone()]);
        c.ignored = vec![("octo-org/billing".to_string(), 42)];
        let ev = event(1001, "you", false, "2026-10-27T10:00:00Z");
        assert_eq!(candidate(&r, &c, &api_issue(42), &ev, &[]), None);
        assert_eq!(candidate(&r, &c, &api_issue(43), &ev, &[]), Some(Ok(())));
    }

    #[test]
    fn a_label_from_before_the_repo_was_added_is_a_skip() {
        let r = repo("rw-0123abcd");
        let c = cfg(vec![r.clone()]);
        let before = event(9, "you", false, "2026-10-25T23:00:00Z");
        let at_baseline = event(10, "you", false, "2026-10-26T00:00:00Z");
        assert_eq!(
            candidate(&r, &c, &api_issue(42), &before, &[]),
            Some(Err("Labeled before you added this repo".to_string()))
        );
        assert_eq!(candidate(&r, &c, &api_issue(42), &at_baseline, &[]), Some(Ok(())));
    }

    #[test]
    fn forced_when_the_author_is_not_on_the_list() {
        let r = repo("rw-0123abcd");
        let mut c = cfg(vec![r.clone()]);
        let started = run_for("ir-000000000001", "rw-0123abcd", 41, 900, "2026-10-26T08:00:00Z", Some("2026-10-26T08:01:00Z"), pr_open());
        let snap = snapshot(42, "someone", "you");
        assert_eq!(
            forced_reasons(&r, "you", &c, &snap, &[started.clone()]),
            vec!["Written by @someone, who is not on your list".to_string()]
        );
        c.allowed = vec!["Someone".to_string()];
        assert!(forced_reasons(&r, "you", &c, &snap, &[started]).is_empty());
    }

    #[test]
    fn an_author_who_is_the_labeler_or_you_is_not_forced() {
        let r = repo("rw-0123abcd");
        let c = cfg(vec![r.clone()]);
        let started = run_for("ir-000000000001", "rw-0123abcd", 41, 900, "2026-10-26T08:00:00Z", Some("2026-10-26T08:01:00Z"), pr_open());
        let by_labeler = snapshot(42, "helper", "helper");
        let by_you = snapshot(43, "you", "stranger");
        assert!(forced_reasons(&r, "you", &c, &by_labeler, &[started.clone()]).is_empty());
        assert!(forced_reasons(&r, "you", &c, &by_you, &[started]).is_empty());
    }

    #[test]
    fn hidden_text_and_a_cut_force_the_card() {
        let r = repo("rw-0123abcd");
        let c = cfg(vec![r.clone()]);
        let started = run_for("ir-000000000001", "rw-0123abcd", 41, 900, "2026-10-26T08:00:00Z", Some("2026-10-26T08:01:00Z"), pr_open());
        let mut hidden = snapshot(42, "you", "you");
        hidden.notes = CleanNotes { html_comments: 1, invisible_chars: 0, cut: false };
        let mut zero_width = snapshot(42, "you", "you");
        zero_width.notes = CleanNotes { html_comments: 0, invisible_chars: 2, cut: false };
        let mut cut = snapshot(42, "you", "you");
        cut.notes = CleanNotes { html_comments: 0, invisible_chars: 0, cut: true };
        assert_eq!(forced_reasons(&r, "you", &c, &hidden, &[started.clone()]), vec!["Hidden text was removed from the issue".to_string()]);
        assert_eq!(forced_reasons(&r, "you", &c, &zero_width, &[started.clone()]), vec!["Hidden text was removed from the issue".to_string()]);
        assert_eq!(forced_reasons(&r, "you", &c, &cut, &[started]), vec!["The issue text was too long and was cut".to_string()]);
    }

    #[test]
    fn the_first_run_in_a_repo_is_forced_and_a_declined_one_does_not_count() {
        let r = repo("rw-0123abcd");
        let c = cfg(vec![r.clone()]);
        let snap = snapshot(42, "you", "you");
        let first = vec!["This is the first run in this repo".to_string()];
        assert_eq!(forced_reasons(&r, "you", &c, &snap, &[]), first);
        let declined = run_for("ir-000000000002", "rw-0123abcd", 41, 900, "2026-10-26T08:00:00Z", None, RunState::Declined);
        assert_eq!(forced_reasons(&r, "you", &c, &snap, &[declined]), first);
        let other_repo = run_for("ir-000000000003", "rw-ffffffff", 41, 901, "2026-10-26T08:00:00Z", Some("2026-10-26T08:01:00Z"), pr_open());
        assert_eq!(forced_reasons(&r, "you", &c, &snap, &[other_repo]), first);
        let started = run_for("ir-000000000004", "rw-0123abcd", 40, 902, "2026-10-25T08:00:00Z", Some("2026-10-25T08:01:00Z"), pr_open());
        assert!(forced_reasons(&r, "you", &c, &snap, &[started]).is_empty());
    }

    #[test]
    fn day_count_counts_starts_in_the_last_24_hours_for_one_repo() {
        let now = ms("2026-10-28T12:00:00Z");
        let runs = vec![
            run_for("ir-000000000001", "rw-aaaaaaaa", 1, 1, "2026-10-28T11:00:00Z", Some("2026-10-28T11:00:00Z"), RunState::Starting),
            run_for("ir-000000000002", "rw-aaaaaaaa", 2, 2, "2026-10-27T13:00:00Z", Some("2026-10-27T13:00:00Z"), pr_open()),
            run_for("ir-000000000003", "rw-aaaaaaaa", 3, 3, "2026-10-27T11:00:00Z", Some("2026-10-27T11:00:00Z"), pr_open()),
            run_for("ir-000000000004", "rw-aaaaaaaa", 4, 4, "2026-10-28T11:30:00Z", None, RunState::Queued { why: "day cap".to_string() }),
            run_for("ir-000000000005", "rw-bbbbbbbb", 5, 5, "2026-10-28T11:30:00Z", Some("2026-10-28T11:30:00Z"), RunState::Starting),
        ];
        assert_eq!(day_count(&runs, "rw-aaaaaaaa", now), 2);
        assert_eq!(day_count(&runs, "rw-bbbbbbbb", now), 1);
        assert_eq!(day_count(&runs, "rw-cccccccc", now), 0);
    }

    #[test]
    fn next_queued_respects_the_parallel_cap() {
        let a = RepoWatch { id: "rw-aaaaaaaa".to_string(), ..repo("rw-aaaaaaaa") };
        let b = RepoWatch { id: "rw-bbbbbbbb".to_string(), ..repo("rw-bbbbbbbb") };
        let running = RunState::Running { handle: "issue-10-a1b2".into(), operation_id: "op-1".into(), pane_id: None };
        let runs = vec![
            run_for("ir-000000000010", "rw-bbbbbbbb", 10, 10, "2026-10-28T08:00:00Z", Some("2026-10-28T08:01:00Z"), running),
            run_for("ir-000000000011", "rw-aaaaaaaa", 11, 11, "2026-10-28T09:00:00Z", None, RunState::Queued { why: "slot".into() }),
        ];
        let now = ms("2026-10-28T12:00:00Z");
        let mut c = cfg(vec![a.clone(), b.clone()]);
        assert_eq!(next_queued(&runs, &c, now), None);
        c.parallel = 2;
        assert_eq!(next_queued(&runs, &c, now), Some("ir-000000000011".to_string()));
        c.parallel = 0;
        assert_eq!(next_queued(&runs, &c, now), None);
    }

    #[test]
    fn next_queued_takes_the_oldest_and_skips_capped_or_paused_repos() {
        let now = ms("2026-10-28T12:00:00Z");
        let capped = RepoWatch { runs_per_day: 1, ..repo("rw-aaaaaaaa") };
        let paused = RepoWatch { paused: true, ..repo("rw-bbbbbbbb") };
        let open = repo("rw-cccccccc");
        let used_today = run_for("ir-000000000020", "rw-aaaaaaaa", 20, 20, "2026-10-28T06:00:00Z", Some("2026-10-28T06:01:00Z"), pr_open());
        let queued_capped = run_for("ir-000000000021", "rw-aaaaaaaa", 21, 21, "2026-10-28T07:00:00Z", None, RunState::Queued { why: "day cap".into() });
        let queued_paused = run_for("ir-000000000022", "rw-bbbbbbbb", 22, 22, "2026-10-28T08:00:00Z", None, RunState::Queued { why: "slot".into() });
        let queued_newer = run_for("ir-000000000023", "rw-cccccccc", 23, 23, "2026-10-28T10:00:00Z", None, RunState::Queued { why: "slot".into() });
        let queued_oldest = run_for("ir-000000000024", "rw-cccccccc", 24, 24, "2026-10-28T09:00:00Z", None, RunState::Queued { why: "slot".into() });
        let runs = vec![used_today, queued_capped, queued_paused, queued_newer, queued_oldest];
        let mut c = cfg(vec![capped, paused, open]);
        c.parallel = 3;
        assert_eq!(next_queued(&runs, &c, now), Some("ir-000000000024".to_string()));
    }
}
