//! Tests for the allowed-command approval policy.

use super::*;

fn allow(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[test]
fn approves_only_an_exact_listed_program() {
    let screen = "Run this command?\nNot in allowlist: python\nAllow once (y)";
    assert_eq!(approved_program(screen, &allow(&["python"])).as_deref(), Some("python"));
    assert_eq!(approved_program(screen, &allow(&["Python"])).as_deref(), Some("python"));
    assert!(approved_program(screen, &allow(&["pytest"])).is_none());
    assert!(approved_program(screen, &[]).is_none());
}

#[test]
fn refuses_ambiguous_or_shell_like_prompts() {
    let two = "Not in allowlist: python\nNot in allowlist: rm";
    assert!(approved_program(two, &allow(&["python", "rm"])).is_none());
    let chained = "Not in allowlist: python; rm -rf x";
    assert!(approved_program(chained, &allow(&["python"])).is_none());
    let pathy = "Not in allowlist: ./python";
    assert!(approved_program(pathy, &allow(&["python"])).is_none());
    assert!(approved_program("nothing to see", &allow(&["python"])).is_none());
}

#[test]
fn validation_accepts_bare_names_only() {
    assert!(validate(&allow(&["python", "git", "cargo"])).is_ok());
    assert!(validate(&allow(&["rm -rf"])).is_err());
    assert!(validate(&allow(&["/bin/sh"])).is_err());
    assert!(validate(&allow(&[""])).is_err());
    assert!(validate(&vec!["a".to_string(); 11]).is_err());
}

const PROJECT: &str = r"C:\x";

fn ok(command: &str, items: &[&str]) -> bool {
    approved_command(command, &allow(items), Some(PROJECT)).is_some()
}

#[test]
fn pattern_entries_validate() {
    assert!(validate(&allow(&["python todo.py add *", "git status", "python -m pytest -q *"])).is_ok());
    for bad in [
        "python todo.py add x && rm -rf /",
        "python x > out.txt",
        "python $(whoami)",
        "sudo ls",
        "rm -rf build",
        "./python x.py",
        "python x; ls",
    ] {
        assert!(validate(&allow(&[bad])).is_err(), "must reject entry {bad:?}");
    }
}

#[test]
fn cd_prefix_is_stripped_before_judging() {
    let list = ["python todo.py add *"];
    assert!(ok(r#"cd "C:\x" && python todo.py add "buy milk""#, &list));
    assert!(ok(r#"cd /d "C:\x\sub" && python todo.py add a b"#, &list));
    assert!(ok("python todo.py add x", &list));
    assert!(!ok(r#"cd "C:\x""#, &list), "cd alone is not the real command");
    assert!(!ok(r#"cd "C:\other" && python todo.py add x"#, &list));
    assert!(!ok(r#"cd "C:\x\..\other" && python todo.py add x"#, &list));
    assert!(!ok(r#"cd "C:\xylophone" && python todo.py add x"#, &list));
    assert_eq!(
        leading_cd_dir(r#"cd "C:\x" && python a.py"#).as_deref(),
        Some(r"C:\x")
    );
    assert!(leading_cd_dir("python a.py").is_none());
}

#[test]
fn glob_semantics() {
    assert!(ok("python todo.py add", &["python todo.py add *"]));
    assert!(ok("python todo.py add a b c", &["python todo.py add *"]));
    assert!(!ok("python todo.py list", &["python todo.py add *"]));
    assert!(!ok("python todo.py add x", &["python todo.py"]));
    assert!(ok("python todo.py", &["python todo.py"]));
    assert!(ok("python -m pytest tests/test_a.py", &["python -m pytest tests/*"]));
    assert!(!ok("python -m pytest other/test_a.py", &["python -m pytest tests/*"]));
    assert!(ok("Python.exe a.py 1", &["python a.py *"]));
    assert!(ok("git status", &["git"]));
    assert!(ok("git push --force", &["git"]), "a bare name allows any arguments");
}

#[test]
fn chained_segments_must_all_match() {
    let list = ["python todo.py add *"];
    assert!(!ok("python todo.py add x && rm -rf /", &list));
    assert!(!ok(r#"cd "C:\x" && python todo.py add x && rm -rf /"#, &list));
    assert!(ok("python todo.py add x && python todo.py add y", &list));
    assert!(!ok("python todo.py add x && python evil.py", &list));
    assert!(ok("python a.py && git status", &["python a.py", "git"]));
}

#[test]
fn shell_syntax_is_refused() {
    let list = ["python todo.py add *", "python", "git", "rm"];
    for bad in [
        "python todo.py add x; rm -rf /",
        "python todo.py add x || rm y",
        "python todo.py add x | tee log",
        "python todo.py add x > ..\\out.txt",
        "python todo.py add x >> out.txt",
        "python todo.py add x < in.txt",
        "python todo.py add x &",
        "python todo.py add $(whoami)",
        "python todo.py add `whoami`",
        "python todo.py add \"$HOME\"",
        "python todo.py add \"%PATH%\"",
        "python todo.py add 'a && rm -rf /'",
        "python todo.py add x\nrm -rf /",
        "python todo.py add \"unterminated",
        "python todo.py add \"a\\\" && rm x\"",
        "python todo.py add x^&rm",
        "python todo.py add (a)",
        "python todo.py add trunc\u{2026}",
        "sudo python x.py",
        "runas /user:admin python x.py",
        "rm -rf build",
        "rm -r build",
        "rm -fr build",
        "git status && rm /s x",
        "./python x.py",
        "C:\\evil\\python.exe todo.py add x",
    ] {
        assert!(!ok(bad, &list), "must refuse: {bad:?}");
    }
    assert!(!ok("", &list));
    assert!(!ok("   ", &list));
    assert!(!ok("&& python x", &list));
    assert!(!ok("python x.py", &[]));
}

#[test]
fn operators_inside_double_quotes_are_literal() {
    assert!(ok(
        r#"python todo.py add "buy milk && eggs; today""#,
        &["python todo.py add *"]
    ));
}

#[test]
fn without_a_project_the_cd_target_is_not_checked() {
    let list = allow(&["python todo.py add *"]);
    assert!(approved_command(r#"cd "C:\anywhere" && python todo.py add x"#, &list, None).is_some());
}

#[test]
fn marker_path_never_approves_hard_denied_programs() {
    assert!(approved_program("Not in allowlist: sudo", &allow(&["sudo"])).is_none());
}

fn command_prompt(command: &str) -> serde_json::Value {
    serde_json::json!({ "kind": "command_approval", "command": command, "fully_drawn": true })
}

#[test]
fn installs_with_a_lookalike_or_a_link_are_never_auto_approved() {
    let list = allow(&["npm install *"]);
    assert!(validate(&list).is_ok(), "the pattern itself is still valid");
    assert!(approved_command("npm install lodash", &list, Some(PROJECT)).is_some());
    assert!(approved_command("npm install lodahs", &list, Some(PROJECT)).is_none());
    assert!(approved_command("npm install github:x/y", &list, Some(PROJECT)).is_none());
    assert!(approved_command("npm install lodash && npm install lodahs", &list, Some(PROJECT)).is_none());
    assert!(decide(&command_prompt("npm install lodash"), "", &list, Some(PROJECT)).is_some());
    assert!(decide(&command_prompt("npm install lodahs"), "", &list, Some(PROJECT)).is_none());
    assert!(decide(&command_prompt("npm install github:x/y"), "", &list, Some(PROJECT)).is_none());
    assert!(hard_denied(&allow(&["npm", "install", "lodahs"])));
    assert!(hard_denied(&allow(&["pip", "install", "reqests"])));
    assert!(!hard_denied(&allow(&["npm", "install", "lodash"])));
}
