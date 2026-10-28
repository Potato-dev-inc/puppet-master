//! Red to Green: the ownership rules for a watched pull request and the offer rules (SPEC n33
//! section 3.3). Pure: no I/O. `owns` runs on every poll and refuses anything that is not a pull
//! request Puppet Master opened. `should_offer` decides whether one failure raises one card. The
//! rest are the day count, the one-at-a-time rule, the poll timing and the 14-day watch limit.

use super::github::{ApiPull, CiRead};
use super::model::{
    valid_sha, FailedCheck, FixRun, FixState, Settings, Watch, WatchState, BACKOFF_MAX_S, CHECKS_SHOWN_MAX,
    CI_FIX_FORK, CI_FIX_HEAD_MOVED, CI_FIX_NOT_PM_BRANCH, CI_FIX_NOT_YOURS, CI_FIX_PR_CLOSED, HUNG_CHECK_MS,
    POLL_PENDING_S, POLL_PR_S, WATCH_IDLE_DAYS,
};

const DAY_MS: u64 = 24 * 3600 * 1000;

/// What a failure on a Puppet Master commit leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// No card: checks still run, the failure was already offered, or the watch is not active.
    Nothing,
    /// Ask the person once.
    Card,
    /// The tries for this pull request are used up. A notice, no fix button.
    TriesUsed,
    /// The fixes for the day are used up. The row says so and the check is tried again tomorrow.
    DayCap,
}

/// Is this pull request one Puppet Master may watch and fix? Checked on every poll. The error is
/// the code and a plain English line. Order: closed, fork, author, base repo, branch, head commit.
pub fn owns(w: &Watch, me: &str, pull: &ApiPull, default_branch: Option<&str>) -> Result<(), (&'static str, String)> {
    if pull.state != "open" || pull.merged {
        return Err((CI_FIX_PR_CLOSED, "This pull request is closed or merged.".to_string()));
    }
    match pull.head_repo.as_deref() {
        Some(head) if head.eq_ignore_ascii_case(&pull.base_repo) => {}
        _ => return Err((CI_FIX_FORK, "This pull request comes from a fork. Puppet Master won't fix it.".to_string())),
    }
    if me.is_empty() || pull.author.is_empty() || !pull.author.eq_ignore_ascii_case(me) {
        return Err((CI_FIX_NOT_YOURS, "Someone else opened this pull request.".to_string()));
    }
    if !pull.base_repo.eq_ignore_ascii_case(&w.owner_repo) {
        return Err((CI_FIX_NOT_YOURS, "This pull request is for another repository.".to_string()));
    }
    let default_is_head = default_branch.is_some_and(|d| d.eq_ignore_ascii_case(&pull.head_ref));
    if pull.head_ref != w.branch || !pm_branch_ok(&pull.head_ref, &pull.base_ref) || default_is_head {
        return Err((CI_FIX_NOT_PM_BRANCH, "This branch is not one Puppet Master opened.".to_string()));
    }
    if !w.pushed_shas.iter().any(|sha| sha == &pull.head_sha) {
        return Err((CI_FIX_HEAD_MOVED, "Someone pushed to this branch after Puppet Master.".to_string()));
    }
    Ok(())
}

/// The branch rule `pr::guard_branch` applies. That function is private to `worktrees`, so the same
/// rule is kept here: `pm/` only, never the base or a protected name, no `..`, no leading `+`.
fn pm_branch_ok(branch: &str, base: &str) -> bool {
    let protected = ["main", "master", "trunk", "develop"];
    branch.starts_with("pm/")
        && branch != base
        && !protected.iter().any(|p| branch.eq_ignore_ascii_case(p))
        && !branch.contains("..")
        && !branch.starts_with('+')
}

