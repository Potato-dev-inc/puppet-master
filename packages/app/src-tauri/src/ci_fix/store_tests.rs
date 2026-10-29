//! Store and model tests for Red to Green: id and sha guards, defaults and clamping, the caps,
//! the round trip, the state tags, the log drop on finished fixes, and "a bad id never makes a
//! file". Each test uses its own temp folder (a fresh uuid name), so nothing is shared.

use super::model::{
    valid_fix_id, valid_sha, valid_watch_id, FailedCheck, FixFacts, FixRun, FixState, LogNotes, LogSnapshot,
    Settings, Watch, WatchState, FIX_RECORDS_CAP, WATCHES_CAP,
};
use super::store::{new_fix_id, new_watch_id, Store, STOP_TOO_MANY};
use crate::issue_intake::model::{AgentPick, PushFacts};
use std::fs;
use std::path::PathBuf;

fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("pm-ci-fix-{}", uuid::Uuid::new_v4().simple()))
}

/// A valid lowercase 40-hex commit id, numbered so each one differs.
fn sha(n: u32) -> String {
    format!("{n:040x}")
}

fn watch(id: &str, added_at_ms: u64) -> Watch {
    Watch {
        id: id.to_string(),
        project: "C:/Users/you/work/billing".to_string(),
        handle: format!("copy-{id}"),
        owner_repo: "acme/billing".to_string(),
        branch: "pm/issue-42-a1b2".to_string(),
        base: "main".to_string(),
        pr_number: 61,
        pr_url: "https://github.com/acme/billing/pull/61".to_string(),
        agent: AgentPick { agent_type: "fake-agent".to_string(), account: None },
        agent_label: "Fake agent".to_string(),
        source: "issue".to_string(),
        added_at_ms,
        last_push_ms: 0,
        pushed_shas: vec![sha(1)],
        tries_used: 0,
        state: WatchState::Watching,
    }
}

fn fix(id: &str, watch_id: &str, state: FixState) -> FixRun {
    FixRun {
        id: id.to_string(),
        watch_id: watch_id.to_string(),
        try_no: 1,
        failing_sha: sha(1),
        checks: vec![FailedCheck {
            name: "test".to_string(),
            conclusion: "failure".to_string(),
            job_id: Some(7),
            url: "https://github.com/acme/billing/actions/runs/1".to_string(),
        }],
        log: LogSnapshot {
            jobs: vec![("test".to_string(), vec!["line one".to_string()])],
            notes: LogNotes { lines: 1, ..LogNotes::default() },
            sha8: "0a1b2c3d".to_string(),
        },
        at_ms: 5,
        first_failure_ms: 4,
        approved_by: Some("desktop".to_string()),
        git_fingerprint: None,
        state,
    }
}

fn push_facts() -> PushFacts {
    PushFacts {
        files: 1,
        additions: 2,
        deletions: 0,
        ci_files: Vec::new(),
        secret_files: Vec::new(),
        secret_lines: Vec::new(),
        home_path_lines: Vec::new(),
        internal_files: Vec::new(),
        git_changed: false,
        remote_branch_exists: false,
        issue_changed: false,
        run_status: "completed".to_string(),
        turns: 3,
        minutes: 2,
    }
}

fn fix_facts() -> FixFacts {
    FixFacts {
        push: push_facts(),
        skipped_tests: Vec::new(),
        deleted_tests: Vec::new(),
        ci_run_files: Vec::new(),
        web_lines: Vec::new(),
        remote_moved: false,
        history_rewritten: false,
        wrong_branch: false,
        over_cap: false,
        commits: 1,
    }
}

fn active_watch_count(store: &Store) -> usize {
    store.watches().iter().filter(|item| item.state == WatchState::Watching).count()
}

#[test]
fn ids_and_shas_are_checked() {
    assert!(valid_watch_id("cw-0123abcd"));
    assert!(valid_fix_id("cf-0123456789ab"));
    for bad in ["", "cw-", "cw-0123ABCD", "cw-0123abc", "cw-0123abcde", "cw-../../x", "cw_0123abcd", "xx-0123abcd", " cw-0123abcd"] {
        assert!(!valid_watch_id(bad), "{bad:?} must be refused");
    }
    for bad in ["cf-0123abcdef0", "cf-0123456789AB", "cw-0123456789ab", "cf-../../../../x"] {
        assert!(!valid_fix_id(bad), "{bad:?} must be refused");
    }
    assert!(valid_sha(&sha(255)));
    assert!(!valid_sha(&sha(255).to_uppercase()));
    assert!(!valid_sha(&"a".repeat(39)));
    assert!(!valid_sha(&"a".repeat(41)));
    assert!(!valid_sha("../../../../etc/passwd"));
    assert!(valid_watch_id(&new_watch_id()));
    assert!(valid_fix_id(&new_fix_id()));
}

