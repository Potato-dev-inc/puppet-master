//! Unit tests for the operations store, split by topic. Shared fixtures live here.

use super::store::{current_runtime_id, now_ms, path_for, persist, root};
use super::*;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

mod acceptance;
mod dispatch_race;
mod guard_env;
mod handed_off;
mod idempotency;
mod lifecycle;
mod queries;
mod recovery;
mod waits;
mod watchdog;

pub(super) fn request(dir: &Path, key: &str) -> DelegateWorkRequest {
    DelegateWorkRequest {
        project_path: dir.to_string_lossy().into_owned(),
        task: "do work".into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: key.into(),
        acceptance_criteria: Some(vec!["behavior works".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
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

pub(super) fn temp() -> PathBuf {
    std::env::temp_dir().join(format!("pm-ops-{}", Uuid::new_v4()))
}

/// Scopes the project/task index to a private temp dir for the current test thread, so
/// whole-index scans (e.g. `recover_all_interrupted_operations`) only see this test's projects
/// and not those of tests running (and deleting their dirs) concurrently.
pub(super) struct IsolatedIndex(PathBuf);

impl IsolatedIndex {
    pub(super) fn new() -> Self {
        let dir = temp();
        fs::create_dir_all(&dir).unwrap();
        super::store::set_test_index_dir(Some(dir.clone()));
        Self(dir)
    }
}

impl Drop for IsolatedIndex {
    fn drop(&mut self) {
        super::store::set_test_index_dir(None);
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn starting_in_dispatch(dir: &Path, key: &str) -> OperationSnapshot {
    let (queued, _) = create_operation(request(dir, key)).unwrap();
    mark_operation_state(
        &dir.to_string_lossy(),
        &queued.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap()
}

pub(super) fn marks(stages: &[&str]) -> OperationSnapshot {
    let dir = temp();
    let (mut op, _) = create_operation(request(&dir, &format!("marks-{}", Uuid::new_v4()))).unwrap();
    op.worker.stage_log = stages
        .iter()
        .enumerate()
        .map(|(index, stage)| StageMark {
            stage: (*stage).into(),
            at_ms: index as u64,
        })
        .collect();
    let _ = fs::remove_dir_all(dir);
    op
}
