//! Stop the agent in one pane, and only that agent (Keep going, `handoff/keep_going/`).
//!
//! The agent is found with `agent_proc::agent_pid`: a confirmed child of the pane's host shell (or the
//! pane's own process when it is not a shell). Its tree is ended with `proc_tree::kill_process_tree`,
//! by pid and never by image name, then the stop is polled until the agent is gone. The pane, its
//! host shell and its scrollback stay. Nothing is typed into the pane: a key sent to a vendor's UI is
//! an answer to it, and Keep going never answers one.

use crate::pty::agent_proc::{self, AgentProcess};
use crate::pty::proc_tree;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

/// How often the agent is checked while it is being stopped (milliseconds).
pub const POLL_MS: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopError {
    /// No confirmed agent process is known in the pane (not started yet, or the pane is gone).
    NoPid,
    /// The agent was still running when the wait ran out.
    StillRunning,
}

/// Stop the agent in `pane_id` (root process `root_pid`). `look` says whether the agent is still
/// running; `kill` ends a pid's tree. Waits at most `wait_ms` for `look` to report `Exited`.
pub fn stop_agent_in(
    pane_id: &str,
    root_pid: u32,
    wait_ms: u64,
    look: &dyn Fn() -> AgentProcess,
    kill: &dyn Fn(u32),
) -> Result<(), StopError> {
    let pid = agent_proc::agent_pid(pane_id, root_pid).ok_or(StopError::NoPid)?;
    stop_pid(pid, wait_ms, look, kill)
}

/// Stop the agent in a live pane. The root pid comes from the registry; the pane stays in it. A pane
/// that runs inside Linux is stopped by its `wsl.exe` pid (gone = stopped), then what it left in the
/// distro is signalled by the pane's marker (`wsl::live`).
pub fn stop_agent(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str, wait_ms: u64) -> Result<(), StopError> {
    let (root_pid, place) = {
        let registry = registry.lock();
        match registry.get(pane_id) {
            Some(pane) => (pane.info.pid, pane.info.place.clone()),
            None => (0, None),
        }
    };
    if root_pid == 0 {
        return Err(StopError::NoPid);
    }
    let stopped = match agent_proc::agent_pid(pane_id, root_pid) {
        None => Err(StopError::NoPid),
        Some(pid) => {
            // Judged by the pid itself: once it is gone, the agent is gone. (The pane's state can name
            // another child of the shell as the agent after the kill, and would then wait for nothing.)
            let look = || if crate::proc_util::pid_alive(pid) { AgentProcess::Alive } else { AgentProcess::Exited };
            stop_pid(pid, wait_ms, &look, &proc_tree::kill_process_tree)
        }
    };
    if let Some(place) = place {
        crate::wsl::live::stop_pane(&place.distro, pane_id);
    }
    stopped
}

