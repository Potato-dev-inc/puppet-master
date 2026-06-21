use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::actors::ActorId;
use crate::events::{CommandId, EventEntry, PaneId, SystemEvent};

const EVENT_LOG_FILE_NAME: &str = "events.jsonl";
const PROJECT_STORAGE_DIR_NAME: &str = ".puppet-master";

static EVENT_LOG: OnceCell<Arc<EventLogManager>> = OnceCell::new();

#[derive(Debug)]
pub struct EventLog {
    path: PathBuf,
    writer: Mutex<File>,
}

#[derive(Debug)]
struct EventLogSlot {
    path: PathBuf,
    log: Arc<EventLog>,
}

#[derive(Debug)]
pub struct EventLogManager {
    fallback_path: PathBuf,
    active_project_path: Mutex<Option<PathBuf>>,
    slot: Mutex<EventLogSlot>,
}

impl EventLog {
    pub fn new(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create event log dir {}: {err}", parent.display()))?;
        }
        let writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|err| format!("open event log {}: {err}", path.display()))?;
        Ok(Self {
            path,
            writer: Mutex::new(writer),
        })
    }

    pub fn append(&self, entry: &EventEntry) -> Result<(), String> {
        let json = serde_json::to_string(entry)
            .map_err(|err| format!("serialize event log entry: {err}"))?;
        let mut writer = self.writer.lock();
        writer
            .write_all(json.as_bytes())
            .and_then(|_| writer.write_all(b"\n"))
            .and_then(|_| writer.flush())
            .map_err(|err| format!("write event log {}: {err}", self.path.display()))
    }

    #[allow(dead_code)]
    pub fn read_all(&self) -> Result<Vec<EventEntry>, String> {
        read_entries(&self.path)
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn event_log_path() -> PathBuf {
    crate::app_paths::app_data_dir().join(EVENT_LOG_FILE_NAME)
}

pub fn project_storage_dir(project_path: &Path) -> PathBuf {
    project_path.join(PROJECT_STORAGE_DIR_NAME)
}

pub fn project_event_log_path(project_path: &Path) -> PathBuf {
    project_storage_dir(project_path).join(EVENT_LOG_FILE_NAME)
}

impl EventLogManager {
    pub fn new(fallback_path: PathBuf) -> Result<Self, String> {
        let log = Arc::new(EventLog::new(fallback_path.clone())?);
        Ok(Self {
            fallback_path: fallback_path.clone(),
            active_project_path: Mutex::new(None),
            slot: Mutex::new(EventLogSlot {
                path: fallback_path,
                log,
            }),
        })
    }

    pub fn set_active_project_path(&self, path: Option<PathBuf>) {
        let normalized =
            path.and_then(|candidate| crate::project_path::normalize_project_path(&candidate).ok());
        *self.active_project_path.lock() = normalized;
    }

    pub fn active_project_path(&self) -> Option<PathBuf> {
        self.active_project_path.lock().clone()
    }

    pub fn current_path(&self) -> PathBuf {
        self.active_project_path()
            .map(|project_path| project_event_log_path(&project_path))
            .unwrap_or_else(|| self.fallback_path.clone())
    }

    fn current_log(&self) -> Result<Arc<EventLog>, String> {
        let path = self.current_path();
        let mut slot = self.slot.lock();
        if slot.path == path {
            return Ok(slot.log.clone());
        }
        let log = Arc::new(EventLog::new(path.clone())?);
        *slot = EventLogSlot {
            path,
            log: log.clone(),
        };
        Ok(log)
    }

    pub fn append(&self, entry: &EventEntry) -> Result<(), String> {
        self.current_log()?.append(entry)
    }

    pub fn read_all(&self) -> Result<Vec<EventEntry>, String> {
        read_entries(&self.current_path())
    }
}

pub fn init_global_event_log(path: PathBuf) -> Result<(), String> {
    let log = Arc::new(EventLogManager::new(path)?);
    let _ = EVENT_LOG.set(log);
    Ok(())
}

pub fn set_active_project_path(path: Option<PathBuf>) {
    let Some(log) = EVENT_LOG.get() else {
        tracing::debug!("event log not initialized; cannot set active project path");
        return;
    };
    log.set_active_project_path(path);
}

pub fn active_project_path() -> Option<PathBuf> {
    EVENT_LOG.get().and_then(|log| log.active_project_path())
}

pub fn current_event_log_path() -> PathBuf {
    EVENT_LOG
        .get()
        .map(|log| log.current_path())
        .unwrap_or_else(event_log_path)
}

pub fn append_system_event(payload: SystemEvent) {
    let Some(log) = EVENT_LOG.get() else {
        tracing::debug!("event log not initialized; dropping event");
        return;
    };
    let entry = EventEntry::new(ActorId::system(), CommandId::new(), payload);
    if let Err(err) = log.append(&entry) {
        tracing::warn!(%err, "append event failed");
    }
}

pub fn append_bridge_event(payload: SystemEvent) {
    let Some(log) = EVENT_LOG.get() else {
        tracing::debug!("event log not initialized; dropping bridge event");
        return;
    };
    let entry = EventEntry::new(ActorId::bridge(), CommandId::new(), payload);
    if let Err(err) = log.append(&entry) {
        tracing::warn!(%err, "append bridge event failed");
    }
}

pub fn replay_global_pane_timeline() -> Result<Vec<PaneTimelineEvent>, String> {
    let log = EVENT_LOG
        .get()
        .ok_or_else(|| "event log not initialized".to_string())?;
    replay_pane_timeline_from_entries(log.read_all()?)
}

pub fn read_global_entries() -> Result<Vec<EventEntry>, String> {
    let log = EVENT_LOG
        .get()
        .ok_or_else(|| "event log not initialized".to_string())?;
    log.read_all()
}

pub fn read_entries(path: &Path) -> Result<Vec<EventEntry>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file =
        File::open(path).map_err(|err| format!("open event log {}: {err}", path.display()))?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|err| format!("read event log line {}: {err}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry = serde_json::from_str::<EventEntry>(&line)
            .map_err(|err| format!("parse event log line {}: {err}", index + 1))?;
        entries.push(entry);
    }
    Ok(entries)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaneTimelineEvent {
    pub timestamp_ms: i64,
    pub pane_id: PaneId,
    pub event: String,
}

pub fn replay_pane_timeline_from_entries(
    entries: Vec<EventEntry>,
) -> Result<Vec<PaneTimelineEvent>, String> {
    let mut timeline = Vec::new();
    for entry in entries {
        match entry.payload {
            SystemEvent::PaneSpawned { pane_id, .. } => timeline.push(PaneTimelineEvent {
                timestamp_ms: entry.timestamp_ms,
                pane_id,
                event: "spawned".to_string(),
            }),
            SystemEvent::PaneInputWritten { pane_id, .. } => timeline.push(PaneTimelineEvent {
                timestamp_ms: entry.timestamp_ms,
                pane_id,
                event: "input_written".to_string(),
            }),
            SystemEvent::PaneOutputObserved { pane_id, .. } => timeline.push(PaneTimelineEvent {
                timestamp_ms: entry.timestamp_ms,
                pane_id,
                event: "output_observed".to_string(),
            }),
            SystemEvent::PaneStatusChanged { pane_id, status } => {
                timeline.push(PaneTimelineEvent {
                    timestamp_ms: entry.timestamp_ms,
                    pane_id,
                    event: format!("status:{status}"),
                })
            }
            SystemEvent::PaneKilled { pane_id } => timeline.push(PaneTimelineEvent {
                timestamp_ms: entry.timestamp_ms,
                pane_id,
                event: "killed".to_string(),
            }),
            SystemEvent::McpToolCalled { .. }
            | SystemEvent::McpToolCompleted { .. }
            | SystemEvent::TaskCreated { .. }
            | SystemEvent::TaskClaimed { .. }
            | SystemEvent::TaskLeaseRenewed { .. }
            | SystemEvent::TaskStatusUpdated { .. }
            | SystemEvent::TaskCompleted { .. }
            | SystemEvent::TaskBlocked { .. }
            | SystemEvent::ReviewerAssigned { .. }
            | SystemEvent::ResourceLockAcquired { .. }
            | SystemEvent::ResourceLockReleased { .. }
            | SystemEvent::ResourceLockExpired { .. }
            | SystemEvent::AgentObservation { .. } => {}
        }
    }
    Ok(timeline)
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
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
}
