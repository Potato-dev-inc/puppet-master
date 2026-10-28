//! Tests for `probe.rs` (SPEC n33 WSL-12). A fake `Runner` answers from the WSL-6 fixtures in
//! `wsl/fixtures/` (placeholders only: the distro `Ubuntu-Test`, the folder `you`). No test starts
//! `wsl.exe`; the one live-runner test uses a path that does not exist.

use super::*;
use crate::wsl::model::{
    FACTS_FRESH_MS, LIST_FRESH_MS, WSL_DISTRO_MISSING, WSL_NOT_INSTALLED, WSL_PROBE_TIMEOUT, WSL_START_FAILED,
};
use crate::wsl::stop;
use std::cell::RefCell;

const LIST_EN: &str = include_str!("fixtures/list_en.txt");
const LIST_DE: &[u8] = include_bytes!("fixtures/list_de.utf16");
const LIST_WSL1: &str = include_str!("fixtures/list_wsl1.txt");
const PROBE_OK: &str = include_str!("fixtures/probe_ok.txt");
const PROBE_NOISE: &str = include_str!("fixtures/probe_noise.txt");

/// The nonce written into the fixtures. The fake swaps it for the nonce the call really used.
const NONCE: &str = "pm6nonce";
const DOCKER_ONLY: &str = "  NAME             STATE      VERSION\n  docker-desktop   Running    2\n  rancher-desktop  Stopped    2\n";
const TIMEOUT: Duration = Duration::from_secs(1);

fn programs() -> Vec<String> {
    ["fake-cli", "fake-tui", "missing-cli"].iter().map(|s| s.to_string()).collect()
}

fn ok(text: &str) -> Result<Vec<u8>, RunError> {
    Ok(text.as_bytes().to_vec())
}

fn no_wsl() -> Result<Vec<u8>, RunError> {
    Err(RunError::Start("no wsl here".to_string()))
}

fn list_args() -> Vec<String> {
    vec!["--list".to_string(), "--verbose".to_string()]
}

/// A fake runner. Each call is recorded. `--list` and `--status` get their canned answer. The probe
/// (the call with `probe` as an argument) gets its canned text with the fixture nonce swapped for
/// the nonce in its own arguments, so the nonce check is tested for real.
struct Fake {
    list: Result<Vec<u8>, RunError>,
    status: Result<Vec<u8>, RunError>,
    probe: RefCell<Result<Vec<u8>, RunError>>,
    calls: RefCell<Vec<Vec<String>>>,
}

impl Fake {
    fn new(list: Result<Vec<u8>, RunError>, status: Result<Vec<u8>, RunError>, probe: Result<Vec<u8>, RunError>) -> Self {
        Self { list, status, probe: RefCell::new(probe), calls: RefCell::new(Vec::new()) }
    }

    fn set_probe(&self, probe: Result<Vec<u8>, RunError>) {
        *self.probe.borrow_mut() = probe;
    }

    fn total(&self) -> usize {
        self.calls.borrow().len()
    }

    fn call(&self, index: usize) -> Vec<String> {
        self.calls.borrow()[index].clone()
    }
}

impl Runner for Fake {
    fn run(&self, args: &[String], _timeout: Duration) -> Result<Vec<u8>, RunError> {
        self.calls.borrow_mut().push(args.to_vec());
        if args.iter().any(|a| a == "--list") {
            return self.list.clone();
        }
        if args.iter().any(|a| a == "--status") {
            return self.status.clone();
        }
        let nonce = args.iter().skip_while(|a| a.as_str() != "probe").nth(1).cloned().unwrap_or_default();
        self.probe.borrow().clone().map(|bytes| String::from_utf8_lossy(&bytes).replace(NONCE, &nonce).into_bytes())
    }
}

// status

#[test]
fn not_installed_when_list_and_status_both_fail() {
    let fake = Fake::new(Err(RunError::Failed(Some(1))), Err(RunError::Failed(Some(1))), no_wsl());
    assert_eq!(status(&fake), WslStatus::NotInstalled { command: "wsl --install".to_string() });
    assert_eq!(fake.total(), 2, "status is asked only after the list failed");
}

