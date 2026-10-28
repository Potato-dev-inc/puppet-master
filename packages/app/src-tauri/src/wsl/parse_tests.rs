//! Tests for `parse.rs` (SPEC n33 WSL-6). Fixtures live in `wsl/fixtures/` and use placeholders only:
//! the distro `Ubuntu-Test` and the folder `you`. The programs are fake ids (`fake-cli`, `fake-tui`).

use super::*;
use crate::wsl::model::DistroFacts;

const LIST_EN: &str = include_str!("fixtures/list_en.txt");
const LIST_DOCKER: &str = include_str!("fixtures/list_docker.txt");
const LIST_WSL1: &str = include_str!("fixtures/list_wsl1.txt");
const LIST_HOSTILE: &str = include_str!("fixtures/list_hostile.txt");
const LIST_DE: &[u8] = include_bytes!("fixtures/list_de.utf16");
const PROBE_OK: &str = include_str!("fixtures/probe_ok.txt");
const PROBE_NOISE: &str = include_str!("fixtures/probe_noise.txt");
const PROBE_NO_INTEROP: &str = include_str!("fixtures/probe_no_interop.txt");
const PROBE_ALIAS: &str = include_str!("fixtures/probe_alias.txt");

const NONCE: &str = "pm6nonce";

fn programs() -> Vec<String> {
    ["fake-cli", "fake-tui", "missing-cli"].iter().map(|s| s.to_string()).collect()
}

fn names(distros: &[Distro]) -> Vec<&str> {
    distros.iter().map(|d| d.name.as_str()).collect()
}

fn found(facts: &DistroFacts, program: &str) -> Option<String> {
    facts.programs.get(program).cloned().flatten()
}

/// The probe text with every line that starts with `key=` removed, to test a missing key.
fn without_key(text: &str, key: &str) -> String {
    let needle = format!(":{key}=");
    text.lines().filter(|l| !l.contains(&needle)).map(|l| format!("{l}\n")).collect()
}

fn with_line(text: &str, key: &str, value: &str) -> String {
    let needle = format!(":{key}=");
    text.lines()
        .map(|l| if l.contains(&needle) { format!("PM1:{NONCE}:{key}={value}") } else { l.to_string() })
        .map(|l| format!("{l}\n"))
        .collect()
}

// decode

#[test]
fn decode_reads_utf16le_with_bom_and_drops_cr() {
    let text = decode(LIST_DE);
    assert!(text.starts_with("  NAME"), "got {text:?}");
    assert!(text.contains("Wird ausgeführt"));
    assert!(!text.contains('\r') && !text.contains('\0') && !text.contains('\u{FEFF}'));
}

#[test]
fn decode_reads_utf16le_without_bom_the_same() {
    assert_eq!(decode(&LIST_DE[2..]), decode(LIST_DE));
    // Odd NUL offsets alone mark UTF-16LE (no BOM).
    assert_eq!(decode(&[b'A', 0, b'B', 0]), "AB");
}

#[test]
fn decode_drops_an_odd_trailing_byte() {
    assert_eq!(decode(&[0xFF, 0xFE, b'A', 0, b'B', 0, 0x41]), "AB");
}

#[test]
fn decode_reads_utf8_and_drops_nul_and_cr() {
    assert_eq!(decode(b"a\r\nb\0c"), "a\nbc");
}

#[test]
fn decode_strips_a_utf8_bom() {
    assert_eq!(decode(b"\xEF\xBB\xBFhi"), "hi");
}

#[test]
fn decode_is_lossy_for_bad_utf8() {
    assert_eq!(decode(&[b'o', b'k', 0xFF]), "ok\u{FFFD}");
}

#[test]
fn decode_empty_is_empty() {
    assert_eq!(decode(b""), "");
}

// list

#[test]
fn list_en_reads_default_state_and_version() {
    let distros = list(LIST_EN);
    assert_eq!(names(&distros), ["Ubuntu-Test", "Debian-Test", "Ubuntu-24.04"]);
    assert_eq!(distros.iter().map(|d| d.default).collect::<Vec<_>>(), [true, false, false]);
    assert!(distros.iter().all(|d| d.version == 2));
    assert_eq!(distros[0].state, "Running");
    assert_eq!(distros[1].state, "Stopped");
}

#[test]
fn list_de_utf16_skips_the_german_header_and_keeps_its_state() {
    let distros = list(&decode(LIST_DE));
    assert_eq!(names(&distros), ["Ubuntu-Test", "Debian-Test"]);
    assert_eq!(distros[0].state, "Wird ausgeführt");
    assert_eq!(distros[1].state, "Gestoppt");
    assert!(distros[0].default && !distros[1].default);
}

#[test]
fn list_hides_docker_and_rancher_desktop_case_insensitively() {
    assert_eq!(names(&list(LIST_DOCKER)), ["Ubuntu-Test", "Debian-Test"]);
}

#[test]
fn list_keeps_wsl1_distros_with_version_one() {
    let distros = list(LIST_WSL1);
    assert_eq!(names(&distros), ["Ubuntu-Test", "Ubuntu-Test-2"]);
    assert_eq!(distros.iter().map(|d| d.version).collect::<Vec<_>>(), [1, 2]);
}

