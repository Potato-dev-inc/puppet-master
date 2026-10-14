//! `pm-fake-tui [--agent claude|codex] [--script FILE] [--check FILE]`
//!
//! The script comes from `--script`, else the `PM_FAKE_TUI_SCRIPT` environment variable, else a bare
//! agent of `--agent`'s layout that answers every prompt with "OK".

use pm_fake_tui::script::{Layout, Script};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut script_path: Option<String> = std::env::var("PM_FAKE_TUI_SCRIPT").ok().filter(|p| !p.is_empty());
    let mut agent: Option<Layout> = None;
    let mut check = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--script" => script_path = args.next(),
            "--check" => {
                check = true;
                script_path = args.next();
            }
            "--agent" => {
                agent = match args.next().as_deref() {
                    Some("claude") => Some(Layout::Claude),
                    Some("codex") => Some(Layout::Codex),
                    other => {
                        eprintln!("--agent takes claude or codex, not {other:?}");
                        std::process::exit(2);
                    }
                }
            }
            "--help" | "-h" => {
                println!("pm-fake-tui [--agent claude|codex] [--script FILE.toml|FILE.json] [--check FILE]\nSee tools/pm-fake-tui/README.md for the script format.");
                return;
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let mut script = match script_path {
        Some(path) => match Script::load(std::path::Path::new(&path)) {
            Ok(script) => script,
            Err(err) => {
                eprintln!("pm-fake-tui: {err}");
                std::process::exit(2);
            }
        },
        None => Script::default(),
    };
    if check {
        println!("ok: {} turn(s), {} startup screen(s)", script.turn.len(), script.startup.len());
        return;
    }
    if let Some(layout) = agent {
        script.agent = layout;
    }
    let base_dir = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
    std::process::exit(pm_fake_tui::runtime::run(script, base_dir));
}
