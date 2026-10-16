//! Resolve and Probe: fixed argument lists, limits, timeouts, redaction, subcommand parsing.

use serde_json::json;

use crate::tui::declarative::wizard::locate::{command_name, id_suggestion, locate, version_number};
use crate::tui::declarative::wizard::probe::{parse_subcommands, valid_subcommand};

use super::support::{bin_dir, call, ok, start, Tree};

const CLAP_HELP: &str = "fake-agent 1.4.2
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

#[test]
fn subcommands_are_read_from_the_commands_section_only() {
    assert_eq!(parse_subcommands(CLAP_HELP, "fake-agent"), vec!["run"], "login is on the deny list, help is skipped");
    let other = "USAGE:\n  tool <cmd>\n\nSUBCOMMANDS:\n    chat, c    Start chatting\n    mcp        Manage servers\n    Update     Not lowercase\n\nOPTIONS:\n    run   not a command\n";
    assert_eq!(parse_subcommands(other, "tool"), vec!["chat", "mcp"]);
    let blank_after_heading = "Commands:\n\n  build  Build it\n  test   Test it\n\nOptions:\n  deploy  not a command\n";
    assert_eq!(parse_subcommands(blank_after_heading, "tool"), vec!["build", "test"]);
    assert!(parse_subcommands("no commands here\n  run  something", "tool").is_empty());
}

const YARGS_HELP: &str = "Commands:
  kilo completion          generate shell completion script
  kilo [project]           start kilo tui                                                  [default]
  kilo attach <url>        attach to a running kilo server
  kilo run [message..]     run kilo with a message
  kilo auth                manage AI providers and credentials                  [aliases: providers]
  kilo models [provider]   list all available models

Options:
  -h, --help     show help
";

#[test]
fn rows_that_start_with_the_program_name_are_read_too() {
    // yargs prints `program name <args>  description`
    assert_eq!(parse_subcommands(YARGS_HELP, "kilo"), vec!["completion", "attach", "run", "models"]);
    assert_eq!(parse_subcommands(YARGS_HELP, "kilo.exe"), vec!["completion", "attach", "run", "models"]);
    // for another program the first word is the name (and `kilo` is a valid name)
    assert_eq!(parse_subcommands(YARGS_HELP, "other")[0], "kilo");
    let clap = "Commands:\n  agent       Manage agents\n  chat        AI assistant\n  login       Log in\n  help        Print this message\n\nOptions:\n";
    assert_eq!(parse_subcommands(clap, "kiro-cli"), vec!["agent", "chat"]);
}

#[test]
fn subcommand_names_are_vetted() {
    for ok_name in ["run", "chat", "a1", "my-cmd", "my_cmd"] {
        assert!(valid_subcommand(ok_name), "{ok_name}");
    }
    for bad in ["", "-x", "Run", "help", "update", "login", "rm", "a b", "x;y", "../x", "run$", &"a".repeat(33)] {
        assert!(!valid_subcommand(bad), "{bad}");
    }
}

#[test]
fn the_typed_command_must_be_a_bare_name() {
    assert_eq!(command_name("kiro-cli").unwrap(), "kiro-cli");
    assert_eq!(command_name("  kilo.exe ").unwrap(), "kilo");
    assert_eq!(command_name("Tool.CMD").unwrap(), "Tool");
    for bad in ["", "   ", "a b", "..\\x", "C:\\tools\\x.exe", "x/y", ".hidden", "a;b", "x&y", &"a".repeat(65)] {
        let err = command_name(bad).unwrap_err();
        assert!(err.code == "AGENT_DEF_BAD_BINARY" || err.code == "INVALID_ARGUMENT", "{bad}: {err}");
    }
    assert_eq!(id_suggestion("kiro-cli"), "kiro_cli");
    assert_eq!(id_suggestion("9lives"), "a9lives");
    assert_eq!(id_suggestion("x"), "xx");
}

#[test]
fn version_numbers_are_the_first_dotted_number() {
    assert_eq!(version_number("kiro-cli 2.29.1"), Some("2.29.1".into()));
    assert_eq!(version_number("v7.8.8 (build 12)"), Some("7.8.8".into()));
    assert_eq!(version_number("no number"), None);
}

