//! The one door to wsl.exe: the name allowlist, the refused flags, the call shapes, UTF-16 decoding, the
//! list parser, and the consent for the one elevated call. No wsl.exe runs: a fake guest takes the calls.

use std::path::Path;
use std::sync::Arc;

use super::fakes_tests::{FakeGuest, Scratch};
use super::wsl::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|a| a.to_string()).collect()
}

fn name(n: &str) -> SandboxName {
    SandboxName::new(n).unwrap()
}

#[test]
fn only_our_names_are_valid() {
    for good in ["pm-sbx-main", "pm-sbx-e2e", "pm-sbx-a", "pm-sbx-0-9-a", &format!("pm-sbx-{}", "a".repeat(40))] {
        assert!(valid_name(good), "{good}");
    }
    let too_long = format!("pm-sbx-{}", "a".repeat(41));
    for bad in [
        "", "Ubuntu", "Ubuntu-24.04", "pm-sbx-", "pm-sbx", "pm-sbx-Main", "pm-sbx-a b", "pm-sbx-a_b", "pm-sbx-a;b", "pm-sbx-a\n",
        "pm-sbx-é", "xpm-sbx-main", " pm-sbx-main", "pm-sbx-main ", "PM-SBX-main", &too_long,
    ] {
        assert!(!valid_name(bad), "{bad:?}");
        assert!(SandboxName::new(bad).is_err());
    }
}

#[test]
fn the_gate_refuses_what_would_touch_the_persons_other_distros_or_wsl_itself() {
    let refused = [
        vec!["--shutdown"],
        vec!["--list", "--verbose", "--shutdown"],
        vec!["--shutdown", "pm-sbx-main"],
        vec!["--set-default", "pm-sbx-main"],
        vec!["-s", "pm-sbx-main"],
        vec!["--update"],
        vec!["--manage", "pm-sbx-main", "--move", "D:\\x"],
        vec!["--mount", "\\\\.\\PHYSICALDRIVE1"],
        vec!["--unregister", "Ubuntu"],
        vec!["--unregister", "pm-sbx-"],
        vec!["--terminate", "Ubuntu-24.04"],
        vec!["-t", "docker-desktop"],
        vec!["--terminate"],
        vec!["--unregister", "pm-sbx-main", "Ubuntu"],
        vec!["-d", "Ubuntu", "--exec", "/bin/true"],
        vec!["-d", "pm-sbx-main", "-u", "founder", "--exec", "/bin/true"],
        vec!["-d", "pm-sbx-main", "--exec", "bin/true"],
        vec!["-d", "pm-sbx-main", "--exec"],
        vec!["-d", "pm-sbx-main"],
        vec!["-d", "pm-sbx-main", "-u", "root", "--exec", "/bin/echo", "--shutdown"],
        vec!["-d", "pm-sbx-main", "-u", "root", "--exec", "/bin/echo", "--manage"],
        vec!["--import", "Ubuntu", "D:\\x", "D:\\y.tar"],
        vec!["--import", "pm-sbx-main", "--shutdown", "D:\\y.tar"],
        vec!["--import", "pm-sbx-main", "-x", "D:\\y.tar"],
        vec!["--import-in-place", "pm-sbx-main", "D:\\x.vhdx"],
        vec!["--export", "Ubuntu", "D:\\y.tar"],
        vec!["--install", "Debian", "--name", "pm-sbx-main", "--location", "D:\\x", "--no-launch"],
        vec!["--install", "Ubuntu-24.04", "--name", "Ubuntu", "--location", "D:\\x", "--no-launch"],
        vec!["--install", "Ubuntu-24.04"],
        vec!["--install"],
        vec!["--list", "--all"],
        vec!["--status", "--extra"],
        vec!["--unknown"],
        vec!["--distribution", "pm-sbx-main", "--system", "--exec", "/bin/true"],
        vec![],
    ];
    for case in refused {
        let call = args(&case);
        assert!(check_args(&call).is_err(), "{case:?} must be refused");
    }
}