#[test]
fn defaults_are_the_safe_ones() {
    let defaults = Settings::default();
    assert!(!defaults.enabled, "the Labs switch is off by default");
    assert_eq!(
        (defaults.tries_per_pr, defaults.turns_per_try, defaults.minutes_per_try, defaults.fixes_per_day),
        (2, 15, 20, 6)
    );
    let root = temp_root();
    assert_eq!(Store::at(&root).settings(), defaults);
    assert!(!root.exists(), "reading settings creates nothing");
}

#[test]
fn settings_are_clamped_to_their_ranges() {
    let low = Settings { enabled: true, tries_per_pr: 0, turns_per_try: 999, minutes_per_try: 1, fixes_per_day: 0 };
    assert_eq!(
        low.clamped(),
        Settings { enabled: true, tries_per_pr: 1, turns_per_try: 50, minutes_per_try: 5, fixes_per_day: 1 }
    );
    let high = Settings { enabled: false, tries_per_pr: 9, turns_per_try: 50, minutes_per_try: 500, fixes_per_day: 99 };
    assert_eq!(
        high.clamped(),
        Settings { enabled: false, tries_per_pr: 3, turns_per_try: 50, minutes_per_try: 60, fixes_per_day: 20 }
    );

    let root = temp_root();
    let store = Store::at(&root);
    let saved = store.save_settings(&high).unwrap();
    assert_eq!(saved.minutes_per_try, 60);
    assert_eq!(store.settings(), saved);

    // A hand-edited file with bad numbers reads clamped; missing fields read as the defaults.
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("settings.json"), r#"{"enabled":true,"tries_per_pr":99}"#).unwrap();
    let read = Store::at(&root).settings();
    assert!(read.enabled);
    assert_eq!((read.tries_per_pr, read.turns_per_try, read.minutes_per_try, read.fixes_per_day), (3, 15, 20, 6));
}

#[test]
fn active_watch_cap_stops_the_oldest_idle_watch() {
    let root = temp_root();
    let store = Store::at(&root);
    for n in 0..WATCHES_CAP as u64 {
        store.put_watch(&watch(&format!("cw-{n:08x}"), n)).unwrap();
    }
    assert_eq!(active_watch_count(&store), WATCHES_CAP);

    // The oldest watch (cw-00000000) is busy with a fix, so the next oldest idle one stops.
    store.put_fix(&fix(&new_fix_id(), "cw-00000000", FixState::Asking { question_id: "q1".into() })).unwrap();
    store.put_watch(&watch("cw-000000ff", 99)).unwrap();
    assert_eq!(active_watch_count(&store), WATCHES_CAP);
    assert_eq!(
        store.watch_by_id("cw-00000001").unwrap().state,
        WatchState::Stopped { why: STOP_TOO_MANY.to_string() }
    );
    assert_eq!(store.watch_by_id("cw-00000000").unwrap().state, WatchState::Watching);
    assert_eq!(store.watch_by_id("cw-000000ff").unwrap().state, WatchState::Watching);

    // When every active watch is busy, the new one is refused and nothing is written.
    for (n, item) in store.watches().iter().filter(|item| item.state == WatchState::Watching).enumerate() {
        if item.id != "cw-00000000" {
            store.put_fix(&fix(&format!("cf-{n:012x}"), &item.id, FixState::Starting)).unwrap();
        }
    }
    let before = store.watches().len();
    assert!(store.put_watch(&watch("cw-00000100", 100)).is_err());
    assert_eq!(store.watches().len(), before);
    assert!(store.watch_by_id("cw-00000100").is_none());
}

#[test]
fn fix_cap_keeps_active_fixes_and_drops_the_oldest_finished_one() {
    let root = temp_root();
    let store = Store::at(&root);
    let parent = watch("cw-00000001", 1);
    store.put_watch(&parent).unwrap();

    // The oldest record is still active, so it must survive the cap.
    let active_id = format!("cf-{:012x}", 0);
    store.put_fix(&fix(&active_id, &parent.id, FixState::Asking { question_id: "q0".into() })).unwrap();
    for n in 1..=FIX_RECORDS_CAP as u64 {
        store.put_fix(&fix(&format!("cf-{n:012x}"), &parent.id, FixState::NotPushed)).unwrap();
    }
    let fixes = store.fixes();
    assert_eq!(fixes.len(), FIX_RECORDS_CAP, "the 301st record drops one");
    assert_eq!(fixes[0].id, format!("cf-{:012x}", FIX_RECORDS_CAP as u64), "newest first");
    assert!(store.fix_by_id(&active_id).is_some(), "an active fix is kept");
    assert!(store.fix_by_id(&format!("cf-{:012x}", 1)).is_none(), "the oldest finished fix is dropped");
}

