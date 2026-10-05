//! Unit tests for the event log: append, replay, project paths.

use super::*;
use crate::events::SystemEvent;

fn temp_event_log_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "puppet-master-{name}-{}.jsonl",
        uuid::Uuid::new_v4()
    ))
}

fn temp_project_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("puppet-master-{name}-{}", uuid::Uuid::new_v4()))
}

#[test]
fn jsonl_log_survives_reopen() {
    let path = temp_event_log_path("survives-reopen");
    let log = EventLog::new(path.clone()).unwrap();
    let entry = EventEntry::new(
        ActorId::system(),
        CommandId::new(),
        SystemEvent::PaneKilled {
            pane_id: PaneId("pane-1".to_string()),
            reason: None,
        },
    );
    log.append(&entry).unwrap();
    drop(log);

    let reopened = EventLog::new(path.clone()).unwrap();
    let entries = reopened.read_all().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].payload, SystemEvent::PaneKilled { .. }));
    let _ = fs::remove_file(path);
}

#[test]
fn manager_writes_active_project_events_to_project_storage() {
    let fallback_path = temp_event_log_path("project-fallback");
    let project_dir = temp_project_dir("project-scope");
    fs::create_dir_all(&project_dir).unwrap();
    let manager = EventLogManager::new(fallback_path.clone()).unwrap();
    manager.set_active_project_path(Some(project_dir.clone()));

    let entry = EventEntry::new(
        ActorId::system(),
        CommandId::new(),
        SystemEvent::PaneKilled {
            pane_id: PaneId("pane-1".to_string()),
            reason: None,
        },
    );
    manager.append(&entry).unwrap();

    let project_log = project_event_log_path(&project_dir);
    assert_eq!(manager.current_path(), project_log);
    assert_eq!(read_entries(&project_log).unwrap().len(), 1);
    assert!(read_entries(&fallback_path).unwrap().is_empty());

    let _ = fs::remove_file(project_log);
    let _ = fs::remove_dir_all(project_storage_dir(&project_dir));
    let _ = fs::remove_dir_all(project_dir);
    let _ = fs::remove_file(fallback_path);
}

#[test]
fn replay_reconstructs_pane_timeline() {
    let entries = vec![
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::PaneSpawned {
                pane_id: PaneId("pane-1".to_string()),
                agent_type: "bash".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                cols: 80,
                rows: 24,
            },
        ),
        EventEntry::new(
            ActorId::system(),
            CommandId::new(),
            SystemEvent::PaneStatusChanged {
                pane_id: PaneId("pane-1".to_string()),
                status: "idle".to_string(),
            },
        ),
    ];

    let timeline = replay_pane_timeline_from_entries(entries).unwrap();
    assert_eq!(timeline.len(), 2);
    assert_eq!(timeline[0].event, "spawned");
    assert_eq!(timeline[1].event, "status:idle");
}

fn status_entry(pane: &str, status: &str) -> EventEntry {
    EventEntry::new(
        ActorId::system(),
        CommandId::new(),
        SystemEvent::PaneStatusChanged {
            pane_id: PaneId(pane.to_string()),
            status: status.to_string(),
        },
    )
}

#[test]
fn find_last_entry_returns_the_newest_match_across_chunk_boundaries() {
    let path = temp_event_log_path("find-last");
    let log = EventLog::new(path.clone()).unwrap();
    for index in 0..200 {
        log.append(&status_entry("pane-a", &format!("a{index}"))).unwrap();
        if index % 3 == 0 {
            log.append(&status_entry("pane-b", &format!("b{index}"))).unwrap();
        }
    }
    let wants = |pane: &'static str| {
        move |entry: &EventEntry| {
            matches!(&entry.payload, SystemEvent::PaneStatusChanged { pane_id, .. } if pane_id.0 == pane)
        }
    };
    // A tiny chunk forces lines to straddle chunk boundaries.
    for chunk in [7usize, 64, 1_000, 1 << 20] {
        let a = find_last_entry_in(&path, chunk, wants("pane-a")).unwrap().unwrap();
        assert!(matches!(&a.payload, SystemEvent::PaneStatusChanged { status, .. } if status == "a199"), "chunk {chunk}");
        let b = find_last_entry_in(&path, chunk, wants("pane-b")).unwrap().unwrap();
        assert!(matches!(&b.payload, SystemEvent::PaneStatusChanged { status, .. } if status == "b198"), "chunk {chunk}");
    }
    assert!(find_last_entry_in(&path, 64, wants("pane-missing")).unwrap().is_none());
    let _ = fs::remove_file(path);
}

#[test]
fn find_last_entry_handles_missing_empty_and_unterminated_files() {
    let missing = temp_event_log_path("find-last-missing");
    assert!(find_last_entry_in(&missing, 64, |_| true).unwrap().is_none());
    let empty = temp_event_log_path("find-last-empty");
    fs::write(&empty, "").unwrap();
    assert!(find_last_entry_in(&empty, 64, |_| true).unwrap().is_none());
    // No trailing newline (a writer was interrupted mid-append is not tolerated, but a file that
    // simply lacks the final newline is), and a blank line in the middle.
    let json = serde_json::to_string(&status_entry("p", "x")).unwrap();
    let odd = temp_event_log_path("find-last-odd");
    fs::write(&odd, format!("{json}\n\n{json}")).unwrap();
    assert!(find_last_entry_in(&odd, 16, |_| true).unwrap().is_some());
    let _ = fs::remove_file(empty);
    let _ = fs::remove_file(odd);
}