#[test]
fn the_gate_accepts_the_calls_the_sandbox_makes() {
    let n = name("pm-sbx-main");
    let cases: Vec<(Vec<&str>, Verb)> = vec![
        (vec!["--list", "--verbose"], Verb::List),
        (vec!["-l", "-v"], Verb::List),
        (vec!["--list", "--running", "--quiet"], Verb::List),
        (vec!["--status"], Verb::Status),
        (vec!["--install", "--no-distribution"], Verb::EnableFeature),
        (
            vec!["--install", "Ubuntu-24.04", "--name", "pm-sbx-main", "--location", "D:\\a b\\pm-sbx-main", "--no-launch"],
            Verb::Install(n.clone()),
        ),
        (vec!["--import", "pm-sbx-main", "D:\\x", "D:\\y.tar"], Verb::Import(n.clone())),
        (vec!["--export", "pm-sbx-main", "D:\\y.tar"], Verb::Export(n.clone())),
        (vec!["--unregister", "pm-sbx-main"], Verb::Unregister(n.clone())),
        (vec!["--terminate", "pm-sbx-main"], Verb::Terminate(n.clone())),
        (vec!["-t", "pm-sbx-main"], Verb::Terminate(n.clone())),
        (vec!["-d", "pm-sbx-main", "-u", "pm", "--exec", "/usr/bin/id"], Verb::Guest { name: n.clone(), user: GuestUser::Pm }),
        (vec!["-d", "pm-sbx-main", "--exec", "/bin/sh", "-s"], Verb::Guest { name: n.clone(), user: GuestUser::Root }),
        (
            vec!["--distribution", "pm-sbx-main", "--user", "root", "--exec", "/bin/tar", "-x", "-f", "-"],
            Verb::Guest { name: n, user: GuestUser::Root },
        ),
    ];
    for (case, verb) in cases {
        assert_eq!(check_args(&args(&case)).unwrap(), verb, "{case:?}");
    }
}

#[test]
fn a_refused_call_never_reaches_the_runner() {
    let guest = FakeGuest::with_installed("pm-sbx-main");
    let wsl = guest.wsl();
    assert!(matches!(run(wsl.runner(), Call::new(&["--shutdown"])), Err(WslError::Refused(_))));
    assert!(matches!(run(wsl.runner(), Call::new(&["--terminate", "Ubuntu"])), Err(WslError::Refused(_))));
    assert!(wsl.terminate(&name("pm-sbx-main")).is_ok());
    assert_eq!(guest.joined(), vec!["--terminate pm-sbx-main".to_string()], "only the allowed call arrived");
}

#[test]
fn only_a_call_into_the_distro_takes_stdin_or_stays_running() {
    let guest = FakeGuest::with_installed("pm-sbx-main");
    let runner = guest.wsl();
    let with_stdin = Call::new(&["--list", "--verbose"]).stdin(b"x".to_vec());
    assert!(matches!(run(runner.runner(), with_stdin), Err(WslError::Refused(_))));
    assert!(matches!(spawn_holder(runner.runner(), Call::new(&["--list", "--verbose"])), Err(WslError::Refused(_))));
    let huge = Call::new(&["-d", "pm-sbx-main", "--exec", "/bin/cat"]).stdin(vec![0u8; STDIN_CAP + 1]);
    assert!(matches!(run(runner.runner(), huge), Err(WslError::Refused(_))));
    assert!(guest.calls().is_empty());
}

#[test]
fn control_characters_in_an_argument_are_refused() {
    for bad in ["pm-sbx-main\n--shutdown", "pm-sbx-main\0", "pm-sbx-main\r"] {
        assert!(check_args(&args(&["--terminate", bad])).is_err());
    }
    assert!(check_args(&args(&["--import", "pm-sbx-main", "D:\\x\ny", "D:\\y.tar"])).is_err());
    assert!(check_args(&args(&["--import", "pm-sbx-main", "", "D:\\y.tar"])).is_err());
}

#[test]
fn the_feature_install_needs_the_persons_button() {
    let guest = FakeGuest::new();
    let wsl = guest.wsl();
    // Asking for it without a consent is refused and nothing runs.
    let plain = run(wsl.runner(), Call::new(&["--install", "--no-distribution"]));
    assert!(matches!(plain, Err(WslError::Refused(_))));
    assert!(guest.calls().is_empty());
    // With the consent it runs elevated, and exit 3010 means "restart Windows".
    let result = wsl.enable_windows_feature(Consent::the_person_pressed_the_button()).unwrap();
    assert!(result.restart_needed);
    assert_eq!(guest.st.lock().elevated_seen, 1);
    // No other kind of call is ever elevated.
    wsl.is_available();
    wsl.list().unwrap();
    assert_eq!(guest.st.lock().elevated_seen, 1);
}

#[test]
fn output_is_read_as_utf16_or_utf8() {
    let utf16_bom: Vec<u8> = [0xFF, 0xFE].into_iter().chain("pm-sbx-main\r\n".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
    assert_eq!(decode_text(&utf16_bom), "pm-sbx-main\n");
    let utf16_plain: Vec<u8> = "Running".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    assert_eq!(decode_text(&utf16_plain), "Running");
    assert_eq!(decode_text("hello\r\nwörld".as_bytes()), "hello\nwörld");
    assert_eq!(decode_text(b""), "");
    assert_eq!(decode_text(b"a"), "a");
    // a UTF-8 line from inside the distro that merely has a few NULs is not taken for UTF-16
    assert_eq!(decode_text(b"abcdefgh\0"), "abcdefgh\0");
}

#[test]
fn the_list_is_parsed_from_wsl_l_v() {
    let text = "  NAME            STATE           VERSION\n* Ubuntu          Running         2\n  docker-desktop  Stopped         2\n  pm-sbx-main     Running         2\n  old-one         Stopped         1\n";
    let rows = parse_list(text);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0], Distro { name: "Ubuntu".into(), state: "Running".into(), version: 2, is_default: true });
    assert_eq!(rows[2].name, "pm-sbx-main");
    assert!(!rows[2].is_default);
    assert_eq!(rows[3].version, 1);
    // the header, an empty list message and blank lines give no rows
    assert!(parse_list("Windows Subsystem for Linux has no installed distributions.\n\n").is_empty());
    assert!(parse_list("").is_empty());
    // a localized state word still parses
    assert_eq!(parse_list("  pm-sbx-main     Wird ausgeführt   2\n")[0].name, "pm-sbx-main");
}