/// Should this failure raise a card? One card per (watch, failing sha): any fix already recorded
/// for that sha (asked, declined, expired, running or finished) means no card. A failure is offered
/// only when no check is still running, or after `HUNG_CHECK_MS` from the first failure (a hung
/// check). Then the tries for the pull request, then the fixes for the day.
pub fn should_offer(
    w: &Watch,
    read: &CiRead,
    sha: &str,
    fixes: &[FixRun],
    s: &Settings,
    now_ms: u64,
    first_failure_ms: u64,
) -> Offer {
    let s = s.clamped();
    if !s.enabled || !matches!(w.state, WatchState::Watching) || !valid_sha(sha) {
        return Offer::Nothing;
    }
    let still_running = match read {
        CiRead::Failed { still_running, .. } => *still_running,
        _ => return Offer::Nothing,
    };
    if fixes.iter().any(|f| f.watch_id == w.id && f.failing_sha == sha) {
        return Offer::Nothing;
    }
    if still_running && now_ms.saturating_sub(first_failure_ms) < HUNG_CHECK_MS {
        return Offer::Nothing;
    }
    if w.tries_used >= s.tries_per_pr {
        return Offer::TriesUsed;
    }
    if day_count(fixes, now_ms) >= s.fixes_per_day {
        return Offer::DayCap;
    }
    Offer::Card
}

/// The fixes that count toward the day: every try that was asked and said yes (or is running or
/// finished) in the last 24 hours. A card that was not answered yet, declined or expired does not
/// count. A queued fix counts, because it will spend.
pub fn day_count(fixes: &[FixRun], now_ms: u64) -> u32 {
    fixes
        .iter()
        .filter(|f| !matches!(f.state, FixState::Asking { .. } | FixState::Declined | FixState::Expired))
        .filter(|f| now_ms < f.at_ms.saturating_add(DAY_MS))
        .count() as u32
}

/// True while a fix uses the copy and the agent (one fix at a time, all repositories).
pub fn one_running(fixes: &[FixRun]) -> bool {
    fixes.iter().any(|f| {
        matches!(f.state, FixState::Starting | FixState::Running { .. } | FixState::Checking | FixState::Pushing)
    })
}

/// Is a poll due? Only an active watch is polled. The wait is 60 s while checks run or after a
/// push, else 5 min, and a longer back-off after errors (up to 15 min).
pub fn due(w: &Watch, last_poll_ms: u64, pending: bool, backoff_s: u64, now_ms: u64) -> bool {
    if !matches!(w.state, WatchState::Watching) {
        return false;
    }
    let base = if pending { POLL_PENDING_S } else { POLL_PR_S };
    let wait_s = base.max(backoff_s).min(BACKOFF_MAX_S);
    now_ms >= last_poll_ms.saturating_add(wait_s * 1000)
}

/// Has the watch gone 14 days without a push from Puppet Master?
pub fn expired(w: &Watch, now_ms: u64) -> bool {
    now_ms.saturating_sub(w.last_push_ms) >= WATCH_IDLE_DAYS * DAY_MS
}

