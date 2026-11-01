//! The visible-window guard on fakes: the report is read right, a window or a doubt fails a start closed, and a failed
//! start stops the sandbox.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::sandbox::fakes_tests::FakeGuest;
use super::sandbox::lifecycle::{Lifecycle, LifecycleError, Timing, WindowCheck};
use super::sandbox::wsl::SandboxName;
use super::window_guard::{parse_report, visible_windows, Msrdc, Probe, WINLIST_SCRIPT};

/// Answers with a fixed text (or a failure) and counts the runs.
struct Fixed {
    answer: Result<String, String>,
    runs: AtomicUsize,
}

impl Fixed {
    fn new(answer: Result<&str, &str>) -> Arc<Self> {
        Arc::new(Self { answer: answer.map(str::to_string).map_err(str::to_string), runs: AtomicUsize::new(0) })
    }
}

impl Probe for Fixed {
    fn run(&self, script: &str) -> Result<String, String> {
        assert!(script.contains("EnumWindows"), "the script is the EnumWindows check");
        self.runs.fetch_add(1, Ordering::SeqCst);
        self.answer.clone()
    }
}

#[test]
fn the_script_asks_for_visible_windows_owned_by_msrdc() {
    assert!(WINLIST_SCRIPT.contains("IsWindowVisible") && WINLIST_SCRIPT.contains("msrdc,msrdcw"));
    assert!(WINLIST_SCRIPT.contains("visible msrdc windows:"), "the line the app reads");
    assert!(!WINLIST_SCRIPT.contains("Stop-Process") && !WINLIST_SCRIPT.contains("Remove-"), "it only looks");
}

#[test]
fn the_report_is_read() {
    assert_eq!(parse_report("msrdc pids:  visible msrdc windows: 0\n").unwrap().0, 0);
    let (count, line) = parse_report("noise\nmsrdc pids: 4242,77  visible msrdc windows: 2\n4242|WSLg|0,0,10,10\n").unwrap();
    assert_eq!(count, 2);
    assert_eq!(line, "msrdc pids: 4242,77  visible msrdc windows: 2");
    assert!(parse_report("").is_err());
    assert!(parse_report("visible msrdc windows: many").is_err());
    assert!(parse_report("some other output").is_err());
}

#[test]
fn no_window_is_fine_a_window_is_not() {
    let none = Msrdc::with_probe(Fixed::new(Ok("msrdc pids:   visible msrdc windows: 0")));
    assert!(!none.visible_remote_window());
    let some = Msrdc::with_probe(Fixed::new(Ok("msrdc pids: 12  visible msrdc windows: 1\n12|Ubuntu|0,0,800,600")));
    assert!(some.visible_remote_window());
}

#[test]
fn when_it_cannot_tell_the_start_is_stopped() {
    assert!(Msrdc::with_probe(Fixed::new(Err("powershell is not here"))).visible_remote_window());
    assert!(Msrdc::with_probe(Fixed::new(Ok("garbage"))).visible_remote_window());
    assert!(visible_windows(Fixed::new(Err("x")).as_ref()).is_err());
}

#[test]
fn a_visible_window_after_a_start_stops_the_sandbox_and_makes_an_error_card() {
    let guest = FakeGuest::with_installed("pm-sbx-main");
    let probe = Fixed::new(Ok("msrdc pids: 9  visible msrdc windows: 1"));
    let lifecycle = Lifecycle::new(guest.wsl(), SandboxName::main()).with_timing(Timing::instant()).with_window_check(Arc::new(Msrdc::with_probe(probe.clone())));
    let error = lifecycle.ensure_ready(0).unwrap_err();
    assert_eq!(error, LifecycleError::VisibleWindow);
    assert_eq!(probe.runs.load(Ordering::SeqCst), 1, "checked once, after the start");
    assert!(!guest.holder_alive(), "the sandbox was stopped");
    // the card the person reads
    assert!(error.code().starts_with("SANDBOX_"));
    assert!(error.message().ends_with('.'));
    assert!(!error.message().to_lowercase().contains("msrdc"), "{}", error.message());
}

#[test]
fn a_clean_start_passes_the_guard_and_is_ready() {
    let guest = FakeGuest::with_installed("pm-sbx-main");
    let probe = Fixed::new(Ok("msrdc pids:   visible msrdc windows: 0"));
    let lifecycle = Lifecycle::new(guest.wsl(), SandboxName::main()).with_timing(Timing::instant()).with_window_check(Arc::new(Msrdc::with_probe(probe.clone())));
    assert!(lifecycle.ensure_ready(0).is_ok());
    assert_eq!(probe.runs.load(Ordering::SeqCst), 1);
    assert!(guest.holder_alive());
}

#[test]
fn the_real_probe_is_hidden_and_writes_no_file() {
    let code = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/potato/window_guard.rs")).unwrap();
    let code = code.split("#[cfg(test)]").next().unwrap();
    assert!(code.contains("crate::proc_util::hidden_command(\"powershell.exe\")"));
    assert!(code.contains("-EncodedCommand"));
    assert!(!code.contains("fs::write") && !code.contains("File::create"));
}

#[test]
fn the_host_builds_the_real_lifecycle_with_the_guard() {
    let host = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/potato/sandbox_host.rs")).unwrap();
    assert!(host.contains(".with_window_check(Arc::new(Msrdc::live()))"), "a real sandbox is never built without the guard");
}
