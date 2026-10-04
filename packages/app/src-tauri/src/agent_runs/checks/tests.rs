//! Tests for acceptance check validation and evaluation.

use super::*;

fn probe(code: i32) -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".into(), "/C".into(), format!("exit {code}")]
    } else {
        vec!["sh".into(), "-c".into(), format!("exit {code}")]
    }
}

#[test]
fn command_exits_compares_exit_code_and_reports_output() {
    let root = dir("cmd");
    let ok = Check::CommandExits { command: probe(0), expect_exit: 0, timeout_ms: None };
    let bad = Check::CommandExits { command: probe(3), expect_exit: 0, timeout_ms: None };
    let expected_fail =
        Check::CommandExits { command: probe(3), expect_exit: 3, timeout_ms: None };
    let missing = Check::CommandExits {
        command: vec!["definitely-not-a-real-binary-pm".into()],
        expect_exit: 0,
        timeout_ms: None,
    };
    let results = evaluate(&root, &[ok, bad, expected_fail, missing], None);
    let passed: Vec<bool> = results.iter().map(|r| r.passed).collect();
    assert_eq!(passed, vec![true, false, true, false]);
    assert!(results[1].detail.contains("exited 3"));
    assert!(results[3].detail.contains("could not start"));
}

#[test]
fn command_exits_validation_rejects_empty_argv_and_bad_timeout() {
    let root = dir("cmdval");
    let empty = Check::CommandExits { command: vec![], expect_exit: 0, timeout_ms: None };
    assert!(validate(&root, &[empty]).is_err());
    let slow = Check::CommandExits { command: probe(0), expect_exit: 0, timeout_ms: Some(0) };
    assert!(validate(&root, &[slow]).is_err());
}

fn dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("pm-checks-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn exists(path: &str) -> Check {
    Check::FileExists { path: path.into() }
}

#[test]
fn each_check_type_passes_and_fails() {
    let root = dir("types");
    std::fs::write(root.join("a.txt"), "hello world").unwrap();
    let checks = vec![
        exists("a.txt"),
        exists("missing.txt"),
        Check::FileContains { path: "a.txt".into(), text: "hello".into() },
        Check::FileContains { path: "a.txt".into(), text: "nope".into() },
        Check::FileAbsent { path: "missing.txt".into() },
        Check::FileAbsent { path: "a.txt".into() },
    ];
    let passed: Vec<bool> = evaluate(&root, &checks, None).iter().map(|r| r.passed).collect();
    assert_eq!(passed, vec![true, false, true, false, true, false]);
}

#[test]
fn rejects_absolute_and_escaping_paths() {
    let root = dir("escape");
    let abs = if cfg!(windows) { "C:\\Windows\\win.ini" } else { "/etc/passwd" };
    assert!(validate(&root, &[exists(abs)]).is_err());
    assert!(validate(&root, &[exists("../x")]).is_err());
    assert!(validate(&root, &[exists("a/../../x")]).is_err());
    assert!(validate(&root, &[exists("")]).is_err());
    assert!(validate(&root, &[exists("sub/ok.txt")]).is_ok());
    let results = evaluate(&root, &[exists("../x")], None);
    assert!(!results[0].passed);
}

#[test]
fn caps_check_count_and_requires_text() {
    let root = dir("cap");
    let many: Vec<Check> = (0..21).map(|i| exists(&format!("f{i}"))).collect();
    assert!(validate(&root, &many).is_err());
    assert!(validate(&root, &many[..20]).is_ok());
    let empty = Check::FileContains { path: "a".into(), text: String::new() };
    assert!(validate(&root, &[empty]).is_err());
}

#[test]
fn parses_tagged_json_and_rejects_unknown_type() {
    let ok: Check = serde_json::from_str(r#"{"type":"file_exists","path":"a"}"#).unwrap();
    assert_eq!(ok, exists("a"));
    assert!(serde_json::from_str::<Check>(r#"{"type":"command","cmd":"ls"}"#).is_err());
}

#[test]
fn failure_summary_lists_failures_only() {
    let root = dir("summary");
    let results = evaluate(&root, &[exists("nope")], None);
    assert!(failure_summary(&results).unwrap().contains("nope"));
    std::fs::write(root.join("yes"), "x").unwrap();
    assert!(failure_summary(&evaluate(&root, &[exists("yes")], None)).is_none());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape() {
    let root = dir("link");
    let outside = dir("outside");
    std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
    assert!(validate(&root, &[exists("out/x")]).is_err());
}

#[test]
fn result_contains_checks_reply_text() {
    let root = dir("result");
    let check = Check::ResultContains { text: "PONG".into() };
    let pass = evaluate(&root, std::slice::from_ref(&check), Some("reply: PONG"));
    let miss = evaluate(&root, std::slice::from_ref(&check), Some("nope"));
    let none = evaluate(&root, std::slice::from_ref(&check), None);
    assert!(pass[0].passed && !miss[0].passed && !none[0].passed);
    assert!(validate(&root, &[Check::ResultContains { text: String::new() }]).is_err());
    assert!(validate(&root, &[check]).is_ok());
}
