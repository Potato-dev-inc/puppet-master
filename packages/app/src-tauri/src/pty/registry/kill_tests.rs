//! Closing a pane: the background path returns at once, the waiting path returns with the tree gone.

use super::*;
use crate::pty::proc_tree::test_support::{pid_alive, wait_until_dead};

/// A pane whose shell runs a long sleeper. Returns the pane and the sleeper's pid (a descendant of
/// the pane's own process, so the tree kill must reach it).
fn pane_with_sleeper(id: &str) -> (PaneState, u32) {
    let mut cmd = CommandBuilder::new(if cfg!(windows) { "ping" } else { "sh" });
    if cfg!(windows) {
        cmd.args(["-n", "60", "127.0.0.1"]);
    } else {
        cmd.args(["-c", "sleep 60 & wait"]);
    }
    let pane = PaneRegistry::test_pane_running(id, cmd);
    let pid = pane.info.pid;
    assert!(pid > 0 && pid_alive(pid), "the test process did not start");
    (pane, pid)
}

#[test]
fn kill_pane_returns_before_the_tree_is_stopped_and_the_worker_finishes_it() {
    let registry = Mutex::new(PaneRegistry::default());
    let (pane, pid) = pane_with_sleeper("bg-kill");
    registry.lock().panes.insert("bg-kill".into(), pane);
    kill_pane_with_reason(&registry, "bg-kill", Some("test")).unwrap();
    // Out of the registry (and so out of the UI) right away, whatever the processes are doing.
    assert!(!registry.lock().panes.contains_key("bg-kill"));
    assert!(wait_for_background_kills(Duration::from_secs(30)), "the worker did not finish");
    assert!(wait_until_dead(pid), "the pane's process survived the background kill");
}

#[test]
fn kill_pane_and_wait_returns_with_the_tree_stopped() {
    let registry = Mutex::new(PaneRegistry::default());
    let (pane, pid) = pane_with_sleeper("wait-kill");
    registry.lock().panes.insert("wait-kill".into(), pane);
    kill_pane_and_wait(&registry, "wait-kill", Some("test")).unwrap();
    assert!(!registry.lock().panes.contains_key("wait-kill"));
    // taskkill /F has returned: the process is gone (allow the OS a moment to tear the entry down).
    assert!(wait_until_dead(pid), "the pane's process survived the waiting kill");
}

#[test]
fn kill_all_and_wait_stops_every_pane_and_earlier_background_kills() {
    let registry = Mutex::new(PaneRegistry::default());
    let mut pids = Vec::new();
    for id in ["all-a", "all-b", "all-c"] {
        let (pane, pid) = pane_with_sleeper(id);
        registry.lock().panes.insert(id.into(), pane);
        pids.push(pid);
    }
    // One was closed in the background just before exit.
    kill_pane(&registry, "all-a").unwrap();
    kill_all_and_wait(&registry);
    assert!(registry.lock().panes.is_empty());
    for pid in pids {
        assert!(wait_until_dead(pid), "pid {pid} survived the exit kill");
    }
}

#[test]
fn killing_a_pane_that_is_not_there_is_fine() {
    let registry = Mutex::new(PaneRegistry::default());
    kill_pane(&registry, "nope").unwrap();
    kill_pane_and_wait(&registry, "nope", None).unwrap();
}