#[test]
fn start_finds_the_program_and_reads_its_version() {
    let tree = Tree::new("start");
    let wizard = tree.wizard();
    let out = ok(&wizard, "start", json!({"command": "fake-agent"}));
    assert_eq!(out["state"], "resolved");
    assert_eq!(out["command"], "fake-agent");
    assert_eq!(out["id_suggestion"], "fake_agent");
    assert_eq!(out["program"]["kind"], "native");
    assert_eq!(out["version"]["ok"], true);
    assert_eq!(out["version"]["number"], "1.4.2");
    assert!(out["session"].as_str().unwrap().starts_with("wz_"));
    assert!(out["cost_note"].as_str().unwrap().contains("3 small messages"));
    assert_eq!(out["limits"]["probe_timeout_ms"], 5000);
    assert_eq!(out["limits"]["probe_max_runs"], 16);
    // the work folder exists, in the temp root
    let folders: Vec<_> = std::fs::read_dir(&tree.temp).unwrap().flatten().collect();
    assert_eq!(folders.len(), 1);
    assert!(folders[0].file_name().to_string_lossy().starts_with("pm-wizard-wz_"));
}

#[test]
fn a_missing_program_is_a_clear_404_and_opens_no_session() {
    let tree = Tree::new("start-missing");
    let wizard = tree.wizard();
    let err = call(&wizard, "start", json!({"command": "no-such-program-xyz"})).unwrap_err();
    assert_eq!((err.status, err.code), (404, "AGENT_NOT_INSTALLED"));
    assert!(err.message.contains("no-such-program-xyz"));
    assert_eq!(wizard.session_count(), 0);
    assert_eq!(std::fs::read_dir(&tree.temp).unwrap().count(), 0, "no work folder is left behind");
    let bad = call(&wizard, "start", json!({"command": "a b"})).unwrap_err();
    assert_eq!((bad.status, bad.code), (400, "AGENT_DEF_BAD_BINARY"));
    assert_eq!(call(&wizard, "start", json!({})).unwrap_err().code, "INVALID_ARGUMENT");
}

#[test]
fn locate_does_not_look_in_the_current_folder() {
    let tree = Tree::new("locate-cwd");
    // an empty search list finds nothing, even though the fake exists elsewhere
    assert!(locate("fake-agent", &[]).is_err());
    assert!(locate("fake-agent", &[bin_dir().to_path_buf()]).is_ok());
    let _ = tree;
}

#[test]
fn probe_runs_exactly_the_fixed_list_and_marks_duplicates() {
    let tree = Tree::new("probe");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    let out = ok(&wizard, "probe", json!({"session": id}));
    assert_eq!(out["state"], "probed");
    let labels: Vec<String> = out["runs"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap().to_string()).collect();
    assert_eq!(labels, ["--help", "-h", "--version", "help", "run --help"]);
    let runs = out["runs"].as_array().unwrap();
    assert!(runs[0]["text"].as_str().unwrap().contains("Commands:"));
    assert_eq!(runs[0]["duplicate_of"], json!(null));
    assert_eq!(runs[1]["duplicate_of"], "--help", "-h prints the same help");
    assert_eq!(runs[1]["text"], "");
    assert_eq!(runs[3]["duplicate_of"], "--help");
    assert_eq!(runs[2]["text"].as_str().unwrap().trim(), "fake-agent 1.4.2");
    assert_eq!(runs[4]["args"], json!(["run", "--help"]));
    assert!(runs[4]["text"].as_str().unwrap().contains("--format"));
    assert_eq!(out["subcommands"], json!(["run"]));
    assert_eq!(out["skipped"], json!([]));
    assert_eq!(out["limits"]["max_runs"], 16);
    for run in runs {
        assert_eq!(run["exit"], 0);
        assert_eq!(run["timed_out"], false);
    }
}

