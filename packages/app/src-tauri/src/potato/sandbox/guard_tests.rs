//! Source-scan guards for the sandbox rules (spec "Invariants"). Tests only.
//!
//! - one door: only `wsl.rs` starts a process or names a `wsl.exe` verb, and no other file of the app names
//!   `--shutdown` or the person's WSL settings file or builds a sandbox distro name;
//! - the sandbox folder writes no file on Windows, deletes nothing recursively, logs nothing;
//! - the secrets' values (control token, VNC passwords) are read in two files only, and the app, not the guest, runs no boot step;
//! - generic code names no agent and no chat app;
//! - the fake is not in a release build.
//!
//! "Production code" here is a file's text before its first `#[cfg(test)]`, without `//` comments.

use std::path::{Path, PathBuf};

const AGENTS: [&str; 6] = ["claude", "codex", "cursor", "opencode", "agy", "kilo"];
const CHAT_APPS: [&str; 8] = ["discord", "telegram", "slack", "whatsapp", "imessage", "wechat", "mattermost", "signal-cli"];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn sandbox_dir() -> PathBuf {
    src_dir().join("potato").join("sandbox")
}

/// The production code of a source text: before the first `#[cfg(test)]`, comments cut.
fn production(text: &str) -> String {
    let code = text.split("#[cfg(test)]").next().unwrap_or("");
    code.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

fn is_test_file(rel: &str) -> bool {
    // `live.rs` is the opt-in live test (`PM_SBX_LIVE=1`); it is only compiled for tests (checked below).
    rel == "live.rs" || rel.ends_with("/live.rs") || rel.ends_with("_tests.rs") || rel.ends_with("/tests.rs") || rel == "tests.rs" || rel.contains("/tests/") || rel.starts_with("tests/")
}

/// (relative path with `/`, production code) of every non-test `.rs` file under `dir`.
fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            walk(&path, base, out);
        } else if rel.ends_with(".rs") && !is_test_file(&rel) {
            out.push((rel, production(&std::fs::read_to_string(&path).expect("read source"))));
        }
    }
}

fn sandbox_files() -> Vec<(String, String)> {
    let mut files = Vec::new();
    walk(&sandbox_dir(), &sandbox_dir(), &mut files);
    assert!(files.len() >= 6, "the scan must see the sandbox files, saw {}", files.len());
    files
}

fn all_files() -> Vec<(String, String)> {
    let mut files = Vec::new();
    walk(&src_dir(), &src_dir(), &mut files);
    assert!(files.len() > 200, "the scan must see the whole app, saw {}", files.len());
    files
}

fn lines_with<'a>(code: &'a str, needle: &str) -> Vec<&'a str> {
    code.lines().filter(|l| l.contains(needle)).collect()
}

#[test]
fn only_wsl_rs_starts_a_process() {
    for (name, code) in sandbox_files() {
        if name == "wsl.rs" {
            continue;
        }
        for needle in ["hidden_command", "hide_window", "Command::", "std::process", "tokio::process", "powershell", "\"wsl.exe\"", "ShellExecute"] {
            assert!(!code.contains(needle), "{name} touches processes ({needle}); only wsl.rs does");
        }
    }
    let wsl = &sandbox_files().into_iter().find(|(n, _)| n == "wsl.rs").unwrap().1;
    assert!(wsl.contains("crate::proc_util::hidden_command(&self.exe)"), "the real runner starts wsl.exe hidden");
    assert!(wsl.contains("crate::proc_util::hidden_command(\"powershell.exe\")"), "the elevated call is hidden too");
}

#[test]
fn the_refused_words_are_in_the_gate_and_nowhere_else_in_the_app() {
    let wsl = sandbox_files().into_iter().find(|(n, _)| n == "wsl.rs").unwrap().1;
    for refused in ["--shutdown", "--set-default", "--update", "--manage"] {
        assert!(wsl.contains(&format!("\"{refused}\"")), "{refused} must be in the refused list");
    }
    for (name, code) in all_files() {
        if name == "potato/sandbox/wsl.rs" {
            continue;
        }
        assert!(!code.contains("--shutdown"), "{name} names --shutdown; stopping every distro is never done");
        assert!(!code.to_lowercase().contains(".wslconfig"), "{name} names the WSL settings file");
    }
}

#[test]
fn nobody_else_builds_a_sandbox_distro_name() {
    for (name, code) in all_files() {
        if name.starts_with("potato/sandbox/") {
            continue;
        }
        assert!(!code.contains("pm-sbx-"), "{name} builds a sandbox name; only potato/sandbox/ does");
    }
}

