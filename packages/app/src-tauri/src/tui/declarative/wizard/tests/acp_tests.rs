//! The wizard's chat (ACP) actions: `acp_detect` (text analysis of the probe output) and `acp_finish`
//! (an unapproved chat-only definition), plus a terminal draft that carries an `[acp]` table.

use serde_json::{json, Value};

use crate::tui::declarative::approvals::Approvals;
use crate::tui::declarative::build_for_test;
use crate::tui::declarative::loader::{build, LoadInputs};
use crate::tui::declarative::wizard::acp::detect_in_texts;
use crate::tui::declarative::wizard::probe::{ProbeReport, ProbeRunReport};
use crate::tui::declarative::wizard::{api, Wizard};
use crate::tui::TuiProfile;

use super::support::{call, ok, probed, provenance, start, tested, Tree, GOOD};

fn acp_body(id: &str, extra: Value) -> Value {
    let mut acp = json!({"command": "fake-agent-acp", "args": ["--stdio"], "env_allow": ["FAKE_HOME"]});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        acp[k] = v;
    }
    json!({"session": id, "label": "Fake Chat", "acp": acp})
}

fn with_help(wizard: &Wizard, id: &str, help: &str) {
    let report = ProbeReport {
        runs: vec![ProbeRunReport {
            label: "--help".into(),
            args: vec!["--help".into()],
            exit: Some(0),
            timed_out: false,
            truncated: false,
            bytes: help.len(),
            duplicate_of: None,
            text: help.into(),
            error: None,
        }],
        subcommands: Vec::new(),
        skipped: Vec::new(),
    };
    wizard.end(id, |s| s.probe = Some(report)).unwrap();
}

// ---- detect ---------------------------------------------------------------------------------------------

#[test]
fn a_subcommand_row_is_a_high_candidate() {
    let help = "Usage: mycoder [command]\n\nCommands:\n  mycoder run      run a task\n  mycoder acp      Start as an Agent Client Protocol server\n";
    let d = detect_in_texts("mycoder", &[help]);
    assert_eq!(d.candidates.len(), 1, "{d:?}");
    assert_eq!(d.candidates[0].args, ["acp"]);
    assert!(d.candidates[0].high);
    assert!(d.candidates[0].evidence.contains("acp"));
    assert!(d.mentions_protocol);
    let clap = detect_in_texts("mycoder", &["Commands:\n  run   run a task\n  acp   Run over the protocol\n"]);
    assert_eq!(clap.candidates[0].args, ["acp"]);
}

#[test]
fn flags_are_found_and_the_exact_ones_rank_first() {
    let help = "Options:\n  --acp-port <N>   port for the protocol\n  --experimental-acp   Run as an ACP agent\n  --no-acp  never\n";
    let d = detect_in_texts("mycoder.exe", &[help]);
    let args: Vec<_> = d.candidates.iter().map(|c| c.args.clone()).collect();
    assert_eq!(args, vec![vec!["--experimental-acp".to_string()], vec!["--acp-port".to_string()]]);
    assert!(d.candidates[0].high && !d.candidates[1].high);
}

#[test]
fn stdio_is_a_hint_and_joins_a_candidate_only_on_its_own_line() {
    let help = "  --acp --stdio   speak over stdin and stdout\n  --stdio   something else\n";
    let d = detect_in_texts("x", &[help]);
    assert!(d.stdio_hint.is_some());
    assert_eq!(d.candidates[0].args, ["--acp", "--stdio"]);
    let plain = detect_in_texts("x", &["Options:\n  --acp   Run as ACP\n  --stdio   use stdio\n"]);
    assert_eq!(plain.candidates[0].args, ["--acp"]);
    assert!(plain.stdio_hint.is_some());
}

#[test]
fn the_protocol_named_in_prose_alone_gives_no_candidate() {
    let d = detect_in_texts("x", &["This tool can talk the Agent Client Protocol when asked by an editor.\nUsage: x [options]\n"]);
    assert!(d.candidates.is_empty() && d.mentions_protocol && d.stdio_hint.is_none());
    assert!(detect_in_texts("x", &["nothing here"]).candidates.is_empty());
}

