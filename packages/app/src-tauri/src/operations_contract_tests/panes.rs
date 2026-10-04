//! Operation contract tests: panes.

use super::*;

#[test]
fn separate_operations_can_dispatch_concurrently() {
    let project = Arc::new(Project::new());
    let (first, _) = operations::create_operation(request(&project, "parallel-a")).unwrap();
    let (second, _) = operations::create_operation(request(&project, "parallel-b")).unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));

    let workers = [first, second]
        .into_iter()
        .map(|op| {
            let project_path = project.str();
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            thread::spawn(move || {
                operations::run_operation_dispatch(&project_path, &op.operation_id, |_| {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(now, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(100));
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap()
            })
        })
        .collect::<Vec<_>>();

    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(results
        .iter()
        .all(|op| op.status == OperationStatus::Running));
    assert_eq!(max_active.load(Ordering::SeqCst), 2);
}

#[test]
fn concurrent_observations_receive_monotonically_increasing_revisions() {
    let project = Arc::new(Project::new());
    let queued = operations::create_operation(request(&project, "monotonic-revisions"))
        .unwrap()
        .0;
    let running =
        operations::run_operation_dispatch(&project.str(), &queued.operation_id, |_| Ok(()))
            .unwrap();
    let workers_count = 12;
    let start = Arc::new(Barrier::new(workers_count));
    let workers = (0..workers_count)
        .map(|index| {
            let project = Arc::clone(&project);
            let start = Arc::clone(&start);
            let operation_id = queued.operation_id.clone();
            thread::spawn(move || {
                start.wait();
                operations::mark_operation_observation(
                    &project.str(),
                    &operation_id,
                    Some(format!("observation-{index}")),
                    None,
                    StateSource::Native,
                )
                .unwrap()
                .revision
            })
        })
        .collect::<Vec<_>>();

    let mut revisions = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    revisions.sort_unstable();
    let expected =
        (running.revision + 1..=running.revision + workers_count as u64).collect::<Vec<_>>();
    assert_eq!(revisions, expected);
    assert_eq!(
        operations::get_operation(&project.str(), &queued.operation_id)
            .unwrap()
            .revision,
        running.revision + workers_count as u64
    );
}

#[test]
fn concurrent_reservation_allows_only_one_operation_to_claim_a_pane() {
    let project = Arc::new(Project::new());
    let mut first_request = request(&project, "reserve-pane-a");
    first_request.pane_id = None;
    let mut second_request = request(&project, "reserve-pane-b");
    second_request.pane_id = None;
    let first = operations::create_operation(first_request).unwrap().0;
    let second = operations::create_operation(second_request).unwrap().0;
    for operation in [&first, &second] {
        operations::mark_operation_state(
            &project.str(),
            &operation.operation_id,
            OperationStatus::Starting,
            StateSource::Native,
            Some("dispatching".into()),
            None,
            None,
        )
        .unwrap();
    }

    let start = Arc::new(Barrier::new(2));
    let workers = [first.clone(), second.clone()]
        .into_iter()
        .map(|operation| {
            let project = Arc::clone(&project);
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                operations::reserve_pane(
                    &project.str(),
                    &operation.operation_id,
                    "shared-pane",
                    false,
                )
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();

    let winners = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(winners, 1);
    let loser = results
        .iter()
        .find_map(|result| result.as_ref().err())
        .unwrap();
    assert_eq!(loser.code, "PANE_BUSY");
    let assigned = operations::active_operation_for_pane(&project.str(), "shared-pane")
        .unwrap()
        .unwrap();
    assert!(
        assigned.operation_id == first.operation_id || assigned.operation_id == second.operation_id
    );
}

#[test]
fn requested_pane_id_is_not_busy_for_the_same_operation() {
    let project = Project::new();
    let mut req = request(&project, "self-pane");
    req.pane_id = Some("cursor-tui".into());
    let created = operations::create_operation(req).unwrap().0;
    operations::mark_operation_state(
        &project.str(),
        &created.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    assert!(
        operations::active_operation_for_pane(&project.str(), "cursor-tui")
            .unwrap()
            .is_some()
    );
    assert!(operations::conflicting_pane_operation(
        &project.str(),
        "cursor-tui",
        &created.operation_id,
    )
    .unwrap()
    .is_none());
    operations::reserve_pane(&project.str(), &created.operation_id, "cursor-tui", false).unwrap();
}

#[test]
fn conflicting_pane_operation_finds_the_other_occupant_even_when_self_is_listed_first() {
    let project = Project::new();
    let mut first_req = request(&project, "pane-first");
    first_req.pane_id = Some("shared-tui".into());
    let first = operations::create_operation(first_req).unwrap().0;
    let mut second_req = request(&project, "pane-second");
    second_req.pane_id = Some("shared-tui".into());
    let second = operations::create_operation(second_req).unwrap().0;
    operations::mark_operation_state(
        &project.str(),
        &first.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    operations::mark_operation_state(
        &project.str(),
        &second.operation_id,
        OperationStatus::Starting,
        StateSource::Native,
        Some("dispatching".into()),
        None,
        None,
    )
    .unwrap();
    let vs_first =
        operations::conflicting_pane_operation(&project.str(), "shared-tui", &first.operation_id)
            .unwrap()
            .expect("other occupant");
    let vs_second =
        operations::conflicting_pane_operation(&project.str(), "shared-tui", &second.operation_id)
            .unwrap()
            .expect("other occupant");
    assert_eq!(vs_first.operation_id, second.operation_id);
    assert_eq!(vs_second.operation_id, first.operation_id);
}