#[test]
fn settings_watches_and_fixes_round_trip() {
    let root = temp_root();
    let store = Store::at(&root);
    let saved = store
        .save_settings(&Settings { enabled: true, tries_per_pr: 3, turns_per_try: 12, minutes_per_try: 30, fixes_per_day: 4 })
        .unwrap();
    let parent = watch("cw-0badcafe", 7);
    store.put_watch(&parent).unwrap();
    let asking = fix(&new_fix_id(), &parent.id, FixState::AskPush { question_id: "q2".into(), facts: fix_facts() });
    store.put_fix(&asking).unwrap();

    // A fresh handle on the same folder reads the same things back.
    let again = Store::at(&root);
    assert_eq!(again.settings(), saved);
    assert_eq!(again.watch_by_id(&parent.id), Some(parent.clone()));
    assert_eq!(again.watch_for(&parent.project, &parent.handle), Some(parent.clone()));
    assert_eq!(again.fix_by_id(&asking.id), Some(asking.clone()));
    assert_eq!(again.fix_by_question("q2").map(|item| item.id), Some(asking.id.clone()));
    assert!(again.fix_by_question("nope").is_none());
    assert!(again.fix_by_question("").is_none());
    assert!(again.watch_for(&parent.project, "other-copy").is_none());
}

#[test]
fn watch_for_prefers_the_live_watch() {
    let root = temp_root();
    let store = Store::at(&root);
    let mut old = watch("cw-00000010", 1);
    old.state = WatchState::Closed;
    let live = watch("cw-00000011", 2);
    store.put_watch(&old).unwrap();
    store.put_watch(&Watch { handle: old.handle.clone(), ..live.clone() }).unwrap();
    let found = store.watch_for(&old.project, &old.handle).unwrap();
    assert_eq!(found.id, live.id);
}

