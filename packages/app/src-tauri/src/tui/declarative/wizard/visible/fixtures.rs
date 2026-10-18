//! The captured screens as fixtures (SPEC 11.6): `tui_*.txt` files with their `.expect.toml`, and the
//! masked copy the drafter sees. The expectation file is MERGED from two sources:
//!
//! - Rust writes what the capture proves (an idle screen is not busy and has no prompt, a working
//!   screen is busy, the reply row is not chrome, a dialog is a prompt). A drafter cannot weaken these.
//! - The drafter may add `chrome_lines`, `not_chrome_lines`, `model`, `menu` and, for dialogs,
//!   `prompt_kind`, each checked against the capture it belongs to.
//!
//! The fixtures on disk keep the real test words; everything shown to an AI goes through `mask_words`.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::tui::declarative::approvals::now_rfc3339;
use crate::tui::declarative::DeclarativeProfile;
use crate::tui::TuiProfile;

use super::super::fixtures::FixtureFile;
use super::super::livetest::mask_words;
use super::super::session::TestWords;
use super::capture::{reply_rows, CapFrame, CaptureSet};
use super::screen::rows;

/// The captures the wizard can make, in the order they are written.
pub const NAMES: [&str; 9] = [
    "tui_trust",
    "tui_firstrun",
    "tui_idle",
    "tui_working",
    "tui_reply",
    "tui_work_working",
    "tui_work_reply",
    "tui_permission",
    "tui_permission_command",
];

/// What a capture is, for the expectation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Idle,
    Working,
    Reply,
    /// The reply to the task that made the agent think and use a tool: the answer sentence is the only row left once
    /// the profile has dropped its chrome, thinking, summary and tool rows.
    WorkReply,
    /// A dialog the profile must read as a prompt (trust, permission).
    Prompt,
    /// A startup screen that is only "not ready" (an update notice, a sign-in page).
    Startup,
}

fn role(name: &str) -> Option<Role> {
    Some(match name {
        "tui_idle" => Role::Idle,
        "tui_working" | "tui_work_working" => Role::Working,
        "tui_reply" => Role::Reply,
        "tui_work_reply" => Role::WorkReply,
        "tui_permission" | "tui_permission_command" | "tui_trust" => Role::Prompt,
        "tui_firstrun" => Role::Startup,
        _ => return None,
    })
}

/// One thing wrong with an expectation (a drafter extra, or the profile against what the capture proves).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub name: String,
    pub code: &'static str,
    pub message: String,
}

fn problem(name: &str, message: impl Into<String>) -> Problem {
    Problem { name: name.to_string(), code: "FIXTURE_EXPECT_INVALID", message: message.into() }
}

fn q(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

fn list(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|i| q(i)).collect::<Vec<_>>().join(", "))
}

/// Put the real words back into text that came from the drafter (it only ever saw `<word-1>` ...).
pub fn unmask_words(text: &str, words: &TestWords) -> String {
    text.replace("<word-1>", &words.one).replace("<word-2>", &words.two).replace("<word-3>", &words.sky)
}

/// The extras a drafter may add, validated against the capture they belong to.
#[derive(Debug, Default)]
struct Extras {
    chrome_lines: Vec<String>,
    not_chrome_lines: Vec<String>,
    model: Option<String>,
    menu: Option<bool>,
    prompt_kind: Option<String>,
    thinking_lines: Vec<String>,
    summary_lines: Vec<String>,
    tool_lines: Vec<String>,
}

