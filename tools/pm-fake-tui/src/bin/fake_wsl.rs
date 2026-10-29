//! `pm-fake-wsl`: a stand-in for `wsl.exe` in Puppet Master's Linux Inside end-to-end tests. Dev only; the
//! tests point `PM_FAKE_WSL_EXE` at it (the app honours that only in builds with feature `dev-fake-tui`), so the
//! real `wsl.exe` is never started.
//!
//! It answers the calls the app makes, told apart the way `wsl.exe` would see them:
//! * `--list --verbose`: one WSL 2 distro, `* Ubuntu-Test Running 2`, in UTF-16LE like the real tool. `--status`: ok.
//! * A probe (`-d D --exec /bin/sh -c <script> probe <nonce> <program>...`): the facts lines the app reads, with
//!   `home_windows` = `PM_FAKE_WSL_HOME` (a temp Windows folder standing in for `\\wsl.localhost\...\home\you`),
//!   `mount_root=/mnt/`, and every asked program at `/usr/bin/<program>`.
//! * A launch (`-d D --cd /mnt/d/x --exec /bin/sh -c <launcher> puppet-master <program> <args>...`): the folder is
//!   mapped back (`/mnt/d/x` is `D:\x`); only the names in `WSLENV` reach the program (they are logged); the
//!   program run is `PM_FAKE_TUI_EXE` with the args after the program. The harness's own `PM_FAKE_TUI_*` settings
//!   pass too (they stand for the script on the Linux disk). The launcher's `PM_WSL_PATH` is removed, as it would be.
//! * A stop (`... puppet-master-stop <pane>`): logs it, ends the processes started under that pane id (by pid),
//!   prints `pm-stopped=<n>`.
//!
//! `PM_FAKE_WSL_LOG=<file>` gets one JSON line per call: `list`, `status`, `probe`, `launch`, `stop`.

use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

const DISTRO: &str = "Ubuntu-Test";
const MOUNT_ROOT: &str = "/mnt/";
/// Variables the program keeps besides the `WSLENV` names: just enough for a Windows process to start.
const BASE_ENV: &[&str] = &["SystemRoot", "windir", "ComSpec", "PATHEXT", "PATH", "TEMP", "TMP"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn log(entry: Value) {
    let Some(path) = std::env::var_os("PM_FAKE_WSL_LOG").filter(|p| !p.is_empty()) else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{entry}");
    }
}

fn run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--list") {
        log(json!({ "ev": "list" }));
        return print_list();
    }
    if args.iter().any(|a| a == "--status") {
        log(json!({ "ev": "status" }));
        println!("Default Distribution: {DISTRO}");
        return 0;
    }
    let mut distro = String::new();
    let mut cd: Option<String> = None;
    let mut rest: &[String] = &[];
    let mut at = 0;
    while at < args.len() {
        match args[at].as_str() {
            "-d" => {
                distro = args.get(at + 1).cloned().unwrap_or_default();
                at += 2;
            }
            "--cd" => {
                cd = args.get(at + 1).cloned();
                at += 2;
            }
            "--exec" => {
                rest = &args[at + 1..];
                break;
            }
            _ => at += 1,
        }
    }
    // /bin/sh -c <script> <argv0> <payload...>
    if rest.len() < 4 || rest[0] != "/bin/sh" || rest[1] != "-c" {
        eprintln!("pm-fake-wsl: not a call it knows: {args:?}");
        return 2;
    }
    let (argv0, payload) = (rest[3].as_str(), &rest[4..]);
    match argv0 {
        "probe" => probe(&distro, payload),
        "puppet-master-stop" => stop(&distro, payload),
        "puppet-master" => launch(&distro, cd.as_deref(), payload),
        other => {
            eprintln!("pm-fake-wsl: unknown script {other}");
            2
        }
    }
}

/// UTF-16LE with a BOM, like `wsl.exe --list --verbose` without `WSL_UTF8`.
fn print_list() -> i32 {
    let text = format!("  NAME            STATE           VERSION\r\n* {DISTRO}     Running         2\r\n");
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let mut out = std::io::stdout();
    let _ = out.write_all(&bytes);
    let _ = out.flush();
    0
}