#[test]
fn state_tags_are_stable() {
    let watch_cases = [
        (WatchState::Watching, r#"{"state":"watching"}"#.to_string()),
        (WatchState::Paused { why: "x".into() }, r#"{"state":"paused","why":"x"}"#.to_string()),
        (
            WatchState::Stopped { why: "You stopped watching".into() },
            r#"{"state":"stopped","why":"You stopped watching"}"#.to_string(),
        ),
        (WatchState::Closed, r#"{"state":"closed"}"#.to_string()),
        (WatchState::Merged, r#"{"state":"merged"}"#.to_string()),
    ];
    for (state, golden) in watch_cases {
        assert_eq!(serde_json::to_string(&state).unwrap(), golden);
        assert_eq!(serde_json::from_str::<WatchState>(&golden).unwrap(), state);
    }

    let fix_cases = [
        (FixState::Asking { question_id: "q1".into() }, r#"{"state":"asking","question_id":"q1"}"#.to_string()),
        (FixState::Declined, r#"{"state":"declined"}"#.to_string()),
        (FixState::Expired, r#"{"state":"expired"}"#.to_string()),
        (
            FixState::Queued { why: "busy".into(), since_ms: 5 },
            r#"{"state":"queued","why":"busy","since_ms":5}"#.to_string(),
        ),
        (FixState::Starting, r#"{"state":"starting"}"#.to_string()),
        (
            FixState::Running {
                route: "same_pane".into(),
                handle: "cifix-61-a1b2".into(),
                operation_id: "op1".into(),
                pane_id: None,
            },
            r#"{"state":"running","route":"same_pane","handle":"cifix-61-a1b2","operation_id":"op1","pane_id":null}"#
                .to_string(),
        ),
        (FixState::Checking, r#"{"state":"checking"}"#.to_string()),
        (FixState::Pushing, r#"{"state":"pushing"}"#.to_string()),
        (FixState::Pushed { sha: sha(2) }, format!(r#"{{"state":"pushed","sha":"{}"}}"#, sha(2))),
        (FixState::Green { sha: sha(2) }, format!(r#"{{"state":"green","sha":"{}"}}"#, sha(2))),
        (FixState::StillRed { sha: sha(2) }, format!(r#"{{"state":"still_red","sha":"{}"}}"#, sha(2))),
        (FixState::NoChanges { message: "m".into() }, r#"{"state":"no_changes","message":"m"}"#.to_string()),
        (
            FixState::Held { code: "CI_FIX_GIT_CHANGED".into(), message: "m".into() },
            r#"{"state":"held","code":"CI_FIX_GIT_CHANGED","message":"m"}"#.to_string(),
        ),
        (FixState::NotPushed, r#"{"state":"not_pushed"}"#.to_string()),
        (
            FixState::Failed { code: "CI_FIX_PUSH_FAILED".into(), message: "m".into() },
            r#"{"state":"failed","code":"CI_FIX_PUSH_FAILED","message":"m"}"#.to_string(),
        ),
    ];
    for (state, golden) in fix_cases {
        assert_eq!(serde_json::to_string(&state).unwrap(), golden);
        assert_eq!(serde_json::from_str::<FixState>(&golden).unwrap(), state);
    }

    let ask_push = FixState::AskPush { question_id: "q3".into(), facts: fix_facts() };
    let json = serde_json::to_string(&ask_push).unwrap();
    assert!(json.starts_with(r#"{"state":"ask_push","question_id":"q3","facts":"#), "{json}");
    assert_eq!(serde_json::from_str::<FixState>(&json).unwrap(), ask_push);
}

#[test]
fn log_lines_are_kept_only_while_the_fix_is_active() {
    let root = temp_root();
    let store = Store::at(&root);
    let parent = watch("cw-00000020", 1);
    store.put_watch(&parent).unwrap();

    let active = [
        FixState::Asking { question_id: "q".into() },
        FixState::Queued { why: "busy".into(), since_ms: 1 },
        FixState::Starting,
        FixState::Running { route: "new_pane".into(), handle: "cifix-61-a1b2".into(), operation_id: "op".into(), pane_id: None },
        FixState::Checking,
        FixState::AskPush { question_id: "q".into(), facts: fix_facts() },
        FixState::Pushing,
    ];
    let finished = [
        FixState::Declined,
        FixState::Expired,
        FixState::Pushed { sha: sha(3) },
        FixState::Green { sha: sha(3) },
        FixState::StillRed { sha: sha(3) },
        FixState::NoChanges { message: "m".into() },
        FixState::Held { code: "CI_FIX_CI_FILES".into(), message: "m".into() },
        FixState::NotPushed,
        FixState::Failed { code: "CI_FIX_START_FAILED".into(), message: "m".into() },
    ];
    for state in active {
        let id = new_fix_id();
        store.put_fix(&fix(&id, &parent.id, state)).unwrap();
        assert_eq!(store.fix_by_id(&id).unwrap().log.jobs.len(), 1, "an active fix keeps its lines");
    }
    for state in finished {
        let id = new_fix_id();
        let mut item = fix(&id, &parent.id, FixState::Checking);
        item.log.notes.hidden_secrets = 2;
        store.put_fix(&item).unwrap();
        item.state = state;
        store.put_fix(&item).unwrap();
        let stored = store.fix_by_id(&id).unwrap();
        assert!(stored.log.jobs.is_empty(), "a finished fix drops its lines");
        assert_eq!(stored.log.sha8, "0a1b2c3d", "the sha8 stays");
        assert_eq!(stored.log.notes.hidden_secrets, 2, "the counts stay");
    }
}

#[test]
fn a_finished_fix_leaves_no_log_text_on_disk() {
    let root = temp_root();
    let store = Store::at(&root);
    let parent = watch("cw-00000030", 1);
    store.put_watch(&parent).unwrap();
    let id = new_fix_id();
    let mut item = fix(&id, &parent.id, FixState::Checking);
    store.put_fix(&item).unwrap();
    assert!(fs::read_to_string(root.join("fixes.json")).unwrap().contains("line one"));
    item.state = FixState::NotPushed;
    store.put_fix(&item).unwrap();
    assert!(!fs::read_to_string(root.join("fixes.json")).unwrap().contains("line one"));
}

#[test]
fn a_bad_id_never_makes_a_file() {
    let root = temp_root();
    let store = Store::at(&root);
    let parent = watch("cw-00000003", 1);

    assert!(store.put_watch(&Watch { id: "../../escape".into(), ..parent.clone() }).is_err());
    assert!(store.put_watch(&Watch { owner_repo: "acme/../x".into(), ..parent.clone() }).is_err());
    assert!(store.put_watch(&Watch { pushed_shas: vec!["xyz".into()], ..parent.clone() }).is_err());
    assert!(store.put_fix(&fix("cf-../../x", &parent.id, FixState::Starting)).is_err());
    assert!(store.put_fix(&fix(&new_fix_id(), "cw-../x", FixState::Starting)).is_err());
    let mut bad_sha = fix(&new_fix_id(), &parent.id, FixState::Starting);
    bad_sha.failing_sha = "../../x".into();
    assert!(store.put_fix(&bad_sha).is_err());

    assert!(store.watch_by_id("../../escape").is_none());
    assert!(store.fix_by_id("cf-../../x").is_none());
    assert_eq!(store.settings(), Settings::default());
    assert!(!root.exists(), "no refused write may create the store folder");
}
