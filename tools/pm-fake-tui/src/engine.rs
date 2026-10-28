//! The fake agent's brain: a state machine driven by keys and a clock. It owns no terminal and no
//! real time (callers pass `now_ms`), so every behaviour is unit-tested deterministically; the real
//! console loop is `runtime.rs`.

use crate::keys::Key;
use crate::render::{self, Banner, Block, Dialog, Frame, GateView, Mode, Opts, View};
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
    /// A startup screen was passed and the agent is not taking keys yet: first the answered screen
    /// stays drawn (`linger_ms`, `frozen`), then the composer is drawn while it loads (`warm_ms`).
    /// Keys are held and taken in order when both are over.
    Warming { until: u64, held: Vec<Key>, frozen: Option<GateView>, warm_after: u64 },
    Ready,
    /// A turn is running: the queue of steps, one of which may be sleeping.
    Busy,
    Approval { dialog: Dialog, on_allow: Vec<Step>, on_deny: Vec<Step> },
    /// Waiting for the usage limit to reset ("continuing automatically ... esc to cancel").
    LimitWait,
    /// The screen is held still (a question nothing knows) until any key.
    Hold,
    /// The process is going away; the screen stays as it was.
    Exited { frozen: Option<GateView> },
}

struct Sleep {
    verb: String,
    until: u64,
}

/// A `run` step in flight. The command runs on its own thread so this agent keeps its clock going
/// (the spinner moves) the way a real agent does while its tool works; a frozen screen reads as a
/// finished pane to anything watching it.
struct RunningCommand {
    command: String,
    done: std::sync::mpsc::Receiver<(i32, String)>,
}

/// Claude Code collapses a paste longer than this many characters into `[Pasted text #N +M lines]`
/// (2.1.302: `C.length>800`), however few lines it has.
const PASTE_COLLAPSE_CHARS: usize = 800;

/// Text that arrives no more than this long after the previous text is part of the same paste: a
/// program's write reaches the console as one burst, a person types slower.
const PASTE_GAP_MS: u64 = 50;

/// A paste the Claude layout shows as `[Pasted text #id +lines lines]`: the composer bytes
/// `start..end`. The submitted prompt is the full text, as with the real Claude Code.
struct PastedText {
    id: u32,
    start: usize,
    end: usize,
}

