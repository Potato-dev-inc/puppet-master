//! Live check for `stop_in`: starts `sleep 300` under a unique marker inside a real WSL 2 distro,
//! stops it, and asserts it is gone. Ignored by default (it boots a distro and starts `wsl.exe`).
//! Run by hand only:
//!   `PM_WSL_LIVE_DISTRO=<distro> cargo test --lib wsl::stop::live_tests -- --ignored`
//! Only the marker of this run is signalled. The child started here is killed by its own pid.

use super::*;
use crate::proc_util::hidden_command;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// The caller's runner for the live check: one `wsl.exe` call, output as text.
fn run_wsl(args: &[String]) -> Result<String, String> {
    let out = hidden_command("wsl.exe")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not start wsl.exe: {e}"))?;
    if !out.status.success() {
        return Err(format!("wsl.exe exited with {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).replace('\0', ""))
}

#[test]
#[ignore = "boots a WSL 2 distro; set PM_WSL_LIVE_DISTRO and run with --ignored"]
fn stop_ends_only_the_marked_process() {
    let name = std::env::var("PM_WSL_LIVE_DISTRO").expect("set PM_WSL_LIVE_DISTRO to a WSL 2 distro name");
    let distro = DistroName::parse(&name).expect("PM_WSL_LIVE_DISTRO is not a valid distro name");
    let pane = format!("livestop-{}", std::process::id());

    // `exec` makes `sleep` the marked process itself, so the marker is in its environment.
    let mut child = hidden_command("wsl.exe")
        .args([
            "-d",
            distro.as_str(),
            "--exec",
            "/bin/sh",
            "-c",
            r#"PUPPET_MASTER_PANE_ID="$1"; export PUPPET_MASTER_PANE_ID; exec sleep 300"#,
            "live-marker",
            pane.as_str(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start wsl.exe");

    // Give the distro time to start the marked sleep before the stop looks for it.
    std::thread::sleep(Duration::from_secs(3));
    let stopped = stop_in(&distro, &pane, &run_wsl);
    let _ = child.kill(); // our own child, started above
    let _ = child.wait();
    assert!(matches!(stopped, Ok(n) if n >= 1), "expected at least one stopped process, got {stopped:?}");

    // A second stop finds nothing left under this marker.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let again = stop_in(&distro, &pane, &run_wsl);
        if again == Ok(0) {
            break;
        }
        assert!(Instant::now() < deadline, "marked process still there: {again:?}");
        std::thread::sleep(Duration::from_millis(200));
    }
}