/// The shared loop. A look before the kill: an agent already gone is not killed at all, because a
/// pid read earlier may have been given to another process since.
fn stop_pid(pid: u32, wait_ms: u64, look: &dyn Fn() -> AgentProcess, kill: &dyn Fn(u32)) -> Result<(), StopError> {
    if look() == AgentProcess::Exited {
        return Ok(());
    }
    kill(pid);
    let mut waited = 0;
    loop {
        if look() == AgentProcess::Exited {
            return Ok(());
        }
        if waited >= wait_ms {
            return Err(StopError::StillRunning);
        }
        std::thread::sleep(Duration::from_millis(POLL_MS));
        waited += POLL_MS;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// A look that reports `Alive` for the first `alive_looks` calls, then `Exited` (`None` = never).
    fn looks_after(alive_looks: Option<u32>) -> impl Fn() -> AgentProcess {
        let calls = Cell::new(0u32);
        move || {
            let n = calls.get();
            calls.set(n + 1);
            match alive_looks {
                Some(alive) if n >= alive => AgentProcess::Exited,
                _ => AgentProcess::Alive,
            }
        }
    }

    #[test]
    fn an_agent_already_gone_is_not_killed_at_all() {
        let killed = RefCell::new(Vec::new());
        let result = stop_pid(4242, 1_000, &looks_after(Some(0)), &|pid| killed.borrow_mut().push(pid));
        assert_eq!(result, Ok(()));
        assert!(killed.borrow().is_empty(), "a pid read earlier may belong to another process now");
    }

    #[test]
    fn an_agent_that_exits_after_three_polls_is_stopped_once() {
        let killed = RefCell::new(Vec::new());
        // Alive before the kill and at the first two polls; gone at the third poll.
        let result = stop_pid(4242, 2_000, &looks_after(Some(3)), &|pid| killed.borrow_mut().push(pid));
        assert_eq!(result, Ok(()));
        assert_eq!(*killed.borrow(), vec![4242]);
    }

    #[test]
    fn an_agent_that_never_exits_is_still_running_after_the_wait() {
        let killed = RefCell::new(Vec::new());
        let result = stop_pid(4242, 300, &looks_after(None), &|pid| killed.borrow_mut().push(pid));
        assert_eq!(result, Err(StopError::StillRunning));
        assert_eq!(*killed.borrow(), vec![4242], "it was asked to stop once");
    }

    #[test]
    fn a_pane_with_no_confirmed_agent_has_no_pid() {
        // Root 0 is never a process: no agent can be read from it.
        let result = stop_agent_in("agent-stop-no-pid", 0, 100, &looks_after(None), &|_| panic!("no kill"));
        assert_eq!(result, Err(StopError::NoPid));
    }

    #[test]
    fn a_linux_pane_is_followed_up_inside_the_distro_and_a_windows_pane_is_not() {
        use crate::wsl::model::PanePlace;
        let registry = Arc::new(Mutex::new(PaneRegistry::new()));
        let mut linux = PaneRegistry::test_pane_stub("agent-stop-linux-pane");
        linux.info.place = Some(PanePlace {
            kind: "linux".into(),
            distro: "Ubuntu-Test".into(),
            cwd: "/mnt/d/x".into(),
            mount_root: "/mnt/".into(),
            unc_root: "unc".into(),
            home_windows: String::new(),
            warnings: Vec::new(),
        });
        let windows = PaneRegistry::test_pane_stub("agent-stop-windows-pane");
        registry.lock().panes.insert("agent-stop-linux-pane".into(), linux);
        registry.lock().panes.insert("agent-stop-windows-pane".into(), windows);
        let _ = stop_agent(&registry, "agent-stop-linux-pane", 100);
        let _ = stop_agent(&registry, "agent-stop-windows-pane", 100);
        let stops = crate::wsl::live::recorded_stops();
        assert!(stops.contains(&("Ubuntu-Test".to_string(), "agent-stop-linux-pane".to_string())));
        assert!(!stops.iter().any(|(_, pane)| pane == "agent-stop-windows-pane"));
    }

    #[test]
    fn a_pane_missing_from_the_registry_has_no_pid() {
        let registry = Arc::new(Mutex::new(PaneRegistry::new()));
        assert_eq!(stop_agent(&registry, "agent-stop-missing-pane", 100), Err(StopError::NoPid));
    }

    /// Windows: a root that is not a host shell is the agent itself, so the pid it names is the root.
    #[cfg(windows)]
    #[test]
    fn a_non_shell_root_is_the_pid_that_gets_stopped() {
        let me = std::process::id();
        let killed = RefCell::new(Vec::new());
        let result = stop_agent_in("agent-stop-self", me, 1_000, &looks_after(Some(1)), &|pid| killed.borrow_mut().push(pid));
        assert_eq!(result, Ok(()));
        assert_eq!(*killed.borrow(), vec![me]);
    }

    /// Windows: the agent's tree (child and grandchild) is ended; a separate process (a stand-in for the
    /// host shell, which must survive) is left alone.
    #[cfg(windows)]
    #[test]
    fn the_agent_tree_is_ended_and_the_host_shell_survives() {
        use crate::pty::proc_tree::test_support::*;
        let (mut agent, grandchild) = spawn_child_with_grandchild();
        let mut shell = crate::proc_util::hidden_command("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn stand-in shell");
        let agent_pid = agent.id();
        assert!(pid_alive(agent_pid) && pid_alive(grandchild) && pid_alive(shell.id()));

        let look = || if crate::proc_util::pid_alive(agent_pid) { AgentProcess::Alive } else { AgentProcess::Exited };
        let result = stop_pid(agent_pid, 3_000, &look, &proc_tree::kill_process_tree);
        let _ = agent.wait();
        assert_eq!(result, Ok(()));
        assert!(wait_until_dead(agent_pid), "agent still alive");
        assert!(wait_until_dead(grandchild), "agent's grandchild leaked");
        assert!(pid_alive(shell.id()), "the host shell was stopped too");
        let _ = shell.kill();
        let _ = shell.wait();
    }
}