#[test]
fn list_hostile_names_keep_only_valid_ones() {
    let distros = list(LIST_HOSTILE);
    let long_ok = "b".repeat(64);
    assert_eq!(names(&distros), ["Ubuntu-Test", "Ubuntu-Test.1_ok", long_ok.as_str()]);
    assert!(distros[0].default);
    // Shell text, traversal, option-like, separators, non-ASCII, and a 65-character name are all dropped.
    let text = names(&distros).join(" ");
    for bad in ["touch", "rm", "..", "a/b", "Caf", "Fedora", "Lonely", "%PATH%", "Docker-Desktop-Trap"] {
        assert!(!text.contains(bad), "{bad} must not be listed");
    }
    assert!(!distros.iter().any(|d| d.name.len() > 64));
}

#[test]
fn list_skips_header_only_and_no_distro_messages() {
    assert!(list("").is_empty());
    assert!(list("  NAME  STATE  VERSION\n").is_empty());
    assert!(list("Windows Subsystem for Linux has no installed distributions.\n").is_empty());
}

#[test]
fn list_accepts_crlf_tabs_and_an_empty_state() {
    let distros = list("NAME\tSTATE\tVERSION\r\n*\tUbuntu-Test\tRunning\t2\r\n");
    assert_eq!(distros.len(), 1);
    assert_eq!(distros[0].state, "Running");
    let bare = list("H\n* Ubuntu-Test 2\n");
    assert_eq!(bare.len(), 1);
    assert_eq!(bare[0].state, "");
    assert!(bare[0].default);
}

// PROBE_SCRIPT and probe_args

