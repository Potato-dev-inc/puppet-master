use super::*;

/// The spawn log is process-wide; the tests that start and finish it take turns.
static LOG_TURN: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[test]
fn the_flag_is_create_no_window_on_windows_and_nothing_elsewhere() {
    assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
    assert_eq!(hidden_creation_flags(), if cfg!(windows) { 0x0800_0000 } else { 0 });
}

#[test]
fn hidden_commands_are_noted_with_their_program_and_caller() {
    let _turn = LOG_TURN.lock();
    spawn_log::start();
    let _ = hidden_command("pm-test-program-one");
    let _ = hidden_command(std::ffi::OsString::from("pm-test-program-two"));
    let mut plain = Command::new("pm-test-program-three");
    hide_window(&mut plain); // an already-built command is not a new spawn
    let log: Vec<String> = spawn_log::finish().into_iter().filter(|line| line.starts_with("pm-test-program")).collect();
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(log[0].starts_with("pm-test-program-one @ "), "{log:?}");
    assert!(log[1].starts_with("pm-test-program-two @ "), "{log:?}");
    assert!(spawn_log::total() >= 2);
}

#[test]
fn a_hidden_command_runs_and_returns_its_output() {
    let (program, args): (&str, &[&str]) = if cfg!(windows) { ("cmd.exe", &["/C", "echo hidden-ok"]) } else { ("sh", &["-c", "echo hidden-ok"]) };
    let out = hidden_command(program).args(args).output().expect("run");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("hidden-ok"));
}

#[tokio::test]
async fn a_hidden_tokio_command_runs_too() {
    let (program, args): (&str, &[&str]) = if cfg!(windows) { ("cmd.exe", &["/C", "echo hidden-ok"]) } else { ("sh", &["-c", "echo hidden-ok"]) };
    let out = hidden_tokio_command(program).args(args).output().await.expect("run");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("hidden-ok"));
}

#[test]
fn pid_alive_tells_running_from_gone() {
    assert!(pid_alive(std::process::id()));
    assert!(pid_alive(0), "an unknown pid is never declared dead");
    let (program, args): (&str, &[&str]) = if cfg!(windows) { ("cmd.exe", &["/C", "exit 0"]) } else { ("true", &[]) };
    let mut child = hidden_command(program).args(args).spawn().expect("spawn");
    let pid = child.id();
    child.wait().expect("wait");
    #[cfg(windows)]
    assert!(!pid_alive(pid));
    #[cfg(not(windows))]
    let _ = pid;
}

#[test]
fn pid_alive_costs_no_child_process() {
    let _turn = LOG_TURN.lock();
    spawn_log::start();
    for _ in 0..50 {
        assert!(pid_alive(std::process::id()));
    }
    let spawned: Vec<String> = spawn_log::finish().into_iter().filter(|line| line.contains("proc_util.rs")).collect();
    #[cfg(windows)]
    assert!(spawned.is_empty(), "polling a pid must not start a process: {spawned:?}");
    #[cfg(not(windows))]
    let _ = spawned;
}

/// The seam for "does the child really get no console window": the test binary runs itself with
/// this test as the child's whole job, and the child reports `GetConsoleWindow()`. A console
/// program started with `CREATE_NO_WINDOW` has no console window, so the handle is 0.
#[test]
fn console_window_probe_child() {
    if std::env::var_os("PM_PROC_UTIL_PROBE").is_none() {
        return; // the real run of this test is the parent's job below
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetConsoleWindow() -> *mut core::ffi::c_void;
        }
        // SAFETY: no arguments, returns a handle or null.
        let handle = unsafe { GetConsoleWindow() } as usize;
        println!("PM_CONSOLE_WINDOW={handle}");
    }
}

#[cfg(windows)]
fn probe_output(mut command: Command) -> String {
    let exe = std::env::current_exe().expect("test exe");
    let out = command
        .arg("--exact")
        .arg("proc_util::tests::console_window_probe_child")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env("PM_PROC_UTIL_PROBE", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("run {}: {e}", exe.display()));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[cfg(windows)]
#[test]
fn hidden_command_children_have_no_console_window() {
    let exe = std::env::current_exe().expect("test exe");
    let seen = probe_output(hidden_command(&exe));
    assert!(seen.contains("PM_CONSOLE_WINDOW=0"), "the child had a console window: {seen}");
}

#[cfg(windows)]
#[test]
fn hide_window_hides_a_command_built_elsewhere() {
    let exe = std::env::current_exe().expect("test exe");
    let mut command = Command::new(&exe);
    hide_window(&mut command);
    let seen = probe_output(command);
    assert!(seen.contains("PM_CONSOLE_WINDOW=0"), "the child had a console window: {seen}");
}

/// Proof that the probe above can fail: when this test run itself has a console window (a terminal),
/// a plain child inherits it and reports it, while the hidden child reports none. Skipped (and said
/// so) when the run has no console window, for example under a service or a CI runner.
#[cfg(windows)]
#[test]
fn the_probe_tells_a_hidden_child_from_a_plain_one() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetConsoleWindow() -> *mut core::ffi::c_void;
    }
    // SAFETY: no arguments, returns a handle or null.
    if unsafe { GetConsoleWindow() }.is_null() {
        println!("skipped: this run has no console window, so a plain child would create one");
        return;
    }
    let exe = std::env::current_exe().expect("test exe");
    let plain = probe_output(Command::new(&exe));
    assert!(!plain.contains("PM_CONSOLE_WINDOW=0"), "a plain child should inherit the console window: {plain}");
    let hidden = probe_output(hidden_command(&exe));
    assert!(hidden.contains("PM_CONSOLE_WINDOW=0"), "{hidden}");
}

#[test]
fn a_ttl_cache_probes_once_within_its_window_and_again_after() {
    use std::time::{Duration, Instant};
    let cache: TtlCache<&str, String> = TtlCache::new(Duration::from_secs(30));
    let (t0, mut asked) = (Instant::now(), 0);
    let mut ask = |at: Instant, key: &'static str, answer: Option<&str>| {
        cache.get_or_probe_at(at, key, || {
            asked += 1;
            answer.map(str::to_string)
        })
    };
    assert_eq!(ask(t0, "a", Some("1.0")).as_deref(), Some("1.0"));
    assert_eq!(ask(t0 + Duration::from_secs(10), "a", Some("2.0")).as_deref(), Some("1.0"), "remembered");
    assert_eq!(ask(t0 + Duration::from_secs(10), "b", Some("9")).as_deref(), Some("9"), "another key probes");
    assert_eq!(ask(t0 + Duration::from_secs(31), "a", Some("2.0")).as_deref(), Some("2.0"), "expired");
    assert_eq!(ask(t0 + Duration::from_secs(32), "c", None), None);
    assert_eq!(ask(t0 + Duration::from_secs(33), "c", Some("late")).as_deref(), Some("late"), "a failure is asked again");
    assert_eq!(asked, 5);
}
