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
