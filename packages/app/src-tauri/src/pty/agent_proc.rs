//! Is the agent in a pane still running? (host-shell sentinel, process side)
//!
//! Agent panes on Windows run the agent under a host shell: `powershell -NoExit -Command & 'agent'`
//! (`registry::command::build_command`). When the agent exits, the shell stays and shows its prompt,
//! so the pane's own process (the shell) being alive says nothing about the agent. This module looks
//! at the pane's process tree instead:
//!
//! - the pane's root process is a shell (by image name, never by agent name): the agent is its child.
//!   A child that was seen alive for at least [`CONFIRM_MS`] (or seen once and gone at a look more
//!   than that later) and is gone now = the agent exited; none seen yet, or one seen and gone again
//!   within [`CONFIRM_MS`] = still starting (or the command failed before it started,
//!   see `tui::shell::command_not_found`). The host runs the user's PowerShell profile first, and
//!   profiles start helpers (`oh-my-posh init`, `conda ... hook`, `starship`, `git`) that come and go
//!   before the agent starts: a child is remembered as the agent only once it has lived that long
//!   (review R12 1.3), and known prompt/profile tool images are never the agent.
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
/// Helper processes a console host or a shell profile starts that are not the agent: the console
/// host itself, and prompt/profile tools a PowerShell profile commonly runs. Tools, not agents.
const NOT_AGENTS: [&str; 9] = [
    "conhost.exe",
    "openconsole.exe",
    "oh-my-posh.exe",
    "starship.exe",
    "conda.exe",
    "git.exe",
    "zoxide.exe",
    "fnm.exe",
    "mise.exe",
];

/// How long a host shell's child must be seen alive before it counts as the agent (milliseconds).
pub(crate) const CONFIRM_MS: u64 = 1_000;

/// What was seen under one pane's host shell.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Track {
    /// The child confirmed as the agent (seen alive for [`CONFIRM_MS`]).
    pub agent: Option<u32>,
    /// The child seen now that is not confirmed yet, with when it was first seen.
    pub candidate: Option<(u32, u64)>,
}

#[derive(Debug, Default, Clone)]
struct Seen {
    root: u32,
    track: Track,
}

fn now_ms() -> u64 {
    static START: Lazy<std::time::Instant> = Lazy::new(std::time::Instant::now);
    START.elapsed().as_millis() as u64
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
    let now = now_ms();
    let mut seen = SEEN.lock();
    let entry = seen.entry(pane_id.to_string()).or_default();
    if entry.root != root_pid {
        *entry = Seen { root: root_pid, track: Track::default() };
    }
    classify(&procs, root_pid, &mut entry.track, now)
}

