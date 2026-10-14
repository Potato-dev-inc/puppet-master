//! The fake agent's brain: a state machine driven by keys and a clock. It owns no terminal and no
//! real time (callers pass `now_ms`), so every behaviour is unit-tested deterministically; the real
//! console loop is `runtime.rs`.

use crate::keys::Key;
use crate::render::{self, Banner, Block, Dialog, Frame, GateView, Mode, View};
use crate::script::{ApprovalKind, BurstEffect, Gate, GateKind, Layout, Script, Step};
use serde_json::json;
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;

enum Phase {
    /// The loading screen, before the first startup screen or the composer.
    Loading { until: u64 },
    /// Waiting for a startup screen to appear (the composer may already be showing).
    BeforeGate { until: u64, gate: Gate },
    Gate { gate: Gate, selected: usize },
    Ready,
    /// A turn is running: the queue of steps, one of which may be sleeping.
    Busy,
    Approval { dialog: Dialog, on_allow: Vec<Step>, on_deny: Vec<Step> },
    /// Waiting for the usage limit to reset ("continuing automatically ... esc to cancel").
    LimitWait,
    /// The process is going away; the screen stays as it was.
    Exited { frozen: Option<GateView> },
}

struct Sleep {
    verb: String,
    until: u64,
}

pub struct Engine {
    script: Script,
    layout: Layout,
    base_dir: PathBuf,
    banner: Banner,
    cols: usize,
    rows: usize,
    phase: Phase,
    gates: VecDeque<Gate>,
    blocks: Vec<Block>,
    composer: String,
    suggestion: Option<String>,
    pending_suggestion: Option<String>,
    fresh: bool,
    queue: VecDeque<Step>,
    sleep: Option<Sleep>,
    verb: String,
    turn_started: u64,
    turns_done: usize,
    last_typed: Option<u64>,
    notice: Option<(String, Option<u64>)>,
    ctrl_c_armed: bool,
    exit: Option<i32>,
    log: Option<std::fs::File>,
}

impl Engine {
    pub fn new(script: Script, base_dir: PathBuf, cols: usize, rows: usize, now: u64) -> Engine {
        let layout = script.agent;
        let cwd = script.cwd.clone().unwrap_or_else(|| base_dir.display().to_string());
        let banner = match layout {
            Layout::Claude => Banner {
                version: script.version.clone().unwrap_or_else(|| "2.1.299".into()),
                model: script.model.clone().unwrap_or_else(|| "Haiku 5.5 \u{b7} Claude Pro".into()),
                cwd,
            },
            Layout::Codex => Banner {
                version: script.version.clone().unwrap_or_else(|| "0.162.1".into()),
                model: script.model.clone().unwrap_or_else(|| "GPT-6-Luna medium".into()),
                cwd,
            },
        };
        let log = std::env::var("PM_FAKE_TUI_LOG")
            .ok()
            .or_else(|| script.log.clone())
            .filter(|path| !path.is_empty())
            .and_then(|path| std::fs::OpenOptions::new().create(true).append(true).open(path).ok());
        let mut engine = Engine {
            gates: script.startup.iter().cloned().collect(),
            script,
            layout,
            base_dir,
            banner,
            cols,
            rows,
            phase: Phase::Ready,
            blocks: Vec::new(),
            composer: String::new(),
            suggestion: None,
            pending_suggestion: None,
            fresh: true,
            queue: VecDeque::new(),
            sleep: None,
            verb: "Generating".into(),
            turn_started: 0,
            turns_done: 0,
            last_typed: None,
            notice: None,
            ctrl_c_armed: false,
            exit: None,
            log,
        };
        engine.log(now, "start", json!({ "agent": format!("{layout:?}").to_lowercase(), "cols": cols, "rows": rows }));
        let boot = engine.script.boot_ms;
        if boot > 0 {
            engine.phase = Phase::Loading { until: now + boot };
        } else {
            engine.begin_next_startup(now);
        }
        engine
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit
    }

    pub fn resize(&mut self, cols: usize, rows: usize, now: u64) {
        if (cols, rows) != (self.cols, self.rows) {
            self.cols = cols;
            self.rows = rows;
            self.log(now, "resize", json!({ "cols": cols, "rows": rows }));
        }
    }

    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// The text of the composer right now (what a typed prompt looks like before Enter).
    pub fn composer(&self) -> &str {
        &self.composer
    }

