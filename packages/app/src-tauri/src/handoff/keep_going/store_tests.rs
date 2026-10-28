//! Store tests: records, settings, briefs and the id guards. Each test uses its own temp folder.

use super::model::{valid_id, Back, BriefMeta, Handover, Reason, Settings, Side, State, Target, RECORDS_CAP};
use super::store::{new_id, Store};
use std::fs;
use std::path::PathBuf;

/// A temp folder for one test, removed at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        Scratch(std::env::temp_dir().join(format!("pm-kg-store-{}", uuid::Uuid::new_v4().simple())))
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

fn target(text: &str) -> Target {
    Target::parse(text).expect("test target parses")
}

fn record(id: &str, pane: &str) -> Handover {
    Handover {
        id: id.to_string(),
        chain_id: id.to_string(),
        hop: 1,
        at_ms: 1_000,
        reason: Reason::UsageLimit,
        from: Side {
            pane_id: pane.to_string(),
            agent_id: "fake_a".to_string(),
            account: None,
            label: "Fake A".to_string(),
            handle: Some("h-1".to_string()),
            operation_id: Some("op-1".to_string()),
        },
        to: Side {
            pane_id: format!("{pane}-new"),
            agent_id: "fake_b".to_string(),
            account: Some("work".to_string()),
            label: "Fake B · Work".to_string(),
            handle: None,
            operation_id: None,
        },
        project: "C:\\Users\\you\\proj".to_string(),
        folder: "C:\\Users\\you\\proj".to_string(),
        worktree_handle: None,
        from_reset_at_ms: Some(2_000),
        goal: "Fix the total".to_string(),
        back: Back::AtReset { due_ms: 3_000, tries: 0 },
        brief: BriefMeta {
            file: "C:\\Users\\you\\app\\keep-going\\briefs\\x.md".to_string(),
            sha8: "abcdef12".to_string(),
            bytes: 10,
            trimmed: vec!["files".to_string()],
            turns: 2,
        },
        state: State::Running,
    }
}

#[test]
fn record_round_trips_and_is_found_by_id_and_pane() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let rec = record("kg-0123456789ab", "pane-1");
    store.put(&rec).unwrap();

    assert!(scratch.0.join("records.json").exists());
    assert_eq!(store.records(), vec![rec.clone()]);
    assert_eq!(store.by_id("kg-0123456789ab"), Some(rec.clone()));
    assert_eq!(store.from_pane("pane-1"), Some(rec.clone()));
    assert_eq!(store.to_pane("pane-1-new"), Some(rec.clone()));
    assert_eq!(store.from_pane("pane-1-new"), None, "the new side is not the old side");
    assert_eq!(store.to_pane("nope"), None);
}

#[test]
fn put_updates_in_place_and_new_records_go_first() {
    let scratch = Scratch::new();
    let store = scratch.store();
    store.put(&record("kg-000000000001", "p1")).unwrap();
    store.put(&record("kg-000000000002", "p2")).unwrap();
    let ids = |store: &Store| store.records().into_iter().map(|r| r.id).collect::<Vec<_>>();
    assert_eq!(ids(&store), vec!["kg-000000000002", "kg-000000000001"], "newest first");

    let mut updated = record("kg-000000000001", "p1");
    updated.state = State::Failed { code: "C".into(), message: "m".into() };
    store.put(&updated).unwrap();
    assert_eq!(ids(&store), vec!["kg-000000000002", "kg-000000000001"], "an update keeps its place");
    assert_eq!(store.by_id("kg-000000000001"), Some(updated));
}

#[test]
fn cap_drops_the_oldest_records() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let id = |n: usize| format!("kg-{n:012x}");
    for n in 1..=RECORDS_CAP + 5 {
        store.put(&record(&id(n), &format!("p{n}"))).unwrap();
    }
    let records = store.records();
    assert_eq!(records.len(), RECORDS_CAP);
    assert_eq!(records[0].id, id(RECORDS_CAP + 5), "the newest is first");
    assert!(records.iter().all(|r| r.id != id(1)), "the oldest is gone");
    assert_eq!(records.last().unwrap().id, id(6), "the 200th newest is kept");
}

#[test]
fn put_refuses_a_bad_id_and_writes_nothing() {
    let scratch = Scratch::new();
    let store = scratch.store();
    assert!(store.put(&record("../x", "p")).is_err());
    assert!(store.put(&record("kg-0123456789AB", "p")).is_err());
    assert!(!scratch.0.join("records.json").exists());
    assert_eq!(store.by_id("../x"), None);
}

#[test]
fn corrupt_records_file_reads_as_empty() {
    let scratch = Scratch::new();
    fs::create_dir_all(&scratch.0).unwrap();
    fs::write(scratch.0.join("records.json"), "not json").unwrap();
    let store = scratch.store();
    assert!(store.records().is_empty());
    store.put(&record("kg-0123456789ab", "p")).unwrap();
    assert_eq!(store.records().len(), 1, "a new write replaces the bad file");
}

#[test]
fn settings_default_when_no_file() {
    let scratch = Scratch::new();
    let settings = scratch.store().settings();
    assert_eq!(settings, Settings::default());
    assert!(settings.ask_back);
    assert!(settings.order.is_empty());
}

#[test]
fn settings_round_trip_with_dedup_and_cap() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let mut order = vec![
        target("fake_a"),
        target("fake_a"),
        target("fake_a@work"),
        Target { agent_id: "../x".into(), account: None },
        target("fake_b"),
    ];
    order.extend((0..10).map(|n| target(&format!("fake_c{n}"))));
    let saved = store.save_settings(&Settings { order, ask_back: false }).unwrap();

    let expected: Vec<Target> = ["fake_a", "fake_a@work", "fake_b", "fake_c0", "fake_c1", "fake_c2", "fake_c3", "fake_c4"]
        .iter()
        .map(|text| target(text))
        .collect();
    assert_eq!(saved.order, expected, "duplicates and bad targets dropped, cap of 8");
    assert!(!saved.ask_back, "the switch is kept as given");
    assert_eq!(store.settings(), saved, "what is saved is what is read back");
}

#[test]
fn corrupt_settings_file_reads_as_default() {
    let scratch = Scratch::new();
    fs::create_dir_all(&scratch.0).unwrap();
    fs::write(scratch.0.join("settings.json"), "{ nope").unwrap();
    assert_eq!(scratch.store().settings(), Settings::default());
}

#[test]
fn brief_round_trips_under_briefs_folder() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let path = store.write_brief("kg-0123456789ab", "hello\n").unwrap();
    assert!(path.ends_with("kg-0123456789ab.md"), "{path}");
    assert!(scratch.0.join("briefs").join("kg-0123456789ab.md").exists());
    assert_eq!(store.read_brief("kg-0123456789ab").unwrap(), "hello\n");
}

#[test]
fn bad_brief_id_writes_and_reads_nothing() {
    let scratch = Scratch::new();
    let store = scratch.store();
    for bad in ["../x", "kg-0123456789AB", "kg-0123456789", "", "kg-../../x"] {
        assert!(store.write_brief(bad, "text").is_err(), "{bad:?} must be refused");
        assert!(store.read_brief(bad).is_err(), "{bad:?} must be refused");
    }
    assert!(!scratch.0.exists(), "nothing was created");
}

#[test]
fn new_ids_are_valid_and_distinct() {
    let ids: Vec<String> = (0..50).map(|_| new_id()).collect();
    assert!(ids.iter().all(|id| valid_id(id)), "{ids:?}");
    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}
