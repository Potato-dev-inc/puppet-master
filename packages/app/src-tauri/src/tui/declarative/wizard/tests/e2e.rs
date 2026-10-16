//! End to end with the fake CLI as the agent being learned and a stand-in drafter that replays
//! recorded drafts: start -> probe -> (draft -> verify -> live test)* -> finish. Zero tokens.

use serde_json::{json, Value};

use crate::tui::declarative::approvals::Approvals;
use crate::tui::declarative::check::check_report;
use crate::tui::declarative::loader::{build, LoadInputs};
use crate::tui::declarative::wizard::Wizard;

use super::support::{call, ok, start, Tree, GOOD, NO_PATH, WRONG_PATH};

/// Replays recorded replies, one per round, like a drafter that needs fix rounds.
struct StandInDrafter {
    replies: Vec<&'static str>,
    round: usize,
}

impl StandInDrafter {
    fn new(replies: &[&'static str]) -> Self {
        Self { replies: replies.to_vec(), round: 0 }
    }

    fn next(&mut self) -> &'static str {
        let reply = self.replies[self.round.min(self.replies.len() - 1)];
        self.round += 1;
        reply
    }
}

struct Result {
    rounds: usize,
    transcript: Vec<String>,
    finished: Value,
}

/// The same loop the front end runs (`lib/agentWizard.ts`), against the Rust steps.
fn run_loop(wizard: &Wizard, id: &str, drafter: &mut StandInDrafter, max_rounds: usize) -> Option<Result> {
    let mut transcript = Vec::new();
    for round in 1..=max_rounds {
        let toml = drafter.next();
        let v = ok(wizard, "verify", json!({"session": id, "toml": toml}));
        if v["ok"] != true {
            transcript.push(format!("round {round} verify: {}", v["text"].as_str().unwrap()));
            continue;
        }
        let t = ok(wizard, "livetest", json!({"session": id, "confirm": true}));
        if t["ok"] != true {
            transcript.push(format!("round {round} livetest: {}", t["text"].as_str().unwrap()));
            continue;
        }
        let finished = ok(
            wizard,
            "finish",
            json!({"session": id, "provenance": {"drafter": "Stand-in drafter", "drafter_kind": "other", "prompt_version": "1", "rounds": round}}),
        );
        return Some(Result { rounds: round, transcript, finished });
    }
    None
}

#[test]
fn a_good_first_draft_goes_from_command_to_unapproved_definition() {
    let tree = Tree::new("e2e-good");
    let wizard = tree.wizard();
    let started = ok(&wizard, "start", json!({"command": "fake-agent"}));
    let id = started["session"].as_str().unwrap().to_string();
    let probe = ok(&wizard, "probe", json!({"session": id}));
    assert!(probe["runs"][0]["text"].as_str().unwrap().contains("Commands:"));

    let result = run_loop(&wizard, &id, &mut StandInDrafter::new(&[GOOD]), 5).expect("finished");
    assert_eq!(result.rounds, 1);
    assert!(result.transcript.is_empty());
    assert_eq!(result.finished["id"], "fake_agent");

    let file = tree.agents.join("fake_agent.toml");
    let report = check_report(&file, None, &["claude", "codex"]).unwrap();
    assert_eq!(report.status, "ran");
    assert_eq!(report.results.len(), 4);
    assert!(report.results.iter().all(|r| r.passed), "{:?}", report.results);

    let ids = ["claude", "codex", "opencode", "opencode_native", "cursor", "cursor_agent", "cmd", "powershell", "bash"];
    let inputs = LoadInputs { bundled: &[], user_dir: Some(&tree.agents), code_ids: &ids, projects: &[], extra: &[] };
    let built = build(&inputs, &Approvals::load(&tree.root.join("approvals.json")));
    assert_eq!(built.entries[0].state.as_str(), "needs_approval");
    assert!(tree.agents.join("fake_agent.wizard.json").is_file());
    assert_eq!(wizard.session_count(), 0);
    assert_eq!(std::fs::read_dir(&tree.temp).unwrap().count(), 0, "temp folders are cleaned up");
}

#[test]
fn two_bad_drafts_are_fixed_from_the_checker_text_and_the_real_output() {
    let tree = Tree::new("e2e-fix");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    ok(&wizard, "probe", json!({"session": id}));

    // round 1: invalid; round 2: valid but reads the wrong field; round 3: right
    let result = run_loop(&wizard, &id, &mut StandInDrafter::new(&[NO_PATH, WRONG_PATH, GOOD]), 5).expect("finished");
    assert_eq!(result.rounds, 3);
    assert_eq!(result.transcript.len(), 2);
    assert!(result.transcript[0].contains("AGENT_DEF_INCONSISTENT at `headless.output.result.path`"), "{}", result.transcript[0]);
    assert!(result.transcript[1].contains("livetest") && result.transcript[1].contains("\"event\":\"end\""), "{}", result.transcript[1]);
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(tree.agents.join("fake_agent.wizard.json")).unwrap()).unwrap();
    assert_eq!(prov["rounds"], 3);
    assert_eq!(prov["drafter"]["name"], "Stand-in drafter");
    assert_eq!(check_report(&tree.agents.join("fake_agent.toml"), None, &["claude"]).unwrap().results.len(), 4);
}

#[test]
fn a_drafter_that_never_gets_it_right_stops_after_the_round_limit_and_writes_nothing() {
    let tree = Tree::new("e2e-giveup");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    ok(&wizard, "probe", json!({"session": id}));
    assert!(run_loop(&wizard, &id, &mut StandInDrafter::new(&[NO_PATH]), 5).is_none());
    assert!(!tree.agents.exists(), "nothing is written for a draft that never passed");
    ok(&wizard, "cancel", json!({"session": id}));
    assert_eq!(std::fs::read_dir(&tree.temp).unwrap().count(), 0);
}

#[test]
fn the_wizard_cannot_overwrite_a_definition_made_by_an_earlier_run() {
    let tree = Tree::new("e2e-twice");
    let wizard = tree.wizard();
    let first = start(&wizard, "fake-agent");
    ok(&wizard, "probe", json!({"session": first}));
    run_loop(&wizard, &first, &mut StandInDrafter::new(&[GOOD]), 5).expect("first run");
    let before = std::fs::read_to_string(tree.agents.join("fake_agent.toml")).unwrap();

    let second = start(&wizard, "fake-agent");
    ok(&wizard, "probe", json!({"session": second}));
    let v = ok(&wizard, "verify", json!({"session": second, "toml": GOOD}));
    assert_eq!(v["ok"], false);
    assert!(v["text"].as_str().unwrap().contains("WIZARD_ID_TAKEN"), "{}", v["text"]);
    assert_eq!(std::fs::read_to_string(tree.agents.join("fake_agent.toml")).unwrap(), before);
    let _ = call;
}