    fn log(&mut self, now: u64, event: &str, fields: serde_json::Value) {
        if let Some(file) = self.log.as_mut() {
            let mut line = json!({ "t_ms": now, "ev": event });
            if let (Some(map), Some(extra)) = (line.as_object_mut(), fields.as_object()) {
                map.extend(extra.clone());
            }
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }

    // ---- startup ----------------------------------------------------------------------------

    fn begin_next_startup(&mut self, now: u64) {
        match self.gates.pop_front() {
            Some(gate) => self.phase = Phase::BeforeGate { until: now + gate.after_ms, gate },
            None => {
                self.phase = Phase::Ready;
                self.log(now, "ready", json!({}));
            }
        }
    }

    // ---- clock ------------------------------------------------------------------------------

    pub fn tick(&mut self, now: u64) {
        if let Some((_, Some(until))) = &self.notice {
            if now >= *until {
                self.notice = None;
                self.ctrl_c_armed = false;
            }
        }
        match &self.phase {
            Phase::Loading { until } if now >= *until => self.begin_next_startup(now),
            Phase::BeforeGate { until, .. } if now >= *until => {
                if let Phase::BeforeGate { gate, .. } = std::mem::replace(&mut self.phase, Phase::Ready) {
                    self.log(now, "gate_shown", json!({ "gate": format!("{:?}", gate.gate) }));
                    self.phase = Phase::Gate { gate, selected: 0 };
                }
            }
            Phase::Busy if self.sleep.as_ref().is_some_and(|s| now >= s.until) => {
                self.sleep = None;
                self.run_queue(now);
            }
            _ => {}
        }
    }

    // ---- keys -------------------------------------------------------------------------------

    pub fn on_key(&mut self, key: Key, now: u64) {
        // Typed characters are not logged one by one (the `prompt` event carries the submitted text).
        if !matches!(key, Key::Char(_) | Key::Paste(_)) {
            self.log(now, "key", json!({ "key": format!("{key:?}") }));
        }
        // `0` ignores the key; `1` is a composer drawn before a startup screen (it takes typing).
        let kind = match &self.phase {
            Phase::Loading { .. } | Phase::Exited { .. } => 0,
            Phase::BeforeGate { gate, .. } => u8::from(gate.composer_first),
            Phase::Gate { .. } => 2,
            Phase::Ready => 3,
            Phase::Busy => 4,
            Phase::Approval { .. } => 5,
            Phase::LimitWait => 6,
        };
        match kind {
            // A composer that is already on screen takes typing; Enter does not submit yet.
            1 if !matches!(key, Key::Enter) => self.compose(key, now),
            2 => self.gate_key(key, now),
            3 => self.ready_key(key, now),
            4 if matches!(key, Key::Esc | Key::CtrlC) && self.script.interrupt => self.interrupt(now),
            5 => self.approval_key(key, now),
            6 if key == Key::Esc => {
                self.notice = None;
                self.blocks.push(Block::Note(
                    "Automatic continue cancelled. Your session will wait for you instead; /rate-limit-options can arm it again.".into(),
                ));
                self.log(now, "limit_wait_cancelled", json!({}));
                self.phase = Phase::Ready;
            }
            _ => {}
        }
    }

    fn gate_key(&mut self, key: Key, now: u64) {
        let Phase::Gate { gate, selected } = &mut self.phase else { return };
        let options = if gate.gate == GateKind::UpdateMenu { 3 } else { 2 };
        let mut confirm: Option<usize> = None;
        match key {
            Key::Up => *selected = selected.saturating_sub(1),
            Key::Down => *selected = (*selected + 1).min(options - 1),
            Key::Enter => confirm = Some(*selected),
            Key::Char(c) if gate.gate == GateKind::UpdateMenu || self.layout == Layout::Codex => {
                if let Some(n) = c.to_digit(10).filter(|n| (1..=options as u32).contains(n)) {
                    confirm = Some(n as usize - 1);
                }
            }
            Key::Esc if gate.honour_esc => {
                // Update menu: "esc skip". Trust: "Esc to cancel" is the refusal.
                confirm = Some(if gate.gate == GateKind::UpdateMenu { 1 } else { options - 1 });
                // Claude's trust dialog puts "No, exit" first.
                if gate.gate == GateKind::Trust && self.layout == Layout::Claude {
                    confirm = Some(0);
                }
            }
            _ => {}
        }
        let Some(choice) = confirm else { return };
        let (kind, layout) = (gate.gate, self.layout);
        match kind {
            GateKind::UpdateMenu => match choice {
                0 => {
                    self.log(now, "gate_answer", json!({ "gate": "update_menu", "answer": "update_now" }));
                    self.blocks.push(Block::Note("Updating Codex... (the fake exits instead)".into()));
                    self.exit_with(0, now);
                }
                n => {
                    let answer = if n == 1 { "skip" } else { "skip_until_next_version" };
                    self.log(now, "gate_answer", json!({ "gate": "update_menu", "answer": answer }));
                    self.begin_next_startup(now);
                }
            },
            GateKind::Trust => {
                // Claude: option 0 is "No, exit". Codex: option 1 is "No, quit".
                let declined = if layout == Layout::Claude { choice == 0 } else { choice == 1 };
                if declined {
                    self.log(now, "gate_answer", json!({ "gate": "trust", "answer": "decline" }));
                    self.exit_with(1, now);
                } else {
                    self.log(now, "gate_answer", json!({ "gate": "trust", "answer": "trust" }));
                    self.begin_next_startup(now);
                }
            }
        }
    }

    fn compose(&mut self, key: Key, now: u64) {
        match key {
            Key::Char(c) => {
                self.composer.push(c);
                self.suggestion = None;
                self.last_typed = Some(now);
            }
            Key::Paste(text) => {
                self.composer.push_str(&text.replace("\r\n", "\n").replace('\r', "\n"));
                self.suggestion = None;
                self.last_typed = Some(now);
            }
            Key::Backspace => {
                self.composer.pop();
            }
            Key::CtrlU => self.composer.clear(),
            _ => {}
        }
    }

    fn ready_key(&mut self, key: Key, now: u64) {
        match key {
            Key::Enter => {
                let burst = self.script.burst_ms();
                let quick = burst > 0 && self.last_typed.is_some_and(|at| now.saturating_sub(at) < burst);
                if quick {
                    match self.script.burst_effect() {
                        BurstEffect::Newline => self.composer.push('\n'),
                        BurstEffect::Drop => {}
                    }
                    self.log(now, "enter_in_paste_burst", json!({ "effect": format!("{:?}", self.script.burst_effect()) }));
                    return;
                }
                let prompt = self.composer.trim_end().to_string();
                if prompt.trim().is_empty() {
                    return;
                }
                self.submit(prompt, now);
            }
            Key::CtrlC => {
                if !self.composer.is_empty() {
                    self.composer.clear();
                } else if self.ctrl_c_armed {
                    self.exit_with(0, now);
                } else {
                    self.ctrl_c_armed = true;
                    self.notice = Some(("Press Ctrl-C again to exit".into(), Some(now + 2000)));
                }
            }
            other => self.compose(other, now),
        }
    }

    fn approval_key(&mut self, key: Key, now: u64) {
        let Phase::Approval { dialog, .. } = &mut self.phase else { return };
        let options = dialog.options(self.layout).len();
        let mut confirm: Option<usize> = None;
        match key {
            Key::Up => dialog.selected = dialog.selected.saturating_sub(1),
            Key::Down => dialog.selected = (dialog.selected + 1).min(options - 1),
            Key::Enter => confirm = Some(dialog.selected),
            Key::Esc => confirm = Some(options - 1),
            Key::Char(c) => {
                if let Some(n) = c.to_digit(10).filter(|n| (1..=options as u32).contains(n)) {
                    confirm = Some(n as usize - 1);
                } else if self.layout == Layout::Codex {
                    confirm = match c {
                        'y' => Some(0),
                        'a' => Some(1),
                        'n' => Some(options - 1),
                        _ => None,
                    };
                }
            }
            _ => {}
        }
        let Some(choice) = confirm else { return };
        let Phase::Approval { dialog, on_allow, on_deny } = std::mem::replace(&mut self.phase, Phase::Busy) else { return };
        let labels = dialog.options(self.layout);
        let allowed = choice + 1 < labels.len();
        self.log(
            now,
            "approval_answer",
            json!({ "answer": if allowed { "allow" } else { "deny" }, "option": choice + 1, "label": labels[choice], "kind": format!("{:?}", dialog.kind).to_lowercase() }),
        );
        let subject = match dialog.kind {
            ApprovalKind::Bash => dialog.command.clone(),
            _ => dialog.path.clone(),
        };
        if allowed {
            self.blocks.push(self.allowed_row(&dialog, &subject));
            for step in on_allow.into_iter().rev() {
                self.queue.push_front(step);
            }
        } else {
            self.blocks.push(Block::Note(match (self.layout, dialog.kind) {
                (Layout::Claude, ApprovalKind::Bash) => "User rejected tool use".into(),
                (Layout::Claude, _) => format!("User rejected update to {subject}"),
                (Layout::Codex, _) => format!("You canceled the request to run {subject}"),
            }));
            self.queue = on_deny.into_iter().collect();
        }
        self.run_queue(now);
    }

    fn allowed_row(&self, dialog: &Dialog, subject: &str) -> Block {
        let label = match (self.layout, dialog.kind) {
            (Layout::Claude, ApprovalKind::Bash) => format!("Bash({subject})"),
            (Layout::Claude, ApprovalKind::Edit) => format!("Update({subject})"),
            (Layout::Claude, ApprovalKind::Create) => format!("Write({subject})"),
            (Layout::Codex, ApprovalKind::Bash) => format!("Ran {subject}"),
            (Layout::Codex, _) => format!("Edited {subject}"),
        };
        Block::Tool { label, detail: None, failed: None }
    }

    // ---- turns ------------------------------------------------------------------------------

    fn submit(&mut self, prompt: String, now: u64) {
        self.log(now, "prompt", json!({ "text": prompt }));
        self.blocks.push(Block::User(prompt.clone()));
        self.composer.clear();
        self.suggestion = None;
        self.pending_suggestion = None;
        self.fresh = false;
        self.queue = match self.script.turn_for(&prompt) {
            Some(turn) => turn.steps.iter().cloned().collect(),
            None => VecDeque::from([Step::Think { ms: 300, verb: None }, Step::Say { text: "OK".into() }]),
        };
        self.turn_started = now;
        self.verb = "Generating".into();
        self.phase = Phase::Busy;
        self.run_queue(now);
    }

    fn run_queue(&mut self, now: u64) {
        while matches!(self.phase, Phase::Busy) && self.sleep.is_none() {
            let Some(step) = self.queue.pop_front() else {
                self.finish_turn(now, true);
                return;
            };
            match step {
                Step::Think { ms, verb } => {
                    if let Some(verb) = verb {
                        self.verb = verb;
                    }
                    self.sleep = Some(Sleep { verb: self.verb.clone(), until: now + ms });
                }
                Step::Wait { ms } => self.sleep = Some(Sleep { verb: self.verb.clone(), until: now + ms }),
                Step::Say { text } => self.blocks.push(Block::Reply(text)),
                Step::Tool { label, detail, ms, failed } => {
                    self.blocks.push(Block::Tool { label, detail, failed });
                    if ms > 0 {
                        self.sleep = Some(Sleep { verb: self.verb.clone(), until: now + ms });
                    }
                }
                Step::Approve { kind, command, path, diff, auto_mode_option, on_allow, on_deny } => {
                    let dialog = Dialog {
                        kind,
                        command: command.unwrap_or_default(),
                        path: path.unwrap_or_default(),
                        diff,
                        auto_mode_option,
                        selected: 0,
                    };
                    self.log(now, "approval_shown", json!({ "kind": format!("{kind:?}").to_lowercase(), "command": dialog.command, "path": dialog.path }));
                    self.phase = Phase::Approval { dialog, on_allow, on_deny };
                    return;
                }
                Step::WriteFile { path, content, append, quiet } => self.write_file(&path, &content, append, quiet, now),
                Step::Suggest { text } => self.pending_suggestion = Some(text),
                Step::Limit { text, reset, auto_continue } => {
                    let reset = reset.unwrap_or_else(|| "9:45 PM".into());
                    let text = text.unwrap_or_else(|| match self.layout {
                        Layout::Codex => format!("You\u{2019}ve hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/settings/usage to purchase more credits or try again at {reset}."),
                        Layout::Claude => format!("You've hit your session limit \u{b7} resets {reset}"),
                    });
                    self.log(now, "limit", json!({ "text": text }));
                    self.blocks.push(Block::Error(text));
                    self.queue.clear();
                    if auto_continue {
                        self.notice = Some((format!("Usage limit reached \u{b7} continuing automatically at {reset} \u{b7} esc to cancel"), None));
                        self.phase = Phase::LimitWait;
                    } else {
                        self.finish_turn(now, false);
                    }
                    return;
                }
                Step::Exit { code } => {
                    self.exit_with(code, now);
                    return;
                }
                Step::Crash { message, code } => {
                    let message = message.unwrap_or_else(|| "thread 'main' panicked at src/agent.rs:42:5: the fake agent crashed".into());
                    self.blocks.push(Block::Raw(message.lines().map(str::to_string).collect()));
                    self.exit_with(code, now);
                    return;
                }
                Step::Screen { text } => self.blocks.push(Block::Raw(text.lines().map(str::to_string).collect())),
            }
        }
    }

    fn write_file(&mut self, path: &str, content: &str, append: bool, quiet: bool, now: u64) {
        let target = self.base_dir.join(path);
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut options = std::fs::OpenOptions::new();
            options.create(true).write(true);
            if append {
                options.append(true);
            } else {
                options.truncate(true);
            }
            options.open(&target)?.write_all(content.as_bytes())
        })();
        self.log(now, "write_file", json!({ "path": path, "ok": result.is_ok(), "error": result.as_ref().err().map(|e| e.to_string()) }));
        if !quiet {
            let lines = content.lines().count();
            let (label, detail) = match (&result, self.layout) {
                (Ok(()), Layout::Claude) => (format!("Write({path})"), format!("Wrote {lines} lines to {path}")),
                (Ok(()), Layout::Codex) => (format!("Edited {path}"), format!("wrote {lines} lines")),
                (Err(e), _) => (format!("Write({path})"), format!("Error: {e}")),
            };
            self.blocks.push(Block::Tool { label, detail: Some(detail), failed: None });
        }
    }

    fn interrupt(&mut self, now: u64) {
        self.log(now, "interrupted", json!({}));
        self.queue.clear();
        self.sleep = None;
        self.blocks.push(Block::Note(match self.layout {
            Layout::Claude => "Interrupted \u{b7} What should Claude do instead?".into(),
            Layout::Codex => "Conversation interrupted - tell the model what to do differently.".into(),
        }));
        self.finish_turn(now, true);
    }

    fn finish_turn(&mut self, now: u64, summary: bool) {
        if summary {
            let secs = now.saturating_sub(self.turn_started) / 1000;
            let (h, m) = wall_clock();
            let text = match self.layout {
                Layout::Claude => {
                    const VERBS: [&str; 4] = ["Churned", "Brewed", "Cogitated", "Crunched"];
                    format!("\u{273b} {} for {secs}s \u{b7} done {h}:{m:02}", VERBS[self.turns_done % VERBS.len()])
                }
                Layout::Codex => {
                    let (h12, ampm) = (if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" });
                    format!("Worked for {secs}s \u{2022} {h12}:{m:02} {ampm}")
                }
            };
            self.blocks.push(Block::Summary(text));
        }
        self.turns_done += 1;
        self.suggestion = self.pending_suggestion.take();
        self.sleep = None;
        self.phase = Phase::Ready;
        self.log(now, "turn_done", json!({ "turn": self.turns_done }));
    }

    fn exit_with(&mut self, code: i32, now: u64) {
        let frozen = match &self.phase {
            Phase::Gate { gate, selected } => Some(GateView { kind: gate.gate, selected: *selected }),
            _ => None,
        };
        self.log(now, "exit", json!({ "code": code }));
        self.exit = Some(code);
        self.queue.clear();
        self.sleep = None;
        self.phase = Phase::Exited { frozen };
    }

    // ---- drawing ----------------------------------------------------------------------------

    pub fn frame(&self, now: u64) -> Frame {
        let mode = match &self.phase {
            Phase::Loading { .. } => Mode::Loading,
            Phase::BeforeGate { gate, .. } if !gate.composer_first => Mode::Loading,
            Phase::BeforeGate { .. } | Phase::Ready | Phase::Busy | Phase::LimitWait => Mode::Ready,
            Phase::Gate { gate, selected } => Mode::Gate(GateView { kind: gate.gate, selected: *selected }),
            Phase::Approval { dialog, .. } => Mode::Approval(dialog.clone()),
            Phase::Exited { frozen: Some(gate) } => Mode::Gate(gate.clone()),
            Phase::Exited { frozen: None } => Mode::Ready,
        };
        let busy = match &self.phase {
            Phase::Busy => Some((self.sleep.as_ref().map_or(self.verb.as_str(), |s| s.verb.as_str()), now.saturating_sub(self.turn_started))),
            _ => None,
        };
        render::frame(&View {
            layout: self.layout,
            cols: self.cols,
            rows: self.rows,
            banner: &self.banner,
            blocks: &self.blocks,
            mode,
            composer: &self.composer,
            suggestion: self.suggestion.as_deref(),
            fresh: self.fresh,
            busy,
            notice: self.notice.as_ref().map(|(text, _)| text.as_str()),
            tick: now / 200,
        })
    }

    /// The whole screen as plain text, for tests.
    pub fn plain(&self, now: u64) -> String {
        self.frame(now).rows.iter().map(|row| strip(row)).collect::<Vec<_>>().join("\n")
    }
}

