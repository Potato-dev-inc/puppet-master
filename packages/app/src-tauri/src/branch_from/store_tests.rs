//! Store tests: records, briefs and the id guards. Each test uses its own temp folder.

use super::model::{Branch, BranchFrom, BriefMeta, CopyFacts, NewSide, Point, SourceSide, State, When, RECORDS_CAP};
use super::store::{id_ok, new_id, Store};
use std::fs;
use std::path::PathBuf;

/// A temp folder for one test, removed at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        Scratch(std::env::temp_dir().join(format!("pm-bf-store-{}", uuid::Uuid::new_v4().simple())))
    }

    fn store(&self) -> Store {
        Store::at(self.0.clone())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn record(id: &str, from_pane: &str) -> Branch {
    Branch {
        id: id.to_string(),
        at_ms: 1_000,
        from: SourceSide {
            pane_id: from_pane.to_string(),
            agent_id: "fake_a".to_string(),
            account: None,
            label: "Fake A".to_string(),
            cwd: "C:\\Users\\you\\work\\demo".to_string(),
        },
        point: Point {
            checkpoint_id: "5".to_string(),
            turn: 5,
            kind: "after".to_string(),
            at_ms: 900,
            label: "After turn 5".to_string(),
            files: 2,
        },
        when: Some(When::After),
        to: NewSide {
            agent_id: "fake_b".to_string(),
            account: Some("work".to_string()),
            label: "Fake B".to_string(),
            pane_id: format!("{id}-pane"),
            handle: Some(format!("{id}-handle")),
            operation_id: None,
        },
        project: "C:\\Users\\you\\work\\demo".to_string(),
        copy: Some(CopyFacts {
            handle: format!("{id}-handle"),
            branch: format!("pm/{id}"),
            path: format!("C:\\Users\\you\\work\\demo\\.puppet-master\\worktrees\\{id}"),
            start_commit: Some("abc1234".to_string()),
            written: 2,
            removed: 1,
            failed: vec!["big.bin".to_string()],
        }),
        parent_branch: None,
        brief: BriefMeta { file: format!("C:\\Users\\you\\branches\\briefs\\{id}.md"), sha8: "0a1b2c3d".to_string(), bytes: 120, turns: 2, trimmed: vec![] },
        state: State::Running,
    }
}

#[test]
fn a_record_round_trips_through_the_store() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let first = record("bf-000000000001", "pane-a");
    store.put(&first).unwrap();
    assert_eq!(store.records(), vec![first.clone()]);
    assert_eq!(store.by_id("bf-000000000001"), Some(first.clone()));
    assert_eq!(store.from_pane("pane-a"), Some(first.clone()));
    assert_eq!(store.to_pane("bf-000000000001-pane"), Some(first.clone()));
    assert_eq!(store.from_pane("pane-z"), None);

    let mut changed = first.clone();
    changed.state = State::Failed { code: "BRANCH_START_FAILED".into(), message: "x".into() };
    changed.from.label = "Renamed".into();
    store.put(&changed).unwrap();
    let all = store.records();
    assert_eq!(all, vec![changed], "an update keeps the one record");
    let from = BranchFrom::SavePoint { checkpoint_id: "5".into() };
    assert_eq!(serde_json::from_str::<BranchFrom>(&serde_json::to_string(&from).unwrap()).unwrap(), from);
}

#[test]
fn a_new_record_goes_first_and_the_oldest_are_dropped_past_the_cap() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let mut ids = Vec::new();
    for _ in 0..(RECORDS_CAP + 3) {
        let id = new_id();
        store.put(&record(&id, "pane-a")).unwrap();
        ids.push(id);
    }
    let kept = store.records();
    assert_eq!(kept.len(), RECORDS_CAP);
    assert_eq!(kept[0].id, *ids.last().unwrap(), "newest first");
    assert!(kept.iter().all(|r| r.id != ids[0]), "the oldest record was dropped");
    assert!(kept.iter().any(|r| r.id == ids[3]), "the fourth-oldest is still kept");
}

#[test]
fn a_bad_id_writes_nothing() {
    let scratch = Scratch::new();
    let store = scratch.store();
    assert!(store.put(&record("bf-../../evil", "pane-a")).is_err());
    assert!(store.put(&record("kg-000000000001", "pane-a")).is_err());
    assert!(store.write_brief("bf-../../evil", "secret").is_err());
    assert!(store.read_brief("bf-../../evil").is_err());
    assert!(store.by_id("bf-../../evil").is_none());
    assert!(!scratch.0.exists(), "no file or folder was made for a bad id");
    assert!(store.records().is_empty());
}

#[test]
fn a_brief_is_written_under_the_store_and_read_back() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let id = "bf-0123456789ab";
    let path = store.write_brief(id, "the brief").unwrap();
    assert!(path.ends_with("bf-0123456789ab.md"), "{path}");
    assert_eq!(store.read_brief(id).unwrap(), "the brief");
}

#[test]
fn the_hooks_folder_is_made_empty_under_the_store() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let dir = store.empty_hooks_dir().unwrap();
    assert_eq!(dir, scratch.0.join("hooks"));
    assert!(dir.is_dir());
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn id_ok_takes_only_bf_and_twelve_lowercase_hex() {
    assert!(id_ok("bf-0123456789ab"));
    assert!(id_ok("bf-fedcba987654"));
    assert!(!id_ok("bf-0123456789A"), "upper case is refused");
    assert!(!id_ok("bf-0123456789"), "too short");
    assert!(!id_ok("bf-0123456789abc"), "too long");
    assert!(!id_ok("bf-0123456789ag"), "not hex");
    assert!(!id_ok("kg-0123456789ab"), "another feature's id");
    assert!(!id_ok("bf-../0123456"), "path characters");
    assert!(!id_ok(""));
}

#[test]
fn new_id_is_bf_and_twelve_lowercase_hex() {
    for _ in 0..50 {
        let id = new_id();
        assert!(id.starts_with("bf-"), "{id}");
        assert_eq!(id.len(), 15, "{id}");
        assert!(id_ok(&id), "{id}");
    }
    assert_ne!(new_id(), new_id());
}

#[test]
fn branch_audit_kind_has_its_own_name() {
    assert_eq!(crate::audit::Kind::Branch.as_str(), "branch.started");
}
