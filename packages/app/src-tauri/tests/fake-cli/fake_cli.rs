// A tiny fake headless coding agent for the Agent Wizard tests. Std only; the tests compile it with
// `rustc` and copy it to other names. It spends no tokens and is fully deterministic.
//
//   fake-agent --version | --help | -h | help | help run
//   fake-agent run [--format json] [-m MODEL] [-s SESSION] [--] <task>
//
// `run --format json` prints JSON lines:
//   {"event":"start","session":"fk-0001","model":"fake/fast","resumed":false,"cwd":"<working folder>"}
//   {"event":"message","delta":"po"}
//   {"event":"end","session":"fk-0001","reason":"stop","text":"pong"}
// A task "Reply with exactly the word: X" is answered with X, anything else with "ok". A model other
// than fake/fast or fake/slow prints {"event":"error","message":"Model not found: ..."} and exits 1.
// A session id that does not start with "fk-" is an error too.
//
// The program name selects odd behaviour (the tests copy the exe): *hang* sleeps for a minute,
// *flood* prints about 200 KB, *secret* prints a token-shaped string in its help, *silent* prints
// nothing, *raw* answers `run` in plain text without any JSON, *linger* finishes its events and then
// keeps running for a minute (like an agent that does not exit after its last event).
use std::io::Write;
use std::path::Path;

const HELP: &str = "fake-agent 1.4.2
A tiny fake coding agent for tests

Usage: fake-agent <COMMAND>

Commands:
  run     Run one task and print JSON events
  login   Sign in to the fake service
  help    Print this message

Options:
  -h, --help       Print help
  -V, --version    Print version
";

const RUN_HELP: &str = "Run one task and print JSON events

Usage: fake-agent run [OPTIONS] [TASK]...

Options:
      --format <FORMAT>   Output format: text or json [default: text]
  -m, --model <MODEL>     Model to use: fake/fast or fake/slow
  -s, --session <ID>      Continue the conversation with this session id
  -h, --help              Print help
";

fn main() {
    let exe = std::env::args().next().unwrap_or_default();
    let stem = Path::new(&exe)
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--warmup"] {
        return; // the tests start every copy once, so a virus scanner's first look is not counted in a timeout
    }
    if stem.contains("hang") {
        std::thread::sleep(std::time::Duration::from_secs(60));
        return;
    }
    if stem.contains("flood") {
        let mut out = std::io::stdout().lock();
        for i in 0..6000 {
            let _ = writeln!(out, "flood line {i:05} ................................");
        }
        return;
    }
    if stem.contains("silent") {
        return;
    }
    if stem.contains("secret") {
        println!("secret-agent help\nexport KEY=sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWX\nCommands:\n  run  Run a task");
        return;
    }
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => println!("fake-agent 1.4.2"),
        Some("--help") | Some("-h") => print!("{HELP}"),
        Some("help") => match args.get(1).map(String::as_str) {
            None => print!("{HELP}"),
            Some("run") => print!("{RUN_HELP}"),
            Some(other) => {
                eprintln!("no help for {other}");
                std::process::exit(2);
            }
        },
        Some("run") => {
            run(&args[1..], stem.contains("raw"));
            if stem.contains("linger") {
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        }
        _ => {
            eprintln!("error: unknown command; try --help");
            std::process::exit(2);
        }
    }
}

fn answer(task: &str) -> String {
    match task.split_once("Reply with exactly the word:") {
        Some((_, word)) => word.trim().to_string(),
        None => "ok".to_string(),
    }
}

fn event(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

fn run(args: &[String], raw: bool) {
    let mut format = "text".to_string();
    let mut model = "fake/fast".to_string();
    let mut session: Option<String> = None;
    let mut task = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                i += 1;
                format = args.get(i).cloned().unwrap_or_default();
            }
            "-m" | "--model" => {
                i += 1;
                model = args.get(i).cloned().unwrap_or_default();
            }
            "-s" | "--session" => {
                i += 1;
                session = args.get(i).cloned();
            }
            "--" => {
                task.extend(args[i + 1..].iter().cloned());
                break;
            }
            other => task.push(other.to_string()),
        }
        i += 1;
    }
    let task = task.join(" ");
    let text = answer(&task);
    if raw {
        println!("Thinking...\n{text}");
        return;
    }
    if model != "fake/fast" && model != "fake/slow" {
        if format == "json" {
            event(&format!("{{\"event\":\"error\",\"message\":\"Model not found: {model}.\"}}"));
        } else {
            eprintln!("Model not found: {model}.");
        }
        std::process::exit(1);
    }
    if let Some(id) = &session {
        if !id.starts_with("fk-") {
            event(&format!("{{\"event\":\"error\",\"message\":\"Unknown session {id}.\"}}"));
            std::process::exit(1);
        }
    }
    let id = session.clone().unwrap_or_else(|| "fk-0001".to_string());
    if format != "json" {
        println!("{text}");
        return;
    }
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default().replace('\\', "/");
    event(&format!("{{\"event\":\"start\",\"session\":\"{id}\",\"model\":\"{model}\",\"resumed\":{},\"cwd\":\"{cwd}\"}}", session.is_some()));
    let mid = text.len() / 2;
    event(&format!("{{\"event\":\"message\",\"delta\":\"{}\"}}", &text[..mid]));
    event(&format!("{{\"event\":\"message\",\"delta\":\"{}\"}}", &text[mid..]));
    event(&format!("{{\"event\":\"end\",\"session\":\"{id}\",\"reason\":\"stop\",\"text\":\"{text}\"}}"));
}