fn parse_extras(name: &str, frame: &CapFrame, text: &str, words: &TestWords) -> Result<Extras, Vec<Problem>> {
    let mut problems = Vec::new();
    let table: toml::Table = match unmask_words(text, words).parse() {
        Ok(t) => t,
        Err(e) => return Err(vec![problem(name, format!("the expectation block for {name} is not valid TOML: {e}"))]),
    };
    let screen = frame.text.to_lowercase();
    let on_screen = |entry: &str| screen.contains(&entry.trim().to_lowercase()) && !entry.trim().is_empty();
    let mut extras = Extras::default();
    for (key, value) in &table {
        match key.as_str() {
            "chrome_lines" | "not_chrome_lines" => {
                let Some(array) = value.as_array() else {
                    problems.push(problem(name, format!("`{key}` must be a list of strings")));
                    continue;
                };
                for item in array {
                    match item.as_str() {
                        Some(s) if on_screen(s) => {
                            if key == "chrome_lines" {
                                extras.chrome_lines.push(s.to_string());
                            } else {
                                extras.not_chrome_lines.push(s.to_string());
                            }
                        }
                        Some(s) => problems.push(problem(name, format!("`{key}` entry {s:?} is not text that appears on the {name} capture"))),
                        None => problems.push(problem(name, format!("`{key}` entries must be strings"))),
                    }
                }
            }
            "thinking_lines" | "summary_lines" | "tool_lines" => {
                let Some(array) = value.as_array() else {
                    problems.push(problem(name, format!("`{key}` must be a list of strings")));
                    continue;
                };
                for item in array {
                    match item.as_str() {
                        Some(s) if on_screen(s) => match key.as_str() {
                            "thinking_lines" => extras.thinking_lines.push(s.to_string()),
                            "summary_lines" => extras.summary_lines.push(s.to_string()),
                            _ => extras.tool_lines.push(s.to_string()),
                        },
                        Some(s) => problems.push(problem(name, format!("`{key}` entry {s:?} is not text that appears on the {name} capture"))),
                        None => problems.push(problem(name, format!("`{key}` entries must be strings"))),
                    }
                }
            }
            "model" => match value.as_str() {
                Some(s) if on_screen(s) => extras.model = Some(s.to_string()),
                Some(s) => problems.push(problem(name, format!("`model` {s:?} does not appear on the {name} capture"))),
                None => problems.push(problem(name, "`model` must be a string")),
            },
            "menu" => match value.as_bool() {
                Some(b) => extras.menu = Some(b),
                None => problems.push(problem(name, "`menu` must be true or false")),
            },
            "prompt_kind" => match value.as_str() {
                Some(s) if s.is_empty() || (s.len() <= 40 && s.chars().all(|c| c.is_ascii_lowercase() || c == '_')) => {
                    extras.prompt_kind = Some(s.to_string())
                }
                _ => problems.push(problem(name, "`prompt_kind` must be a short lowercase word such as trust or command_approval")),
            },
            other => problems.push(problem(
                name,
                format!("`{other}` is not an expectation key you may add; the allowed keys are chrome_lines, not_chrome_lines, thinking_lines, summary_lines, tool_lines, model, menu, prompt_kind"),
            )),
        }
    }
    if problems.is_empty() { Ok(extras) } else { Err(problems) }
}

/// The text of `<name>.expect.toml` for one frame, or what is wrong with it.
fn expect_for(
    name: &str,
    frame: &CapFrame,
    profile: &DeclarativeProfile,
    words: &TestWords,
    extra: Option<&str>,
) -> Result<String, Vec<Problem>> {
    let Some(role) = role(name) else { return Err(vec![problem(name, format!("{name} is not a capture the wizard makes"))]) };
    let extras = match extra {
        Some(text) if !text.trim().is_empty() => parse_extras(name, frame, text, words)?,
        _ => Extras::default(),
    };
    let mut problems = Vec::new();
    let mut out: Vec<String> = Vec::new();
    let screen = frame.text.as_str();
    match role {
        Role::Idle | Role::Reply | Role::WorkReply => {
            match profile.busy_marker(screen) {
                Some(true) => problems.push(problem(
                    name,
                    format!("the profile reads {name} as busy, but the agent was idle. `[tui.busy] working_regex` matches text that is also on the idle screen"),
                )),
                Some(false) => out.push("busy = false".into()),
                None => out.push("busy = \"unknown\"".into()),
            }
            out.push("prompt_kind = \"\"".into());
            out.push(format!("menu = {}", extras.menu.unwrap_or(false)));
            out.push("ready = true".into());
        }
        Role::Working => {
            out.push("busy = true".into());
            out.push("prompt_kind = \"\"".into());
            out.push("ready = false".into());
            if let Some(menu) = extras.menu {
                out.push(format!("menu = {menu}"));
            }
        }
        Role::Prompt => {
            // The draft's OWN `[[tui.prompt]]` must detect the question: one that only the shared heuristics
            // recognise gives the agent no card of its own (the pane would read "idle" behind it). A kind the
            // drafter writes in its expect block never stands in for that.
            let detected = profile
                .declared_prompt(screen, false)
                .and_then(|p| p.get("kind").and_then(|k| k.as_str()).map(str::to_string))
                .filter(|k| !k.is_empty());
            match (detected, extras.prompt_kind.clone().filter(|k| !k.is_empty())) {
                (Some(found), Some(said)) if found != said => problems.push(problem(
                    name,
                    format!("the {name} capture is read as `{found}` by the profile's `[[tui.prompt]]` tables, but the expectation says `{said}`"),
                )),
                (Some(found), _) => out.push(format!("prompt_kind = {}", q(&found))),
                (None, _) => problems.push(problem(
                    name,
                    format!("no `[[tui.prompt]]` in the file matches the {name} capture, so a question like this would not become a card the person can answer. Add a `[[tui.prompt]]` whose `detect_regex` matches it (a trust-this-folder question: `kind = \"workspace_trust\"`, with `allow_once` and `deny` choices)"),
                )),
            }
            out.push("ready = false".into());
            if let Some(menu) = extras.menu {
                out.push(format!("menu = {menu}"));
            }
        }
        Role::Startup => {
            out.push("ready = false".into());
            if let Some(menu) = extras.menu {
                out.push(format!("menu = {menu}"));
            }
        }
    }
    if let Some(model) = &extras.model {
        out.push(format!("model = {}", q(model)));
    }
    let mut chrome = extras.chrome_lines.clone();
    chrome.dedup();
    if !chrome.is_empty() {
        out.push(format!("chrome_lines = {}", list(&chrome)));
    }
    for (key, lines) in [("thinking_lines", &extras.thinking_lines), ("summary_lines", &extras.summary_lines), ("tool_lines", &extras.tool_lines)] {
        if !lines.is_empty() {
            out.push(format!("{key} = {}", list(lines)));
        }
    }
    let mut not_chrome = extras.not_chrome_lines.clone();
    if matches!(role, Role::Reply | Role::WorkReply) {
        let word = if role == Role::Reply { &words.one } else { &words.sky };
        if let Some(row) = frame.text.lines().map(str::trim).find(|r| {
            let l = r.to_lowercase();
            l.contains(&word.to_lowercase()) && !l.contains("reply with") && !l.contains("exactly the word")
        }) {
            not_chrome.push(row.to_string());
            if role == Role::WorkReply {
                // What the capture proves: once the profile drops its chrome, thinking, summary and tool rows, the
                // rows after the task's echo are exactly the answer.
                out.push("reply_after = \"reply with exactly\"".into());
                out.push(format!("reply_rows = {}", list(&[row.to_string()])));
            }
        }
    }
    not_chrome.dedup();
    if !not_chrome.is_empty() {
        out.push(format!("not_chrome_lines = {}", list(&not_chrome)));
    }
    if problems.is_empty() {
        Ok(out.join("\n") + "\n")
    } else {
        Err(problems)
    }
}