#[test]
fn no_distro_when_list_fails_but_status_works() {
    let fake = Fake::new(Err(RunError::Failed(Some(1))), ok("Windows Subsystem for Linux is installed."), no_wsl());
    assert_eq!(status(&fake), WslStatus::NoDistro { command: "wsl --install -d Ubuntu".to_string() });
}

#[test]
fn no_distro_when_the_list_has_only_docker_distros() {
    let fake = Fake::new(ok(DOCKER_ONLY), no_wsl(), no_wsl());
    assert_eq!(status(&fake), WslStatus::NoDistro { command: "wsl --install -d Ubuntu".to_string() });
    assert_eq!(fake.total(), 1);
}

#[test]
fn ready_from_the_english_list_with_the_default_first() {
    let fake = Fake::new(ok(LIST_EN), no_wsl(), no_wsl());
    let WslStatus::Ready { distros } = status(&fake) else { panic!("expected ready") };
    let names: Vec<&str> = distros.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["Ubuntu-Test", "Debian-Test", "Ubuntu-24.04"]);
    assert!(distros[0].default);
    assert_eq!(fake.total(), 1, "a listed install needs one call");
}

#[test]
fn ready_from_the_german_utf16_list() {
    let fake = Fake::new(Ok(LIST_DE.to_vec()), no_wsl(), no_wsl());
    let WslStatus::Ready { distros } = status(&fake) else { panic!("expected ready") };
    let names: Vec<&str> = distros.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["Ubuntu-Test", "Debian-Test"]);
    assert_eq!(distros[1].version, 2);
}

#[test]
fn ready_keeps_version_one_distros_for_the_settings_list() {
    let fake = Fake::new(ok(LIST_WSL1), no_wsl(), no_wsl());
    let WslStatus::Ready { distros } = status(&fake) else { panic!("expected ready") };
    let versions: Vec<u8> = distros.iter().map(|d| d.version).collect();
    assert_eq!(versions, [1, 2]);
}

#[test]
fn broken_when_the_list_times_out_and_status_is_not_tried() {
    let fake = Fake::new(Err(RunError::Timeout), no_wsl(), no_wsl());
    assert_eq!(status(&fake), WslStatus::Broken { line: BROKEN_LINE.to_string() });
    assert_eq!(fake.total(), 1);
}

#[test]
fn broken_when_the_status_times_out() {
    let fake = Fake::new(Err(RunError::Failed(Some(1))), Err(RunError::Timeout), no_wsl());
    assert_eq!(status(&fake), WslStatus::Broken { line: BROKEN_LINE.to_string() });
}

// the live runner's guards (nothing here starts a process)

fn missing_exe() -> LiveRunner {
    LiveRunner::new(PathBuf::from(r"Z:\pm-test-missing\wsl.exe"))
}

#[test]
fn live_runner_refuses_admin_flags_before_any_process_starts() {
    let runner = missing_exe();
    let flags = [
        "--install",
        "--shutdown",
        "--terminate",
        "--unregister",
        "--set-version",
        "--import",
        "--set-default",
        "--update",
        "-t",
        "--cd",
        "--list=x",
    ];
    for flag in flags {
        let args = vec![flag.to_string()];
        assert_eq!(runner.run(&args, TIMEOUT), Err(RunError::Refused), "{flag}");
    }
    // Refused anywhere in the list, not only first.
    let args = vec!["-d".to_string(), "Ubuntu-Test".to_string(), "--shutdown".to_string()];
    assert_eq!(runner.run(&args, TIMEOUT), Err(RunError::Refused));
}

#[test]
fn every_argv_this_module_builds_passes_the_flag_check() {
    assert!(!refused(&list_args()));
    assert!(!refused(&["--status".to_string()]));
    let mut probe = vec!["-d".to_string(), "Ubuntu-Test".to_string()];
    probe.extend(parse::probe_args(NONCE, &programs()).unwrap());
    assert!(!refused(&probe));
    let distro = DistroName::parse("Ubuntu-Test").unwrap();
    assert!(!refused(&stop::stop_args(&distro, "pane-1").unwrap()));
}