#[test]
fn detect_needs_a_probe_and_reads_only_the_stored_text() {
    let tree = Tree::new("acp-detect");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    let err = call(&wizard, "acp_detect", json!({"session": id})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_BAD_STATE"));
    ok(&wizard, "probe", json!({"session": id}));
    let out = ok(&wizard, "acp_detect", json!({"session": id}));
    assert_eq!((out["ok"].clone(), out["command"].clone()), (json!(true), json!("fake-agent")));
    assert_eq!(out["candidates"], json!([]), "the fake program does not offer ACP: an empty answer is normal");
    with_help(&wizard, &id, "Commands:\n  acp   Agent Client Protocol server over stdio\n");
    let out = ok(&wizard, "acp_detect", json!({"session": id}));
    assert_eq!(out["candidates"][0]["command"], "fake-agent");
    assert_eq!(out["candidates"][0]["args"], json!(["acp"]));
    assert_eq!(out["candidates"][0]["confidence"], "high");
    assert!(out["candidates"][0]["evidence"].as_str().unwrap().contains("acp"));
    assert!(call(&wizard, "acp_detect", json!({})).is_err());
}

// ---- finish ---------------------------------------------------------------------------------------------

#[test]
fn acp_finish_writes_an_unapproved_chat_only_definition() {
    let tree = Tree::new("acp-finish");
    let wizard = tree.wizard();
    let id = probed(&wizard, "fake-agent");
    let out = ok(&wizard, "acp_finish", acp_body(&id, json!({"resume_command": ["--resume", "{session}"]})));
    assert_eq!((out["id"].clone(), out["state"].clone(), out["chat_only"].clone()), (json!("fake_agent"), json!("needs_approval"), json!(true)));
    let text = std::fs::read_to_string(tree.agents.join("fake_agent.toml")).unwrap();
    assert!(!text.contains("[headless") && !text.contains("[tui"), "{text}");
    let profile = build_for_test(&text).expect("it loads");
    let table = profile.acp_table().expect("the chat lane's table");
    assert_eq!((table.id.as_str(), table.label.as_str(), table.command.as_str()), ("fake_agent", "Fake Chat", "fake-agent-acp"));
    assert_eq!(table.args, ["--stdio"]);
    assert_eq!(table.env_allow, ["FAKE_HOME"]);
    assert_eq!(table.resume_command, ["--resume", "{session}"]);
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(tree.agents.join("fake_agent.wizard.json")).unwrap()).unwrap();
    assert_eq!(prov["drafter"], json!({"name": "wizard-acp", "kind": "other", "model": null}));
    assert!(!tree.agents.join("fake_agent").exists(), "a chat-only definition has no fixtures");
    assert!(!tree.root.join("agent-approvals.json").exists(), "never approved");
    let ids = vec!["claude", "codex"];
    let built = build(
        &LoadInputs { bundled: &[], user_dir: Some(&tree.agents), code_ids: &ids, projects: &[], extra: &[] },
        &Approvals::load(&tree.root.join("agent-approvals.json")),
    );
    assert_eq!((built.entries.len(), built.entries[0].state.as_str()), (1, "needs_approval"));
    assert!(built.profiles.is_empty());
    assert_eq!(wizard.session_count(), 0, "the session is finished");
}

#[test]
fn an_explicit_id_is_used_and_an_existing_one_is_never_overwritten() {
    let tree = Tree::new("acp-id");
    let wizard = tree.wizard();
    let id = probed(&wizard, "fake-agent");
    let mut body = acp_body(&id, json!({}));
    body["id"] = json!("fake_chat");
    assert_eq!(ok(&wizard, "acp_finish", body)["id"], "fake_chat");
    let id2 = probed(&wizard, "fake-agent");
    let mut again = acp_body(&id2, json!({}));
    again["id"] = json!("fake_chat");
    let err = call(&wizard, "acp_finish", again).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_ID_TAKEN"));
    // the session survives and can finish under another id
    let mut other = acp_body(&id2, json!({}));
    other["id"] = json!("fake_chat_two");
    assert_eq!(ok(&wizard, "acp_finish", other)["ok"], true);
}