/// `# capture:` header of a screen fixture.
pub fn header(command: &str, version: Option<&str>, grid: (u16, u16)) -> String {
    format!(
        "# capture: {command} {}, pane {}x{}, puppet-master wizard capture, {}; user name scrubbed\n",
        version.unwrap_or("(version unknown)"),
        grid.0,
        grid.1,
        &now_rfc3339()[..10],
    )
}

/// Everything `finish` and `verify` write for the screen captures: for each capture its text and its
/// merged expectation. A capture whose expectation is invalid is left out and reported.
pub fn files(
    set: &CaptureSet,
    profile: &DeclarativeProfile,
    words: &TestWords,
    extras: &BTreeMap<String, String>,
    head: &str,
) -> (Vec<FixtureFile>, Vec<Problem>) {
    let mut files = Vec::new();
    let mut problems = Vec::new();
    for name in NAMES {
        let Some(frame) = set.get(name) else { continue };
        match expect_for(name, frame, profile, words, extras.get(name).map(String::as_str)) {
            Ok(expect) => {
                files.push(FixtureFile { name: format!("{name}.txt"), text: format!("{head}{}\n", frame.text.trim_end()) });
                files.push(FixtureFile { name: format!("{name}.expect.toml"), text: expect });
            }
            Err(p) => problems.extend(p),
        }
    }
    for name in extras.keys() {
        if role(name).is_none() || !set.has(name) {
            problems.push(problem(name, format!("there is no capture named {name}; the expectation blocks must be named after a capture")));
        }
    }
    (files, problems)
}

/// The captures as the drafter and the preview see them: masked, with the rows that varied between
/// frames, plus the facts.
pub fn masked_json(set: &CaptureSet, words: &TestWords) -> Value {
    let mask = |t: &str| mask_words(t, words);
    let files: Vec<Value> = NAMES
        .iter()
        .filter_map(|name| set.get(name))
        .map(|f| json!({"name": f.name, "text": mask(&f.text), "variants": f.variants.iter().map(|v| mask(v)).collect::<Vec<_>>()}))
        .collect();
    json!({
        "grid": {"cols": set.grid.0, "rows": set.grid.1},
        "files": files,
        "facts": {
            "exit_method": set.exit_method,
            "partial": set.partial,
            "skipped": set.skipped,
            "hello_written": set.hello_written,
            "writes_without_asking": set.writes_without_asking,
            "runs_without_asking": set.runs_without_asking,
            "startup_dialogs": set.startup_dialogs,
            "exit_screen": set.exit_screen.as_deref().map(|s| mask(s)),
            "notes": set.notes,
            "answered": set.answered.iter().map(|a| a.to_json(&mask)).collect::<Vec<_>>(),
            // Measured, not guessed: the pane was made narrower after the reply and the rows holding the test word
            // were counted in the viewer's scrollback plus screen. `null`: not measured. The wizard writes
            // `[features] reprints_on_resize` from this; the draft does not need to.
            "reprints_on_resize": set.resize.as_ref().map(|r| r.reprints),
            "resize_probe": set.resize.as_ref().map(|r| r.to_json(&mask)),
        },
    })
}

/// A frame counts as holding an answer when a row other than the instruction's echo has the word.
pub fn has_reply_row(frame: &CapFrame, words: &TestWords) -> bool {
    reply_rows(&frame.text, &words.one) > 0
}

/// Non-blank rows of a frame (used by tests and the excerpts).
pub fn row_count(frame: &CapFrame) -> usize {
    rows(&frame.text).len()
}
