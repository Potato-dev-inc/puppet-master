//! Contract tests for the public durable operation API.
//!
//! These tests deliberately use the public entry points and fake dispatch callbacks so
//! they exercise persistence, idempotency, waiting, and lifecycle behavior without a PTY.
//! Shared fixtures live here; the tests are split by topic.

use crate::operations::{self, DelegateWorkRequest, OperationStatus, StateSource};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

mod cancellation;
mod completion;
mod idempotency;
mod locks;
mod panes;
mod recovery;
mod waits;

/// A temp project plus a private, thread-scoped operation index: whole-index scans such as
/// startup recovery then only see this test's project, not those of tests running (and deleting
/// their dirs) at the same time.
struct Project(PathBuf, PathBuf);

impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("pm-operation-contract-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        let index = std::env::temp_dir().join(format!("pm-operation-contract-index-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&index).unwrap();
        operations::set_test_index_dir(Some(index.clone()));
        Self(path, index)
    }

    fn str(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        operations::set_test_index_dir(None);
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn request(project: &Project, key: &str) -> DelegateWorkRequest {
    DelegateWorkRequest {
        project_path: project.str(),
        task: "implement feature".into(),
        agent_type: "codex".into(),
        pane_id: Some("pane-a".into()),
        idempotency_key: key.into(),
        acceptance_criteria: Some(vec!["behavior works".into()]),
        task_id: Some("task-a".into()),
        exclusive: false,
        locks: vec!["file:src/main.rs".into()],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    }
}

fn request_with_worker_assigned_fields(project: &Project, key: &str) -> DelegateWorkRequest {
    let mut req = request(project, key);
    req.pane_id = None;
    req.task_id = None;
    req.exclusive = false;
    req.locks.clear();
    req
}

fn locked_operation(project: &Project, key: &str, lock: &str) -> operations::OperationSnapshot {
    let mut req = request(project, key);
    req.pane_id = None;
    req.task_id = None;
    req.locks = vec![lock.into()];
    operations::create_operation(req).unwrap().0
}