#[test]
fn the_validators_own_message_comes_back_and_nothing_is_written() {
    let tree = Tree::new("acp-refuse");
    let wizard = tree.wizard();
    let id = probed(&wizard, "fake-agent");
    let err = call(&wizard, "acp_finish", acp_body(&id, json!({"command": "node", "args": ["adapter.js"]}))).unwrap_err();
    assert_eq!(err.code, "AGENT_DEF_SHELL_INTERPRETER", "{err}");
    assert!(err.message.contains("allow_interpreter"), "{}", err.message);
    let err = call(&wizard, "acp_finish", acp_body(&id, json!({"env_allow": ["PATH"]}))).unwrap_err();
    assert!(err.code.starts_with("AGENT_DEF"), "{err}");
    let err = call(&wizard, "acp_finish", acp_body(&id, json!({"env_allow": ["bad name"]}))).unwrap_err();
    assert!(err.code.starts_with("AGENT_DEF"), "{err}");
    let err = call(&wizard, "acp_finish", acp_body(&id, json!({"resume_command": ["--resume"]}))).unwrap_err();
    assert_eq!(err.status, 400);
    let err = call(&wizard, "acp_finish", acp_body(&id, json!({"args": ["--token=sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"]}))).unwrap_err();
    assert_eq!(err.status, 400, "{err}");
    let mut no_label = acp_body(&id, json!({}));
    no_label["label"] = json!("  ");
    assert_eq!(call(&wizard, "acp_finish", no_label).unwrap_err().status, 400);
    assert!(!tree.agents.join("fake_agent.toml").exists() && !tree.agents.join("fake_agent.wizard.json").exists());
    assert_eq!(wizard.session_count(), 1, "a refused finish keeps the session");
    assert_eq!(ok(&wizard, "acp_finish", acp_body(&id, json!({})))["ok"], true);
}

#[test]
fn both_actions_are_routes_and_a_session_is_required() {
    assert!(api::ACTIONS.contains(&"acp_detect") && api::ACTIONS.contains(&"acp_finish"));
    let tree = Tree::new("acp-routes");
    let wizard = tree.wizard();
    assert_eq!(call(&wizard, "acp_finish", json!({})).unwrap_err().status, 400);
    assert_eq!(call(&wizard, "acp_detect", json!({"session": "wz_none"})).unwrap_err().status, 404);
}

#[test]
fn a_terminal_draft_with_an_acp_table_verifies_and_finishes() {
    let tree = Tree::new("acp-both");
    let wizard = tree.wizard();
    let draft = format!("{}\n[acp]\ncommand = \"fake-agent-acp\"\nargs = [\"--stdio\"]\nenv_allow = [\"FAKE_HOME\"]\n", GOOD.trim_end());
    let id = tested(&wizard, "fake-agent", &draft);
    let out = ok(&wizard, "finish", json!({"session": id, "provenance": provenance()}));
    assert_eq!((out["ok"].clone(), out["state"].clone()), (json!(true), json!("needs_approval")));
    let text = std::fs::read_to_string(tree.agents.join("fake_agent.toml")).unwrap();
    assert_eq!(text, draft, "the verified text, byte for byte");
    let profile = build_for_test(&text).unwrap();
    assert_eq!(profile.acp_table().unwrap().command, "fake-agent-acp");
}

#[test]
fn a_bad_acp_table_in_a_terminal_draft_is_a_verify_problem() {
    let tree = Tree::new("acp-both-bad");
    let wizard = tree.wizard();
    let id = probed(&wizard, "fake-agent");
    let draft = format!("{}\n[acp]\ncommand = \"node\"\n", GOOD.trim_end());
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft}));
    assert_eq!(v["ok"], false);
    assert!(v.to_string().contains("AGENT_DEF_SHELL_INTERPRETER"), "{v}");
}

