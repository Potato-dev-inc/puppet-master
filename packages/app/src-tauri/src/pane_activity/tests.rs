use super::*;
use std::cell::{Cell, RefCell};

/// The tracker state is process-global, so tests that tick run one at a time.
use super::TEST_SERIAL as SERIAL;

fn snap(screen: &str, status: PaneStatus) -> Option<PaneSnapshot> {
    Some(PaneSnapshot { agent_type: "cursor_agent".into(), screen: screen.into(), status })
}

#[test]
fn a_burst_of_output_costs_one_recompute_per_tick() {
    let _serial = SERIAL.lock();
    let reads = Cell::new(0);
    let emitted = RefCell::new(Vec::<String>::new());
    let screen = RefCell::new("  $ python -m pytest -q 9.3s\n".to_string());
    let read = |_: &str| {
        reads.set(reads.get() + 1);
        snap(&screen.borrow(), PaneStatus::Running)
    };
    let emit = |_: &str, a: &Activity| emitted.borrow_mut().push(a.text.clone());
    let alive = |_: &str| true;

    // 5,000 chunks arrive between two ticks.
    for _ in 0..5000 {
        note_output("thr-1");
    }
    assert_eq!(tick_with(&read, &alive, &emit), 1);
    assert_eq!(reads.get(), 1, "one screen read for the whole burst");
    assert_eq!(*emitted.borrow(), vec!["Running python -m pytest -q"]);

    // No new output: the next tick does nothing at all.
    assert_eq!(tick_with(&read, &alive, &emit), 0);
    assert_eq!(reads.get(), 1);

    // Output that leaves the screen identical is read but not reclassified or re-emitted.
    note_output("thr-1");
    assert_eq!(tick_with(&read, &alive, &emit), 0);
    assert_eq!(reads.get(), 2);
    assert_eq!(emitted.borrow().len(), 1);
}

#[test]
fn only_a_changed_phrase_is_emitted() {
    let _serial = SERIAL.lock();
    let emitted = RefCell::new(Vec::<String>::new());
    let screen = RefCell::new("  $ npm test 1.0s\n".to_string());
    let read = |_: &str| snap(&screen.borrow(), PaneStatus::Running);
    let emit = |_: &str, a: &Activity| emitted.borrow_mut().push(a.text.clone());
    let alive = |_: &str| true;

    note_output("chg-1");
    tick_with(&read, &alive, &emit);
    // The elapsed-time token ticks: the screen differs but the phrase does not.
    *screen.borrow_mut() = "  $ npm test 2.0s\n".into();
    note_output("chg-1");
    assert_eq!(tick_with(&read, &alive, &emit), 1, "screen changed, so it is recomputed");
    assert_eq!(emitted.borrow().len(), 1, "same phrase: no event");
    // Now it really moves on.
    *screen.borrow_mut() = "    Grepped \"todo\" in src\n".into();
    note_output("chg-1");
    tick_with(&read, &alive, &emit);
    assert_eq!(*emitted.borrow(), vec!["Running npm test", "Searching \"todo\" in src"]);
    assert_eq!(snapshot().get("chg-1").map(|a| a.text.as_str()), Some("Searching \"todo\" in src"));
}

#[test]
fn a_status_change_alone_updates_the_phrase() {
    let _serial = SERIAL.lock();
    let emitted = RefCell::new(Vec::<String>::new());
    let status = Cell::new(PaneStatus::Running);
    let read = |_: &str| snap("  nothing special\n", status.get());
    let emit = |_: &str, a: &Activity| emitted.borrow_mut().push(a.text.clone());
    let alive = |_: &str| true;
    note_output("st-1");
    tick_with(&read, &alive, &emit);
    status.set(PaneStatus::Idle);
    note_output("st-1");
    tick_with(&read, &alive, &emit);
    assert_eq!(*emitted.borrow(), vec!["Working", "Idle"]);
}

#[test]
fn closed_panes_are_forgotten() {
    let _serial = SERIAL.lock();
    let read = |_: &str| snap("x\n", PaneStatus::Idle);
    note_output("gone-1");
    tick_with(&read, &|_| true, &|_, _| {});
    assert!(snapshot().contains_key("gone-1"));
    tick_with(&read, &|id| id != "gone-1", &|_, _| {});
    assert!(!snapshot().contains_key("gone-1"));
}

#[test]
fn visible_tail_keeps_the_last_non_empty_lines() {
    let screen = (1..=40).map(|i| format!("line {i}\n\n")).collect::<String>();
    let tail = visible_tail(&screen);
    assert_eq!(tail.lines().count(), TAIL_LINES);
    assert!(tail.ends_with("line 40"));
}