fn probe(distro: &str, payload: &[String]) -> i32 {
    let Some(nonce) = payload.first() else { return 2 };
    let programs = &payload[1..];
    let Some(home) = std::env::var_os("PM_FAKE_WSL_HOME").filter(|h| !h.is_empty()) else {
        eprintln!("pm-fake-wsl: PM_FAKE_WSL_HOME is not set");
        return 1;
    };
    log(json!({ "ev": "probe", "distro": distro, "programs": programs }));
    let line = |key: &str, value: &str| println!("PM1:{nonce}:{key}={value}");
    line("uid", "1000");
    line("home", "/home/you");
    line("home_windows", &home.to_string_lossy());
    line("mount_root", MOUNT_ROOT);
    line("git", "2.43.0");
    line("interop", "1");
    line("networking", "nat");
    line("path", "/usr/local/bin:/usr/bin:/bin");
    for program in programs {
        line(&format!("prog:{program}"), &format!("/usr/bin/{program}"));
    }
    0
}

/// `/mnt/d/work/x` is `D:\work\x`; `/mnt/d` is `D:\`. Anything else is not a Windows drive folder.
fn windows_folder(linux: &str) -> Option<PathBuf> {
    let rest = linux.strip_prefix(MOUNT_ROOT)?;
    let (drive, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let mut chars = drive.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    if chars.next().is_some() {
        return None;
    }
    Some(PathBuf::from(format!("{}:\\{}", letter.to_ascii_uppercase(), tail.replace('/', "\\"))))
}

/// The names `WSLENV` lists, without their `/flags`.
fn wslenv_names(wslenv: &str) -> Vec<String> {
    wslenv.split(':').filter(|e| !e.is_empty()).map(|e| e.split('/').next().unwrap_or(e).to_string()).collect()
}

fn procs_file() -> Option<PathBuf> {
    let log = std::env::var_os("PM_FAKE_WSL_LOG").filter(|p| !p.is_empty())?;
    let mut name = PathBuf::from(log).into_os_string();
    name.push(".procs");
    Some(PathBuf::from(name))
}

fn launch(distro: &str, cd: Option<&str>, payload: &[String]) -> i32 {
    let Some((program, program_args)) = payload.split_first() else { return 2 };
    let Some(folder) = cd.and_then(windows_folder) else {
        eprintln!("pm-fake-wsl: --cd {cd:?} is not a folder on a Windows drive");
        return 1;
    };
    let Some(fake_tui) = std::env::var_os("PM_FAKE_TUI_EXE").filter(|p| !p.is_empty()) else {
        eprintln!("pm-fake-wsl: PM_FAKE_TUI_EXE is not set");
        return 1;
    };
    let wslenv = std::env::var("WSLENV").unwrap_or_default();
    // Only what WSLENV lists crosses. The launcher takes PM_WSL_PATH out again before the program starts.
    let mut crossed: Vec<(String, String)> = Vec::new();
    for name in wslenv_names(&wslenv) {
        if let Ok(value) = std::env::var(&name) {
            crossed.push((name, value));
        }
    }
    let pane = crossed.iter().find(|(name, _)| name == "PUPPET_MASTER_PANE_ID").map(|(_, value)| value.clone());
    log(json!({
        "ev": "launch",
        "pane": pane,
        "distro": distro,
        "cd": cd,
        "folder": folder.to_string_lossy(),
        "program": program,
        "args": program_args,
        "wslenv": wslenv,
        "env": crossed.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
    }));

    let mut command = Command::new(fake_tui);
    command.args(program_args).current_dir(&folder).env_clear();
    for name in BASE_ENV {
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
    for (name, value) in std::env::vars() {
        if name.starts_with("PM_FAKE_TUI_") {
            command.env(name, value);
        }
    }
    for (name, value) in &crossed {
        if name != "PM_WSL_PATH" {
            command.env(name, value);
        }
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("pm-fake-wsl: cannot start the program: {error}");
            return 1;
        }
    };
    if let (Some(pane), Some(file)) = (pane, procs_file()) {
        if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
            let _ = writeln!(out, "{pane}\t{}", child.id());
        }
    }
    child.wait().ok().and_then(|status| status.code()).unwrap_or(1)
}

fn stop(distro: &str, payload: &[String]) -> i32 {
    let Some(pane) = payload.first() else { return 2 };
    log(json!({ "ev": "stop", "distro": distro, "pane": pane }));
    let mut stopped = 0;
    if let Some(text) = procs_file().and_then(|file| std::fs::read_to_string(file).ok()) {
        for line in text.lines() {
            let Some((id, pid)) = line.split_once('\t') else { continue };
            if id != pane {
                continue;
            }
            // By pid only, never by name; a process that is already gone makes taskkill fail, which is fine.
            let killed = Command::new("taskkill")
                .args(["/PID", pid, "/T", "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if killed {
                stopped += 1;
            }
        }
    }
    println!("pm-stopped={stopped}");
    0
}