/// Escape sequences removed.
pub fn strip(row: &str) -> String {
    let mut out = String::new();
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// UTC hours and minutes (the fake has no time-zone database; the digits only need to look right).
fn wall_clock() -> (u64, u64) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    ((secs / 3600) % 24, (secs / 60) % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(toml: &str) -> Engine {
        let script = Script::parse(toml, false).unwrap();
        Engine::new(script, std::env::temp_dir().join("pm-fake-tui-unit"), 100, 30, 0)
    }

    fn type_text(e: &mut Engine, text: &str, now: u64) {
        for c in text.chars() {
            e.on_key(Key::Char(c), now);
        }
    }

    #[test]
    fn a_simple_turn_runs_to_its_summary_and_the_composer_comes_back() {
        let mut e = engine("[[turn]]\non_prompt = \"hello\"\nsteps = [{ do = \"think\", ms = 500 }, { do = \"say\", text = \"Hi there\" }]");
        e.tick(0);
        type_text(&mut e, "hello world", 10);
        e.on_key(Key::Enter, 600);
        e.tick(700);
        assert!(e.plain(700).contains("esc to interrupt"), "{}", e.plain(700));
        assert!(e.plain(700).contains("Generating"));
        e.tick(1200);
        let screen = e.plain(1200);
        assert!(screen.contains("\u{25cf} Hi there") && screen.contains("\u{273b} Churned for 0s"), "{screen}");
        assert!(!screen.contains("esc to interrupt"));
    }

    #[test]
    fn an_approval_waits_for_a_key_and_runs_the_right_branch() {
        let toml = "[[turn]]\nsteps = [{ do = \"approve\", kind = \"bash\", command = \"npm test\", on_allow = [{ do = \"say\", text = \"allowed\" }], on_deny = [{ do = \"say\", text = \"denied\" }] }, { do = \"say\", text = \"after\" }]";
        let mut allowed = engine(toml);
        type_text(&mut allowed, "go", 0);
        allowed.on_key(Key::Enter, 500);
        let dialog = allowed.plain(500);
        assert!(dialog.contains("Bash command") && dialog.contains("Do you want to proceed?") && dialog.contains("\u{276f} 1. Yes"), "{dialog}");
        allowed.on_key(Key::Down, 600);
        assert!(allowed.plain(600).contains("\u{276f} 2. Yes, and don\u{2019}t ask again for: npm *"), "{}", allowed.plain(600));
        allowed.on_key(Key::Up, 610);
        allowed.on_key(Key::Enter, 700);
        let screen = allowed.plain(700);
        assert!(screen.contains("allowed") && screen.contains("after") && !screen.contains("denied"), "{screen}");

        let mut denied = engine(toml);
        type_text(&mut denied, "go", 0);
        denied.on_key(Key::Enter, 500);
        denied.on_key(Key::Esc, 600);
        let screen = denied.plain(600);
        assert!(screen.contains("denied") && !screen.contains("after") && screen.contains("User rejected tool use"), "{screen}");
    }

    #[test]
    fn the_trust_dialog_exits_on_no_and_continues_on_yes() {
        let mut no = engine("[[startup]]\ngate = \"trust\"");
        no.tick(0);
        assert!(no.plain(0).contains("Quick safety check"));
        no.on_key(Key::Enter, 10);
        assert_eq!(no.exit_code(), Some(1));
        assert!(no.plain(20).contains("Quick safety check"), "the dialog stays on screen under the shell prompt");

        let mut yes = engine("[[startup]]\ngate = \"trust\"");
        yes.tick(0);
        yes.on_key(Key::Down, 5);
        yes.on_key(Key::Enter, 10);
        assert_eq!(yes.exit_code(), None);
        assert!(yes.plain(20).contains("\u{276f}\u{a0}"), "{}", yes.plain(20));
    }

    #[test]
    fn the_codex_update_menu_appears_late_and_esc_skips_it() {
        let mut e = engine("agent = \"codex\"\nboot_ms = 100\n[[startup]]\ngate = \"update_menu\"\nafter_ms = 500\ncomposer_first = true");
        e.tick(50);
        assert!(e.plain(50).contains("loading"));
        e.tick(120);
        let early = e.plain(120);
        assert!(early.contains("Ask Codex to do anything") && !early.contains("Update available"), "{early}");
        e.tick(700);
        assert!(e.plain(700).contains("Skip until next version"));
        e.on_key(Key::Esc, 710);
        assert_eq!(e.exit_code(), None);
        assert!(e.plain(720).contains("Ask Codex to do anything"));
    }

    #[test]
    fn codex_turns_swallow_a_quick_enter_as_a_line_break() {
        let mut e = engine("agent = \"codex\"");
        type_text(&mut e, "do it", 1000);
        e.on_key(Key::Enter, 1050);
        assert_eq!(e.composer(), "do it\n", "Enter 50 ms after the text is part of the paste");
        e.on_key(Key::Backspace, 1100);
        e.on_key(Key::Enter, 1300);
        assert!(e.plain(1300).contains("\u{203a} do it"), "{}", e.plain(1300));
    }

    #[test]
    fn a_suggestion_is_dim_and_vanishes_when_typed_over() {
        let mut e = engine("[[turn]]\nsteps = [{ do = \"suggest\", text = \"commit this\" }]");
        type_text(&mut e, "x", 0);
        e.on_key(Key::Enter, 100);
        e.tick(100);
        let raw = e.frame(100).rows.join("\n");
        assert!(raw.contains("\u{1b}[2mcommit this"), "{raw:?}");
        type_text(&mut e, "y", 200);
        assert!(!e.plain(200).contains("commit this"));
    }

    #[test]
    fn exit_and_crash_end_the_process() {
        let mut e = engine("[[turn]]\nsteps = [{ do = \"crash\", message = \"boom\", code = 7 }]");
        type_text(&mut e, "x", 0);
        e.on_key(Key::Enter, 100);
        assert_eq!(e.exit_code(), Some(7));
        assert!(e.plain(100).contains("boom"));
    }

    #[test]
    fn esc_interrupts_a_turn_unless_the_script_says_it_ignores_esc() {
        let toml = "[[turn]]\nsteps = [{ do = \"think\", ms = 5000 }, { do = \"say\", text = \"late\" }]";
        let mut e = engine(toml);
        type_text(&mut e, "x", 0);
        e.on_key(Key::Enter, 100);
        e.on_key(Key::Esc, 200);
        assert!(e.plain(200).contains("Interrupted") && !e.plain(200).contains("esc to interrupt"));
        let mut stubborn = engine(&format!("interrupt = false\n{toml}"));
        type_text(&mut stubborn, "x", 0);
        stubborn.on_key(Key::Enter, 100);
        stubborn.on_key(Key::Esc, 200);
        assert!(stubborn.plain(200).contains("esc to interrupt"));
    }

    #[test]
    fn write_file_writes_under_the_working_directory() {
        let dir = std::env::temp_dir().join(format!("pm-fake-tui-write-{}", std::process::id()));
        let script = Script::parse("[[turn]]\nsteps = [{ do = \"write_file\", path = \"sub/out.txt\", content = \"hello\\n\" }]", false).unwrap();
        let mut e = Engine::new(script, dir.clone(), 100, 30, 0);
        type_text(&mut e, "x", 0);
        e.on_key(Key::Enter, 100);
        assert_eq!(std::fs::read_to_string(dir.join("sub/out.txt")).unwrap(), "hello\n");
        assert!(e.plain(100).contains("Write(sub/out.txt)"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