#[test]
fn only_our_distros_are_listed_and_running_is_asked_by_name() {
    let guest = FakeGuest::with_installed("pm-sbx-main");
    guest.st.lock().installed.push("docker-desktop".to_string());
    let wsl = guest.wsl();
    let ours = wsl.list_ours().unwrap();
    assert_eq!(ours.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), vec!["pm-sbx-main"]);
    assert!(wsl.is_installed(&name("pm-sbx-main")).unwrap());
    assert!(!wsl.is_installed(&name("pm-sbx-other")).unwrap());
    assert!(!wsl.is_running(&name("pm-sbx-main")).unwrap());
    guest.st.lock().running.push("pm-sbx-main".to_string());
    assert!(wsl.is_running(&name("pm-sbx-main")).unwrap());
}

#[test]
fn the_helpers_build_the_expected_arguments() {
    let guest = FakeGuest::new();
    let wsl = guest.wsl();
    let n = name("pm-sbx-main");
    wsl.install_distro(&n, Path::new("D:\\data\\potato\\sandbox\\pm-sbx-main")).unwrap();
    wsl.export(&n, Path::new("D:\\snap\\base.tar")).unwrap();
    wsl.import(&n, Path::new("D:\\data\\d"), Path::new("D:\\snap\\base.tar")).unwrap();
    wsl.terminate(&n).unwrap();
    wsl.unregister(&n).unwrap();
    wsl.exec(&n, GuestUser::Root, &["/bin/true"], None, SHORT).unwrap();
    let joined = guest.joined();
    assert_eq!(joined[0], "--install Ubuntu-24.04 --name pm-sbx-main --location D:\\data\\potato\\sandbox\\pm-sbx-main --no-launch");
    assert_eq!(joined[1], "--export pm-sbx-main D:\\snap\\base.tar");
    assert_eq!(joined[2], "--import pm-sbx-main D:\\data\\d D:\\snap\\base.tar");
    assert_eq!(joined[3], "--terminate pm-sbx-main");
    assert_eq!(joined[4], "--unregister pm-sbx-main");
    assert_eq!(joined[5], "-d pm-sbx-main -u root --exec /bin/true");
}

#[test]
fn a_failed_call_carries_a_short_cleaned_detail() {
    let out = Output { code: Some(1), stdout: Vec::new(), stderr: b"\nsomething broke sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij\nsecond line".to_vec() };
    match out.ok() {
        Err(WslError::Failed { code, detail }) => {
            assert_eq!(code, Some(1));
            assert!(detail.starts_with("something broke"));
            assert!(!detail.contains("sk-ant-api03-ABCDEF"));
            assert!(!detail.contains("second line"));
        }
        other => panic!("{other:?}"),
    }
    assert!(Output { code: Some(0), ..Default::default() }.ok().is_ok());
    assert!(Output { code: None, ..Default::default() }.ok().is_err());
}

#[test]
fn is_available_reads_the_status_call() {
    let guest = FakeGuest::new();
    assert!(guest.wsl().is_available());
    guest.st.lock().wsl_ok = false;
    assert!(!guest.wsl().is_available());
}

#[test]
fn the_real_runner_looks_for_wsl_exe_under_system32() {
    let scratch = Scratch::new("wslexe");
    assert!(exe_under(&scratch.0).is_none());
    std::fs::create_dir_all(scratch.0.join("System32")).unwrap();
    std::fs::write(scratch.0.join("System32").join("wsl.exe"), b"").unwrap();
    assert_eq!(exe_under(&scratch.0), Some(scratch.0.join("System32").join("wsl.exe")));
}

#[test]
fn the_holder_is_a_sleep_inside_our_distro() {
    let guest = FakeGuest::with_installed("pm-sbx-main");
    let wsl = guest.wsl();
    let mut holder = wsl.spawn_holder(&name("pm-sbx-main")).unwrap();
    assert!(holder.alive());
    assert_eq!(guest.joined(), vec!["-d pm-sbx-main -u root --exec /bin/sleep infinity".to_string()]);
    holder.stop();
    assert!(!holder.alive());
    // the dyn Runner is shareable
    let _shared: Arc<dyn Runner> = guest;
}