/// The checks a fix record keeps and a card shows: at most `CHECKS_SHOWN_MAX`. `read_ci` returns
/// every failing check; the cap is applied where the record is made.
pub fn shown_checks(checks: &[FailedCheck]) -> &[FailedCheck] {
    &checks[..checks.len().min(CHECKS_SHOWN_MAX)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_fix::github::{parse_pull, read_ci, ApiCheck};
    use crate::ci_fix::model::{LogSnapshot, LogNotes};
    use crate::issue_intake::model::AgentPick;

    const PULL_OPEN: &str = include_str!("fixtures/gh/pull_open.json");
    const PULL_FORK: &str = include_str!("fixtures/gh/pull_fork.json");
    const PULL_OTHER_AUTHOR: &str = include_str!("fixtures/gh/pull_other_author.json");
    const PULL_MERGED: &str = include_str!("fixtures/gh/pull_merged.json");

    const HEAD: &str = "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d";
    const OLD_HEAD: &str = "2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e";
    const NOW: u64 = 1_800_000_000_000;
    const DAY: u64 = 24 * 3600 * 1000;

    fn sha_n(n: u64) -> String {
        format!("{:040x}", n)
    }

    fn pull(json: &str) -> ApiPull {
        parse_pull(json).expect("fixture parses")
    }

    fn watch() -> Watch {
        Watch {
            id: "cw-0a1b2c3d".to_string(),
            project: "/repos/billing".to_string(),
            handle: "h-1".to_string(),
            owner_repo: "acme/billing".to_string(),
            branch: "pm/issue-42-a1b2".to_string(),
            base: "main".to_string(),
            pr_number: 61,
            pr_url: "https://github.com/acme/billing/pull/61".to_string(),
            agent: AgentPick { agent_type: "fake-agent".to_string(), account: None },
            agent_label: "Fake agent".to_string(),
            source: "issue".to_string(),
            added_at_ms: NOW - 10 * DAY,
            last_push_ms: NOW,
            pushed_shas: vec![HEAD.to_string()],
            tries_used: 0,
            state: WatchState::Watching,
        }
    }

    fn check() -> FailedCheck {
        FailedCheck { name: "test".to_string(), conclusion: "failure".to_string(), job_id: Some(7), url: String::new() }
    }

    fn failed(still_running: bool) -> CiRead {
        CiRead::Failed { checks: vec![check()], still_running }
    }

    fn fix(id: &str, watch_id: &str, sha: &str, at_ms: u64, state: FixState) -> FixRun {
        FixRun {
            id: id.to_string(),
            watch_id: watch_id.to_string(),
            try_no: 1,
            failing_sha: sha.to_string(),
            checks: vec![],
            log: LogSnapshot { jobs: vec![], notes: LogNotes::default(), sha8: String::new() },
            at_ms,
            first_failure_ms: at_ms,
            approved_by: None,
            git_fingerprint: None,
            state,
        }
    }

    fn settings() -> Settings {
        Settings { enabled: true, ..Settings::default() }
    }

    #[test]
    fn owns_accepts_own_open_pm_pull_request() {
        assert_eq!(owns(&watch(), "you", &pull(PULL_OPEN), Some("main")), Ok(()));
    }

    #[test]
    fn owns_fork_pr_is_refused_with_fork_code() {
        // The fork is also by someone else; the fork check comes first, so the code is CI_FIX_FORK.
        let err = owns(&watch(), "you", &pull(PULL_FORK), Some("main")).unwrap_err();
        assert_eq!(err.0, CI_FIX_FORK);
    }

    #[test]
    fn owns_deleted_fork_is_refused_with_fork_code() {
        let mut p = pull(PULL_OPEN);
        p.head_repo = None;
        assert_eq!(owns(&watch(), "you", &p, Some("main")).unwrap_err().0, CI_FIX_FORK);
    }

    #[test]
    fn owns_other_author_is_refused() {
        assert_eq!(owns(&watch(), "you", &pull(PULL_OTHER_AUTHOR), Some("main")).unwrap_err().0, CI_FIX_NOT_YOURS);
    }

    #[test]
    fn owns_empty_login_is_refused() {
        assert_eq!(owns(&watch(), "", &pull(PULL_OPEN), Some("main")).unwrap_err().0, CI_FIX_NOT_YOURS);
    }

    #[test]
    fn owns_base_in_another_repo_is_refused() {
        let mut p = pull(PULL_OPEN);
        p.head_repo = Some("acme/other".to_string());
        p.base_repo = "acme/other".to_string();
        assert_eq!(owns(&watch(), "you", &p, Some("main")).unwrap_err().0, CI_FIX_NOT_YOURS);
    }

    #[test]
    fn owns_head_ref_that_differs_from_the_copy_is_refused() {
        let mut p = pull(PULL_OPEN);
        p.head_ref = "pm/issue-43-c3d4".to_string();
        assert_eq!(owns(&watch(), "you", &p, Some("main")).unwrap_err().0, CI_FIX_NOT_PM_BRANCH);
    }

    #[test]
    fn owns_base_equal_to_head_is_refused() {
        let mut p = pull(PULL_OPEN);
        p.base_ref = p.head_ref.clone();
        assert_eq!(owns(&watch(), "you", &p, Some("main")).unwrap_err().0, CI_FIX_NOT_PM_BRANCH);
    }

    #[test]
    fn owns_default_branch_is_refused() {
        let p = pull(PULL_OPEN);
        assert_eq!(owns(&watch(), "you", &p, Some("pm/issue-42-a1b2")).unwrap_err().0, CI_FIX_NOT_PM_BRANCH);
    }

    #[test]
    fn owns_protected_branch_name_is_refused() {
        let mut w = watch();
        w.branch = "develop".to_string();
        let mut p = pull(PULL_OPEN);
        p.head_ref = "develop".to_string();
        assert_eq!(owns(&w, "you", &p, Some("main")).unwrap_err().0, CI_FIX_NOT_PM_BRANCH);
    }

    #[test]
    fn owns_head_sha_not_pushed_by_pm_is_refused() {
        let mut w = watch();
        w.pushed_shas = vec![OLD_HEAD.to_string()];
        assert_eq!(owns(&w, "you", &pull(PULL_OPEN), Some("main")).unwrap_err().0, CI_FIX_HEAD_MOVED);
    }

    #[test]
    fn owns_closed_or_merged_pull_request_is_refused() {
        assert_eq!(owns(&watch(), "you", &pull(PULL_MERGED), Some("main")).unwrap_err().0, CI_FIX_PR_CLOSED);
        let mut closed = pull(PULL_OPEN);
        closed.state = "closed".to_string();
        assert_eq!(owns(&watch(), "you", &closed, Some("main")).unwrap_err().0, CI_FIX_PR_CLOSED);
    }

    #[test]
    fn fork_pr_failure_is_never_offered() {
        // owns refuses the fork, so the engine pauses the watch; a paused watch raises no card.
        let mut w = watch();
        w.state = WatchState::Paused { why: "fork".to_string() };
        let offer = should_offer(&w, &failed(false), HEAD, &[], &settings(), NOW, NOW);
        assert_eq!(offer, Offer::Nothing);
    }

    #[test]
    fn one_card_per_watch_and_sha() {
        let w = watch();
        let states = [
            FixState::Asking { question_id: "q-1".to_string() },
            FixState::Declined,
            FixState::Expired,
            FixState::Pushed { sha: OLD_HEAD.to_string() },
            FixState::Green { sha: HEAD.to_string() },
            FixState::StillRed { sha: HEAD.to_string() },
            FixState::Failed { code: "X".to_string(), message: String::new() },
        ];
        for state in states {
            let fixes = vec![fix("cf-000000000001", &w.id, HEAD, NOW - 1000, state.clone())];
            assert_eq!(
                should_offer(&w, &failed(false), HEAD, &fixes, &settings(), NOW, NOW),
                Offer::Nothing,
                "state {state:?} must not raise a second card"
            );
        }
    }

    #[test]
    fn a_new_sha_or_another_watch_raises_a_card() {
        let w = watch();
        let fixes = vec![
            fix("cf-000000000001", &w.id, OLD_HEAD, NOW - 1000, FixState::Declined),
            fix("cf-000000000002", "cw-ffffffff", HEAD, NOW - 1000, FixState::Declined),
        ];
        assert_eq!(should_offer(&w, &failed(false), HEAD, &fixes, &settings(), NOW, NOW), Offer::Card);
    }

    #[test]
    fn checks_still_running_wait_then_offer_after_the_hung_limit() {
        let w = watch();
        let first = NOW;
        assert_eq!(should_offer(&w, &failed(true), HEAD, &[], &settings(), first, first), Offer::Nothing);
        assert_eq!(
            should_offer(&w, &failed(true), HEAD, &[], &settings(), first + HUNG_CHECK_MS - 1, first),
            Offer::Nothing
        );
        assert_eq!(
            should_offer(&w, &failed(true), HEAD, &[], &settings(), first + HUNG_CHECK_MS, first),
            Offer::Card
        );
    }

    #[test]
    fn no_check_running_offers_at_once() {
        let w = watch();
        assert_eq!(should_offer(&w, &failed(false), HEAD, &[], &settings(), NOW, NOW), Offer::Card);
    }

    #[test]
    fn non_failure_reads_offer_nothing() {
        let w = watch();
        let reads = [
            CiRead::NoChecks,
            CiRead::Running,
            CiRead::Passed,
            CiRead::Waiting { why: "Waiting for approval on GitHub".to_string() },
        ];
        for read in reads {
            assert_eq!(should_offer(&w, &read, HEAD, &[], &settings(), NOW, NOW), Offer::Nothing);
        }
    }

    #[test]
    fn tries_used_gives_the_tries_notice() {
        let mut w = watch();
        w.tries_used = 2;
        assert_eq!(should_offer(&w, &failed(false), HEAD, &[], &settings(), NOW, NOW), Offer::TriesUsed);
        w.tries_used = 1;
        assert_eq!(should_offer(&w, &failed(false), HEAD, &[], &settings(), NOW, NOW), Offer::Card);
    }

    #[test]
    fn tries_limit_follows_the_setting() {
        let mut w = watch();
        w.tries_used = 2;
        let mut s = settings();
        s.tries_per_pr = 3;
        assert_eq!(should_offer(&w, &failed(false), HEAD, &[], &s, NOW, NOW), Offer::Card);
    }

    #[test]
    fn day_cap_gives_the_day_cap_offer() {
        let w = watch();
        let six: Vec<FixRun> = (0..6)
            .map(|n| fix(&format!("cf-00000000000{n}"), &w.id, &sha_n(n + 1), NOW - 3600_000, FixState::Pushed { sha: sha_n(n + 1) }))
            .collect();
        assert_eq!(should_offer(&w, &failed(false), HEAD, &six, &settings(), NOW, NOW), Offer::DayCap);
        assert_eq!(should_offer(&w, &failed(false), HEAD, &six[..5], &settings(), NOW, NOW), Offer::Card);
    }

    #[test]
    fn day_cap_ignores_old_and_unanswered_fixes() {
        let w = watch();
        let old: Vec<FixRun> = (0..6)
            .map(|n| fix(&format!("cf-00000000000{n}"), &w.id, &sha_n(n + 1), NOW - DAY - 1, FixState::Pushed { sha: sha_n(n + 1) }))
            .collect();
        assert_eq!(should_offer(&w, &failed(false), HEAD, &old, &settings(), NOW, NOW), Offer::Card);
        let asked: Vec<FixRun> = (0..6)
            .map(|n| fix(&format!("cf-00000000000{n}"), &w.id, &sha_n(n + 1), NOW - 1000, FixState::Declined))
            .collect();
        assert_eq!(should_offer(&w, &failed(false), HEAD, &asked, &settings(), NOW, NOW), Offer::Card);
    }

    #[test]
    fn day_count_counts_started_fixes_in_24_hours() {
        let w = watch();
        let fixes = vec![
            fix("cf-000000000001", &w.id, &sha_n(1), NOW - 1000, FixState::Pushed { sha: sha_n(9) }),
            fix("cf-000000000002", &w.id, &sha_n(2), NOW - 1000, FixState::Queued { why: "busy".to_string(), since_ms: NOW }),
            fix("cf-000000000003", &w.id, &sha_n(3), NOW - 1000, FixState::Asking { question_id: "q".to_string() }),
            fix("cf-000000000004", &w.id, &sha_n(4), NOW - DAY - 1, FixState::Pushed { sha: sha_n(9) }),
        ];
        assert_eq!(day_count(&fixes, NOW), 2);
    }

    #[test]
    fn one_running_is_true_while_a_fix_uses_the_copy() {
        let w = watch();
        let busy = [
            FixState::Starting,
            FixState::Running { route: "new_pane".to_string(), handle: "cifix-61-0000".to_string(), operation_id: "op".to_string(), pane_id: None },
            FixState::Checking,
            FixState::Pushing,
        ];
        for state in busy {
            assert!(one_running(&[fix("cf-000000000001", &w.id, HEAD, NOW, state)]));
        }
        let idle = [
            FixState::Asking { question_id: "q".to_string() },
            FixState::Queued { why: "busy".to_string(), since_ms: NOW },
            FixState::AskPush { question_id: "q".to_string(), facts: fix_facts() },
            FixState::Declined,
            FixState::Green { sha: HEAD.to_string() },
        ];
        for state in idle {
            assert!(!one_running(&[fix("cf-000000000001", &w.id, HEAD, NOW, state)]));
        }
        assert!(!one_running(&[]));
    }

    fn fix_facts() -> crate::ci_fix::model::FixFacts {
        use crate::issue_intake::model::PushFacts;
        crate::ci_fix::model::FixFacts {
            push: PushFacts {
                files: 0,
                additions: 0,
                deletions: 0,
                ci_files: vec![],
                secret_files: vec![],
                secret_lines: vec![],
                home_path_lines: vec![],
                internal_files: vec![],
                git_changed: false,
                remote_branch_exists: false,
                issue_changed: false,
                run_status: "completed".to_string(),
                turns: 0,
                minutes: 0,
            },
            skipped_tests: vec![],
            deleted_tests: vec![],
            ci_run_files: vec![],
            web_lines: vec![],
            remote_moved: false,
            history_rewritten: false,
            wrong_branch: false,
            over_cap: false,
            commits: 0,
        }
    }

    #[test]
    fn due_waits_for_pending_or_backoff() {
        let w = watch();
        let last = 1_000_000;
        // checks running or after a push: 60 s
        assert!(!due(&w, last, true, 0, last + 59_999));
        assert!(due(&w, last, true, 0, last + 60_000));
        // quiet PR: 5 min
        assert!(!due(&w, last, false, 0, last + 299_999));
        assert!(due(&w, last, false, 0, last + 300_000));
        // back-off after errors: 15 min at most
        assert!(!due(&w, last, false, 900, last + 899_999));
        assert!(due(&w, last, false, 900, last + 900_000));
        assert!(due(&w, last, false, 5000, last + 900_000));
        assert!(!due(&w, last, false, 5000, last + 899_999));
    }

    #[test]
    fn paused_or_stopped_watch_is_never_due() {
        let mut w = watch();
        w.state = WatchState::Paused { why: "fork".to_string() };
        assert!(!due(&w, 0, false, 0, NOW));
        w.state = WatchState::Stopped { why: "You stopped watching".to_string() };
        assert!(!due(&w, 0, false, 0, NOW));
    }

    #[test]
    fn expired_at_fourteen_days_without_a_push() {
        let w = watch();
        let t = w.last_push_ms;
        assert!(!expired(&w, t + WATCH_IDLE_DAYS * DAY - 1));
        assert!(expired(&w, t + WATCH_IDLE_DAYS * DAY));
    }

    #[test]
    fn inactive_settings_or_bad_input_offer_nothing() {
        let w = watch();
        let mut off = settings();
        off.enabled = false;
        assert_eq!(should_offer(&w, &failed(false), HEAD, &[], &off, NOW, NOW), Offer::Nothing);
        assert_eq!(should_offer(&w, &failed(false), "not-a-sha", &[], &settings(), NOW, NOW), Offer::Nothing);
    }

    #[test]
    fn the_failed_check_list_comes_from_read_ci() {
        let json = include_str!("fixtures/gh/checks_failed.json");
        let checks: Vec<ApiCheck> = crate::ci_fix::github::parse_check_runs(json).expect("fixture parses");
        let read = read_ci(&checks);
        let w = watch();
        assert_eq!(should_offer(&w, &read, HEAD, &[], &settings(), NOW, NOW), Offer::Card);
    }

    #[test]
    fn shown_checks_caps_at_ten() {
        let many: Vec<FailedCheck> = (0..12).map(|_| check()).collect();
        assert_eq!(shown_checks(&many).len(), CHECKS_SHOWN_MAX);
        assert_eq!(shown_checks(&many[..3]).len(), 3);
        assert_eq!(shown_checks(&[]).len(), 0);
    }
}