#[test]
fn a_missing_wsl_exe_is_a_start_failure() {
    assert!(matches!(missing_exe().run(&list_args(), TIMEOUT), Err(RunError::Start(_))));
}

#[test]
fn exe_is_looked_up_under_system32_only_when_it_exists() {
    assert_eq!(exe_under(Path::new(r"Z:\pm-test-missing")), None);
}

// probe

#[test]
fn probe_distro_fills_the_distro_and_the_time_and_puts_dash_d_first() {
    let fake = Fake::new(no_wsl(), no_wsl(), ok(PROBE_OK));
    let facts = probe_distro(&fake, "Ubuntu-Test", &programs(), 4_242).unwrap();
    assert_eq!(facts.distro, "Ubuntu-Test");
    assert_eq!(facts.checked_at_ms, 4_242);
    assert_eq!(facts.uid, 1000);
    assert_eq!(facts.home, "/home/you");
    assert_eq!(facts.mount_root, "/mnt/");
    assert!(facts.interop);
    assert_eq!(facts.programs["fake-tui"].as_deref(), Some("/usr/local/bin/fake-tui"));
    assert_eq!(facts.programs["missing-cli"], None);
    let args = fake.call(0);
    assert_eq!(&args[..3], ["-d", "Ubuntu-Test", "--exec"]);
    assert!(!args.iter().any(|a| a == "--list"));
}

#[test]
fn probe_distro_reads_past_rc_noise_and_other_nonces() {
    let fake = Fake::new(no_wsl(), no_wsl(), ok(PROBE_NOISE));
    let facts = probe_distro(&fake, "Ubuntu-Test", &programs(), 0).unwrap();
    assert_eq!(facts.uid, 1000);
    assert_eq!(facts.programs["fake-cli"].as_deref(), Some("/home/you/.local/bin/fake-cli"));
}

#[test]
fn probe_distro_without_a_required_key_is_start_failed() {
    let text: String = PROBE_OK.lines().filter(|l| !l.contains(":home_windows=")).map(|l| format!("{l}\n")).collect();
    let fake = Fake::new(no_wsl(), no_wsl(), ok(&text));
    assert_eq!(probe_distro(&fake, "Ubuntu-Test", &programs(), 0).unwrap_err().code, WSL_START_FAILED);
}

#[test]
fn probe_distro_maps_run_failures_to_codes() {
    let cases: Vec<(Result<Vec<u8>, RunError>, &str)> = vec![
        (Err(RunError::Timeout), WSL_PROBE_TIMEOUT),
        (Err(RunError::Start("x".to_string())), WSL_NOT_INSTALLED),
        (Err(RunError::Failed(Some(1))), WSL_START_FAILED),
        (Err(RunError::Refused), WSL_START_FAILED),
    ];
    for (probe, code) in cases {
        let fake = Fake::new(no_wsl(), no_wsl(), probe);
        assert_eq!(probe_distro(&fake, "Ubuntu-Test", &programs(), 0).unwrap_err().code, code);
    }
}

#[test]
fn probe_distro_refuses_a_bad_distro_or_program_before_any_call() {
    let fake = Fake::new(no_wsl(), no_wsl(), ok(PROBE_OK));
    for bad in ["", "-x", "a b", "a/b", "Ubuntu;rm"] {
        assert_eq!(probe_distro(&fake, bad, &programs(), 0).unwrap_err().code, WSL_DISTRO_MISSING, "{bad:?}");
    }
    let bad_program = vec!["bad name".to_string()];
    assert_eq!(probe_distro(&fake, "Ubuntu-Test", &bad_program, 0).unwrap_err().code, WSL_START_FAILED);
    assert_eq!(fake.total(), 0, "a refused name never reaches the runner");
}

#[test]
fn new_nonces_are_valid_and_differ() {
    let (a, b) = (new_nonce(), new_nonce());
    assert_ne!(a, b);
    assert!(parse::probe_args(&a, &[]).is_ok());
    assert!(a.len() <= 64);
}

#[test]
fn every_program_the_profiles_ask_for_is_a_valid_probe_name() {
    assert!(parse::probe_args(NONCE, &asked_programs()).is_ok());
}

