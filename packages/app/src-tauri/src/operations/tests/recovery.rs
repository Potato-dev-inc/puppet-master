//! Operations tests: recovery.

use super::*;

#[test]
fn recover_all_visits_nonselected_indexed_projects() {
    let _index = IsolatedIndex::new();
    let selected = temp();
    let other_project = temp();
    let (queued, _) = create_operation(request(&selected, "recover-selected")).unwrap();
    mark_operation_state(
        &selected.to_string_lossy(),
        &queued.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    let (other, _) = create_operation(request(&other_project, "recover-other")).unwrap();
    for (project, operation_id) in [
        (&selected, queued.operation_id.clone()),
        (&other_project, other.operation_id.clone()),
    ] {
        let path = path_for(&project.to_string_lossy(), &operation_id).unwrap();
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["runtime_id"] = serde_json::Value::String("previous-runtime".into());
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    }
    let recovered = recover_all_interrupted_operations().unwrap();
    let recovered_ids = recovered
        .iter()
        .map(|op| op.operation_id.as_str())
        .collect::<Vec<_>>();
    assert!(recovered_ids.contains(&queued.operation_id.as_str()));
    assert!(recovered_ids.contains(&other.operation_id.as_str()));
    assert!(recovered
        .iter()
        .all(|op| op.status == OperationStatus::Failed));
    let _ = fs::remove_dir_all(selected);
    let _ = fs::remove_dir_all(other_project);
}

#[test]
fn recover_all_skips_project_removed_after_indexing() {
    // The index is process-wide by default; scan only this test's projects, not those of
    // concurrently running tests that are creating and deleting theirs.
    let _index = IsolatedIndex::new();
    let removed_project = temp();
    create_operation(request(&removed_project, "removed-project-indexed")).unwrap();
    fs::remove_dir_all(&removed_project).unwrap();
    assert!(recover_all_interrupted_operations().is_ok());
}
