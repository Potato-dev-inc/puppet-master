//! Stopping a pane's processes inside the distro (spec section 3.4). Pure command building: the
//! stop script is a fixed constant, and the pane id reaches it only as a positional argument.
//!
//! Only processes that this pane started are signalled: the script looks for the exact line
//! `PUPPET_MASTER_PANE_ID=<pane>` in each process environment and only for processes that the same
//! Linux user owns. It never matches by name and never shuts WSL down.

use super::model::DistroName;

/// `$0` of the stop script (shown in `ps` only).
pub const STOP_ARGV0: &str = "puppet-master-stop";

/// POSIX sh. `$1` = pane id. Signals (TERM, then KILL after 2 s) every process of this Linux user
/// whose environment has the exact line `PUPPET_MASTER_PANE_ID=$1`, skips its own shell and its
/// parent, and prints `pm-stopped=<n>` (the number of processes it sent TERM to).
/// Note: `-O` tests that the process directory is owned by the effective user (same as `id -u`).
/// A process that has already exited, or is a zombie, has no environment left and is not counted.
pub const STOP_SCRIPT: &str = r##"want="PUPPET_MASTER_PANE_ID=$1"
me=$$
up=$PPID
marked() {
  [ "$1" != "$me" ] && [ "$1" != "$up" ] && [ -O "/proc/$1" ] && tr '\0' '\n' < "/proc/$1/environ" 2>/dev/null | grep -qxF -- "$want"
}
targets=""
count=0
for dir in /proc/[0-9]*; do
  pid=${dir#/proc/}
  if marked "$pid"; then
    kill -TERM "$pid" 2>/dev/null
    targets="$targets $pid"
    count=$((count + 1))
  fi
done
i=0
while [ -n "$targets" ] && [ "$i" -lt 20 ]; do
  sleep 0.1
  left=""
  for pid in $targets; do
    if marked "$pid"; then left="$left $pid"; fi
  done
  targets=$left
  i=$((i + 1))
done
for pid in $targets; do
  if marked "$pid"; then kill -KILL "$pid" 2>/dev/null; fi
done
echo "pm-stopped=$count""##;

/// A pane id is 1 to 80 ASCII letters, digits, `_` or `-`. Anything else never reaches a runner.
pub fn valid_pane_id(id: &str) -> bool {
    (1..=80).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Why a stop did not succeed. Nothing here is shown as a code: the caller decides the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopError {
    /// The pane id failed `valid_pane_id`; nothing ran.
    BadPaneId,
    /// The `wsl.exe` call failed or timed out (the runner's message).
    Run(String),
    /// The call returned, but no `pm-stopped=<n>` line came back.
    NoReport,
}

/// The `wsl.exe` argv (without the program itself): the fixed script, `$0`, then the pane id.
pub fn stop_args(distro: &DistroName, pane_id: &str) -> Result<Vec<String>, StopError> {
    if !valid_pane_id(pane_id) {
        return Err(StopError::BadPaneId);
    }
    Ok(vec![
        "-d".to_string(),
        distro.as_str().to_string(),
        "--exec".to_string(),
        "/bin/sh".to_string(),
        "-c".to_string(),
        STOP_SCRIPT.to_string(),
        STOP_ARGV0.to_string(),
        pane_id.to_string(),
    ])
}

/// The count from the last `pm-stopped=<n>` line (digits only), or `None` for any other output.
pub fn parse_stopped(output: &str) -> Option<u32> {
    output.lines().rev().find_map(|line| {
        let rest = line.trim().strip_prefix("pm-stopped=")?;
        if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        rest.parse().ok()
    })
}

/// Runs the stop through `run` (the caller's `wsl.exe` runner, with its own timeout) and returns
/// how many processes were signalled.
pub fn stop_in(
    distro: &DistroName,
    pane_id: &str,
    run: &dyn Fn(&[String]) -> Result<String, String>,
) -> Result<u32, StopError> {
    let args = stop_args(distro, pane_id)?;
    let output = run(&args).map_err(StopError::Run)?;
    parse_stopped(&output).ok_or(StopError::NoReport)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn distro() -> DistroName {
        DistroName::parse("Ubuntu-Test").unwrap()
    }

    #[test]
    fn args_run_the_fixed_script_with_the_pane_id_last() {
        let args = stop_args(&distro(), "pane-7_B").unwrap();
        assert_eq!(
            args,
            ["-d", "Ubuntu-Test", "--exec", "/bin/sh", "-c", STOP_SCRIPT, STOP_ARGV0, "pane-7_B"]
        );
    }

    #[test]
    fn pane_id_is_never_written_into_the_script() {
        assert!(!STOP_SCRIPT.contains("pane-7_B"));
        let args = stop_args(&distro(), "pane-7_B").unwrap();
        assert_eq!(args[5], STOP_SCRIPT);
    }

    #[test]
    fn script_matches_the_exact_marker_and_the_same_user() {
        for needle in [
            "PUPPET_MASTER_PANE_ID=$1",
            "grep -qxF",
            "[ -O \"/proc/$1\" ]",
            "kill -TERM",
            "kill -KILL",
            "$PPID",
            "pm-stopped=$count",
        ] {
            assert!(STOP_SCRIPT.contains(needle), "script lacks {needle:?}");
        }
    }

    #[test]
    fn bad_pane_ids_are_refused_before_any_call() {
        let calls = Cell::new(0);
        let run = |_: &[String]| {
            calls.set(calls.get() + 1);
            Ok::<String, String>("pm-stopped=0".to_string())
        };
        let long = "a".repeat(81);
        for bad in ["", "a b", "a;b", "$(touch x)", "x\"", "pane\n", "a/b", "a.b", long.as_str()] {
            assert_eq!(stop_args(&distro(), bad), Err(StopError::BadPaneId), "{bad:?}");
            assert_eq!(stop_in(&distro(), bad, &run), Err(StopError::BadPaneId), "{bad:?}");
        }
        assert_eq!(calls.get(), 0, "a refused id never reaches the runner");
    }

    #[test]
    fn good_pane_ids_are_accepted_at_the_edges() {
        assert!(valid_pane_id("a"));
        assert!(valid_pane_id("A_b-9"));
        assert!(valid_pane_id(&"z".repeat(80)));
        assert!(!valid_pane_id(&"z".repeat(81)));
    }

    #[test]
    fn parse_stopped_reads_the_count_line() {
        assert_eq!(parse_stopped("pm-stopped=3\n"), Some(3));
        assert_eq!(parse_stopped("noise\npm-stopped=0\r\n"), Some(0));
        assert_eq!(parse_stopped("pm-stopped=2\npm-stopped=5\n"), Some(5), "the last line wins");
    }

    #[test]
    fn parse_stopped_refuses_anything_else() {
        assert_eq!(parse_stopped(""), None);
        assert_eq!(parse_stopped("pm-stopped=\n"), None);
        assert_eq!(parse_stopped("pm-stopped=x\n"), None);
        assert_eq!(parse_stopped("pm-stopped=-1\n"), None);
        assert_eq!(parse_stopped("pm-stopped=99999999999999999999\n"), None);
        assert_eq!(parse_stopped("stopped=2\n"), None);
    }

    #[test]
    fn stop_in_returns_the_count_from_a_good_run() {
        let seen = std::cell::RefCell::new(Vec::new());
        let run = |args: &[String]| {
            seen.borrow_mut().push(args.to_vec());
            Ok("pm-stopped=2\n".to_string())
        };
        assert_eq!(stop_in(&distro(), "pane-1", &run), Ok(2));
        assert_eq!(seen.borrow()[0], stop_args(&distro(), "pane-1").unwrap());
    }

    #[test]
    fn stop_in_reports_a_timeout_or_failed_call() {
        let run = |_: &[String]| Err("wsl.exe timed out after 8000 ms".to_string());
        assert_eq!(
            stop_in(&distro(), "pane-1", &run),
            Err(StopError::Run("wsl.exe timed out after 8000 ms".to_string()))
        );
    }

    #[test]
    fn stop_in_refuses_garbage_output() {
        let run = |_: &[String]| Ok("hello there\n".to_string());
        assert_eq!(stop_in(&distro(), "pane-1", &run), Err(StopError::NoReport));
    }
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod live_tests;
