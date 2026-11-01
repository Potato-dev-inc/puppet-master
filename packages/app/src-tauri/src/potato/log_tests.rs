//! Why a brain stopped: the reason in the "stopped" note comes from the log ring, already cleaned.

use super::log::{last_reason, stopped_note, LogRing};

fn ring(lines: &[&str]) -> Vec<String> {
    let mut ring = LogRing::new();
    for line in lines {
        ring.push(line, &[]);
    }
    ring.lines()
}

#[test]
fn a_node_crash_gives_its_error_line_not_the_stack() {
    let lines = ring(&[
        "node:fs:2744",
        "    const out = binding.lstat(base, false, undefined, true /* throwIfNoEntry */);",
        "                        ^",
        "",
        "Error: EISDIR: illegal operation on a directory, lstat 'C:'",
        "    at Object.realpathSync (node:fs:2744:25)",
        "{",
        "  errno: -4068,",
        "  code: 'EISDIR',",
        "}",
        "",
        "Node.js v22.20.0",
        "the brain exited with code 1",
    ]);
    assert_eq!(last_reason(&lines).unwrap(), "Error: EISDIR: illegal operation on a directory, lstat 'C:'");
    assert_eq!(
        stopped_note("Mochi", &lines),
        "Mochi's brain stopped: Error: EISDIR: illegal operation on a directory, lstat 'C:'. Press Start again."
    );
}

#[test]
fn the_brains_own_refusal_line_is_the_reason() {
    let lines = ring(&["potato-brain 0.1.0: ready", "potato-brain: the app refused this brain", "the brain exited with code 3"]);
    assert_eq!(last_reason(&lines).unwrap(), "the app refused this brain");
}

#[test]
fn without_an_error_line_the_exit_the_app_saw_is_the_reason() {
    assert_eq!(last_reason(&ring(&["the brain exited with code 2"])).unwrap(), "the brain exited with code 2");
    assert_eq!(last_reason(&ring(&["the brain did not say hello in time"])).unwrap(), "the brain did not say hello in time");
    assert_eq!(last_reason(&ring(&["could not start the brain: program not found"])).unwrap(), "could not start the brain: program not found");
}

#[test]
fn an_old_failed_turn_before_ready_or_a_greeting_is_not_blamed() {
    let lines = ring(&["potato-brain: a turn failed: PROVIDER_AUTH", "potato-brain 0.1.0: ready", "the brain exited with code 137"]);
    assert_eq!(last_reason(&lines).unwrap(), "the brain exited with code 137");
    // a turn that failed after it was ready is not a reason either: the process was killed, not the turn
    let lines = ring(&["potato-brain 0.1.0: ready", "potato-brain: a turn failed: PROVIDER_AUTH", "the brain exited with code 1"]);
    assert_eq!(last_reason(&lines).unwrap(), "the brain exited with code 1");
}

#[test]
fn an_error_older_than_the_last_start_is_not_blamed() {
    let lines = ring(&["Error: old trouble", "the brain exited with code 1", "potato-brain 0.1.0: ready", "the brain exited with code 9"]);
    assert_eq!(last_reason(&lines).unwrap(), "the brain exited with code 9");
}

#[test]
fn an_empty_ring_keeps_the_plain_sentence() {
    assert_eq!(last_reason(&[]), None);
    assert_eq!(stopped_note("Mochi", &[]), "Mochi's brain stopped. Press Start again.");
}

#[test]
fn the_reason_is_short_and_secrets_are_already_gone() {
    let mut ring = LogRing::new();
    ring.push(&format!("Error: {}", "x".repeat(400)), &[]);
    ring.push("Error: bad key sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij in the request", &[]);
    let lines = ring.lines();
    let reason = last_reason(&lines).unwrap();
    assert!(!reason.contains("ABCDEFGHIJKLMNOPQRSTUVWXYZ"), "{reason}");
    let long = last_reason(&lines[..1]).unwrap();
    assert!(long.chars().count() <= 160, "{}", long.chars().count());
}