pub struct Engine {
    script: Script,
    layout: Layout,
    base_dir: PathBuf,
    banner: Banner,
    opts: Opts,
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
    running: Option<RunningCommand>,
    verb: String,
    turn_started: u64,
    turns_done: usize,
    last_prompt: String,
    last_typed: Option<u64>,
    /// The burst of text being typed or pasted now: where it starts in the composer, when its last
    /// text arrived.
    burst: Option<(usize, u64)>,
    pastes: Vec<PastedText>,
    pastes_made: u32,
    notice: Option<(String, Option<u64>)>,
    ctrl_c_armed: bool,
    exit: Option<i32>,
    log: Option<std::fs::File>,
    transcript: Option<crate::transcript::Transcript>,
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
            Layout::Nimbus => Banner {
                version: script.version.clone().unwrap_or_else(|| "0.4.2".into()),
                model: script.model.clone().unwrap_or_else(|| "local".into()),
                cwd,
            },
        };
        let log = std::env::var("PM_FAKE_TUI_LOG")
            .ok()
            .or_else(|| script.log.clone())
            .filter(|path| !path.is_empty())
            .and_then(|path| std::fs::OpenOptions::new().create(true).append(true).open(path).ok());
        let transcript = crate::transcript::Transcript::from_settings(script.transcript.as_deref(), script.transcript_shape.as_deref(), &base_dir);
        let mut engine = Engine {
            gates: script.startup.iter().cloned().collect(),
            opts: Opts {
                backlog_rows: script.backlog_rows,
                banner_rows: script.banner_rows.clone(),
                status_model: script.status_model.clone(),
                status_model_busy_only: script.status_model_when == crate::script::StatusWhen::Busy,
                footer_right: script.footer_right.clone(),
                account_row: script.account_row.clone(),
                status_model_clip: script.status_model_clip,
                composer_glyph: script.composer_glyph.clone(),
            },
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
            running: None,
            verb: "Generating".into(),
            turn_started: 0,
            turns_done: 0,
            last_prompt: String::new(),
            last_typed: None,
            burst: None,
            pastes: Vec::new(),
            pastes_made: 0,
            notice: None,
            ctrl_c_armed: false,
            exit: None,
            log,
            transcript,
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

    /// Keys handled per loop turn; 0 = all of them (`Script::input_keys_per_turn`).
    pub fn keys_per_turn(&self) -> usize {
        self.script.input_keys_per_turn
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
            Phase::Busy if self.running.is_some() => {
                if let Some((code, tail)) = self.command_result() {
                    self.finish_command(code, tail, now);
                    self.run_queue(now);
                }
            }
            Phase::Warming { until, .. } if now >= *until => {
                if let Phase::Warming { held, warm_after, .. } = std::mem::replace(&mut self.phase, Phase::Ready) {
                    if warm_after > 0 {
                        // The answered screen is gone; the agent still loads behind its composer.
                        self.log(now, "warming", json!({ "ms": warm_after }));
                        self.phase = Phase::Warming { until: now + warm_after, held, frozen: None, warm_after: 0 };
                        return;
                    }
                    self.log(now, "warm_done", json!({ "held_keys": held.len() }));
                    self.begin_next_startup(now);
                    // The keys typed while it loaded arrive now, in one go and in order.
                    for key in held {
                        self.dispatch(key, now);
                    }
                }
            }
            _ => {}
        }
    }

    /// After a startup screen is passed: on to the next one, or first keep `view` drawn for
    /// `linger_ms` and then load for `warm_ms`, holding keys.
    fn after_gate(&mut self, view: GateView, linger_ms: u64, warm_ms: u64, now: u64) {
        if linger_ms > 0 {
            self.phase = Phase::Warming { until: now + linger_ms, held: Vec::new(), frozen: Some(view), warm_after: warm_ms };
        } else if warm_ms > 0 {
            self.log(now, "warming", json!({ "ms": warm_ms }));
            self.phase = Phase::Warming { until: now + warm_ms, held: Vec::new(), frozen: None, warm_after: 0 };
        } else {
            self.begin_next_startup(now);
        }
    }

    // ---- keys -------------------------------------------------------------------------------

    pub fn on_key(&mut self, key: Key, now: u64) {
        // Typed characters are not logged one by one (the `prompt` event carries the submitted text).
        if !matches!(key, Key::Char(_) | Key::Paste(_)) {
            self.log(now, "key", json!({ "key": format!("{key:?}") }));
        } else if matches!(self.phase, Phase::Gate { .. } | Phase::BeforeGate { .. } | Phase::Hold) {
            // Text typed at a startup screen or a question nothing knows: tests that say "nothing was typed
            // into the dialog" read this (the characters are not otherwise logged).
            self.log(now, "typed_at_gate", json!({ "chars": match &key { Key::Char(_) => 1, Key::Paste(text) => text.chars().count(), _ => 0 } }));
        }
        if let Phase::Warming { held, .. } = &mut self.phase {
            held.push(key);
            return;
        }
        self.dispatch(key, now);
    }

    fn dispatch(&mut self, key: Key, now: u64) {
        // `0` ignores the key; `1` is a composer drawn before a startup screen (it takes typing).
        let kind = match &self.phase {
            Phase::Loading { .. } | Phase::Exited { .. } | Phase::Warming { .. } => 0,
            Phase::BeforeGate { gate, .. } => u8::from(gate.composer_first),
            Phase::Gate { .. } => 2,
            Phase::Ready => 3,
            Phase::Busy => 4,
            Phase::Approval { .. } => 5,
            Phase::LimitWait => 6,
            Phase::Hold => 7,
        };
        match kind {
            // A composer that is already on screen takes typing; Enter does not submit yet.
            1 if !matches!(key, Key::Enter) => self.compose(key, now),
            2 => self.gate_key(key, now),
            3 => self.ready_key(key, now),
            4 if matches!(key, Key::Esc | Key::CtrlC) && self.script.interrupt => self.interrupt(now),
            5 => self.approval_key(key, now),
            7 => self.finish_turn(now, true),
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
            Key::Char(c)
                if gate.gate == GateKind::UpdateMenu
                    || (self.layout != Layout::Claude
                        && matches!(gate.style, crate::script::TrustStyle::Numbered | crate::script::TrustStyle::FolderAccess | crate::script::TrustStyle::FolderAccessBack)) =>
            {
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
        let (kind, layout, warm_ms, linger_ms, style) = (gate.gate, self.layout, gate.warm_ms, gate.linger_ms, gate.style);
        let view = GateView { kind: gate.gate, selected: *selected, style: gate.style, animate: gate.animate };
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
                    self.after_gate(view, linger_ms, warm_ms, now);
                }
            },
            GateKind::Trust => {
                // Claude: option 0 is "No, exit". Codex: option 1 is "No, quit".
                let declined = if layout == Layout::Claude || (layout == Layout::Nimbus && style == crate::script::TrustStyle::ArrowNoFirst) { choice == 0 } else { choice == 1 };
                if declined {
                    self.log(now, "gate_answer", json!({ "gate": "trust", "answer": "decline" }));
                    self.exit_with(1, now);
                } else {
                    self.log(now, "gate_answer", json!({ "gate": "trust", "answer": "trust" }));
                    self.after_gate(view, linger_ms, warm_ms, now);
                }
            }
        }
    }

    fn compose(&mut self, key: Key, now: u64) {
        match key {
            Key::Char(c) => {
                self.take_text(c.encode_utf8(&mut [0; 4]), now);
                self.suggestion = None;
                self.last_typed = Some(now);
            }
            Key::Paste(text) => {
                self.take_text(&text.replace("\r\n", "\n").replace('\r', "\n"), now);
                self.suggestion = None;
                self.last_typed = Some(now);
            }
            Key::Backspace => {
                // A paste placeholder goes as a whole.
                match self.pastes.last() {
                    Some(paste) if paste.end == self.composer.len() => {
                        self.composer.truncate(paste.start);
                        self.pastes.pop();
                    }
                    _ => {
                        self.composer.pop();
                    }
                }
                self.burst = None;
            }
            Key::CtrlU => self.clear_composer(),
            _ => {}
        }
    }

    fn clear_composer(&mut self) {
        self.composer.clear();
        self.pastes.clear();
        self.burst = None;
    }

    /// Text into the composer. Claude Code takes a burst of text as one paste, and shows a paste of
    /// more than [`PASTE_COLLAPSE_CHARS`] characters, or with more line breaks than
    /// `min(rows - 10, 2)`, as `[Pasted text #N +M lines]` instead of its text (2.1.302's prompt
    /// input). The other layouts show all of it.
    fn take_text(&mut self, text: &str, now: u64) {
        let start = match self.burst {
            Some((start, at)) if now.saturating_sub(at) <= PASTE_GAP_MS => start,
            _ => self.composer.len(),
        };
        self.composer.push_str(text);
        self.burst = Some((start, now));
        if self.layout != Layout::Claude || self.script.no_paste_collapse {
            return;
        }
        let pasted = &self.composer[start..];
        let breaks = pasted.matches('\n').count();
        if pasted.chars().count() <= PASTE_COLLAPSE_CHARS && breaks <= self.rows.saturating_sub(10).min(2) {
            return;
        }
        let end = self.composer.len();
        match self.pastes.last_mut() {
            Some(paste) if paste.start == start => paste.end = end,
            _ => {
                self.pastes_made += 1;
                self.pastes.push(PastedText { id: self.pastes_made, start, end });
                self.log(now, "paste_collapsed", json!({ "id": self.pastes_made }));
            }
        }
    }

    /// The composer as drawn: Claude's collapsed pastes as their placeholders.
    fn shown_composer(&self) -> std::borrow::Cow<'_, str> {
        if self.pastes.is_empty() {
            return std::borrow::Cow::Borrowed(&self.composer);
        }
        let mut out = String::new();
        let mut at = 0;
        for paste in &self.pastes {
            out.push_str(&self.composer[at..paste.start]);
            match self.composer[paste.start..paste.end].matches('\n').count() {
                0 => out.push_str(&format!("[Pasted text #{}]", paste.id)),
                lines => out.push_str(&format!("[Pasted text #{} +{lines} lines]", paste.id)),
            }
            at = paste.end;
        }
        out.push_str(&self.composer[at..]);
        std::borrow::Cow::Owned(out)
    }

    fn ready_key(&mut self, key: Key, now: u64) {
        match key {
            Key::Enter => {
                let burst = self.script.burst_ms();
                let quick = burst > 0 && self.last_typed.is_some_and(|at| now.saturating_sub(at) < burst);
                if quick {
                    match self.script.burst_effect() {
                        BurstEffect::Newline => self.take_text("\n", now),
                        BurstEffect::Drop => {}
                    }
                    self.log(now, "enter_in_paste_burst", json!({ "effect": format!("{:?}", self.script.burst_effect()) }));
                    return;
                }
                let prompt = self.composer.trim_end().to_string();
                if prompt.trim().is_empty() {
                    return;
                }
                // The agent no profile knows quits on `/exit` (the Agent Wizard types it on request).
                if self.layout == Layout::Nimbus && prompt.trim() == "/exit" {
                    self.log(now, "prompt", json!({ "text": prompt }));
                    self.exit_with(0, now);
                    return;
                }
                self.submit(prompt, now);
            }
            Key::CtrlC => {
                if !self.composer.is_empty() {
                    self.clear_composer();
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
        use render::NimbusMenu as M;
        let styled = self.layout == Layout::Nimbus && matches!(dialog.menu, M::Inverse | M::Boxed | M::Buttons | M::ButtonsArrows | M::Inline | M::AlwaysOnly | M::Blind);
        let buttons = self.layout == Layout::Nimbus && matches!(dialog.menu, M::Buttons | M::ButtonsArrows);
        match key {
            // the styled questions answer only to their own keys: rows move with Up / Down, a row of buttons with Tab (`buttons`)
            // or Left / Right (`buttons_arrows`), the inline question with its letters; Enter confirms (not the inline one)
            Key::Up | Key::Down if styled && (buttons || dialog.menu == M::Inline) => {}
            Key::Tab if buttons && dialog.menu == M::Buttons => dialog.selected = (dialog.selected + 1) % options,
            Key::Right if buttons && dialog.menu == M::ButtonsArrows => dialog.selected = (dialog.selected + 1).min(options - 1),
            Key::Left if buttons && dialog.menu == M::ButtonsArrows => dialog.selected = dialog.selected.saturating_sub(1),
            Key::Enter if dialog.menu == M::Inline => {}
            Key::Esc if styled => {}
            Key::Char('y' | 'Y') if self.layout == Layout::Nimbus && dialog.menu == M::Inline => confirm = Some(0),
            Key::Char('n' | 'N') if self.layout == Layout::Nimbus && dialog.menu == M::Inline => confirm = Some(1),
            Key::Char(_) if styled => {}
            Key::Up => dialog.selected = dialog.selected.saturating_sub(1),
            Key::Down => dialog.selected = (dialog.selected + 1).min(options - 1),
            Key::Enter => confirm = Some(dialog.selected),
            Key::Esc => confirm = Some(options - 1),
            // The arrow menu of a nimbus file question takes arrows and Enter only; the y/n line takes neither digits
            // beyond its two rows nor letters beyond y and n; a numbered command menu takes its digits.
            Key::Char(_) if self.layout == Layout::Nimbus && dialog.menu == render::NimbusMenu::File => {}
            Key::Char(c) if self.layout == Layout::Nimbus && matches!(dialog.menu, render::NimbusMenu::Command | render::NimbusMenu::RunNumbered | render::NimbusMenu::BulletsNumbered) => {
                if let Some(n) = c.to_digit(10).filter(|n| (1..=options as u32).contains(n)) {
                    confirm = Some(n as usize - 1);
                }
            }
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
                } else if self.layout == Layout::Nimbus {
                    confirm = match c {
                        'y' | 'Y' => Some(0),
                        'n' | 'N' => Some(options - 1),
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
                (Layout::Nimbus, _) => format!("denied: {subject}"),
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
            (Layout::Nimbus, _) => format!("allowed: {subject}"),
        };
        Block::Tool { label, detail: None, failed: None }
    }

    // ---- turns ------------------------------------------------------------------------------

    fn submit(&mut self, prompt: String, now: u64) {
        self.log(now, "prompt", json!({ "text": prompt }));
        if let Some(transcript) = &self.transcript {
            transcript.user(&prompt);
        }
        self.last_prompt = prompt.clone();
        if self.script.echo_task || self.layout != Layout::Nimbus {
            self.blocks.push(Block::User(prompt.clone()));
        }
        self.clear_composer();
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
        while matches!(self.phase, Phase::Busy) && self.sleep.is_none() && self.running.is_none() {
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
                Step::Say { text } => {
                    let text = self.fill(&text);
                    if let Some(transcript) = &self.transcript {
                        transcript.assistant(&text);
                    }
                    self.blocks.push(Block::Reply(text));
                }
                Step::Lines { count, template } => {
                    let template = template.unwrap_or_else(|| "line {n} of the long answer".into());
                    let text: Vec<String> = (1..=count).map(|n| template.replace("{n}", &n.to_string())).collect();
                    self.blocks.push(Block::Reply(text.join("\n")));
                }
                Step::Link { text, target, before, after } => {
                    // OSC 8: `ESC ] 8 ; ; URI ESC \` opens the link, the same with an empty URI closes it.
                    let open = format!("{ESC}]8;;{target}{ESC}{BACKSLASH}");
                    let close = format!("{ESC}]8;;{ESC}{BACKSLASH}");
                    let row = format!("{}{before}{open}{text}{close}{after}", if self.layout == Layout::Nimbus { "reply: " } else { "" });
                    self.blocks.push(Block::Raw(vec![row]));
                }
                Step::Tool { label, detail, ms, failed } => {
                    if let Some(transcript) = self.transcript.as_mut() {
                        transcript.tool(&label, detail.as_deref().unwrap_or(""));
                    }
                    self.blocks.push(Block::Tool { label, detail, failed });
                    if ms > 0 {
                        self.sleep = Some(Sleep { verb: self.verb.clone(), until: now + ms });
                    }
                }
                Step::Approve { kind, command, path, diff, auto_mode_option, style, on_allow, on_deny } => {
                    let menu = match (self.layout, style.unwrap_or(self.script.approvals), kind) {
                        (Layout::Nimbus, crate::script::Approvals::Menus, ApprovalKind::Bash) => render::NimbusMenu::Command,
                        (Layout::Nimbus, crate::script::Approvals::Menus, _) => render::NimbusMenu::File,
                        (Layout::Nimbus, crate::script::Approvals::Inverse, _) => render::NimbusMenu::Inverse,
                        (Layout::Nimbus, crate::script::Approvals::Boxed, _) => render::NimbusMenu::Boxed,
                        (Layout::Nimbus, crate::script::Approvals::Buttons, _) => render::NimbusMenu::Buttons,
                        (Layout::Nimbus, crate::script::Approvals::ButtonsArrows, _) => render::NimbusMenu::ButtonsArrows,
                        (Layout::Nimbus, crate::script::Approvals::Inline, _) => render::NimbusMenu::Inline,
                        (Layout::Nimbus, crate::script::Approvals::AlwaysOnly, _) => render::NimbusMenu::AlwaysOnly,
                        (Layout::Nimbus, crate::script::Approvals::Blind, _) => render::NimbusMenu::Blind,
                        (Layout::Nimbus, crate::script::Approvals::RunNumbered, _) => render::NimbusMenu::RunNumbered,
                        (Layout::Nimbus, crate::script::Approvals::YesMissed, _) => render::NimbusMenu::YesMissed,
                        (Layout::Nimbus, crate::script::Approvals::BulletsNumbered, _) => render::NimbusMenu::BulletsNumbered,
                        _ => render::NimbusMenu::YesNo,
                    };
                    let dialog = Dialog {
                        menu,
                        kind,
                        command: command.unwrap_or_default(),
                        path: path.unwrap_or_default(),
                        diff,
                        auto_mode_option,
                        selected: 0,
                    };
                    let mut dialog = dialog;
                    if self.layout == Layout::Nimbus {
                        dialog.selected = self.script.approval_cursor.min(dialog.options(self.layout).len() - 1);
                    }
                    self.log(now, "approval_shown", json!({ "kind": format!("{kind:?}").to_lowercase(), "command": dialog.command, "path": dialog.path }));
                    self.phase = Phase::Approval { dialog, on_allow, on_deny };
                    return;
                }
                Step::WriteFile { path, content, append, quiet } => {
                    let content = self.fill(&content);
                    self.write_file(&path, &content, append, quiet, now)
                }
                Step::Run { command } => {
                    let command = self.fill(&command);
                    self.run_command(&command)
                }
                Step::Suggest { text } => self.pending_suggestion = Some(text),
                Step::Limit { text, reset, auto_continue } => {
                    let reset = reset.unwrap_or_else(|| "9:45 PM".into());
                    let text = text.unwrap_or_else(|| match self.layout {
                        Layout::Codex => format!("You\u{2019}ve hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/settings/usage to purchase more credits or try again at {reset}."),
                        Layout::Nimbus => format!("quota used up, resets {reset}"),
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
                Step::Hold { text } => {
                    self.blocks.push(Block::Raw(text.lines().map(str::to_string).collect()));
                    self.sleep = None;
                    self.phase = Phase::Hold;
                    return;
                }
            }
        }
    }

    /// `{last_word}` in a step's text is the last word of the prompt that started the turn, without a
    /// final stop: a scripted agent can then answer "reply with exactly the word X" for any X.
    fn fill(&self, text: &str) -> String {
        let last = self.last_prompt.split_whitespace().last().unwrap_or("").trim_end_matches(['.', '!', '?']).to_string();
        text.replace("{last_word}", &last)
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
                (Ok(()), Layout::Nimbus) => (format!("wrote {path}"), format!("{lines} line(s)")),
                (Err(e), _) => (format!("Write({path})"), format!("Error: {e}")),
            };
            self.blocks.push(Block::Tool { label, detail: Some(detail), failed: None });
        }
    }

    /// `Step::Run`: starts the command on a thread of its own (the queue waits for it in `tick`).
    fn run_command(&mut self, command: &str) {
        let (send, done) = std::sync::mpsc::channel();
        let (line, dir) = (command.to_string(), self.base_dir.clone());
        std::thread::spawn(move || {
            #[cfg(windows)]
            let mut cmd = {
                // The command line goes to cmd.exe as it is written: no argument quoting in between.
                use std::os::windows::process::CommandExt;
                let mut c = std::process::Command::new("cmd");
                c.arg("/C").raw_arg(&line);
                c
            };
            #[cfg(not(windows))]
            let mut cmd = {
                let mut c = std::process::Command::new("sh");
                c.args(["-c", &line]);
                c
            };
            let out = cmd.current_dir(dir).stdin(std::process::Stdio::null()).output();
            let _ = send.send(match &out {
                Ok(o) => {
                    let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
                    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                    let from = lines.len().saturating_sub(5);
                    (o.status.code().unwrap_or(-1), lines[from..].join("
"))
                }
                Err(e) => (-1, e.to_string()),
            });
        });
        self.running = Some(RunningCommand { command: command.to_string(), done });
    }

    /// The finished command's exit code and output tail, once there is one.
    fn command_result(&mut self) -> Option<(i32, String)> {
        use std::sync::mpsc::TryRecvError;
        match self.running.as_ref()?.done.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some((-1, "the command thread ended without an answer".into())),
        }
    }

    fn finish_command(&mut self, code: i32, tail: String, now: u64) {
        let Some(RunningCommand { command, .. }) = self.running.take() else { return };
        self.log(now, "run", json!({ "command": command, "code": code, "output": tail }));
        let (label, detail) = match self.layout {
            Layout::Claude => (format!("Bash({command})"), format!("exit {code}")),
            Layout::Codex => (format!("Ran {command}"), format!("exit {code}")),
            Layout::Nimbus => (format!("ran {command}"), format!("exit {code}")),
        };
        self.blocks.push(Block::Tool { label, detail: Some(detail), failed: (code != 0).then_some(code) });
    }

    fn interrupt(&mut self, now: u64) {
        self.log(now, "interrupted", json!({}));
        self.queue.clear();
        self.sleep = None;
        self.running = None;
        self.blocks.push(Block::Note(match self.layout {
            Layout::Claude => "Interrupted \u{b7} What should Claude do instead?".into(),
            Layout::Codex => "Conversation interrupted - tell the model what to do differently.".into(),
            Layout::Nimbus => "interrupted".into(),
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
                Layout::Nimbus => match &self.script.summary_row {
                    Some(row) => row.replace("{secs}", &secs.to_string()),
                    None => format!("done in {secs}s"),
                },
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
            Phase::Gate { gate, selected } => Some(GateView { kind: gate.gate, selected: *selected, style: gate.style, animate: gate.animate }),
            _ => None,
        };
        self.log(now, "exit", json!({ "code": code }));
        self.exit = Some(code);
        self.queue.clear();
        self.sleep = None;
        self.running = None;
        self.phase = Phase::Exited { frozen };
    }

    // ---- drawing ----------------------------------------------------------------------------

    pub fn frame(&self, now: u64) -> Frame {
        self.build_frame(now, true)
    }

    /// Every row of the conversation (inline painting): not cut to the screen's height.
    pub fn frame_all(&self, now: u64) -> Frame {
        self.build_frame(now, false)
    }

    pub fn inline(&self) -> bool {
        self.script.inline
    }

    pub fn reprints_on_resize(&self) -> bool {
        self.script.reprint_on_resize
    }

    fn build_frame(&self, now: u64, fit_height: bool) -> Frame {
        let mode = match &self.phase {
            Phase::Loading { .. } => Mode::Loading,
            Phase::BeforeGate { gate, .. } if !gate.composer_first => Mode::Loading,
            // The answered screen still drawn (the agent redraws late).
            Phase::Warming { frozen: Some(view), .. } => Mode::Gate(view.clone()),
            // Still loading after a startup screen, but drawn like a ready composer (that is the trap).
            Phase::BeforeGate { .. } | Phase::Warming { .. } | Phase::Ready | Phase::Busy | Phase::LimitWait => Mode::Ready,
            Phase::Gate { gate, selected } => Mode::Gate(GateView { kind: gate.gate, selected: *selected, style: gate.style, animate: gate.animate }),
            Phase::Approval { dialog, .. } => Mode::Approval(dialog.clone()),
            Phase::Hold => Mode::Hold,
            Phase::Exited { frozen: Some(gate) } => Mode::Gate(gate.clone()),
            Phase::Exited { frozen: None } => Mode::Ready,
        };
        let busy = match &self.phase {
            Phase::Busy => Some((self.sleep.as_ref().map_or(self.verb.as_str(), |s| s.verb.as_str()), now.saturating_sub(self.turn_started))),
            _ => None,
        };
        let composer = self.shown_composer();
        let view = View {
            layout: self.layout,
            cols: self.cols,
            rows: self.rows,
            banner: &self.banner,
            blocks: &self.blocks,
            mode,
            composer: &composer,
            suggestion: self.suggestion.as_deref(),
            fresh: self.fresh,
            busy,
            notice: self.notice.as_ref().map(|(text, _)| text.as_str()),
            tick: now / 200,
            opts: &self.opts,
        };
        if fit_height {
            render::frame(&view)
        } else {
            render::frame_all(&view)
        }
    }

    /// The whole screen as plain text, for tests.
    pub fn plain(&self, now: u64) -> String {
        self.frame(now).rows.iter().map(|row| strip(row)).collect::<Vec<_>>().join("\n")
    }
}

const ESC: char = '\x1b';
const BACKSLASH: char = '\\';

/// Escape sequences removed.
pub fn strip(row: &str) -> String {
    let mut out = String::new();
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            render::take_escape(&mut chars);
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

    /// The 0.162.1 trust dialog as the live pane draws it, and the same dialog reached from Codex's Agent Command
    /// Center (second row `Back to ...`, footer `esc back`).
    #[test]
    fn codex_folder_access_draws_the_live_dialog_and_the_back_row_declines() {
        let mut e = engine("agent = \"codex\"\n[[startup]]\ngate = \"trust\"\nstyle = \"folder_access\"");
        e.tick(0);
        let s = e.plain(0);
        assert!(s.contains("Folder access") && s.contains("Trust this folder?") && s.contains("1. Trust and continue") && s.contains("2. Quit") && s.contains("enter continue \u{b7} esc quit"), "{s}");
        e.on_key(Key::Enter, 10);
        assert_eq!(e.exit_code(), None, "Enter on the pointed row trusts");

        let mut back = engine("agent = \"codex\"\n[[startup]]\ngate = \"trust\"\nstyle = \"folder_access_back\"");
        back.tick(0);
        let s = back.plain(0);
        assert!(s.contains("2. Back to Agent Command Center") && s.contains("enter continue \u{b7} esc back") && !s.contains("Quit"), "{s}");
        back.on_key(Key::Down, 10);
        back.on_key(Key::Enter, 20);
        assert_eq!(back.exit_code(), Some(1), "the Back row declines");
    }

    /// A CLI update that redraws the input box: only the glyph that starts the composer row changes.
    #[test]
    fn a_composer_glyph_changes_the_codex_input_row_and_nothing_else() {
        let mut real = engine("agent = \"codex\"");
        real.tick(0);
        let mut changed = engine("agent = \"codex\"\ncomposer_glyph = \"\u{bb}\"");
        changed.tick(0);
        let (a, b) = (real.plain(500), changed.plain(500));
        assert!(a.contains("\u{203a} Ask Codex to do anything"), "{a}");
        assert!(b.contains("\u{bb} Ask Codex to do anything") && !b.contains('\u{203a}'), "{b}");
        assert_eq!(a.replace('\u{203a}', "\u{bb}"), b, "only the glyph differs");
    }

    /// Codex 0.162.1 after Trust (live v0.2.0 hunt, cx5): the composer is drawn at once, but typed
    /// keys show up only when it has finished loading, all at once and in order.
    #[test]
    fn after_trust_a_warming_codex_holds_keys_and_takes_them_in_order_when_ready() {
        let mut e = engine("agent = \"codex\"\n[[startup]]\ngate = \"trust\"\nwarm_ms = 3000\n[[turn]]\nsteps = [{ do = \"say\", text = \"done\" }]");
        e.tick(0);
        e.on_key(Key::Enter, 10); // Trust and continue
        let warming = e.plain(20);
        assert!(warming.contains("Ask Codex to do anything") && !warming.contains("Do you trust"), "{warming}");
        type_text(&mut e, "first", 100);
        e.on_key(Key::CtrlU, 200);
        type_text(&mut e, "second", 300);
        assert_eq!(e.composer(), "", "nothing shows while it loads");
        e.tick(2999);
        assert_eq!(e.composer(), "");
        e.tick(3010);
        assert_eq!(e.composer(), "second", "the held keys arrive in order: the clear wiped the first copy");
        e.on_key(Key::Enter, 3500);
        assert!(e.plain(3600).contains("done"), "{}", e.plain(3600));
    }

    /// Codex redraws its trust dialog away a moment after the key: until then it is still drawn,
    /// and further keys are held for whatever comes next.
    #[test]
    fn a_lingering_dialog_stays_drawn_and_holds_the_keys_that_follow() {
        let mut e = engine("agent = \"codex\"\n[[startup]]\ngate = \"trust\"\nlinger_ms = 400");
        e.tick(0);
        e.on_key(Key::Enter, 10);
        assert!(e.plain(100).contains("Do you trust"), "still drawn: {}", e.plain(100));
        e.on_key(Key::Enter, 120);
        e.on_key(Key::Enter, 200);
        e.tick(500);
        assert!(!e.plain(500).contains("Do you trust") && e.plain(500).contains("Ask Codex to do anything"), "{}", e.plain(500));
        assert_eq!(e.composer(), "", "the held Enters met an empty composer");
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

    /// Claude Code 2.1.302 shows a paste with more than two line breaks as a placeholder; Enter
    /// sends the whole text.
    #[test]
    fn claude_shows_a_long_paste_as_a_placeholder_and_sends_all_of_it() {
        let mut e = engine("agent = \"claude\"\npaste_burst_ms = 40\npaste_burst_effect = \"newline\"");
        // A program's write: text and Enters in one burst (the Enters are line breaks in it).
        for (i, line) in ["Mission: one", "two", "three", "four"].iter().enumerate() {
            if i > 0 {
                e.on_key(Key::Enter, 1000);
            }
            type_text(&mut e, line, 1000);
        }
        let screen = e.plain(1000);
        assert!(screen.contains("\u{276f}\u{a0}[Pasted text #1 +3 lines]"), "{screen}");
        assert!(!screen.contains("Mission: one"), "{screen}");
        assert_eq!(e.composer(), "Mission: one\ntwo\nthree\nfour");
        e.on_key(Key::Enter, 1200);
        assert_eq!(e.last_prompt, "Mission: one\ntwo\nthree\nfour");
    }

    #[test]
    fn claude_keeps_short_or_typed_text_in_the_box() {
        // Two line breaks stay as text.
        let mut e = engine("agent = \"claude\"\npaste_burst_ms = 40\npaste_burst_effect = \"newline\"");
        type_text(&mut e, "a", 0);
        e.on_key(Key::Enter, 0);
        type_text(&mut e, "b", 0);
        e.on_key(Key::Enter, 0);
        type_text(&mut e, "c", 0);
        assert!(e.plain(0).contains("\u{276f}\u{a0}a"), "{}", e.plain(0));
        // A person typing (keys further apart than a burst) never makes a paste, however long.
        let mut e = engine("agent = \"claude\"");
        for i in 0..900u64 {
            e.on_key(Key::Char('x'), 1000 + i * 100);
        }
        assert!(!e.plain(100_000).contains("Pasted text"));
        // One write of more than 800 characters is a paste, even on one line.
        let mut e = engine("agent = \"claude\"");
        type_text(&mut e, &"y".repeat(801), 0);
        assert!(e.plain(0).contains("\u{276f}\u{a0}[Pasted text #1]"), "{}", e.plain(0));
    }

    #[test]
    fn backspace_takes_a_paste_placeholder_away_whole_and_codex_never_collapses() {
        let mut e = engine("agent = \"claude\"");
        type_text(&mut e, "hi ", 0);
        e.on_key(Key::Paste("one\ntwo\nthree\nfour".into()), 500);
        assert!(e.plain(500).contains("\u{276f}\u{a0}hi [Pasted text #1 +3 lines]"), "{}", e.plain(500));
        e.on_key(Key::Backspace, 600);
        assert_eq!(e.composer(), "hi ");
        let mut e = engine("agent = \"codex\"");
        e.on_key(Key::Paste("one\ntwo\nthree\nfour".into()), 0);
        assert!(!e.plain(0).contains("Pasted text"), "{}", e.plain(0));
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
    fn run_executes_a_command_in_the_working_directory_and_logs_its_exit() {
        let dir = std::env::temp_dir().join(format!("pm-fake-tui-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let command = if cfg!(windows) { "echo ran> ran.txt" } else { "echo ran > ran.txt" };
        let script = Script::parse(&format!("[[turn]]
steps = [{{ do = \"run\", command = {command:?} }}, {{ do = \"run\", command = \"exit 3\" }}]"), false).unwrap();
        let mut e = Engine::new(script, dir.clone(), 100, 30, 0);
        type_text(&mut e, "x", 0);
        e.on_key(Key::Enter, 100);
        let t = wait_for_turn(&mut e, 100);
        assert!(dir.join("ran.txt").is_file(), "the command ran in the working directory");
        let screen = e.plain(t);
        assert!(screen.contains("Bash(echo ran") && screen.contains("exit 0") && screen.contains("exit 3"), "{screen}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Ticks (a real clock, 10 ms apart) until the turn is over; returns the last `now`.
    fn wait_for_turn(e: &mut Engine, from: u64) -> u64 {
        let mut t = from;
        for _ in 0..3000 {
            if !e.plain(t).contains("esc to interrupt") {
                return t;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            t += 10;
            e.tick(t);
        }
        panic!("the turn did not end: {}", e.plain(t));
    }

    #[test]
    fn a_running_command_does_not_freeze_the_screen() {
        let dir = std::env::temp_dir().join(format!("pm-fake-tui-runbusy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let command = if cfg!(windows) { "ping -n 3 127.0.0.1 > nul" } else { "sleep 2" };
        let script = Script::parse(&format!("[[turn]]
steps = [{{ do = \"run\", command = {command:?} }}, {{ do = \"say\", text = \"Done.\" }}]"), false).unwrap();
        let mut e = Engine::new(script, dir.clone(), 100, 30, 0);
        type_text(&mut e, "x", 0);
        let started = std::time::Instant::now();
        e.on_key(Key::Enter, 100);
        // `on_key` came back while the command still runs: the turn is open, the spinner is drawn and moves.
        assert!(started.elapsed() < std::time::Duration::from_millis(1500), "on_key waited for the command");
        let first = e.plain(100);
        assert!(first.contains("esc to interrupt") && !first.contains("Done."), "{first}");
        let moves = (1..8).any(|n| e.plain(100 + n * 130) != first);
        assert!(moves, "the spinner moves while the command runs");
        let t = wait_for_turn(&mut e, 100);
        assert!(e.plain(t).contains("Done."), "{}", e.plain(t));
        let _ = std::fs::remove_dir_all(dir);
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

    #[test]
    fn a_transcript_is_written_only_when_asked_and_only_under_the_working_directory() {
        let dir = std::env::temp_dir().join(format!("pm-fake-tui-transcript-{}", std::process::id()));
        let toml = "transcript = \"chats/s.jsonl\"\n[[turn]]\nsteps = [{ do = \"tool\", label = \"ls\", detail = \"a.txt\" }, { do = \"say\", text = \"Done.\" }]";
        let mut e = Engine::new(Script::parse(toml, false).unwrap(), dir.clone(), 100, 30, 0);
        type_text(&mut e, "hello", 0);
        e.on_key(Key::Enter, 100);
        let lines: Vec<String> = std::fs::read_to_string(dir.join("chats/s.jsonl")).unwrap().lines().map(str::to_string).collect();
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert_eq!(lines[0], r#"{"content":"hello","role":"user"}"#);
        assert!(lines[1].contains(r#""name":"ls""#) && lines[2].contains(r#""role":"tool""#));
        assert_eq!(lines[3], r#"{"content":"Done.","role":"assistant"}"#);
        // a path that leaves the folder is ignored
        for bad in ["../x.jsonl", "/abs.jsonl"] {
            assert!(crate::transcript::Transcript::from_settings(Some(bad), None, &dir).is_none(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(dir);
        let plain = Engine::new(Script::parse("[[turn]]\nsteps = [{ do = \"say\", text = \"x\" }]", false).unwrap(), std::env::temp_dir().join("pm-fake-tui-no-transcript"), 100, 30, 0);
        assert!(plain.transcript.is_none());
    }
}

#[cfg(test)]
mod nimbus_tests {
    use super::*;

    fn engine(toml: &str) -> Engine {
        let script = Script::parse(&format!("agent = \"nimbus\"\n{toml}"), false).unwrap();
        Engine::new(script, std::env::temp_dir().join("pm-fake-tui-nimbus-unit"), 120, 40, 0)
    }

    fn type_text(e: &mut Engine, text: &str, now: u64) {
        for c in text.chars() {
            e.on_key(Key::Char(c), now);
        }
    }

    #[test]
    fn banner_rows_print_a_logo_and_a_name_and_version_row_under_the_banner() {
        let mut e = engine("banner_rows = [\"\\u2584\\u2580 Nimbus CLI 1.4.2.\", \"Signed in as dev@example.com\"]");
        e.tick(0);
        let rows: Vec<String> = e.plain(0).lines().map(str::to_string).collect();
        assert!(rows[0].starts_with("NIMBUS 0.4.2"), "{rows:?}");
        assert_eq!(rows[1], "\u{2584}\u{2580} Nimbus CLI 1.4.2.", "{rows:?}");
        assert_eq!(rows[2], "Signed in as dev@example.com", "{rows:?}");
        assert!(rows[3].trim().is_empty(), "{rows:?}");
    }

    #[test]
    fn nimbus_asks_about_the_folder_then_shows_a_boxed_input_row() {
        let mut e = engine("[[startup]]\ngate = \"trust\"");
        e.tick(0);
        let trust = e.plain(0);
        assert!(trust.contains("Do you trust the files in this folder?") && trust.contains("1. Yes, trust this folder"), "{trust}");
        e.on_key(Key::Enter, 10);
        assert_eq!(e.exit_code(), None);
        let ready = e.plain(20);
        assert!(ready.contains("| > Type a task..."), "{ready}");
        assert!(ready.starts_with("NIMBUS 0.4.2"), "{ready}");
        // declining ends the process and leaves the question drawn
        let mut no = engine("[[startup]]\ngate = \"trust\"");
        no.tick(0);
        no.on_key(Key::Char('2'), 10);
        assert_eq!(no.exit_code(), Some(1));
    }

    #[test]
    fn nimbus_spins_while_it_works_and_answers_with_the_last_word_of_the_prompt() {
        let mut e = engine("[[turn]]\nsteps = [{ do = \"think\", ms = 1000 }, { do = \"say\", text = \"{last_word}\" }]");
        type_text(&mut e, "Reply with exactly the word: pong-abcdef", 0);
        assert!(e.plain(0).contains("| > Reply with exactly the word: pong-abcdef"));
        e.on_key(Key::Enter, 100);
        let working = e.plain(300);
        assert!(working.contains("Generating...") && working.contains("ctrl+c to stop"), "{working}");
        assert!(working.contains("> Reply with exactly the word: pong-abcdef"));
        e.tick(1200);
        let done = e.plain(1200);
        assert!(done.contains("reply: pong-abcdef") && done.contains("done in 1s") && !done.contains("ctrl+c to stop"), "{done}");
    }

    #[test]
    fn nimbus_asks_before_it_creates_a_file_and_takes_y_or_n() {
        let toml = "[[turn]]\nsteps = [{ do = \"approve\", kind = \"create\", path = \"hello.txt\", on_allow = [{ do = \"say\", text = \"made it\" }], on_deny = [{ do = \"say\", text = \"not made\" }] }]";
        let mut yes = engine(toml);
        type_text(&mut yes, "make hello.txt", 0);
        yes.on_key(Key::Enter, 10);
        assert!(yes.plain(20).contains("Allow nimbus to create hello.txt? [y/n]"), "{}", yes.plain(20));
        assert!(!yes.plain(20).contains("Type a task"), "the question replaces the input box");
        yes.on_key(Key::Char('y'), 30);
        assert!(yes.plain(40).contains("reply: made it") && yes.plain(40).contains("note: allowed") || yes.plain(40).contains("allowed: hello.txt"), "{}", yes.plain(40));
        let mut no = engine(toml);
        type_text(&mut no, "make hello.txt", 0);
        no.on_key(Key::Enter, 10);
        no.on_key(Key::Char('n'), 30);
        let screen = no.plain(40);
        assert!(screen.contains("denied: hello.txt") && !screen.contains("made it"), "{screen}");
    }

    #[test]
    fn nimbus_draws_the_arrow_menu_trust_question_and_only_the_arrows_and_enter_answer_it() {
        let mut e = engine("[[startup]]\ngate = \"trust\"\nstyle = \"arrow\"");
        e.tick(0);
        let s = e.plain(0);
        assert!(s.contains("> Yes, I trust this folder") && s.contains("  No, exit") && s.contains("Navigate") && s.contains("enter Confirm"), "{s}");
        // digits do nothing in this menu
        e.on_key(Key::Char('1'), 10);
        assert!(e.plain(10).contains("Do you trust"), "digits do not answer an arrow menu");
        e.on_key(Key::Enter, 20);
        assert_eq!(e.exit_code(), None);
        assert!(e.plain(30).contains("| > Type a task..."), "{}", e.plain(30));
    }

    #[test]
    fn nimbus_arrow_menu_with_the_cursor_on_no_needs_down_then_enter() {
        let mut e = engine("[[startup]]\ngate = \"trust\"\nstyle = \"arrow_no_first\"");
        e.tick(0);
        let s = e.plain(0);
        assert!(s.contains("> No, exit") && s.contains("  Yes, I trust this folder"), "{s}");
        e.on_key(Key::Down, 10);
        assert!(e.plain(10).contains("> Yes, I trust this folder"), "{}", e.plain(10));
        e.on_key(Key::Enter, 20);
        assert!(e.plain(30).contains("| > Type a task..."));
        // Enter on the first row declines
        let mut no = engine("[[startup]]\ngate = \"trust\"\nstyle = \"arrow_no_first\"");
        no.tick(0);
        no.on_key(Key::Enter, 10);
        assert_eq!(no.exit_code(), Some(1));
    }

    #[test]
    fn nimbus_always_menu_has_no_plain_yes() {
        let mut e = engine("[[startup]]\ngate = \"trust\"\nstyle = \"always\"");
        e.tick(0);
        let s = e.plain(0);
        assert!(s.contains("> Yes, always trust this folder") && !s.contains("I trust this folder"), "{s}");
    }

    #[test]
    fn a_held_screen_has_no_composer_and_no_spinner_and_never_changes_until_a_key() {
        let toml = "[[turn]]\nsteps = [{ do = \"hold\", text = \"Where should hello.txt go?\\n  project folder   scratch folder\" }]";
        let mut e = engine(toml);
        type_text(&mut e, "make hello.txt", 0);
        e.on_key(Key::Enter, 10);
        let first = e.plain(500);
        assert!(first.contains("Where should hello.txt go?") && !first.contains("Type a task") && !first.contains("ctrl+c to stop"), "{first}");
        assert_eq!(e.plain(60_000), first, "nothing moves");
        e.on_key(Key::Enter, 61_000);
        let after = e.plain(61_000);
        assert!(after.contains("| > Type a task...") && after.contains("done in"), "{after}");
    }

    #[test]
    fn nimbus_quits_on_slash_exit() {
        let mut e = engine("");
        type_text(&mut e, "/exit", 0);
        e.on_key(Key::Enter, 10);
        assert_eq!(e.exit_code(), Some(0));
    }

    #[test]
    fn an_animated_trust_question_changes_on_every_redraw_but_its_words_do_not() {
        let mut e = engine("[[startup]]\ngate = \"trust\"\nanimate = true");
        e.tick(0);
        let a = e.plain(0);
        let b = e.plain(200);
        assert_ne!(a, b, "a spinner and a counter move");
        for screen in [&a, &b] {
            assert!(screen.contains("Do you trust the files in this folder?") && screen.contains("Waiting for your answer..."), "{screen}");
        }
        e.on_key(Key::Char('1'), 300);
        assert_eq!(e.exit_code(), None, "the digit still answers it");
    }

    #[test]
    fn a_status_model_shows_in_the_footer_or_only_on_the_working_row() {
        let mut always = engine("model = \"zeta-banner\"\nstatus_model = \"zeta-live\"");
        assert!(always.plain(0).contains("model: zeta-live | /help for commands"), "{}", always.plain(0));
        assert!(always.plain(0).contains("NIMBUS 0.4.2 | zeta-banner"), "{}", always.plain(0));
        let mut busy = engine("status_model = \"zeta-live\"\nstatus_model_when = \"busy\"\n[[turn]]\nsteps = [{ do = \"think\", ms = 1000 }, { do = \"say\", text = \"ok\" }]");
        assert!(!busy.plain(0).contains("zeta-live"));
        type_text(&mut busy, "go", 0);
        busy.on_key(Key::Enter, 10);
        assert!(busy.plain(300).contains("ctrl+c to stop  model: zeta-live"), "{}", busy.plain(300));
        busy.tick(1500);
        assert!(!busy.plain(1500).contains("zeta-live"), "{}", busy.plain(1500));
        let _ = &mut always;
    }

    #[test]
    fn a_backlog_pushes_the_banner_out_of_a_short_screen_and_a_right_footer_reaches_the_last_column() {
        let e = engine("backlog_rows = 60\nfooter_right = \"effort: high\"");
        let all = e.frame_all(0);
        assert!(all.rows.len() > 60 && all.rows[0].contains("NIMBUS"), "{}", all.rows.len());
        let fit = e.frame(0);
        assert_eq!(fit.rows.len(), 40);
        assert!(!fit.rows.iter().any(|row| row.contains("NIMBUS")), "the banner scrolled off");
        let footer = render::visible_len(fit.rows.last().unwrap());
        assert_eq!(footer, 120, "the footer ends in the last column: {:?}", fit.rows.last());
        assert!(strip(fit.rows.last().unwrap()).ends_with("effort: high"));
        let mut narrow = engine("footer_right = \"effort: high\"");
        narrow.resize(100, 40, 5);
        assert_eq!(render::visible_len(narrow.frame(5).rows.last().unwrap()), 100, "redrawn for the new width");
    }

    #[test]
    fn menus_style_asks_a_command_with_a_numbered_menu_and_a_file_with_an_arrow_menu() {
        let toml = "approvals = \"menus\"\n[[turn]]\nsteps = [{ do = \"approve\", kind = \"bash\", command = \"python --version\", on_allow = [{ do = \"say\", text = \"ran\" }] }, { do = \"approve\", kind = \"create\", path = \"hello.txt\", on_allow = [{ do = \"say\", text = \"wrote\" }] }]";
        let mut e = engine(toml);
        type_text(&mut e, "go", 0);
        e.on_key(Key::Enter, 10);
        let command = e.plain(20);
        assert!(command.contains("Run this command?") && command.contains("> 1. Allow once") && command.contains("  3. Deny"), "{command}");
        assert!(!command.contains("[y/n]"));
        e.on_key(Key::Char('y'), 30);
        assert!(e.plain(40).contains("Run this command?"), "y does nothing on a numbered menu");
        e.on_key(Key::Char('1'), 50);
        let file = e.plain(60);
        assert!(file.contains("Write hello.txt?") && file.contains("\u{276f} Yes, write it") && file.contains("up/down to move, enter to confirm"), "{file}");
        e.on_key(Key::Char('1'), 70);
        assert!(e.plain(80).contains("Write hello.txt?"), "digits do nothing on the arrow menu");
        e.on_key(Key::Down, 90);
        e.on_key(Key::Down, 100);
        assert!(e.plain(110).contains("\u{276f} No, keep it as it is"), "{}", e.plain(110));
        e.on_key(Key::Enter, 120);
        let done = e.plain(130);
        assert!(done.contains("denied: hello.txt") && !done.contains("wrote"), "{done}");
    }

    fn styled_toml(style: &str, cursor: usize) -> String {
        format!("approvals = \"{style}\"\napproval_cursor = {cursor}\n[[turn]]\nsteps = [{{ do = \"approve\", kind = \"bash\", command = \"echo ok\", on_allow = [{{ do = \"say\", text = \"ran\" }}], on_deny = [{{ do = \"say\", text = \"not run\" }}] }}]")
    }

    fn ask(toml: &str) -> Engine {
        let mut e = engine(toml);
        type_text(&mut e, "go", 0);
        e.on_key(Key::Enter, 10);
        e
    }

    #[test]
    fn the_inverse_menu_draws_its_cursor_as_inverse_video_and_no_marker() {
        let mut e = ask(&styled_toml("inverse", 2));
        let frame = e.frame(20);
        let plain = e.plain(20);
        assert!(plain.contains("Allow nimbus to run `echo ok`?") && plain.contains("Allow once") && plain.contains("Deny"), "{plain}");
        assert!(plain.lines().filter(|l| l.contains("Allow once") || l.contains("Deny")).all(|l| !l.contains('>') && !l.contains('\u{276f}')), "no marker: {plain}");
        let deny = frame.rows.iter().find(|r| r.contains("Deny")).expect("the Deny row");
        assert!(deny.contains("\x1b[7m"), "the cursor row is inverse: {deny:?}");
        let once = frame.rows.iter().find(|r| r.contains("Allow once")).expect("the Allow once row");
        assert!(!once.contains("\x1b[7m"), "{once:?}");
        e.on_key(Key::Char('y'), 30);
        e.on_key(Key::Char('1'), 31);
        assert!(e.plain(40).contains("Allow nimbus to run"), "letters and digits do nothing");
        e.on_key(Key::Up, 50);
        e.on_key(Key::Up, 60);
        assert!(e.frame(70).rows.iter().find(|r| r.contains("Allow once")).unwrap().contains("\x1b[7m"));
        e.on_key(Key::Enter, 80);
        assert!(e.plain(90).contains("allowed"), "{}", e.plain(90));
    }

    #[test]
    fn the_boxed_menu_sits_in_a_box_with_a_marker() {
        let e = ask(&styled_toml("boxed", 1));
        let plain = e.plain(20);
        assert!(plain.contains("\u{2502} \u{276f} Always allow in this project") && plain.contains("\u{256d}") && plain.contains("\u{2570}"), "{plain}");
    }

    #[test]
    fn a_button_row_moves_with_tab_only_or_with_left_and_right_only() {
        let mut tab = ask(&styled_toml("buttons", 2));
        let row = tab.plain(20).lines().find(|l| l.contains("Allow once")).unwrap().to_string();
        assert!(row.contains("Allow once") && row.contains("Always allow in this project") && row.contains("Deny"), "{row}");
        assert!(tab.frame(20).rows.iter().find(|r| r.contains("Allow once")).unwrap().contains("\x1b[7m Deny \x1b[27m"));
        tab.on_key(Key::Right, 30);
        tab.on_key(Key::Left, 31);
        assert!(tab.frame(40).rows.iter().find(|r| r.contains("Allow once")).unwrap().contains("\x1b[7m Deny \x1b[27m"), "arrows do nothing");
        tab.on_key(Key::Tab, 50);
        assert!(tab.frame(60).rows.iter().find(|r| r.contains("Allow once")).unwrap().contains("\x1b[7m Allow once \x1b[27m"), "Tab wraps to the first button");
        let mut arrows = ask(&styled_toml("buttons_arrows", 0));
        arrows.on_key(Key::Tab, 30);
        assert!(arrows.frame(40).rows.iter().find(|r| r.contains("Allow once")).unwrap().contains("\x1b[7m Allow once \x1b[27m"), "Tab does nothing");
        arrows.on_key(Key::Right, 50);
        assert!(arrows.frame(60).rows.iter().find(|r| r.contains("Allow once")).unwrap().contains("\x1b[7m Always allow in this project \x1b[27m"));
    }

    #[test]
    fn the_inline_question_takes_its_letters_and_the_always_only_menu_has_no_plain_yes() {
        let mut inline = ask(&styled_toml("inline", 0));
        assert!(inline.plain(20).contains("[Y]es / [N]o"), "{}", inline.plain(20));
        inline.on_key(Key::Enter, 30);
        assert!(inline.plain(40).contains("[Y]es / [N]o"), "Enter does nothing");
        inline.on_key(Key::Char('y'), 50);
        assert!(inline.plain(60).contains("allowed"), "{}", inline.plain(60));
        let always = ask(&styled_toml("always_only", 0));
        let plain = always.plain(20);
        assert!(plain.contains("Always allow in this project") && plain.contains("Deny") && !plain.contains("Allow once"), "{plain}");
        let blind = ask(&styled_toml("blind", 0));
        let rows = blind.frame(20).rows;
        assert!(rows.iter().all(|r| !r.contains("\x1b[7m")) && blind.plain(20).contains("Allow once"), "nothing shows where the cursor is");
    }

    #[test]
    fn the_run_numbered_menu_has_other_wording_and_takes_its_digits() {
        let mut e = ask(&styled_toml("run_numbered", 0));
        let plain = e.plain(20);
        assert!(plain.contains("> 1. Run command") && plain.contains("2. Always run") && plain.contains("3. Cancel"), "{plain}");
        e.on_key(Key::Char('1'), 30);
        assert!(e.plain(40).contains("ran") || e.plain(40).contains("allowed"), "{}", e.plain(40));
    }

    #[test]
    fn the_bullets_numbered_menu_draws_status_bullets_above_long_numbered_options_and_takes_its_digits() {
        let mut e = ask(&styled_toml("bullets_numbered", 0));
        let plain = e.plain(20);
        for want in [
            "\u{25CF} Edited 1 file (hello.txt)",
            "\u{25CF} Explored 1 file (hello.txt) (ctrl+o to expand)",
            "Run this command?",
            "> 1. Yes, run command",
            "  2. Yes, and always allow in this conversation for commands that start with 'echo ok'",
            "  3. Yes, and always allow for commands that start with 'echo ok' (Persist to settings.json)",
            "  4. No, cancel",
        ] {
            assert!(plain.lines().any(|l| l.trim_end() == want), "{want} in {plain}");
        }
        e.on_key(Key::Down, 30);
        assert!(e.plain(40).contains("> 2. Yes, and always allow in this conversation"), "{}", e.plain(40));
        e.on_key(Key::Char('1'), 50);
        assert!(e.plain(60).contains("allowed") || e.plain(60).contains("ran"), "{}", e.plain(60));
    }

    #[test]
    fn the_yes_missed_menu_draws_its_yes_row_at_another_indent() {
        let mut e = ask(&styled_toml("yes_missed", 1));
        let plain = e.plain(20);
        let row = |text: &str| plain.lines().find(|l| l.contains(text)).unwrap_or_else(|| panic!("{text} in {plain}")).to_string();
        let indent = |l: &str| l.chars().take_while(|c| *c == ' ').count();
        assert_eq!(indent(&row("Yes, run command")), 1, "{plain}");
        assert!(row("Always run in this project").trim_start().starts_with("> "), "the cursor starts on the second row: {plain}");
        e.on_key(Key::Up, 30);
        assert!(e.plain(40).contains(" > Yes, run command"), "{}", e.plain(40));
        e.on_key(Key::Enter, 50);
        assert!(e.plain(60).contains("ran"), "{}", e.plain(60));
    }

    #[test]
    fn one_turn_may_ask_two_differently_drawn_questions() {
        let toml = "approvals = \"inverse\"\n[[turn]]\nsteps = [{ do = \"approve\", kind = \"bash\", command = \"a\", on_allow = [{ do = \"say\", text = \"one\" }] }, { do = \"approve\", kind = \"bash\", command = \"b\", style = \"run_numbered\", on_allow = [{ do = \"say\", text = \"two\" }] }]";
        let mut e = ask(toml);
        assert!(e.plain(20).contains("Allow once") && !e.plain(20).contains("Run command"), "{}", e.plain(20));
        e.on_key(Key::Enter, 30);
        assert!(e.plain(40).contains("> 1. Run command"), "{}", e.plain(40));
    }

    #[test]
    fn a_clipped_status_model_is_cut_in_the_footer_and_on_the_working_row_but_not_in_the_banner() {
        let mut e = engine("model = \"zeta-banner-pro\"\nstatus_model = \"zeta-banner-pro\"\nstatus_model_clip = 8\n[[turn]]\nsteps = [{ do = \"think\", ms = 1000 }, { do = \"say\", text = \"ok\" }]");
        let idle = e.plain(0);
        assert!(idle.contains("NIMBUS 0.4.2 | zeta-banner-pro |"), "{idle}");
        assert!(idle.lines().any(|row| row.trim_end().ends_with("| model: zeta-ban")), "{idle}");
        assert!(!idle.contains("model: zeta-banner"), "{idle}");
        let mut busy = engine("status_model = \"zeta-banner-pro\"\nstatus_model_when = \"busy\"\nstatus_model_clip = 8\n[[turn]]\nsteps = [{ do = \"think\", ms = 1000 }]");
        type_text(&mut busy, "go", 0);
        busy.on_key(Key::Enter, 10);
        assert!(busy.plain(300).contains("ctrl+c to stop  model: zeta-ban"), "{}", busy.plain(300));
        assert!(!busy.plain(300).contains("zeta-banner"), "{}", busy.plain(300));
        let _ = &mut e;
    }

    #[test]
    fn the_row_after_a_turn_can_be_a_thought_for_summary() {
        let mut e = engine("summary_row = \"\u{25b8} Thought for {secs}s, 298 tokens\"\n[[turn]]\nsteps = [{ do = \"think\", ms = 3000 }, { do = \"say\", text = \"ok\" }]");
        type_text(&mut e, "go", 0);
        e.on_key(Key::Enter, 10);
        e.tick(3100);
        let done = e.plain(3200);
        assert!(done.contains("reply: ok"), "{done}");
        assert!(done.contains("\u{25b8} Thought for 3s, 298 tokens"), "{done}");
        assert!(!done.contains("done in"), "{done}");
    }

    #[test]
    fn a_lines_reply_is_taller_than_the_pane_and_the_pane_keeps_the_tail() {
        let mut e = engine("[[turn]]\nsteps = [{ do = \"lines\", count = 70, template = \"row {n} of the long answer\" }]");
        type_text(&mut e, "go", 0);
        e.on_key(Key::Enter, 10);
        e.tick(500);
        let all = e.frame_all(500);
        let text: Vec<String> = all.rows.iter().map(|r| strip(r)).collect();
        assert!(text.iter().any(|r| r == "reply: row 1 of the long answer"), "{text:?}");
        assert!(text.iter().any(|r| r == "  row 70 of the long answer"), "{text:?}");
        let fit = e.frame(500);
        assert_eq!(fit.rows.len(), 40);
        let fit_text: Vec<String> = fit.rows.iter().map(|r| strip(r)).collect();
        assert!(!fit_text.iter().any(|r| r.contains("row 1 of")), "the start of the reply scrolled off");
        assert!(fit_text.iter().any(|r| r.contains("row 70 of")));
    }

    #[test]
    fn a_link_step_draws_an_osc8_hyperlink_whose_text_is_the_shortened_path() {
        let mut e = engine("[[turn]]\nsteps = [{ do = \"link\", before = \"Edited \", text = \".../x.py\", target = \"file:///C:/Users/you/proj/src/deep/x.py\", after = \" (2 lines)\" }]");
        type_text(&mut e, "go", 0);
        e.on_key(Key::Enter, 10);
        e.tick(500);
        let frame = e.frame(500);
        let raw = frame.rows.iter().find(|r| r.contains("x.py")).expect("the link row");
        let esc = '\u{1b}';
        assert!(raw.contains(&format!("{esc}]8;;file:///C:/Users/you/proj/src/deep/x.py{esc}\\")), "{raw:?}");
        assert!(raw.contains(&format!("{esc}]8;;{esc}\\")), "{raw:?}");
        assert_eq!(strip(raw), "reply: Edited .../x.py (2 lines)");
        assert_eq!(render::visible_len(raw), "reply: Edited .../x.py (2 lines)".chars().count());
    }

    #[test]
    fn echo_task_false_draws_no_row_for_the_submitted_task() {
        let mut e = engine("echo_task = false
[[turn]]
steps = [{ do = \"say\", text = \"the answer\" }]");
        type_text(&mut e, "a very particular question", 0);
        e.on_key(Key::Enter, 10);
        e.tick(500);
        let screen = e.plain(500);
        assert!(screen.contains("reply: the answer"), "{screen}");
        assert!(!screen.contains("a very particular question"), "{screen}");
    }

    #[test]
    fn an_account_row_is_drawn_under_the_footer_from_startup_on() {
        let mut e = engine("account_row = \"someone@example.com (Nimbus Starter Quota)\"
[[turn]]
steps = [{ do = \"say\", text = \"ok\" }]");
        e.tick(500);
        let plain = e.plain(40);
        assert!(plain.contains("someone@example.com (Nimbus Starter Quota)"), "{plain}");
        type_text(&mut e, "go", 600);
        e.on_key(Key::Enter, 700);
        e.tick(1500);
        assert!(e.plain(40).contains("someone@example.com (Nimbus Starter Quota)"), "still there after a turn");
    }

    #[test]
    fn nimbus_rows_fit_the_terminal() {
        let mut e = engine("[[turn]]\nsteps = [{ do = \"say\", text = \"a long reply that goes on and on for a while so that it has to wrap around the edge of the terminal at least once\" }]");
        type_text(&mut e, "go", 0);
        e.on_key(Key::Enter, 10);
        e.tick(500);
        let frame = e.frame(500);
        assert!(frame.rows.len() <= 40);
        assert!(frame.rows.iter().all(|row| render::visible_len(row) <= 120));
    }
}