/// Pure decision over one snapshot taken at `now_ms`, updating what is remembered in `track`.
pub(crate) fn classify(procs: &[ProcEntry], root_pid: u32, track: &mut Track, now_ms: u64) -> AgentProcess {
    let Some(root) = procs.iter().find(|p| p.pid == root_pid) else {
        return AgentProcess::Exited;
    };
    if !HOST_SHELLS.contains(&root.image.as_str()) {
        return AgentProcess::Alive;
    }
    let child = procs
        .iter()
        .filter(|p| p.parent == root_pid && p.pid != root_pid)
        .find(|p| !NOT_AGENTS.contains(&p.image.as_str()));
    match child {
        Some(child) => {
            if track.agent != Some(child.pid) {
                let since = match track.candidate {
                    Some((pid, since)) if pid == child.pid => since,
                    _ => now_ms,
                };
                track.candidate = Some((child.pid, since));
                if now_ms.saturating_sub(since) >= CONFIRM_MS {
                    track.agent = Some(child.pid);
                    track.candidate = None;
                }
            }
            AgentProcess::Alive
        }
        None => match (track.agent, track.candidate.take()) {
            (Some(_), _) => AgentProcess::Exited,
            // Seen and gone again within a second (a profile helper before the agent started): it
            // is forgotten and the pane is still starting.
            (None, Some((_, since))) if now_ms.saturating_sub(since) < CONFIRM_MS => AgentProcess::Starting,
            // Seen once, and gone at a look more than a second later: nobody looked in between, so
            // it may well have run for minutes (the agent killed at its input box, with its last
            // frame still on screen). That is an exit, as before.
            (None, Some((pid, _))) => {
                track.agent = Some(pid);
                AgentProcess::Exited
            }
            (None, None) => AgentProcess::Starting,
        },
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
        let mut track = Track::default();
        assert_eq!(classify(&[host.clone()], 10, &mut track, 0), AgentProcess::Starting);
        assert_eq!(classify(&[host.clone(), agent.clone()], 10, &mut track, 100), AgentProcess::Alive);
        assert_eq!(classify(&[host.clone(), agent], 10, &mut track, 1_200), AgentProcess::Alive);
        assert_eq!(track.agent, Some(11), "confirmed after a second alive");
        assert_eq!(classify(&[host.clone()], 10, &mut track, 1_300), AgentProcess::Exited, "seen, then gone");
        let mut fresh = Track::default();
        assert_eq!(
            classify(&[host, p(12, 10, "conhost.exe")], 10, &mut fresh, 0),
            AgentProcess::Starting,
            "a console helper is not the agent"
        );
    }

    /// Review R12 1.3: the host runs the user's PowerShell profile before `& 'agent'`, and the
    /// profile's helper (oh-my-posh, conda's hook, starship, git) was remembered as the agent: when
    /// it exited before the agent started, the pane read `Exited` and a healthy run failed
    /// `AGENT_EXITED`.
    #[test]
    fn a_short_lived_profile_child_before_the_agent_is_not_the_agent() {
        let host = p(10, 1, "powershell.exe");
        let mut track = Track::default();
        // A profile script's own tool (an image no list knows) lives 300 ms.
        let helper = p(30, 10, "profile-helper.exe");
        assert_eq!(classify(&[host.clone(), helper.clone()], 10, &mut track, 0), AgentProcess::Alive);
        assert_eq!(classify(&[host.clone(), helper], 10, &mut track, 300), AgentProcess::Alive);
        // The gap before `& 'claude'` starts its process.
        assert_eq!(classify(&[host.clone()], 10, &mut track, 400), AgentProcess::Starting, "not Exited");
        // The agent starts and stays.
        let agent = p(31, 10, "node.exe");
        assert_eq!(classify(&[host.clone(), agent.clone()], 10, &mut track, 600), AgentProcess::Alive);
        assert_eq!(classify(&[host.clone(), agent], 10, &mut track, 1_700), AgentProcess::Alive);
        assert_eq!(track.agent, Some(31));
        assert_eq!(classify(&[host], 10, &mut track, 1_800), AgentProcess::Exited);
    }

    #[test]
    fn a_child_seen_once_and_gone_at_a_much_later_look_has_exited() {
        // Sparse looks: nobody looked while it ran, so it may have run for minutes.
        let host = p(10, 1, "powershell.exe");
        let mut track = Track::default();
        assert_eq!(classify(&[host.clone(), p(50, 10, "node.exe")], 10, &mut track, 0), AgentProcess::Alive);
        assert_eq!(classify(&[host.clone()], 10, &mut track, 5_000), AgentProcess::Exited);
        assert_eq!(classify(&[host], 10, &mut track, 5_100), AgentProcess::Exited, "and stays exited");
    }

    #[test]
    fn known_prompt_and_profile_tools_are_never_the_agent_however_long_they_run() {
        let host = p(10, 1, "powershell.exe");
        let mut track = Track::default();
        for (at, image) in [(0, "oh-my-posh.exe"), (1_500, "conda.exe"), (3_000, "starship.exe"), (4_500, "git.exe")] {
            assert_eq!(classify(&[host.clone(), p(40, 10, image)], 10, &mut track, at), AgentProcess::Starting, "{image}");
        }
        assert_eq!(classify(&[host], 10, &mut track, 6_000), AgentProcess::Starting);
        assert_eq!(track.agent, None);
    }

    #[test]
    fn a_root_that_is_not_a_shell_is_the_agent_itself() {
        let mut track = Track::default();
        assert_eq!(classify(&[p(20, 1, "cursor-agent.exe")], 20, &mut track, 0), AgentProcess::Alive);
        assert_eq!(classify(&[], 20, &mut track, 10), AgentProcess::Exited, "the pane's process is gone");
    }

    #[cfg(windows)]
    #[test]
    fn the_snapshot_sees_this_test_process() {
        let me = std::process::id();
        let procs = snapshot().expect("toolhelp snapshot");
        assert!(procs.iter().any(|entry| entry.pid == me), "{} processes", procs.len());
    }
}
