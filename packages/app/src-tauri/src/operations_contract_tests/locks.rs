//! Operation contract tests: locks.

use super::*;

#[test]
fn second_run_for_a_held_lock_fails_fast_with_resource_locked() {
    let project = Project::new();
    let first = locked_operation(&project, "lock-a", "file:src/a.rs");
    let second = locked_operation(&project, "lock-b", "file:src/a.rs");
    crate::bridge::acquire_operation_locks(&first).unwrap();

    let error = crate::bridge::fail_operation_on_lock_conflict(&second).unwrap_err();
    assert_eq!(error.code, "RESOURCE_LOCKED");
    assert_eq!(error.context["owner_id"], first.operation_id);
    assert_eq!(error.context["resource_id"], "file:src/a.rs");
    assert!(error.retry_after_ms.is_some());
    let stored = operations::get_operation(&project.str(), &second.operation_id).unwrap();
    assert_eq!(stored.status, OperationStatus::Failed);
    // the failed run must not have stolen or freed the holder's lock
    let third = locked_operation(&project, "lock-c", "file:src/a.rs");
    assert_eq!(
        crate::bridge::acquire_operation_locks(&third).unwrap_err().code,
        "RESOURCE_LOCKED"
    );
}

#[test]
fn lock_is_free_again_after_the_holder_reaches_a_terminal_state() {
    let project = Project::new();
    let first = locked_operation(&project, "lock-a", "file:src/a.rs");
    crate::bridge::acquire_operation_locks(&first).unwrap();
    // re-acquiring by the same owner is idempotent
    crate::bridge::acquire_operation_locks(&first).unwrap();
    crate::bridge::release_operation_locks(&first).unwrap();

    let second = locked_operation(&project, "lock-b", "file:src/a.rs");
    crate::bridge::acquire_operation_locks(&second).unwrap();
}

#[test]
fn different_locks_do_not_conflict_and_release_only_frees_own_locks() {
    let project = Project::new();
    let first = locked_operation(&project, "lock-a", "file:src/a.rs");
    let second = locked_operation(&project, "lock-b", "file:src/b.rs");
    crate::bridge::acquire_operation_locks(&first).unwrap();
    crate::bridge::acquire_operation_locks(&second).unwrap();
    crate::bridge::release_operation_locks(&second).unwrap();
    let other = locked_operation(&project, "lock-c", "file:src/a.rs");
    assert_eq!(
        crate::bridge::acquire_operation_locks(&other).unwrap_err().code,
        "RESOURCE_LOCKED"
    );
    // releasing by a non-owner must not free the holder's lock
    crate::bridge::release_operation_locks(&other).unwrap();
    assert_eq!(
        crate::bridge::acquire_operation_locks(&other).unwrap_err().code,
        "RESOURCE_LOCKED"
    );
}

#[test]
fn concurrent_runs_locking_the_same_file_let_exactly_one_proceed() {
    let project = Arc::new(Project::new());
    let ops = (0..6)
        .map(|index| locked_operation(&project, &format!("race-{index}"), "file:src/race.rs"))
        .collect::<Vec<_>>();
    let start = Arc::new(Barrier::new(ops.len()));
    let threads = ops
        .into_iter()
        .map(|op| {
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                crate::bridge::fail_operation_on_lock_conflict(&op).is_ok()
            })
        })
        .collect::<Vec<_>>();
    let winners = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|won| *won)
        .count();
    assert_eq!(winners, 1);
}

#[test]
fn file_locks_name_the_same_file_however_the_path_is_spelled() {
    let project = Project::new();
    let holder = locked_operation(&project, "lock-a", "file:src/a.rs");
    crate::bridge::acquire_operation_locks(&holder).unwrap();
    for (n, spelling) in [
        "file:./src/a.rs",
        r"file:src\a.rs",
        "file:src//a.rs",
        "file:src/sub/../a.rs",
        "file: src/a.rs ",
    ]
    .into_iter()
    .enumerate()
    {
        let other = locked_operation(&project, &format!("lock-v{n}"), spelling);
        assert_eq!(
            crate::bridge::acquire_operation_locks(&other).unwrap_err().code,
            "RESOURCE_LOCKED",
            "{spelling} must conflict with file:src/a.rs"
        );
    }
    // a genuinely different file and a non-file resource with the same name stay independent
    let other_file = locked_operation(&project, "lock-b", "file:src/b.rs");
    crate::bridge::acquire_operation_locks(&other_file).unwrap();
    let task_named_like_path = locked_operation(&project, "lock-c", "task:src/a.rs");
    crate::bridge::acquire_operation_locks(&task_named_like_path).unwrap();
}

#[test]
fn a_lock_taken_with_one_spelling_is_released_by_another() {
    let project = Project::new();
    let first = locked_operation(&project, "lock-a", "file:./src/a.rs");
    crate::bridge::acquire_operation_locks(&first).unwrap();
    crate::bridge::release_operation_locks(&first).unwrap();
    let second = locked_operation(&project, "lock-b", r"file:src\a.rs");
    crate::bridge::acquire_operation_locks(&second).unwrap();
}
