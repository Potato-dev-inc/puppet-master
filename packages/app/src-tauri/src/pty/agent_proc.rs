//! Is the agent in a pane still running? (host-shell sentinel, process side)
//!
//! Agent panes on Windows run the agent under a host shell: `powershell -NoExit -Command & 'agent'`
//! (`registry::command::build_command`). When the agent exits, the shell stays and shows its prompt,
//! so the pane's own process (the shell) being alive says nothing about the agent. This module looks
//! at the pane's process tree instead:
//!
//! - the pane's root process is a shell (by image name, never by agent name): the agent is its child.
//!   A child seen once and gone now = the agent exited; none seen yet = still starting (or the command
//!   failed before it started, see `tui::shell::command_not_found`).
//! - any other root process is the agent itself: alive while that process runs.
//!
//! Read on demand (a Toolhelp32 process snapshot, a few milliseconds), no watcher thread. What was
//! seen is remembered per pane and root pid, so "seen, then gone" survives between reads.
//!
//! Why tracking and not dropping `-NoExit` for run-spawned panes: see `docs/features/typing-permit.md`
//! (the leftover shell keeps the error text a run needs to explain itself, user panes keep their
//! shell, and a standby pane is spawned before anyone knows a run will use it).

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;

/// What the process tree says about the agent in a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentProcess {
    /// The agent process is running.
    Alive,
    /// The agent process was running and is gone (or the pane's own process ended).
    Exited,
    /// The host shell has not started the agent yet (or the command failed before a process existed).
    Starting,
    /// No process information (no pid, not Windows, the snapshot failed). Never vetoes anything.
    Unknown,
}

/// One process from a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcEntry {
    pub pid: u32,
    pub parent: u32,
    /// Lowercase image name (`powershell.exe`).
    pub image: String,
}

/// Image names of the shells a pane's agent can be hosted by. Shells, not agents: generic.
const HOST_SHELLS: [&str; 3] = ["powershell.exe", "pwsh.exe", "cmd.exe"];
/// Helper processes a console host may start that are not the agent.
const NOT_AGENTS: [&str; 2] = ["conhost.exe", "openconsole.exe"];

#[derive(Debug, Default, Clone)]
struct Seen {
    root: u32,
    /// The agent child last seen under a host shell.
    agent: Option<u32>,
}

static SEEN: Lazy<Mutex<HashMap<String, Seen>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// The agent's state in `pane_id`, whose root process is `root_pid` (`PaneInfo::pid`).
pub fn state(pane_id: &str, root_pid: u32) -> AgentProcess {
    if root_pid == 0 {
        return AgentProcess::Unknown;
    }
    let Some(procs) = snapshot() else {
        return AgentProcess::Unknown;
    };
    let mut seen = SEEN.lock();
    let entry = seen.entry(pane_id.to_string()).or_default();
    if entry.root != root_pid {
        *entry = Seen { root: root_pid, agent: None };
    }
    let (verdict, agent) = classify(&procs, root_pid, entry.agent);
    entry.agent = agent;
    verdict
}

/// Pure decision over one snapshot: the verdict and the agent pid to remember.
pub(crate) fn classify(procs: &[ProcEntry], root_pid: u32, seen_agent: Option<u32>) -> (AgentProcess, Option<u32>) {
    let Some(root) = procs.iter().find(|p| p.pid == root_pid) else {
        return (AgentProcess::Exited, seen_agent);
    };
    if !HOST_SHELLS.contains(&root.image.as_str()) {
        return (AgentProcess::Alive, seen_agent);
    }
    let child = procs
        .iter()
        .filter(|p| p.parent == root_pid && p.pid != root_pid)
        .find(|p| !NOT_AGENTS.contains(&p.image.as_str()));
    match (child, seen_agent) {
        (Some(child), _) => (AgentProcess::Alive, Some(child.pid)),
        (None, Some(_)) => (AgentProcess::Exited, seen_agent),
        (None, None) => (AgentProcess::Starting, None),
    }
}

/// Every process on the machine, or `None` when the snapshot is not available.
#[cfg(windows)]
pub fn snapshot() -> Option<Vec<ProcEntry>> {
    #[repr(C)]
    #[allow(non_snake_case)]
    struct ProcessEntry32W {
        dwSize: u32,
        cntUsage: u32,
        th32ProcessID: u32,
        th32DefaultHeapID: usize,
        th32ModuleID: u32,
        cntThreads: u32,
        th32ParentProcessID: u32,
        pcPriClassBase: i32,
        dwFlags: u32,
        szExeFile: [u16; 260],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut core::ffi::c_void;
        fn Process32FirstW(snapshot: *mut core::ffi::c_void, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: *mut core::ffi::c_void, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    }
    const TH32CS_SNAPPROCESS: u32 = 0x2;
    let invalid = usize::MAX as *mut core::ffi::c_void;
    // SAFETY: a plain Win32 snapshot walk; the entry struct matches PROCESSENTRY32W and dwSize is
    // set before the first call, the handle is closed on every path.
    unsafe {
        let handle = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if handle.is_null() || handle == invalid {
            return None;
        }
        let mut entry: ProcessEntry32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
        let mut out = Vec::new();
        let mut ok = Process32FirstW(handle, &mut entry);
        while ok != 0 {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            out.push(ProcEntry {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                image: String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase(),
            });
            ok = Process32NextW(handle, &mut entry);
        }
        CloseHandle(handle);
        Some(out)
    }
}

#[cfg(not(windows))]
pub fn snapshot() -> Option<Vec<ProcEntry>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, parent: u32, image: &str) -> ProcEntry {
        ProcEntry { pid, parent, image: image.into() }
    }

    #[test]
    fn a_shell_root_is_alive_only_through_its_child() {
        let host = p(10, 1, "powershell.exe");
        let agent = p(11, 10, "node.exe");
        assert_eq!(classify(&[host.clone()], 10, None), (AgentProcess::Starting, None));
        assert_eq!(classify(&[host.clone(), agent], 10, None), (AgentProcess::Alive, Some(11)));
        assert_eq!(classify(&[host.clone()], 10, Some(11)), (AgentProcess::Exited, Some(11)), "seen, then gone");
        assert_eq!(
            classify(&[host, p(12, 10, "conhost.exe")], 10, None),
            (AgentProcess::Starting, None),
            "a console helper is not the agent"
        );
    }

    #[test]
    fn a_root_that_is_not_a_shell_is_the_agent_itself() {
        assert_eq!(classify(&[p(20, 1, "cursor-agent.exe")], 20, None).0, AgentProcess::Alive);
        assert_eq!(classify(&[], 20, None).0, AgentProcess::Exited, "the pane's process is gone");
    }

    #[cfg(windows)]
    #[test]
    fn the_snapshot_sees_this_test_process() {
        let me = std::process::id();
        let procs = snapshot().expect("toolhelp snapshot");
        assert!(procs.iter().any(|entry| entry.pid == me), "{} processes", procs.len());
    }
}