#[test]
fn the_sandbox_never_touches_the_wsl_settings_or_the_persons_home() {
    for (name, code) in sandbox_files() {
        for needle in [".wslconfig", "USERPROFILE", "home_dir", "current_dir", "APPDATA", "C:\\\\", "/mnt/"] {
            assert!(!code.contains(needle), "{name} names {needle}; the sandbox lives only under the app data folder it is given");
        }
    }
}

#[test]
fn the_sandbox_folder_writes_no_file_on_windows_and_deletes_nothing_recursively() {
    for (name, code) in sandbox_files() {
        for needle in ["fs::write(", "File::create", "OpenOptions", "write_private", "fs::copy(", "fs::rename(", "remove_dir_all", "set_permissions"] {
            assert!(!code.contains(needle), "{name} uses {needle}");
        }
        if name != "mod.rs" {
            assert!(!code.contains("remove_file"), "{name} deletes a file; only WslSandbox::remove deletes its own reset points");
        }
    }
    // the one place that deletes: only files that end in .tar, one by one
    let modrs = sandbox_files().into_iter().find(|(n, _)| n == "mod.rs").unwrap().1;
    assert_eq!(lines_with(&modrs, "remove_file").len(), 1);
    assert!(modrs.contains("e == \"tar\""));
}

#[test]
fn the_sandbox_logs_nothing() {
    // The token and the screen are secrets; the folder has no logging at all, so nothing can leak into one.
    for (name, code) in sandbox_files() {
        for needle in ["tracing::", "println!", "eprintln!", "dbg!", "log::", "eprint!", "print!("] {
            assert!(!code.contains(needle), "{name} uses {needle}");
        }
    }
}

#[test]
fn the_secrets_values_are_read_in_two_files_only() {
    for (name, code) in sandbox_files() {
        let reads = lines_with(&code, ".expose()").len();
        match name.as_str() {
            "control.rs" => assert_eq!(reads, 1, "control.rs reads the token for the one header"),
            // the push into the distro (one line: token, view, control) and the check that the two passwords differ
            "secrets.rs" => assert_eq!(reads, 2, "secrets.rs reads the values for the push and for the difference check"),
            // the one fragment of the iframe address (`fragment_password`): never a query, a log line or a path
            "screen.rs" => assert_eq!(reads, 1, "screen.rs reads a password for the fragment of the iframe address only"),
            _ => assert_eq!(reads, 0, "{name} reads a secret"),
        }
    }
    // outside the sandbox nothing names the types
    for (name, code) in all_files() {
        if !name.starts_with("potato/sandbox/") {
            for ty in ["ControlToken", "VncPassword", "ScreenSecrets", "RunSecrets"] {
                assert!(!code.contains(ty), "{name} names {ty}");
            }
        }
    }
}

#[test]
fn the_secret_types_cannot_be_copied_printed_or_saved() {
    let secrets = sandbox_files().into_iter().find(|(n, _)| n == "secrets.rs").unwrap().1;
    for ty in ["ControlToken", "VncPassword", "ScreenSecrets", "RunSecrets"] {
        let at = secrets.find(&format!("pub struct {ty}")).unwrap_or_else(|| panic!("the type {ty}"));
        let attrs = secrets[..at].lines().rev().take(3).collect::<String>();
        assert!(!attrs.contains("derive"), "{ty} has no derive (no Clone, Serialize or Debug): {attrs}");
        assert!(secrets.contains(&format!("impl std::fmt::Debug for {ty}")), "{ty} has a Debug that prints nothing");
        assert!(!secrets.contains(&format!("impl std::fmt::Display for {ty}")), "{ty}");
        assert!(!secrets.contains(&format!("impl Clone for {ty}")), "{ty}");
        assert!(!secrets.contains(&format!("impl serde::Serialize for {ty}")), "{ty}");
    }
    // the two that hold a value wipe it when dropped
    assert!(secrets.contains("impl Drop for ControlToken") && secrets.contains("impl Drop for VncPassword"));
    // control.rs only uses the type, it does not define it
    let control = sandbox_files().into_iter().find(|(n, _)| n == "control.rs").unwrap().1;
    assert!(!control.contains("pub struct ControlToken"));
}

