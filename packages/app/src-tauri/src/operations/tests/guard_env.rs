//! Operations tests: the env guard a run starts under is saved with its worker and survives a
//! reload and every later save of the snapshot.

use super::*;

#[test]
fn the_guard_survives_a_snapshot_round_trip_and_later_saves() {
    let dir = temp();
    let mut asked = request(&dir, "guard-round-trip");
    asked.worker.guard_env = Some("no_push".into());
    let (created, _) = create_operation(asked).unwrap();
    assert_eq!(created.worker.guard_env.as_deref(), Some("no_push"));

    let reloaded = get_operation(&dir.to_string_lossy(), &created.operation_id).unwrap();
    assert_eq!(reloaded.worker.guard_env.as_deref(), Some("no_push"), "kept on disk");

    // A restart reads the file back, and a later save of the same snapshot keeps the guard.
    let marked = mark_operation_state(
        &dir.to_string_lossy(),
        &created.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    assert_eq!(marked.worker.guard_env.as_deref(), Some("no_push"));
    let again = get_operation(&dir.to_string_lossy(), &created.operation_id).unwrap();
    assert_eq!(again.worker.guard_env.as_deref(), Some("no_push"));
}

#[test]
fn a_run_without_a_guard_writes_no_guard_field_and_an_old_snapshot_reads_as_none() {
    let dir = temp();
    let (created, _) = create_operation(request(&dir, "guard-none")).unwrap();
    assert_eq!(created.worker.guard_env, None);
    let raw = serde_json::to_value(&created.worker).unwrap();
    assert!(raw.get("guard_env").is_none(), "{raw}");
    // A worker record written before the field existed.
    let old: WorkerPersist = serde_json::from_value(serde_json::json!({"name": "w"})).unwrap();
    assert_eq!(old.guard_env, None);
    let guarded = WorkerPersist { guard_env: Some("no_push".into()), ..Default::default() };
    let back: WorkerPersist = serde_json::from_value(serde_json::to_value(&guarded).unwrap()).unwrap();
    assert_eq!(back, guarded);
}