#[test]
fn a_probe_that_never_ends_times_out_and_the_wizard_moves_on() {
    let tree = Tree::new("probe-hang");
    let wizard = tree.wizard_with(|env| {
        env.limits.version_timeout = std::time::Duration::from_millis(300);
        env.limits.probe_timeout = std::time::Duration::from_millis(300);
    });
    let id = start(&wizard, "hang-agent");
    let out = ok(&wizard, "probe", json!({"session": id}));
    let runs = out["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 4, "the four base runs; no subcommands to ask about");
    for run in runs {
        assert_eq!(run["timed_out"], true);
        assert_eq!(run["exit"], json!(null));
    }
}

#[test]
fn probe_output_is_capped() {
    let tree = Tree::new("probe-cap");
    let wizard = tree.wizard_with(|env| env.limits.probe_max_bytes = 2000);
    let id = start(&wizard, "flood-agent");
    let out = ok(&wizard, "probe", json!({"session": id}));
    let first = &out["runs"][0];
    assert_eq!(first["truncated"], true);
    assert!(first["text"].as_str().unwrap().len() <= 2000, "{}", first["text"].as_str().unwrap().len());
    assert!(first["bytes"].as_u64().unwrap() >= 1000);
    assert_eq!(first["exit"], 0, "a child that prints a lot is not blocked");
}

#[test]
fn probe_output_is_redacted_before_it_is_returned_or_kept() {
    let tree = Tree::new("probe-redact");
    let wizard = tree.wizard();
    let id = start(&wizard, "secret-agent");
    let out = ok(&wizard, "probe", json!({"session": id}));
    let all = out.to_string();
    assert!(!all.contains("ABCDEFGHIJKLMNOP"), "the token must be masked: {all}");
    assert!(out["runs"][0]["text"].as_str().unwrap().contains("Commands:"));
    assert_eq!(out["subcommands"], json!(["run"]));
    let kept = wizard.read(&id, |s| format!("{:?}", s.probe)).unwrap();
    assert!(!kept.contains("ABCDEFGHIJKLMNOP"), "the stored copy is masked too");
}

#[test]
fn the_run_limit_is_enforced_and_the_rest_is_listed_as_skipped() {
    let tree = Tree::new("probe-limit");
    let wizard = tree.wizard_with(|env| env.limits.probe_max_runs = 3);
    let id = start(&wizard, "fake-agent");
    let out = ok(&wizard, "probe", json!({"session": id}));
    assert_eq!(out["runs"].as_array().unwrap().len(), 3);
    let skipped = out["skipped"].as_array().unwrap();
    assert!(skipped.iter().any(|s| s["args"] == json!(["help"]) && s["reason"] == "run limit reached"), "{skipped:?}");
    assert!(skipped.iter().any(|s| s["args"] == json!(["run", "--help"])), "{skipped:?}");
}

#[test]
fn probes_do_not_wait_for_input() {
    // The fake prints its version and exits; a program that waited for stdin would hit the timeout.
    let tree = Tree::new("probe-stdin");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    let started = std::time::Instant::now();
    ok(&wizard, "probe", json!({"session": id}));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn tools_that_open_documentation_windows_are_only_asked_for_their_version() {
    // `git --help` opens a manual page in a browser; a copy of the fake named `git` stands in for it.
    let tree = Tree::new("probe-docs");
    let wizard = tree.wizard();
    let id = start(&wizard, "git");
    let out = ok(&wizard, "probe", json!({"session": id}));
    let labels: Vec<&str> = out["runs"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["--version"]);
    let skipped = out["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 3);
    assert!(skipped[0]["reason"].as_str().unwrap().contains("documentation windows"));
    assert_eq!(out["subcommands"], json!([]));
    assert!(crate::tui::declarative::wizard::probe::opens_documentation("NPM"));
    assert!(!crate::tui::declarative::wizard::probe::opens_documentation("kiro-cli"));
}

#[test]
fn flags_are_asked_for_under_the_subcommand_and_help_sub_is_only_the_fallback() {
    let tree = Tree::new("probe-sub");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    let out = ok(&wizard, "probe", json!({"session": id}));
    let sub = out["runs"].as_array().unwrap().iter().find(|r| r["label"] == "run --help").expect("run --help ran");
    assert!(sub["text"].as_str().unwrap().contains("--session"), "the flags are in the text: {sub}");
    let labels: Vec<&str> = out["runs"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap()).collect();
    assert!(!labels.contains(&"help run"), "run --help worked, so there is no need for help run: {labels:?}");

    // a program with no `run --help` falls back to `help run`
    let id = start(&wizard, "oldstyle-agent");
    let out = ok(&wizard, "probe", json!({"session": id}));
    let labels: Vec<&str> = out["runs"].as_array().unwrap().iter().map(|r| r["label"].as_str().unwrap()).collect();
    assert_eq!(&labels[4..], ["run --help", "help run"], "{labels:?}");
    let fallback = out["runs"].as_array().unwrap().iter().find(|r| r["label"] == "help run").unwrap();
    assert!(fallback["text"].as_str().unwrap().contains("--session"));
    let failed = out["runs"].as_array().unwrap().iter().find(|r| r["label"] == "run --help").unwrap();
    assert_eq!(failed["exit"], 2);
}

#[test]
fn subcommand_probes_keep_the_same_limits_as_the_base_probes() {
    let tree = Tree::new("probe-sub-limits");
    let wizard = tree.wizard_with(|env| env.limits.probe_max_bytes = 40);
    let id = start(&wizard, "fake-agent");
    let out = ok(&wizard, "probe", json!({"session": id}));
    for run in out["runs"].as_array().unwrap() {
        assert!(run["text"].as_str().unwrap().len() <= 40, "{run}");
    }
    assert!(out["runs"].as_array().unwrap().len() <= 16);
}
