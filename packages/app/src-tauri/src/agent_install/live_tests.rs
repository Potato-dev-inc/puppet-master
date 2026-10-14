//! Live checks on a real Windows PC. They are `#[ignore]`d because they need winget and the
//! network; run them by hand:
//!
//! `cargo test --lib agent_install::live_tests -- --ignored --nocapture`
//!
//! Nothing here installs anything. `winget show` only reads the package catalogue.

use super::command::{parse_done, powershell_script};
use super::plan::{plan_install, Action, Method, Step, Tools};
use std::io::Read;
use std::time::{Duration, Instant};

/// What the install button would show for each agent on THIS PC (the dry run). Runs nothing.
#[test]
#[ignore = "reads this PC's PATH; prints the plans"]
fn dry_run_prints_every_plan_for_this_pc() {
    let tools = Tools::detect();
    println!("tools: winget={:?} present={:?}", tools.winget, tools.present);
    for id in ["claude", "codex", "opencode", "cursor_agent"] {
        let (plan, _) = super::plan_for(id, &tools).unwrap();
        println!("--- {} ({id})", plan.label);
        match &plan.blocked {
            Some(reason) => println!("    blocked: {reason}"),
            None => {
                for (i, s) in plan.steps.iter().enumerate() {
                    println!(
                        "    step {}: {}\n      from {}  high_risk={}  method={:?}",
                        i + 1,
                        s.command,
                        s.source,
                        s.high_risk,
                        s.method
                    );
                    println!("      powershell -NoLogo -NoExit -Command <{}>", powershell_script(s));
                }
            }
        }
    }
}

/// The PATH refresh against this PC's registry: read-only, prints what a refresh would add.
#[test]
#[ignore = "reads the registry; sets this test process's PATH"]
fn path_refresh_reads_the_real_registry() {
    let before = std::env::var("PATH").unwrap_or_default();
    let refresh = super::path_refresh::refresh_process_path();
    let after = std::env::var("PATH").unwrap_or_default();
    println!("PATH entries before: {}  after: {}", before.split(';').count(), after.split(';').count());
    println!("added: {:#?}", refresh.added);
    assert!(after.len() >= before.len());
}

/// One harmless real command through the exact pane mechanism: `powershell.exe -NoLogo -NoExit
/// -Command <script>` on a ConPTY, the done marker, and the exit code. The command is
/// `winget show --id Anthropic.ClaudeCode -e --source winget`: it prints package details and
/// installs nothing.
#[test]
#[ignore = "needs winget and the network; installs nothing"]
fn a_harmless_winget_show_runs_in_a_pty_pane_and_reports_its_exit_code() {
    let tools = Tools::detect();
    let exe = tools.winget.clone().expect("winget is not on this PC");
    let args: Vec<String> = ["show", "--id", "Anthropic.ClaudeCode", "-e", "--source", "winget"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let step = Step {
        tool: None,
        label: "Claude Code (winget show, nothing is installed)".into(),
        method: Method::Winget,
        action: Action::Argv { exe, args },
        command: "winget show --id Anthropic.ClaudeCode -e --source winget".into(),
        source: "https://code.claude.com/docs/en/setup".into(),
        high_risk: false,
        note: String::new(),
    };
    let script = powershell_script(&step);

    let mut cmd = portable_pty::CommandBuilder::new("powershell.exe");
    cmd.args(["-NoLogo", "-NoExit", "-Command", script.as_str()]);
    use portable_pty::{native_pty_system, PtySize};
    let pair = native_pty_system()
        .openpty(PtySize { rows: 30, cols: 120, pixel_width: 0, pixel_height: 0 })
        .expect("open a PTY");
    let mut child = pair.slave.spawn_command(cmd).expect("start the pane's PowerShell");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("reader");
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut raw = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut code = None;
    while Instant::now() < deadline && code.is_none() {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(500)) {
            raw.extend(chunk);
        }
        code = parse_done(&crate::pty::ansi::strip_ansi(&String::from_utf8_lossy(&raw)));
    }
    let text = crate::pty::ansi::strip_ansi(&String::from_utf8_lossy(&raw));
    println!("---- what the pane showed ----\n{text}\n---- end ----");
    let _ = child.kill(); // closes the window
    assert!(text.contains("Puppet Master is running this for you"), "the banner names what runs");
    assert!(text.contains("winget show --id Anthropic.ClaudeCode"), "the shown command is printed");
    assert!(text.contains("Anthropic"), "winget answered");
    assert_eq!(code, Some(0), "the done marker carries winget's exit code");
}

#[test]
fn the_plan_for_a_pc_without_winget_is_blocked_not_guessed() {
    // Not ignored: documents the dry-run contract without touching the PC.
    let spec = crate::tui::install::parse_install_toml(
        "[install.windows]\nsource=\"https://example.com/s\"\nwinget=\"A.B\"\n",
    )
    .unwrap();
    let plan = plan_install("t", "T", spec.as_ref(), &Tools::default());
    assert!(plan.steps.is_empty() && plan.blocked.is_some());
}
