//! Keep the computer awake while Night Shift runs (SPEC n32, section 3.9).
//!
//! [`keep_awake`] returns a [`Guard`]. Dropping the guard lets the computer sleep again. What the
//! guard holds depends on the system:
//! - Windows: a thread that sets `ES_CONTINUOUS | ES_SYSTEM_REQUIRED` on itself and waits. The flag
//!   lasts only as long as that thread. Drop wakes the thread, it clears the flag and exits.
//! - macOS: `caffeinate -i -s -w <our pid>`. Drop kills that child by the pid we spawned. `-w` ends
//!   it by itself if Puppet Master crashes.
//! - Linux: `systemd-inhibit ... sleep infinity`, killed the same way.
//!
//! No process is ever found or stopped by its name. The lid and the power button still sleep the
//! computer; that is the system's own rule.

use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use crate::proc_util::hidden_command;

/// The keep-awake hold. Dropping it releases the computer.
pub struct Guard {
    held: Held,
}

enum Held {
    /// Windows: the thread that holds the execution state, and the channel that tells it to clear it.
    #[cfg(windows)]
    Thread { stop: Option<Sender<()>>, thread: Option<JoinHandle<()>> },
    /// macOS / Linux: the helper process we spawned, killed by its pid on drop.
    Child(Child),
}

impl Drop for Guard {
    fn drop(&mut self) {
        match &mut self.held {
            #[cfg(windows)]
            Held::Thread { stop, thread } => {
                if let Some(stop) = stop.take() {
                    // The thread may already be gone; nothing to do then.
                    let _ = stop.send(());
                }
                if let Some(thread) = thread.take() {
                    let _ = thread.join();
                }
            }
            Held::Child(child) => {
                // Kill by the pid we spawned, then reap it so no zombie is left behind.
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// Start keeping the computer awake. `reason` is the text the system shows for the hold (macOS and
/// Linux). `Err("not available")` means this system has no helper for it (for example, the helper
/// program is missing). Other errors are text for the page.
pub fn keep_awake(reason: &str) -> Result<Guard, String> {
    #[cfg(windows)]
    {
        // Windows keeps no text for the hold, so `reason` is not used there.
        let _ = reason;
        keep_awake_windows()
    }
    #[cfg(not(windows))]
    {
        let argv = command_for(std::env::consts::OS, std::process::id(), reason).ok_or_else(|| "not available".to_string())?;
        spawn_child(&argv)
    }
}

/// The helper command that keeps the computer awake on `os` (`std::env::consts::OS` names), or
/// `None` where there is no such helper (Windows uses the thread instead, and unknown systems have
/// none). `pid` is our own process id, which `caffeinate -w` watches. Pure: nothing is started.
pub fn command_for(os: &str, pid: u32, reason: &str) -> Option<Vec<String>> {
    match os {
        "macos" => Some(vec!["caffeinate".into(), "-i".into(), "-s".into(), "-w".into(), pid.to_string()]),
        "linux" => Some(vec![
            "systemd-inhibit".into(),
            "--what=idle:sleep".into(),
            "--who=Puppet Master".into(),
            format!("--why={reason}"),
            "--mode=block".into(),
            "sleep".into(),
            "infinity".into(),
        ]),
        _ => None,
    }
}

/// Start the helper `argv` (program first). A missing program is `"not available"`.
#[cfg(not(windows))]
fn spawn_child(argv: &[String]) -> Result<Guard, String> {
    let (program, args) = argv.split_first().ok_or_else(|| "not available".to_string())?;
    let child = hidden_command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => "not available".to_string(),
            _ => format!("could not keep the computer awake: {err}"),
        })?;
    Ok(Guard { held: Held::Child(child) })
}

/// Set the execution state on a new thread and keep that thread alive until the guard drops.
#[cfg(windows)]
fn keep_awake_windows() -> Result<Guard, String> {
    use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};

    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<bool>();
    let thread = std::thread::Builder::new()
        .name("night-shift-keep-awake".into())
        .spawn(move || {
            // SAFETY: plain Win32 call. The state belongs to this thread, and it is cleared below
            // before the thread exits.
            let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            let ok = previous != 0; // the call returns NULL on failure
            let _ = ready_tx.send(ok);
            if !ok {
                return;
            }
            // Wait for the stop. A dropped sender also ends the wait.
            let _ = stop_rx.recv();
            // SAFETY: as above; this clears the flag we set on this thread.
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        })
        .map_err(|err| format!("could not start the keep-awake thread: {err}"))?;
    match ready_rx.recv() {
        Ok(true) => Ok(Guard { held: Held::Thread { stop: Some(stop_tx), thread: Some(thread) } }),
        _ => {
            let _ = thread.join();
            Err("could not keep the computer awake".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::proc_tree::test_support::{pid_alive, wait_until_dead};

    #[test]
    fn macos_command_watches_our_pid_and_never_sleeps_idle_or_system() {
        assert_eq!(
            command_for("macos", 4242, "Night Shift"),
            Some(vec!["caffeinate".into(), "-i".into(), "-s".into(), "-w".into(), "4242".into()])
        );
    }

    #[test]
    fn linux_command_inhibits_idle_and_sleep_with_the_reason() {
        assert_eq!(
            command_for("linux", 7, "Night Shift"),
            Some(vec![
                "systemd-inhibit".into(),
                "--what=idle:sleep".into(),
                "--who=Puppet Master".into(),
                "--why=Night Shift".into(),
                "--mode=block".into(),
                "sleep".into(),
                "infinity".into(),
            ])
        );
    }

    #[test]
    fn windows_and_unknown_systems_have_no_helper_command() {
        assert_eq!(command_for("windows", 1, "Night Shift"), None);
        assert_eq!(command_for("freebsd", 1, "Night Shift"), None);
    }

    /// A harmless child that runs for about 30 seconds, for the drop test.
    fn long_sleeper() -> Child {
        #[cfg(windows)]
        let mut command = {
            let mut c = hidden_command("ping");
            c.args(["-n", "30", "127.0.0.1"]);
            c
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut c = hidden_command("sleep");
            c.arg("30");
            c
        };
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("spawn sleeper")
    }

    #[test]
    fn dropping_the_guard_kills_the_child_by_its_pid() {
        let child = long_sleeper();
        let pid = child.id();
        assert!(pid_alive(pid), "the sleeper should be running before the drop");
        let guard = Guard { held: Held::Child(child) };
        drop(guard);
        assert!(wait_until_dead(pid), "the sleeper should be gone after the drop");
    }

    #[cfg(windows)]
    #[test]
    fn keep_awake_starts_and_its_thread_exits_on_drop() {
        // Sets the real flag for a moment. Cannot read the flag back, so this only checks that
        // the hold starts and that dropping it returns (the thread is joined).
        let guard = keep_awake("Night Shift").expect("keep_awake should start on Windows");
        drop(guard);
    }
}