#[test]
fn the_app_runs_no_boot_step_the_guest_owns_the_boot_verdict() {
    // pm-sbx-boot.sh (wsl.conf [boot]) cuts WSLg, sets the walls and proves them, then writes boot.done or boot.failed.
    // The app waits for that file; it never runs those scripts and never reads the old duplicate flag.
    for (name, code) in sandbox_files() {
        for needle in ["pm-sbx-egress", "pm-no-wslg", "/run/pm-no-wslg"] {
            // install.rs only lists the egress scripts among the guest files it ships
            if name == "install.rs" && needle == "pm-sbx-egress" {
                continue;
            }
            assert!(!code.contains(needle), "{name} names {needle}; the guest's boot hook owns it");
        }
    }
    let lifecycle = sandbox_files().into_iter().find(|(n, _)| n == "lifecycle.rs").unwrap().1;
    assert!(lifecycle.contains("/run/pm-sbx/boot.done") && lifecycle.contains("/run/pm-sbx/boot.failed"));
}

#[test]
fn the_live_test_is_compiled_for_tests_only() {
    let text = std::fs::read_to_string(sandbox_dir().join("mod.rs")).unwrap();
    assert!(text.contains("#[cfg(test)]\nmod live;") || text.contains("#[cfg(test)]\r\nmod live;"), "mod live; must sit under #[cfg(test)]");
    let live = std::fs::read_to_string(sandbox_dir().join("live.rs")).unwrap();
    assert!(live.contains("#[ignore"), "the live test is ignored by default");
    assert!(live.contains("PM_SBX_LIVE"), "and needs PM_SBX_LIVE=1");
    assert!(live.contains("pm-sbx-e2e"), "it uses its own distro name");
}

#[test]
fn the_client_talks_to_loopback_only() {
    for (name, code) in sandbox_files() {
        for line in lines_with(&code, "http://") {
            assert!(line.contains("http://127.0.0.1"), "{name}: {line}");
        }
        assert!(!code.contains("https://"), "{name} names an internet address");
    }
    let control = sandbox_files().into_iter().find(|(n, _)| n == "control.rs").unwrap().1;
    assert!(control.contains(".redirects(0)"), "a redirect could carry the token elsewhere");
}

#[test]
fn only_the_button_makes_a_consent() {
    for (name, code) in all_files() {
        if name == "potato/sandbox/wsl.rs" {
            continue;
        }
        assert!(
            !code.contains("the_person_pressed_the_button"),
            "{name} makes a Consent; only the command bound to the person's button may (it is added with the settings page)"
        );
        assert!(!code.contains("enable_windows_feature") || name.starts_with("commands/"), "{name} turns on the Linux feature");
    }
    // an install and a start never reach for it
    for (name, code) in sandbox_files() {
        if name != "wsl.rs" {
            assert!(!code.contains("enable_windows_feature"), "{name}");
        }
    }
}

#[test]
fn generic_sandbox_code_names_no_agent_and_no_chat_app() {
    for (name, code) in sandbox_files() {
        let lower = code.to_lowercase();
        for word in AGENTS {
            assert!(!lower.contains(word), "{name} names an agent ({word})");
        }
        for word in CHAT_APPS {
            assert!(!lower.contains(word), "{name} names a chat app ({word})");
        }
    }
}

#[test]
fn the_fake_is_not_in_a_release_build() {
    let text = std::fs::read_to_string(sandbox_dir().join("mod.rs")).unwrap();
    let at = text.find("pub struct FakeSandbox").expect("the fake");
    let attrs: Vec<&str> = text[..at].lines().rev().take(3).collect();
    assert!(attrs.iter().any(|l| l.contains("#[cfg(any(test, feature = \"dev-fake-tui\"))]")), "{attrs:?}");
    let at = text.find("impl Sandbox for FakeSandbox").unwrap();
    assert!(text[..at].lines().rev().take(2).any(|l| l.contains("#[cfg(any(test, feature = \"dev-fake-tui\"))]")));
}

#[test]
fn the_labs_flag_is_off_by_default_and_has_a_schema_entry() {
    let defaults = crate::settings_store::default_public_settings();
    assert_eq!(defaults["potato_sandbox_enabled"], false);
    let schema = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("shared").join("src").join("schemas.ts")).unwrap();
    assert!(schema.contains("potato_sandbox_enabled: z.boolean().optional()"));
}

#[test]
fn the_scanner_sees_a_violation() {
    let code = production("fn f() { let _ = std::fs::write(p, t); // fs::copy( is a comment\n}\n#[cfg(test)]\nmod t { fn g() { dbg!(1); } }");
    assert!(code.contains("fs::write("));
    assert!(!code.contains("fs::copy("), "comments are cut");
    assert!(!code.contains("dbg!"), "test code is cut");
    assert!(is_test_file("wsl/guard_tests.rs") && is_test_file("pty/tests.rs") && !is_test_file("potato/sandbox/wsl.rs"));
}