#[test]
fn probe_script_takes_the_nonce_then_programs_as_arguments() {
    assert!(PROBE_SCRIPT.starts_with("nonce=$1\nshift\n"));
    assert!(PROBE_SCRIPT.contains(r#"for prog in "$@"; do"#));
    assert!(!PROBE_SCRIPT.contains("eval"));
}

#[test]
fn probe_script_has_no_kill_or_wsl_control_commands() {
    for banned in ["pkill", "killall", "kill ", "taskkill", "shutdown", "terminate", "--install", "unregister"] {
        assert!(!PROBE_SCRIPT.contains(banned), "{banned} must not appear in the probe script");
    }
}

#[test]
fn probe_args_builds_the_exec_argv_after_the_distro() {
    let args = probe_args(NONCE, &["fake-cli".to_string(), "fake-tui".to_string()]).unwrap();
    assert_eq!(
        args,
        vec!["--exec", "/bin/sh", "-c", PROBE_SCRIPT, "probe", NONCE, "fake-cli", "fake-tui"]
    );
}

#[test]
fn probe_args_script_text_never_depends_on_the_programs() {
    let none = probe_args(NONCE, &[]).unwrap();
    let some = probe_args(NONCE, &["fake-cli".to_string()]).unwrap();
    assert_eq!(none[3], some[3]);
    assert_eq!(none[3], PROBE_SCRIPT);
}

#[test]
fn probe_args_refuses_bad_program_names_before_any_call() {
    let bad = [
        String::new(),
        "a b".to_string(),
        "x;rm".to_string(),
        "$(id)".to_string(),
        "a/b".to_string(),
        "x\"y".to_string(),
        "p".repeat(65),
    ];
    for name in bad {
        let got = probe_args(NONCE, std::slice::from_ref(&name));
        assert_eq!(got, Err(ProbeError::BadProgram(name.clone())));
    }
    assert!(probe_args(NONCE, &["fake.cli_2-x".to_string()]).is_ok());
}

#[test]
fn probe_args_refuses_a_bad_nonce() {
    let long = "x".repeat(65);
    for nonce in ["", "a b", "n;1", long.as_str()] {
        assert_eq!(probe_args(nonce, &[]), Err(ProbeError::BadNonce), "nonce {nonce:?}");
    }
}

// probe

#[test]
fn probe_ok_reads_every_fact() {
    let facts = probe(PROBE_OK, NONCE, &programs()).unwrap();
    assert_eq!(facts.uid, 1000);
    assert_eq!(facts.home, "/home/you");
    assert_eq!(facts.home_windows, r"\\wsl.localhost\Ubuntu-Test\home\you");
    assert_eq!(facts.mount_root, "/mnt/");
    assert_eq!(facts.git.as_deref(), Some("2.43.0"));
    assert!(facts.interop);
    assert_eq!(facts.networking.as_deref(), Some("nat"));
    assert_eq!(
        facts.path,
        "/home/you/.local/bin:/home/you/.nvm/versions/node/v20.11.1/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    );
    assert_eq!(found(&facts, "fake-cli").as_deref(), Some("/home/you/.local/bin/fake-cli"));
    assert_eq!(found(&facts, "fake-tui").as_deref(), Some("/usr/local/bin/fake-tui"));
    assert_eq!(found(&facts, "missing-cli"), None);
    // The caller stamps the distro name and the time; the parser leaves them empty.
    assert_eq!(facts.distro, "");
    assert_eq!(facts.checked_at_ms, 0);
}

#[test]
fn probe_ignores_rc_noise_and_lines_with_the_wrong_nonce() {
    let facts = probe(PROBE_NOISE, NONCE, &programs()).unwrap();
    assert_eq!(facts.uid, 1000);
    assert_eq!(facts.home, "/home/you");
    assert_eq!(facts.home_windows, r"\\wsl.localhost\Ubuntu-Test\home\you");
    assert_eq!(facts.mount_root, "/mnt/");
    assert_eq!(facts.path, "/home/you/.local/bin:/usr/bin:/bin");
    assert!(!facts.path.contains("not-real"));
    assert_eq!(found(&facts, "fake-cli").as_deref(), Some("/home/you/.local/bin/fake-cli"));
    // The fake line with the wrong nonce said /tmp/not-real; it does not count.
    assert_eq!(found(&facts, "fake-tui"), None);
}

#[test]
fn probe_no_interop_reads_false_empty_git_none_and_a_mirrored_network() {
    let facts = probe(PROBE_NO_INTEROP, NONCE, &programs()).unwrap();
    assert!(!facts.interop);
    assert_eq!(facts.git, None);
    assert_eq!(facts.networking.as_deref(), Some("mirrored"));
    assert_eq!(found(&facts, "fake-cli"), None);
    assert_eq!(found(&facts, "fake-tui").as_deref(), Some("/usr/bin/fake-tui"));
}

#[test]
fn probe_alias_output_is_not_an_absolute_path() {
    let facts = probe(PROBE_ALIAS, NONCE, &programs()).unwrap();
    assert_eq!(found(&facts, "fake-cli"), None);
    assert_eq!(found(&facts, "fake-tui").as_deref(), Some("/usr/bin/fake-tui"));
}

#[test]
fn probe_missing_home_windows_is_an_error() {
    let text = without_key(PROBE_OK, "home_windows");
    assert_eq!(probe(&text, NONCE, &programs()), Err(ProbeError::Missing("home_windows")));
}

#[test]
fn probe_missing_mount_root_is_an_error() {
    let text = without_key(PROBE_OK, "mount_root");
    assert_eq!(probe(&text, NONCE, &programs()), Err(ProbeError::Missing("mount_root")));
}

#[test]
fn probe_missing_path_or_uid_is_an_error() {
    assert_eq!(probe(&without_key(PROBE_OK, "path"), NONCE, &[]), Err(ProbeError::Missing("path")));
    assert_eq!(probe(&without_key(PROBE_OK, "uid"), NONCE, &[]), Err(ProbeError::Missing("uid")));
}

#[test]
fn probe_refuses_a_windows_home_value_and_a_bad_mount_root() {
    let windows_home = with_line(PROBE_OK, "home_windows", r"D:\other\home\you");
    assert_eq!(probe(&windows_home, NONCE, &[]), Err(ProbeError::BadValue("home_windows")));
    let relative_mount = with_line(PROBE_OK, "mount_root", "mnt");
    assert_eq!(probe(&relative_mount, NONCE, &[]), Err(ProbeError::BadValue("mount_root")));
    let bad_uid = with_line(PROBE_OK, "uid", "root");
    assert_eq!(probe(&bad_uid, NONCE, &[]), Err(ProbeError::BadValue("uid")));
}

#[test]
fn probe_accepts_mount_root_slash_for_wsl_conf_root() {
    let text = with_line(PROBE_OK, "mount_root", "/");
    assert_eq!(probe(&text, NONCE, &[]).unwrap().mount_root, "/");
}

#[test]
fn probe_with_another_nonce_finds_nothing() {
    assert_eq!(probe(PROBE_OK, "other-nonce", &[]), Err(ProbeError::Missing("uid")));
}

#[test]
fn probe_refuses_an_empty_or_odd_nonce() {
    assert_eq!(probe(PROBE_OK, "", &[]), Err(ProbeError::BadNonce));
    assert_eq!(probe(PROBE_OK, "a b", &[]), Err(ProbeError::BadNonce));
}

#[test]
fn probe_keeps_equals_signs_inside_the_path_value() {
    let text = with_line(PROBE_OK, "path", "/x=y:/usr/bin");
    assert_eq!(probe(&text, NONCE, &[]).unwrap().path, "/x=y:/usr/bin");
}

#[test]
fn probe_keeps_only_the_programs_it_asked_for() {
    let text = format!("{PROBE_OK}PM1:{NONCE}:prog:rogue=/bin/sh\n");
    let facts = probe(&text, NONCE, &programs()).unwrap();
    let keys: Vec<&str> = facts.programs.keys().map(String::as_str).collect();
    assert_eq!(keys, ["fake-cli", "fake-tui", "missing-cli"]);
}

#[test]
fn probe_git_and_networking_must_look_like_their_values() {
    let git = with_line(PROBE_OK, "git", "2.43.0;rm");
    assert_eq!(probe(&git, NONCE, &[]).unwrap().git, None);
    let net = with_line(PROBE_OK, "networking", "NAT; x");
    assert_eq!(probe(&net, NONCE, &[]).unwrap().networking, None);
}