/// Detection is by patterns only: the shipped code of `wizard/acp.rs` names no agent.
#[test]
fn the_acp_wizard_code_names_no_agent() {
    let source = include_str!("../acp.rs");
    let shipped: String = source.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n").to_lowercase();
    for name in ["codex", "claude", "gemini", "cursor", "opencode", "kilo", "copilot", "nuphos", "openab", "kiro"] {
        assert!(!shipped.contains(name), "wizard/acp.rs names an agent ({name})");
    }
}

// ---- the real `opencode --help` shape: ASCII art first, an `acp` row among the commands -------------------

const ART_HELP: &str = "\u{2800}                                \u{2584}     \n\u{2588}\u{2580}\u{2580}\u{2588} \u{2588}\u{2580}\u{2580}\u{2588} \u{2588}\u{2580}\u{2580}\u{2588} \u{2588}\u{2580}\u{2580}\u{2584} \u{2588}\u{2580}\u{2580}\u{2580} \u{2588}\u{2580}\u{2580}\u{2588} \u{2588}\u{2580}\u{2580}\u{2588} \u{2588}\u{2580}\u{2580}\u{2588}\n\u{2588}  \u{2588} \u{2588}  \u{2588} \u{2588}\u{2580}\u{2580}\u{2580} \u{2588}  \u{2588} \u{2588}    \u{2588}  \u{2588} \u{2588}  \u{2588} \u{2588}\u{2580}\u{2580}\u{2580}\n\u{2580}\u{2580}\u{2580}\u{2580} \u{2588}\u{2580}\u{2580}\u{2580} \u{2580}\u{2580}\u{2580}\u{2580} \u{2580}  \u{2580} \u{2580}\u{2580}\u{2580}\u{2580} \u{2580}\u{2580}\u{2580}\u{2580} \u{2580}\u{2580}\u{2580}\u{2580} \u{2580}\u{2580}\u{2580}\u{2580}\n\nCommands:\n  opencode completion          generate shell completion script\n  opencode acp                 start ACP (Agent Client Protocol) server\n  opencode mcp                 manage MCP (Model Context Protocol) servers\n  opencode [project]           start opencode tui                                          [default]\n  opencode run [message..]     run opencode with a message\n\nOptions:\n  -h, --help          show help                                                            [boolean]\n  -v, --version       show version number                                                  [boolean]\n";

#[test]
fn help_with_ascii_art_and_a_program_prefixed_row_gives_one_high_candidate() {
    for name in ["opencode", "opencode.cmd", "OpenCode.exe"] {
        let d = detect_in_texts(name, &[ART_HELP]);
        assert_eq!(d.candidates.len(), 1, "{name}: {d:?}");
        assert_eq!(d.candidates[0].args, ["acp"]);
        assert!(d.candidates[0].high);
        assert!(d.mentions_protocol);
    }
}

#[cfg(windows)]
#[test]
fn detect_program_reads_help_through_a_cmd_shim_without_a_session() {
    use crate::tui::declarative::wizard::acp::detect_program;
    let tree = Tree::new("acp-detect-cmd");
    let dir = tree.root.join("shimbin");
    std::fs::create_dir_all(&dir).unwrap();
    let body = ART_HELP.lines().map(|l| format!("echo({}", l.replace('[', "^[").replace(']', "^]").replace('(', "^(").replace(')', "^)"))).collect::<Vec<_>>().join("\r\n");
    std::fs::write(dir.join("shimagent.cmd"), format!("@echo off\r\n{body}\r\n")).unwrap();
    let wizard = tree.wizard_with({
        let dir = dir.clone();
        move |e| e.path_dirs = Some(vec![dir.clone()])
    });
    let out = detect_program(&wizard, "shimagent").expect("detect");
    assert!(out["candidates"].as_array().unwrap().is_empty(), "the fake prints no `shimagent acp` row: {out}");
    assert_eq!(out["mentions_protocol"], true);
    assert!(crate::tui::declarative::wizard::acp::detect_program(&wizard, "no-such-program-zz").is_err());
}
