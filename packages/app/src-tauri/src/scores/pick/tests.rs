//! Guards and end-to-end tests of "Pick for me". The guards read the shipped source as text, so a
//! change that gives an agent a way to reach the pick, lets the module send, write or log anything,
//! or names a real agent in generic code fails here.
//!
//! - `no_task_and_no_network`: no task or prompt word, no http client, no socket and no process
//!   spawn in the pick module's code (comments and string contents are not searched).
//! - `not_reachable_from_agents`: `pick_agent` and `scores::pick` are named only by `lib.rs`,
//!   `commands/scores.rs` and `scores/`. `bridge/`, `tool_registry/` and the other agent-facing
//!   places are among the files scanned.
//! - `the_command_takes_only_the_folder`: the Tauri command has one parameter, the folder path.
//! - `nothing_written_or_logged`: no file write, no log or print macro, no audit, no undo log.
//! - `generic_code_names_no_agent`: no real agent name in the pick module's code.
//! - `pick_has_no_private_text`: private text planted in the inputs never reaches the `Pick`.
//! - `end_to_end_over_scores_rows`: rows built by `scores::build_in` feed the pick, with golden
//!   sentences for a clear gap, a close call, one limit and too few runs.
//!
//! The source scans are plain text scans, the same as `scores/tests.rs`.

use super::live::{pick_for_in, AgentFact, PickIo};
use super::model::{Basis, Metric, Pick, Usual, MAX_CONSIDERED, MAX_LABEL_CHARS, METRIC_ORDER, PICK_DAYS};
use crate::project_path::comparable_project_path;
use crate::scores::aggregate::Labels;
use crate::scores::model::{End, Sample, ScoreRow, ScoreTurn};
use crate::scores::{build_in, Inputs};
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const DAY: u64 = 86_400_000;
/// 2027-01-15 08:00 UTC.
const NOW: u64 = 1_800_000_000_000;

#[test]
fn scaffold_constants_match_the_spec() {
    assert_eq!(PICK_DAYS, 90);
    assert_eq!(MAX_CONSIDERED, 24);
    assert_eq!(MAX_LABEL_CHARS, 40);
    assert_eq!(METRIC_ORDER, [Metric::ChecksGreen, Metric::Merged, Metric::EndedOk]);
}

