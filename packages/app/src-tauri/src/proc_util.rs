//! The one door for starting a helper child process (NOT a pane).
//!
//! Puppet Master is a GUI program with no console. On Windows a console program started from such a
//! process (`powershell.exe`, `cmd.exe`, `git.exe`, `taskkill.exe`, `tasklist.exe`, `where.exe`, a
//! `.cmd` launcher) gets a console window of its own, even when its stdout is piped, so every
//! spawn that forgets `CREATE_NO_WINDOW` flashes a window on screen for as long as the child lives.
//! A run that starts the Cursor agent (its Windows launcher is a PowerShell script) is the worst
//! case: a PowerShell per run, one per version/about probe, one per stop.
//!
//! So no code outside this file calls `Command::new`: it calls [`hidden_command`] (std) or
//! [`hidden_tokio_command`] (tokio), or [`hide_window`] on a command built elsewhere. The guard
//! test `no_direct_command_new` in `proc_util/guard.rs` fails the build when one does. ConPTY panes
//! are the other kind of child: they are started by `portable_pty::CommandBuilder` on a
//! pseudo console, which never opens a window, and are not touched here.
//!
//! Every command made here is also noted in a tiny in-memory spawn log (off unless a test or the
//! `spawn_log` caller turns it on) so a test can count how many processes one action starts.

use std::ffi::OsStr;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// `CREATE_NO_WINDOW`: the child is a console program that gets no console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The process-creation flags every helper child gets (zero where there is no such thing).
pub const fn hidden_creation_flags() -> u32 {
    if cfg!(windows) {
        CREATE_NO_WINDOW
    } else {
        0
    }
}

/// A `std::process::Command` for `program` that never opens a console window.
#[track_caller]
pub fn hidden_command<S: AsRef<OsStr>>(program: S) -> Command {
    let program = program.as_ref();
    let mut command = Command::new(program);
    hide_window(&mut command);
    spawn_log::note(program, std::panic::Location::caller());
    command
}

/// Keep `command` (made elsewhere, for example by a helper that takes a `Command` argument) from
/// opening a console window. Idempotent. Does nothing off Windows.
pub fn hide_window(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(hidden_creation_flags());
    }
    command
}

/// A `tokio::process::Command` for `program` that never opens a console window.
#[allow(dead_code)] // no async spawn exists yet; the guard test points the next one here
#[track_caller]
pub fn hidden_tokio_command<S: AsRef<OsStr>>(program: S) -> tokio::process::Command {
    let program = program.as_ref();
    #[allow(unused_mut)]
    let mut command = tokio::process::Command::new(program);
    #[cfg(windows)]
    command.creation_flags(hidden_creation_flags());
    spawn_log::note(program, std::panic::Location::caller());
    command
}

/// True while process `pid` is running, answered by the OS directly: no child process, so nothing
/// can flash and a poll costs microseconds. Unknown (0) counts as alive, so nothing is declared dead
/// without evidence. Off Windows it asks `kill -0`.
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return true;
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
            fn GetExitCodeProcess(handle: *mut core::ffi::c_void, code: *mut u32) -> i32;
            fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
        }
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const STILL_ACTIVE: u32 = 259;
        // SAFETY: plain Win32 calls; the handle is closed before returning.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false; // no such process
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok == 0 || code == STILL_ACTIVE
        }
    }
    #[cfg(not(windows))]
    {
        // Cannot tell: assume alive, so control is never released on a guess.
        hidden_command("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|out| out.status.success())
            .unwrap_or(true)
    }
}

/// Remembers what a probe process printed for `ttl`, so asking again soon starts no process.
/// Only successful probes (`Some`) are kept: a failure is asked again next time.
pub struct TtlCache<K, V> {
    ttl: std::time::Duration,
    slots: parking_lot::Mutex<std::collections::HashMap<K, (std::time::Instant, V)>>,
}

impl<K: Eq + std::hash::Hash, V: Clone> TtlCache<K, V> {
    pub fn new(ttl: std::time::Duration) -> Self {
        Self { ttl, slots: parking_lot::Mutex::new(std::collections::HashMap::new()) }
    }

    /// The cached value for `key` if it is younger than the ttl, else `probe()` (run WITHOUT the
    /// lock held, so a slow probe never blocks other keys) and, when it answered, remember it.
    pub fn get_or_probe(&self, key: K, probe: impl FnOnce() -> Option<V>) -> Option<V>
    where
        K: Clone,
    {
        self.get_or_probe_at(std::time::Instant::now(), key, probe)
    }

    /// [`Self::get_or_probe`] with the clock passed in (tests).
    pub fn get_or_probe_at(&self, now: std::time::Instant, key: K, probe: impl FnOnce() -> Option<V>) -> Option<V>
    where
        K: Clone,
    {
        if let Some((at, value)) = self.slots.lock().get(&key) {
            if now.saturating_duration_since(*at) < self.ttl {
                return Some(value.clone());
            }
        }
        let value = probe()?;
        self.slots.lock().insert(key, (now, value.clone()));
        Some(value)
    }
}

/// A switchable count of the commands made above, so a test can say "this action started N
/// processes". Off by default: when nobody records, `note` is one relaxed atomic load.
pub mod spawn_log {
    use super::*;
    use parking_lot::Mutex;

    static RECORDING: AtomicBool = AtomicBool::new(false);
    static TOTAL: AtomicUsize = AtomicUsize::new(0);
    static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub(super) fn note(program: &OsStr, at: &std::panic::Location<'_>) {
        TOTAL.fetch_add(1, Ordering::Relaxed);
        if RECORDING.load(Ordering::Relaxed) {
            let file = at.file().replace('\\', "/");
            LOG.lock().push(format!("{} @ {}:{}", program.to_string_lossy(), file, at.line()));
        }
    }

    /// Commands made since the process started.
    pub fn total() -> usize {
        TOTAL.load(Ordering::Relaxed)
    }

    /// Start (and clear) the program-name log.
    pub fn start() {
        LOG.lock().clear();
        RECORDING.store(true, Ordering::Relaxed);
    }

    /// Stop recording and return `"<program> @ <file>:<line>"` for each command, in the order they
    /// were made (the line is where `hidden_command` was called).
    pub fn finish() -> Vec<String> {
        RECORDING.store(false, Ordering::Relaxed);
        std::mem::take(&mut *LOG.lock())
    }
}

#[cfg(test)]
mod guard;
#[cfg(test)]
mod tests;
