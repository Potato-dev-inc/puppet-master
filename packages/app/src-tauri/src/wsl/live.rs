//! The live side of closing a Linux pane (SPEC-n33 section 3.6 F). `stop_pane` runs the fixed stop
//! script inside the distro through `wsl.exe`, after the pane's `wsl.exe` tree was ended by pid. It
//! never panics and never blocks longer than `STOP_TIMEOUT_MS`; a failure is logged and nothing else.
//! The caller is a kill worker thread or another background thread, never the UI thread.

use crate::wsl::model::{DistroName, STOP_TIMEOUT_MS};
use crate::wsl::probe::Runner;
use crate::wsl::stop;
use std::time::Duration;

/// Signal what pane `pane_id` left running in `distro`. Returns how many processes got the first
/// signal, or `None` when the call failed (logged).
#[cfg(not(test))]
pub fn stop_pane(distro: &str, pane_id: &str) -> Option<u32> {
    let exe = crate::wsl::probe::wsl_exe()?;
    stop_pane_with(&crate::wsl::probe::LiveRunner::new(exe), distro, pane_id)
}

/// Tests never start `wsl.exe`: the call is recorded (see `recorded_stops`) and reports no process.
#[cfg(test)]
pub fn stop_pane(distro: &str, pane_id: &str) -> Option<u32> {
    RECORDED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push((distro.to_string(), pane_id.to_string()));
    Some(0)
}

#[cfg(test)]
static RECORDED: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// Every `(distro, pane id)` that `stop_pane` was asked to stop in this test run.
#[cfg(test)]
pub fn recorded_stops() -> Vec<(String, String)> {
    RECORDED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// `stop_pane` through any runner (the timeout is `STOP_TIMEOUT_MS`).
pub fn stop_pane_with(runner: &dyn Runner, distro: &str, pane_id: &str) -> Option<u32> {
    let Some(name) = DistroName::parse(distro) else {
        tracing::warn!("linux stop skipped: the distro name is not valid");
        return None;
    };
    let run = |args: &[String]| -> Result<String, String> {
        runner
            .run(args, Duration::from_millis(STOP_TIMEOUT_MS))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|error| format!("{error:?}"))
    };
    match stop::stop_in(&name, pane_id, &run) {
        Ok(count) => {
            tracing::info!(pane = %pane_id, count, "stopped the pane's processes in Linux");
            Some(count)
        }
        Err(error) => {
            tracing::warn!(pane = %pane_id, "stopping the pane's processes in Linux failed: {error:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wsl::probe::RunError;
    use std::cell::RefCell;

    struct FakeRunner {
        seen: RefCell<Vec<Vec<String>>>,
        answer: Result<Vec<u8>, RunError>,
        timeout: RefCell<Option<Duration>>,
    }

    impl Runner for FakeRunner {
        fn run(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>, RunError> {
            self.seen.borrow_mut().push(args.to_vec());
            *self.timeout.borrow_mut() = Some(timeout);
            self.answer.clone()
        }
    }

    fn runner(answer: Result<Vec<u8>, RunError>) -> FakeRunner {
        FakeRunner { seen: RefCell::new(Vec::new()), answer, timeout: RefCell::new(None) }
    }

    #[test]
    fn the_stop_runs_the_fixed_script_with_the_pane_id_and_the_stop_timeout() {
        let fake = runner(Ok(b"pm-stopped=2\n".to_vec()));
        assert_eq!(stop_pane_with(&fake, "Ubuntu-Test", "pane-9"), Some(2));
        let seen = fake.seen.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].last().map(String::as_str), Some("pane-9"));
        assert!(seen[0].contains(&stop::STOP_SCRIPT.to_string()));
        assert_eq!(*fake.timeout.borrow(), Some(Duration::from_millis(STOP_TIMEOUT_MS)));
    }

    #[test]
    fn a_failed_or_garbled_stop_is_none_and_never_panics() {
        assert_eq!(stop_pane_with(&runner(Err(RunError::Timeout)), "Ubuntu-Test", "pane-9"), None);
        assert_eq!(stop_pane_with(&runner(Ok(b"nonsense".to_vec())), "Ubuntu-Test", "pane-9"), None);
    }

    #[test]
    fn a_bad_distro_or_pane_id_never_reaches_the_runner() {
        let fake = runner(Ok(b"pm-stopped=0".to_vec()));
        assert_eq!(stop_pane_with(&fake, "bad name", "pane-9"), None);
        assert_eq!(stop_pane_with(&fake, "Ubuntu-Test", "pane 9; rm"), None);
        assert!(fake.seen.borrow().is_empty());
    }

    #[test]
    fn the_test_build_records_a_stop_instead_of_starting_wsl() {
        assert_eq!(stop_pane("Ubuntu-Test", "live-record-pane"), Some(0));
        assert!(recorded_stops().contains(&("Ubuntu-Test".to_string(), "live-record-pane".to_string())));
    }
}