// ---- source scans (the same helpers as `scores/tests.rs`) ----

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir` as (path relative to `src/` with `/` separators, text), sorted.
fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir).expect("read dir").filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let rel = path.strip_prefix(src_dir()).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            out.push((rel, fs::read_to_string(&path).expect("read source")));
        }
    }
}

/// Every `.rs` file of the pick module.
fn pick_sources() -> Vec<(String, String)> {
    let mut files = Vec::new();
    sources(&src_dir().join("scores").join("pick"), &mut files);
    assert!(files.len() >= 9, "the pick module has only {} files", files.len());
    files
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `needle` occurs in `line` and does not start in the middle of a longer identifier.
fn occurs_at_word_start(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(at, _)| line[..at].chars().next_back().is_none_or(|c| !is_ident(c)))
}

/// `needle` occurs in `line` as a whole word (no identifier character on either side).
fn occurs_as_word(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(at, _)| {
        let before = line[..at].chars().next_back().is_none_or(|c| !is_ident(c));
        let after = line[at + needle.len()..].chars().next().is_none_or(|c| !is_ident(c));
        before && after
    })
}

/// `text` with `//` and `/* */` comments and the contents of string and char literals blanked to
/// spaces, so only code is left. Line breaks are kept, so line numbers hold. Simple on purpose:
/// no nested block comments, and an escaped quote in a char literal is not handled.
fn code_only(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                blank(&mut out, c[i]);
                i += 1;
            }
            for _ in 0..2 {
                if i < c.len() {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
        } else if ch == 'r' && raw_hashes(&c, i).is_some() {
            let n = raw_hashes(&c, i).unwrap_or(0);
            let mut j = i + n + 2; // past `r`, the hashes and the opening quote
            while j < c.len() && !(c[j] == '"' && (1..=n).all(|k| c.get(j + k) == Some(&'#'))) {
                j += 1;
            }
            let end = (j + n + 1).min(c.len());
            for &x in &c[i..end] {
                blank(&mut out, x);
            }
            i = end;
        } else if ch == '"' {
            blank(&mut out, ch);
            i += 1;
            while i < c.len() && c[i] != '"' {
                let escaped = c[i] == '\\';
                blank(&mut out, c[i]);
                i += 1;
                if escaped && i < c.len() {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
            if i < c.len() {
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '\'' && c.get(i + 1) == Some(&'\\') {
            let mut j = i + 2;
            while j < c.len() && c[j] != '\'' {
                j += 1;
            }
            let end = (j + 1).min(c.len());
            for &x in &c[i..end] {
                blank(&mut out, x);
            }
            i = end;
        } else if ch == '\'' && c.get(i + 2) == Some(&'\'') {
            for &x in &c[i..i + 3] {
                blank(&mut out, x);
            }
            i += 3;
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// The number of `#` of a raw string opening (`r"` or `r#"`...) at `at`, when `r` starts a token.
fn raw_hashes(c: &[char], at: usize) -> Option<usize> {
    if at > 0 && is_ident(c[at - 1]) {
        return None;
    }
    let mut j = at + 1;
    while c.get(j) == Some(&'#') {
        j += 1;
    }
    (c.get(j) == Some(&'"')).then_some(j - at - 1)
}

/// `(file:line  needle)` of every needle found in the code of the pick module. `whole_word`
/// matches a whole word; otherwise the needle only has to start a word (a macro or a path).
fn code_hits(needles: &[&str], whole_word: bool) -> Vec<String> {
    let mut hits = Vec::new();
    for (file, text) in pick_sources() {
        for (index, line) in code_only(&text).lines().enumerate() {
            for needle in needles {
                let found = if whole_word { occurs_as_word(line, needle) } else { occurs_at_word_start(line, needle) };
                if found {
                    hits.push(format!("{file}:{}  `{needle}`", index + 1));
                }
            }
        }
    }
    hits
}

// ---- 1. no task, no prompt, no network ----

/// Words of a task or a prompt, an http client, a socket and a process spawn. None may be code in
/// the pick module: it works on local numbers only and never sees what the person typed.
const FORBIDDEN_WORDS: &[&str] = &[
    "task", "prompt", "quick_ask", "run_ask", "reqwest", "ureq", "hyper", "isahc", "attohttpc", "minreq", "surf", "curl",
    "http", "tungstenite", "TcpStream", "TcpListener", "UdpSocket", "std::net", "tokio::net", "Command::new",
];

#[test]
fn no_task_and_no_network() {
    let hits = code_hits(FORBIDDEN_WORDS, true);
    assert!(hits.is_empty(), "the pick names a task, a prompt, a network or a process:\n  {}", hits.join("\n  "));
}

/// The scanner itself: a comment and a string are not code; an import is.
#[test]
fn the_code_scan_ignores_comments_and_strings() {
    let fixture = "// a task in a comment\nlet text = \"prompt\";\nlet client = reqwest::Client::new();\n";
    let code = code_only(fixture);
    let mut hits: Vec<(usize, &str)> = Vec::new();
    for (index, line) in code.lines().enumerate() {
        for needle in FORBIDDEN_WORDS {
            if occurs_as_word(line, needle) {
                hits.push((index + 1, needle));
            }
        }
    }
    assert_eq!(hits, [(3, "reqwest")], "{hits:?}");
}

// ---- 2. not reachable from agents ----

/// Names that reach the pick: the command, the entry functions and a route-looking string.
const PICK_NAMES: &[&str] = &["pick_agent", "pick_for_folder", "pick_for_in", "scores::pick", "\"/pick", "pick-agent"];

/// Files outside `scores/` that may name the pick: its registration and its command.
const MAY_NAME_PICK: &[&str] = &["lib.rs", "commands/scores.rs"];

/// `(line, needle)` of every pick name in `text`. Comment lines are skipped (a comment routes
/// nothing); string contents are not, because a route is a string.
fn pick_hits(text: &str) -> Vec<(usize, &'static str)> {
    let mut hits = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for &needle in PICK_NAMES {
            if occurs_at_word_start(line, needle) {
                hits.push((index + 1, needle));
            }
        }
    }
    hits
}

#[test]
fn not_reachable_from_agents() {
    let mut files = Vec::new();
    sources(&src_dir(), &mut files);
    assert!(files.len() > 100, "the scan found only {} files; is the source tree layout different?", files.len());
    // The places an agent can reach must be among the files checked, or the guard proves nothing.
    for must in ["tool_registry/", "bridge/", "companion/", "one_brain/", "context_pack.rs"] {
        assert!(files.iter().any(|(file, _)| file.starts_with(must)), "{must} is not scanned");
    }
    let mut hits = Vec::new();
    for (file, text) in &files {
        if file.starts_with("scores/") || MAY_NAME_PICK.contains(&file.as_str()) {
            continue;
        }
        for (line, needle) in pick_hits(text) {
            hits.push(format!("{file}:{line}  `{needle}`"));
        }
    }
    assert!(
        hits.is_empty(),
        "the pick is reachable outside the Tauri command (agents must never see it):\n  {}\n\
         Only lib.rs, commands/scores.rs and scores/ may name it.",
        hits.join("\n  ")
    );
}

/// The route scan itself: a path, a route string and a name are seen; a comment and a longer name
/// are not.
#[test]
fn the_route_scan_sees_routes_and_paths_and_skips_comments() {
    let fixture = "\n// pick_agent is named in this comment only\nuse crate::scores::pick::model::Pick;\nlet route = \"/pick-agent/run\";\nlet my_pick_agent_count = 1;\n";
    assert_eq!(pick_hits(fixture), [(3, "scores::pick"), (4, "\"/pick"), (4, "pick-agent")]);
}

/// The command takes the folder path and nothing else: no task, no text of the person.
#[test]
fn the_command_takes_only_the_folder() {
    let text = fs::read_to_string(src_dir().join("commands").join("scores.rs")).expect("commands/scores.rs");
    assert_eq!(text.matches("pub async fn pick_agent").count(), 1, "one pick_agent command");
    const HEAD: &str = "pub async fn pick_agent(";
    let open = text.find(HEAD).expect("pick_agent is declared in commands/scores.rs") + HEAD.len();
    let close = open + text[open..].find(')').expect("the parameter list ends");
    assert_eq!(text[open..close].trim(), "project: String", "the command takes more than the folder path");
}

// ---- 3. nothing written, nothing logged ----

/// Writes, logs, prints, audit entries and the undo log. None may appear in the pick module's code.
/// Reading is allowed (the tests read the source files), so only the write calls are listed.
const FORBIDDEN_EFFECTS: &[&str] = &[
    "fs::write", "fs::copy", "fs::rename", "fs::remove", "fs::create", "fs::set_permissions", "File::create", "File::options",
    "OpenOptions", "write_all", "create_dir", "remove_file", "remove_dir", "println!", "eprintln!", "print!", "eprint!",
    "dbg!", "info!", "warn!", "error!", "debug!", "trace!", "log::", "tracing::", "audit", "undo_log", "append",
];

#[test]
fn nothing_written_or_logged() {
    let hits = code_hits(FORBIDDEN_EFFECTS, false);
    assert!(hits.is_empty(), "the pick writes or logs something:\n  {}", hits.join("\n  "));
}

// ---- 4. generic code names no agent ----

/// Real agent names. Generic code uses ids such as `fake_a`; the registry supplies the labels.
const AGENT_NAMES: &[&str] = &["claude", "codex", "gemini", "kilo", "opencode", "aider", "copilot", "cursor"];

#[test]
fn generic_code_names_no_agent() {
    let mut hits = Vec::new();
    for (file, text) in pick_sources() {
        for (index, line) in code_only(&text).to_lowercase().lines().enumerate() {
            for name in AGENT_NAMES {
                if occurs_as_word(line, name) {
                    hits.push(format!("{file}:{}  `{name}`", index + 1));
                }
            }
        }
    }
    assert!(hits.is_empty(), "generic pick code names a real agent:\n  {}", hits.join("\n  "));
}

// ---- inputs for the privacy and end-to-end tests ----

const FOLDER: &str = r"C:\Users\you\proj";
const SECRET_FOLDER: &str = r"C:\Users\you\secret-proj";
const SECRET_PANE: &str = "pane-secret-9";
const SECRET_PROMPT: &str = "SECRET-PROMPT fix the login with sk-test-FAKE-0001";
const SECRET_HANDLE: &str = "run-secret-handle";
const EMAIL: &str = "you@example.com";

fn agent_label(id: &str) -> String {
    id.to_string()
}

fn no_account(_: &str) -> Option<String> {
    None
}

fn labels() -> Labels<'static> {
    Labels { agent: &agent_label, account: &no_account }
}

/// `checks.1` finished turns of `agent` in the project `key`; the first `checks.0` had checks green
/// and all ended OK. `account` is the saved login the turns ran under (`None` = the agent's own).
fn seeded(key: &str, agent: &str, account: Option<&str>, checks: (u32, u32)) -> Vec<ScoreTurn> {
    let (green, total) = checks;
    (0..total)
        .map(|i| {
            let started_ms = NOW - DAY + u64::from(i) * 120_000;
            ScoreTurn {
                project: key.to_string(),
                agent_id: agent.to_string(),
                account: account.map(str::to_string),
                pane_id: Some(SECRET_PANE.to_string()),
                started_ms,
                ended_ms: started_ms + 60_000,
                end: End::Ok,
                files_changed: None,
                lines: None,
                asked_you: None,
                checks_green: Some(i < green),
                undone: None,
            }
        })
        .collect()
}

/// The Scores rows of these turns, for the last 90 days, built by the same roll-up as the page.
fn score_rows(turns: Vec<ScoreTurn>) -> Vec<ScoreRow> {
    let inputs = Inputs { turns, ..Inputs::default() };
    build_in(&inputs, None, Some(PICK_DAYS), NOW, &Utc, &labels()).rows
}

/// The facts the pick reads from the app, as a fake. Only generic ids (`fake_a`, `fake_b`).
#[derive(Default)]
struct Fake {
    agents: Vec<AgentFact>,
    rows: Vec<ScoreRow>,
    /// (agent, login) -> limit: `Some(Some(ms))` out until then, `Some(None)` out with no time.
    limits: BTreeMap<(String, Option<String>), Option<u64>>,
    /// Saved logins that may run: (agent, login id).
    logins: BTreeSet<(String, String)>,
    /// Saved login id -> label, as the accounts store returns it.
    login_labels: BTreeMap<String, String>,
    /// The Ask bar default agent.
    preferred: Option<String>,
}

impl Fake {
    fn new(agents: Vec<AgentFact>, rows: Vec<ScoreRow>) -> Self {
        Self { agents, rows, ..Self::default() }
    }

    fn set_limit(&mut self, agent: &str, login: Option<&str>, until: Option<u64>) {
        self.limits.insert((agent.to_string(), login.map(str::to_string)), until);
    }

    fn add_login(&mut self, agent: &str, id: &str, label: &str) {
        self.logins.insert((agent.to_string(), id.to_string()));
        self.login_labels.insert(id.to_string(), label.to_string());
    }
}

impl PickIo for Fake {
    fn agents(&self) -> Vec<AgentFact> {
        self.agents.clone()
    }

    fn runnable(&self, agent: &AgentFact, _folder: &str) -> Result<(), String> {
        if agent.installed {
            Ok(())
        } else {
            Err("Not installed on this computer".to_string())
        }
    }

    fn account_ok(&self, agent_id: &str, account_id: &str) -> bool {
        self.logins.contains(&(agent_id.to_string(), account_id.to_string()))
    }

    fn account_label(&self, account_id: &str) -> Option<String> {
        self.login_labels.get(account_id).cloned()
    }

    fn limit(&self, agent_id: &str, account: Option<&str>) -> Option<Option<u64>> {
        self.limits.get(&(agent_id.to_string(), account.map(str::to_string))).copied()
    }

    fn rows(&self, _folder: &str) -> Result<Vec<ScoreRow>, String> {
        Ok(self.rows.clone())
    }

    fn usual(&self) -> Usual {
        Usual { preferred: self.preferred.clone(), last: None }
    }

    fn when(&self, _ms: u64) -> String {
        "17:00".to_string()
    }
}

fn agent(id: &str) -> AgentFact {
    AgentFact {
        id: id.to_string(),
        label: id.to_string(),
        installed: true,
        dispatchable: true,
        cli: true,
        vendor: None,
    }
}

fn chosen_id(pick: &Pick) -> Option<&str> {
    pick.choice.as_ref().map(|c| c.agent_id.as_str())
}

// ---- 5. no private text in the answer ----

#[test]
fn pick_has_no_private_text() {
    // Planted in the inputs: a folder with a user name, a pane id on every turn, a prompt-like text
    // and a run handle in the Scores rows' own label fields, and an e-mail-like saved login.
    let key = comparable_project_path(Path::new(SECRET_FOLDER));
    let mut turns = seeded(&key, "fake_a", Some("acc-work"), (10, 10));
    turns.extend(seeded(&key, "fake_b", None, (5, 10)));
    let mut rows = score_rows(turns);
    for row in &mut rows {
        row.agent_label = SECRET_PROMPT.to_string();
        row.account_label = SECRET_HANDLE.to_string();
    }
    let mut io = Fake::new(vec![agent("fake_a"), agent("fake_b")], rows);
    io.add_login("fake_a", "acc-work", EMAIL);

    let pick = pick_for_in(&io, SECRET_FOLDER);
    // The pick really used the planted history and login, so the absence below means something.
    assert_eq!(chosen_id(&pick), Some("fake_a"));
    assert_eq!(pick.basis, Basis::Clear);
    let json = serde_json::to_string(&pick).expect("pick json");
    assert!(pick.choice.as_ref().is_some_and(|c| c.label.contains(EMAIL)), "the login label is shown: {json}");

    // Only the login label the accounts store returned may show; everything else is absent.
    let rest = json.replace(EMAIL, "");
    for secret in ["Users", "secret", "C:", "\\", "pane", SECRET_PANE, "SECRET", "handle", "run-", "@", "sk-test", "FAKE-0001"] {
        assert!(!rest.contains(secret), "{secret} leaked into the answer: {json}");
    }
}

// ---- 6. end to end over real Scores rows ----

#[test]
fn end_to_end_over_scores_rows() {
    let key = comparable_project_path(Path::new(FOLDER));
    // Two agents with the given (checks green, turns) each, in one project, as Scores rows.
    let rows_of = |a: (u32, u32), b: (u32, u32)| {
        let mut turns = seeded(&key, "fake_a", None, a);
        turns.extend(seeded(&key, "fake_b", None, b));
        score_rows(turns)
    };
    // The usual agent is fake_b in every case below.
    let usual_b = |rows: Vec<ScoreRow>| {
        let mut io = Fake::new(vec![agent("fake_a"), agent("fake_b")], rows);
        io.preferred = Some("fake_b".to_string());
        io
    };

    // Clear: 10 of 10 against 5 of 10. The spec's example, 9 of 10 against 5 of 10, is NOT a clear
    // gap under `stats::ahead`: the 90% interval of 9 of 10 starts near 0.65, below the top of
    // 5 of 10 (near 0.73). So the clear case uses 10 of 10 here, and the close call below uses 9 of
    // 10. The rule and the 5-run minimum are unchanged.
    let rows = rows_of((10, 10), (5, 10));
    let row_a = rows.iter().find(|r| r.agent_id == "fake_a").expect("a row for fake_a");
    assert_eq!((row_a.turns, row_a.checks_green.count, row_a.checks_green.total), (10, 10, 10));
    let pick = pick_for_in(&usual_b(rows), FOLDER);
    assert_eq!(chosen_id(&pick), Some("fake_a"));
    assert_eq!(pick.basis, Basis::Clear);
    assert_eq!(pick.metric, Some(Metric::ChecksGreen));
    assert_eq!(pick.sample, Sample::Steady, "no early chip at 10 runs");
    assert_eq!(pick.reason, "fake_a passed its checks 10 of 10 times here; fake_b 5 of 10.");
    assert_eq!(pick.considered[1].line, "5 of 10 checks green");
    assert_eq!(pick.notes.len(), 3);

    // Close call: 9 of 10 against 5 of 10 (the intervals overlap). The usual agent is picked.
    let pick = pick_for_in(&usual_b(rows_of((9, 10), (5, 10))), FOLDER);
    assert_eq!(pick.basis, Basis::CloseCall);
    assert_eq!(chosen_id(&pick), Some("fake_b"));
    assert_eq!(pick.reason, "Your agents look about the same here so far, so your usual one, fake_b.");
    assert_eq!(pick.sample, Sample::None);

    // One limit: fake_a (the 10 of 10 agent) is out of usage. fake_b is picked and the sentence
    // names fake_a with its reset time.
    let mut io = usual_b(rows_of((10, 10), (5, 10)));
    io.set_limit("fake_a", None, Some(1_000));
    let pick = pick_for_in(&io, FOLDER);
    assert_eq!(chosen_id(&pick), Some("fake_b"));
    assert_eq!(pick.basis, Basis::OnlyOne);
    assert_eq!(pick.reason, "Only fake_b is ready, because fake_a is out of usage until 17:00.");

    // Too few runs: 4 of 4 each is under the 5-run minimum, so the usual agent is picked.
    let pick = pick_for_in(&usual_b(rows_of((4, 4), (4, 4))), FOLDER);
    assert_eq!(pick.basis, Basis::TooEarly);
    assert_eq!(chosen_id(&pick), Some("fake_b"));
    assert_eq!(pick.reason, "Too early to tell your agents apart here, so your usual one, fake_b.");
}