// cache

#[test]
fn status_is_cached_for_the_list_age_on_a_virtual_clock() {
    let fake = Fake::new(ok(LIST_EN), no_wsl(), no_wsl());
    let mut cache = Cache::default();
    cache.status(&fake, 0, LIST_FRESH_MS);
    assert_eq!(fake.total(), 1);
    cache.status(&fake, LIST_FRESH_MS, LIST_FRESH_MS);
    assert_eq!(fake.total(), 1, "a hit at exactly the max age");
    cache.status(&fake, LIST_FRESH_MS + 1, LIST_FRESH_MS);
    assert_eq!(fake.total(), 2, "expired one millisecond later");
}

#[test]
fn facts_are_cached_for_the_facts_age_on_a_virtual_clock() {
    let fake = Fake::new(no_wsl(), no_wsl(), ok(PROBE_OK));
    let mut cache = Cache::default();
    let asked = programs();
    cache.facts(&fake, "Ubuntu-Test", &asked, 1_000, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 1);
    cache.facts(&fake, "Ubuntu-Test", &asked, 1_000 + FACTS_FRESH_MS, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 1, "a hit at exactly the max age");
    cache.facts(&fake, "Ubuntu-Test", &asked, 1_001 + FACTS_FRESH_MS, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 2, "expired");
}

#[test]
fn a_cached_probe_that_lacks_a_program_asked_now_is_probed_again() {
    let fake = Fake::new(no_wsl(), no_wsl(), ok(PROBE_OK));
    let mut cache = Cache::default();
    cache.facts(&fake, "Ubuntu-Test", &["fake-cli".to_string()], 0, FACTS_FRESH_MS).unwrap();
    cache.facts(&fake, "Ubuntu-Test", &["fake-cli".to_string(), "fake-tui".to_string()], 0, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 2);
}

#[test]
fn invalidate_drops_one_distro_only() {
    let fake = Fake::new(no_wsl(), no_wsl(), ok(PROBE_OK));
    let mut cache = Cache::default();
    let asked = programs();
    cache.facts(&fake, "Ubuntu-Test", &asked, 0, FACTS_FRESH_MS).unwrap();
    cache.facts(&fake, "Debian-Test", &asked, 0, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 2);
    cache.invalidate("Ubuntu-Test");
    cache.facts(&fake, "Ubuntu-Test", &asked, 0, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 3, "the invalidated distro is probed again");
    cache.facts(&fake, "Debian-Test", &asked, 0, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 3, "the other distro is still cached");
}

#[test]
fn a_failed_probe_is_not_cached_and_drops_the_old_entry() {
    let fake = Fake::new(no_wsl(), no_wsl(), Err(RunError::Timeout));
    let mut cache = Cache::default();
    let asked = programs();
    assert!(cache.facts(&fake, "Ubuntu-Test", &asked, 0, FACTS_FRESH_MS).is_err());
    assert_eq!(fake.total(), 1);

    fake.set_probe(ok(PROBE_OK));
    cache.facts(&fake, "Ubuntu-Test", &asked, 0, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 2);
    cache.facts(&fake, "Ubuntu-Test", &asked, 0, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 2, "a good result is cached");

    fake.set_probe(Err(RunError::Failed(Some(1))));
    assert!(cache.check(&fake, "Ubuntu-Test", &asked, 0).is_err());
    fake.set_probe(ok(PROBE_OK));
    cache.facts(&fake, "Ubuntu-Test", &asked, 0, FACTS_FRESH_MS).unwrap();
    assert_eq!(fake.total(), 4, "the failed check dropped the cached facts");
}

#[test]
fn check_always_probes_even_when_the_facts_are_fresh() {
    let fake = Fake::new(no_wsl(), no_wsl(), ok(PROBE_OK));
    let mut cache = Cache::default();
    let asked = programs();
    cache.facts(&fake, "Ubuntu-Test", &asked, 0, FACTS_FRESH_MS).unwrap();
    cache.check(&fake, "Ubuntu-Test", &asked, 1).unwrap();
    assert_eq!(fake.total(), 2);
}
