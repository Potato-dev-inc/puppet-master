//! The guided capture as a state machine (SPEC 11.5). Pure: `Capture::tick` is given a `Look` at the
//! pane and a clock and answers with what the runner should do (`Action::Deliver`). It owns no process
//! and no real time, so every step, every guard and every timeout is tested with scripted screens.
//!
//! Safety, in code (and in the tests of this file):
//! - The only thing the engine can ask for is `Deliver { text }`, a task typed through the app's own
//!   delivery path. There is no key, no answer, no "press Enter". A trust, login or permission
//!   question is only ever recorded and shown to the person.
//! - `may_type` is the single door to `Deliver`. It refuses unless the agent is alive, the screen has
//!   held still for the stable time, nothing is running, no dialog is up, and the screen is positively
//!   an input box (or the person said "it is ready" for this exact picture).

use std::collections::{HashSet, VecDeque};

use serde_json::{json, Value};

use super::super::session::TestWords;
use super::answer::{AnswerCfg, AnswerKey, Answerer, AnsweredQ, Input as AnswerInput, Step as AnswerStep};
use super::styled::StyledScreen;
use super::host::Look;
use super::screen::{asks_trust_or_permission, classify, frame_key, ready_shape, Flavour, FrameBag, ScreenKind, Stability};
use super::verdict::{flavour_of, Decision, Verdict};

/// How long a picture must hold still before the engine acts on it.
pub const STABLE_MS: u64 = 2500;
/// Deliveries (typed messages) one capture may make in all, retries included.
pub const MAX_DELIVERIES: u32 = 6;

#[derive(Debug, Clone)]
pub struct Config {
    pub stable_ms: u64,
    /// After this long a step says it is slow (it keeps trying; the person decides).
    pub start_limit_ms: u64,
    pub message_limit_ms: u64,
    /// Typed, but the screen never changed.
    pub no_reaction_ms: u64,
    /// A settled screen that is not clearly an input box: ask the person.
    pub unsure_ms: u64,
    /// A reply word on one row only, settled: ask the person.
    pub reply_unsure_ms: u64,
    /// The permission step may take this long (not counting the time a question was on screen); then
    /// the person is asked to skip or retry (`stuck`).
    pub permission_wait_ms: u64,
    pub exit_wait_ms: u64,
    /// A screen that reads as "working" by its words but has not moved at all for this long is not
    /// working (a spinner or a timer always moves).
    pub frozen_ms: u64,
    /// After a turn: a screen back at the Ready step's shape, held still this long, means it is over.
    pub idle_done_ms: u64,
    /// After a turn: a screen that holds still this long, is not the Ready step's shape and is not
    /// clearly working, is shown to the person as a question the wizard does not recognise.
    pub question_ms: u64,
    /// Ask the agent a task that makes it think and use one harmless tool (`Work` step, SPEC 11.18), and keep the screens.
    /// The product turns this on (`VisibleEnv::default`); tests of the older flow leave it off and the step reads as skipped.
    pub work_step: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stable_ms: STABLE_MS,
            start_limit_ms: 120_000,
            message_limit_ms: 180_000,
            no_reaction_ms: 40_000,
            unsure_ms: 8_000,
            reply_unsure_ms: 6_000,
            permission_wait_ms: 120_000,
            exit_wait_ms: 15_000,
            frozen_ms: 8_000,
            idle_done_ms: 8_000,
            question_ms: 15_000,
            work_step: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepId {
    Start,
    Ready,
    Message,
    /// A task that makes the agent think and use a tool (SPEC 11.18).
    Work,
    Permission,
    Exit,
}

impl StepId {
    pub const ALL: [StepId; 6] = [StepId::Start, StepId::Ready, StepId::Message, StepId::Work, StepId::Permission, StepId::Exit];

    pub fn as_str(self) -> &'static str {
        match self {
            StepId::Start => "start",
            StepId::Ready => "ready",
            StepId::Message => "message",
            StepId::Work => "work",
            StepId::Permission => "permission",
            StepId::Exit => "exit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StepId::Start => "Starting",
            StepId::Ready => "Ready",
            StepId::Message => "Test message",
            StepId::Work => "Thinking and a tool",
            StepId::Permission => "Permission",
            StepId::Exit => "Exit",
        }
    }

    pub fn parse(text: &str) -> Option<StepId> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Running,
    NeedsYou,
    Done,
    Skipped,
    Stuck,
    Failed,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            StepStatus::Pending => "pending",
            StepStatus::Running => "running",
            StepStatus::NeedsYou => "needs_you",
            StepStatus::Done => "done",
            StepStatus::Skipped => "skipped",
            StepStatus::Stuck => "stuck",
            StepStatus::Failed => "failed",
        }
    }

    fn finished(self) -> bool {
        matches!(self, StepStatus::Done | StepStatus::Skipped | StepStatus::Stuck | StepStatus::Failed)
    }
}

#[derive(Debug, Clone)]
pub struct Step {
    pub id: StepId,
    pub status: StepStatus,
    pub note: String,
    pub frames: u32,
    started_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedsYou {
    pub step: StepId,
    /// `trust`, `login`, `permission`, `question`, `unsure`, `unsure_reply`, `slow`, `typing`, `stuck`.
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAction {
    Skip,
    Retry,
    Stuck,
    Ready,
    TryExit,
    Closed,
    /// "It is done": the permission step's screen is the finished agent, not a question.
    Done,
    /// "It is a question": the permission step's screen is a question the wizard did not recognise.
    Question,
}

impl CaptureAction {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "skip" => Self::Skip,
            "retry" => Self::Retry,
            "stuck" => Self::Stuck,
            "ready" => Self::Ready,
            "try_exit" => Self::TryExit,
            "closed" => Self::Closed,
            "done" => Self::Done,
            "question" => Self::Question,
            _ => return None,
        })
    }
}

/// What the answerer made of a question.
enum Auto {
    /// Pressing, waiting, or just answered: the person is not asked.
    Handled,
    /// The wizard will not answer this one; the text says why.
    Declined(String),
    /// Not the answerer's business (it is off, or not a question it knows).
    NotMine,
}

/// What the runner must do. There is nothing else the engine can ask a pane for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Type `text` as a task through the app's delivery path.
    Deliver { text: String },
    /// Press these keys to answer a trust or one-time approval question in the wizard's OWN pane (SPEC
    /// 11.16). Only produced when the person left "answer setup questions for me" on, only by the
    /// answerer's rules, and the runner re-checks that the pane is the wizard's before pressing.
    Answer { keys: Vec<AnswerKey>, note: String },
    /// Resize the pane's PTY and its viewer together to this grid (the reprint measurement, SPEC 11.17). Nothing is
    /// typed: the agent only sees its window change size.
    Resize { cols: u16, rows: u16 },
}

// ---- the typing guard --------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gate<'a> {
    pub alive: bool,
    pub stable_ms: u64,
    pub need_ms: u64,
    pub kind: ScreenKind,
    pub in_flight: bool,
    pub deliveries: u32,
    pub ready_for: Option<&'a str>,
    pub fingerprint: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Dead,
    InFlight,
    TooMany,
    Blank,
    Shell,
    Dialog,
    Working,
    NotStable,
    NotInput,
}

impl Refusal {
    /// One plain sentence for the step note.
    pub fn say(self) -> &'static str {
        match self {
            Refusal::Dead => "The agent is not running.",
            Refusal::InFlight => "Typing is in progress.",
            Refusal::TooMany => "The wizard has typed enough messages for one capture.",
            Refusal::Blank => "Waiting for the agent to draw something.",
            Refusal::Shell => "The pane shows a shell prompt, not the agent.",
            Refusal::Dialog => "The agent is asking a question. Nothing is typed until you have answered it.",
            Refusal::Working => "The agent is busy. Waiting until it is quiet.",
            Refusal::NotStable => "Waiting for the screen to hold still.",
            Refusal::NotInput => "I cannot tell that the agent is waiting for a task. Press \"It is ready\" if it is.",
        }
    }
}

/// The one door to typing. Every `Action::Deliver` the engine makes passed through here.
pub fn may_type(g: &Gate) -> Result<(), Refusal> {
    if !g.alive {
        return Err(Refusal::Dead);
    }
    if g.in_flight {
        return Err(Refusal::InFlight);
    }
    if g.deliveries >= MAX_DELIVERIES {
        return Err(Refusal::TooMany);
    }
    match g.kind {
        ScreenKind::Blank => return Err(Refusal::Blank),
        ScreenKind::Shell => return Err(Refusal::Shell),
        ScreenKind::Dialog(_) => return Err(Refusal::Dialog),
        ScreenKind::Working => return Err(Refusal::Working),
        ScreenKind::Input | ScreenKind::Unknown => {}
    }
    if g.stable_ms < g.need_ms {
        return Err(Refusal::NotStable);
    }
    if g.kind == ScreenKind::Input || g.ready_for == Some(g.fingerprint) {
        Ok(())
    } else {
        Err(Refusal::NotInput)
    }
}

// ---- the captured frames -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapFrame {
    /// `tui_idle`, `tui_working`, `tui_reply`, `tui_permission` (a file write), `tui_permission_command` (a command
    /// run), `tui_trust`, `tui_firstrun`.
    pub name: String,
    /// Cleaned (secrets masked, user folder scrubbed); the real test words are still in it.
    pub text: String,
    /// Rows that differed between the frames seen for this picture (cleaned).
    pub variants: Vec<String>,
}

/// Everything the guided capture learned.
#[derive(Debug, Clone, Default)]
pub struct CaptureSet {
    pub frames: Vec<CapFrame>,
    pub grid: (u16, u16),
    /// `slash_exit`, `person` or `none`.
    pub exit_method: String,
    /// Step ids whose capture is partial (the person said "it looks stuck", or a step failed).
    pub partial: Vec<String>,
    pub skipped: Vec<String>,
    pub hello_written: bool,
    /// The agent made hello.txt and never asked (a fact for the drafter and the Review screen).
    pub writes_without_asking: bool,
    /// The agent ran the harmless command and never asked.
    pub runs_without_asking: bool,
    pub startup_dialogs: Vec<String>,
    pub exit_screen: Option<String>,
    pub notes: Vec<String>,
    /// The questions the wizard answered for the person in its own pane (empty when the switch was off).
    pub answered: Vec<AnsweredQ>,
    /// What happened to the conversation when the pane was resized after the reply: the measurement behind
    /// `[features] reprints_on_resize`. `None`: it was not measured (the host cannot resize, or the step was skipped).
    pub resize: Option<ResizeProbe>,
}

/// What the viewer's scrollback plus screen holds of the conversation, for the reprint measurement: the rows that
/// hold the test word, and how many rows show up more than once. An agent that prints its whole conversation again
/// on a resize adds copies of both (a conversation taller than the pane pushes the banner and the early turns into
/// the history a second time). A row that is a rule or a handful of characters is no evidence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HistoryCounts {
    /// Cleaned rows that hold the word.
    pub word_rows: Vec<String>,
    /// Extra copies of rows that are not the word's: `sum(copies - 1)`.
    pub extra_copies: usize,
    /// Those rows, once each, for the evidence.
    pub repeated_rows: Vec<String>,
}

impl HistoryCounts {
    pub fn of(history: &str, word: &str) -> Self {
        let word = word.to_lowercase();
        let text = clean(history);
        let mut seen: Vec<(String, String, usize)> = Vec::new();
        let mut word_rows = Vec::new();
        for row in text.lines().map(str::trim).filter(|r| !r.is_empty()) {
            if row.to_lowercase().contains(&word) {
                word_rows.push(row.to_string());
                continue;
            }
            let key = row.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            if key.chars().filter(|c| c.is_alphanumeric()).count() < 10 {
                continue;
            }
            match seen.iter_mut().find(|(k, _, _)| *k == key) {
                Some(entry) => entry.2 += 1,
                None => seen.push((key, row.to_string(), 1)),
            }
        }
        let extra_copies = seen.iter().map(|(_, _, n)| n - 1).sum();
        let repeated_rows = seen.into_iter().filter(|(_, _, n)| *n > 1).map(|(_, row, n)| format!("{row}   (x{n})")).collect();
        Self { word_rows, extra_copies, repeated_rows }
    }

    /// The rows that show the conversation (word rows, then the repeated ones), for the evidence.
    pub fn evidence(&self) -> Vec<String> {
        self.word_rows.iter().cloned().chain(self.repeated_rows.iter().cloned()).collect()
    }

    /// `after` holds more copies of the conversation than `self` (taken before the resize).
    pub fn grew_to(&self, after: &HistoryCounts) -> bool {
        after.word_rows.len() > self.word_rows.len() || after.extra_copies > self.extra_copies
    }
}

/// The reprint measurement: the pane went from `from` to `to` (columns, rows) after the reply, and the rows that
/// hold the test word in the viewer's scrollback plus screen were counted before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResizeProbe {
    pub reprints: bool,
    pub from: (u16, u16),
    pub to: (u16, u16),
    /// Cleaned rows holding the word before the resize (real words in them).
    pub before: Vec<String>,
    /// The same rows afterwards.
    pub after: Vec<String>,
}

impl ResizeProbe {
    pub fn to_json(&self, mask: &dyn Fn(&str) -> String) -> Value {
        json!({
            "reprints": self.reprints,
            "from": {"cols": self.from.0, "rows": self.from.1},
            "to": {"cols": self.to.0, "rows": self.to.1},
            "before_rows": self.before.iter().map(|r| mask(r)).collect::<Vec<_>>(),
            "after_rows": self.after.iter().map(|r| mask(r)).collect::<Vec<_>>(),
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let pair = |key: &str| Some((value[key]["cols"].as_u64()? as u16, value[key]["rows"].as_u64()? as u16));
        let rows = |key: &str| value[key].as_array().map(|a| a.iter().filter_map(|r| r.as_str().map(str::to_string)).collect()).unwrap_or_default();
        Some(ResizeProbe { reprints: value["reprints"].as_bool()?, from: pair("from")?, to: pair("to")?, before: rows("before_rows"), after: rows("after_rows") })
    }
}

impl CaptureSet {
    pub fn get(&self, name: &str) -> Option<&CapFrame> {
        self.frames.iter().find(|f| f.name == name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Everything, with the real words (for the benchmark's `captures.json`; never shown to an AI).
    pub fn to_json(&self) -> Value {
        json!({
            "frames": self.frames.iter().map(|f| json!({"name": f.name, "text": f.text, "variants": f.variants})).collect::<Vec<_>>(),
            "grid": [self.grid.0, self.grid.1],
            "exit_method": self.exit_method,
            "partial": self.partial,
            "skipped": self.skipped,
            "hello_written": self.hello_written,
            "writes_without_asking": self.writes_without_asking,
            "runs_without_asking": self.runs_without_asking,
            "startup_dialogs": self.startup_dialogs,
            "exit_screen": self.exit_screen,
            "notes": self.notes,
            "answered": self.answered.iter().map(|a| a.to_json(&|t: &str| t.to_string())).collect::<Vec<_>>(),
            "resize": self.resize.as_ref().map(|r| r.to_json(&|t: &str| t.to_string())),
        })
    }

    /// The other way: a set read back from `to_json`.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let text = |v: &Value| v.as_str().map(str::to_string).ok_or_else(|| "a capture field is not text".to_string());
        let list = |key: &str| -> Result<Vec<String>, String> {
            value[key].as_array().map(|a| a.iter().map(&text).collect()).unwrap_or_else(|| Ok(Vec::new()))
        };
        let frames = value["frames"]
            .as_array()
            .ok_or("the capture has no frames")?
            .iter()
            .map(|f| {
                Ok(CapFrame {
                    name: text(&f["name"])?,
                    text: text(&f["text"])?,
                    variants: f["variants"].as_array().map(|a| a.iter().map(&text).collect()).unwrap_or_else(|| Ok(Vec::new()))?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let grid = match (value["grid"][0].as_u64(), value["grid"][1].as_u64()) {
            (Some(c), Some(r)) => (c as u16, r as u16),
            _ => (120, 40),
        };
        Ok(CaptureSet {
            frames,
            grid,
            exit_method: value["exit_method"].as_str().unwrap_or("none").to_string(),
            partial: list("partial")?,
            skipped: list("skipped")?,
            hello_written: value["hello_written"].as_bool().unwrap_or(false),
            writes_without_asking: value["writes_without_asking"].as_bool().unwrap_or(false),
            runs_without_asking: value["runs_without_asking"].as_bool().unwrap_or(false),
            startup_dialogs: list("startup_dialogs")?,
            exit_screen: value["exit_screen"].as_str().map(str::to_string),
            notes: list("notes")?,
            resize: ResizeProbe::from_json(&value["resize"]),
            answered: value["answered"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|q| AnsweredQ {
                            place: q["place"].as_str().unwrap_or("capture").to_string(),
                            flavour: q["flavour"].as_str().unwrap_or("").to_string(),
                            label: q["label"].as_str().unwrap_or("").to_string(),
                            keys: q["keys"].as_str().unwrap_or("").to_string(),
                            screen: q["screen"].as_str().unwrap_or("").to_string(),
                            via: q["via"].as_str().unwrap_or("rules").to_string(),
                            read: q["menu"].as_str().unwrap_or("").to_string(),
                            denied: q["denied"].as_str().map(str::to_string),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

// ---- the engine --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnPhase {
    Waiting,
    InFlight,
    Submitted,
}

#[derive(Debug, Clone)]
struct Turn {
    phase: TurnPhase,
    typed_at: u64,
    baseline: Vec<String>,
    working: FrameBag,
    saw_activity: bool,
    /// A permission dialog was recorded for this turn.
    dialog_recorded: bool,
    /// The last time a question was on screen (the permission deadline counts from the later of this
    /// and the typing: waiting for the person is not the agent being slow).
    last_dialog_ms: u64,
    failed_deliveries: u32,
}

impl Turn {
    fn new() -> Self {
        Self {
            phase: TurnPhase::Waiting,
            typed_at: 0,
            baseline: Vec::new(),
            working: FrameBag::default(),
            saw_activity: false,
            dialog_recorded: false,
            last_dialog_ms: 0,
            failed_deliveries: 0,
        }
    }
}

/// How many looks the evidence ring keeps (SPEC 11.15).
pub const DEBUG_FRAMES: usize = 40;

/// What the engine saw at one look, kept so a stop can be explained afterwards.
#[derive(Debug, Clone)]
pub struct DebugFrame {
    pub ms: u64,
    pub step: &'static str,
    pub step_status: &'static str,
    /// The classifier's verdict on this screen, before the engine's own reading.
    pub kind: &'static str,
    /// What the engine made of it (`working` that never moved reads as `frozen`).
    pub reading: &'static str,
    pub stable_ms: u64,
    /// How long no character moved.
    pub quiet_ms: u64,
    pub needs: Option<String>,
    /// Cleaned (real words still in it).
    pub text: String,
    /// Rows drawn highlighted (a filled band, inverse video, bold) on this screen, cleaned.
    pub highlight: Vec<String>,
    /// The menu the answerer's parser read on this screen (`describe_menu`), when it read one.
    pub menu: Option<String>,
}

pub struct Capture {
    cfg: Config,
    words: TestWords,
    grid: (u16, u16),
    steps: Vec<Step>,
    cur: usize,
    stab: Stability,
    kind: ScreenKind,
    screen: String,
    alive: bool,
    ready_for: Option<String>,
    deliveries: u32,
    in_flight: bool,
    frames: Vec<CapFrame>,
    turn: Turn,
    needs: Option<NeedsYou>,
    startup_dialogs: Vec<String>,
    hello_written: bool,
    exit_requested: bool,
    exit_asked_at: Option<u64>,
    exit_method: &'static str,
    exit_screen: Option<String>,
    partial: Vec<&'static str>,
    notes: Vec<String>,
    began_ms: Option<u64>,
    /// A resize of the pane was already noted.
    resized: bool,
    /// The screen reads as "working" but has not moved for `frozen_ms`: the engine reads it as unknown.
    frozen: bool,
    writes_without_asking: bool,
    /// The fingerprint of the screen the permission step showed as a question the classifier did not
    /// recognise (`question` ask); the ask ends when the picture changes.
    unrecognised: Option<String>,
    /// The `tui_permission` frame is a guess (the person has not said it is a question).
    speculative_permission: bool,
    debug: VecDeque<DebugFrame>,
    last_debug_text: String,
    last_debug_ms: u64,
    /// What the classifier said about the latest screen, and when it was looked at.
    seen_kind: ScreenKind,
    last_now: u64,
    /// Answers setup questions in the wizard's own pane (off unless the person left the switch on).
    answerer: Option<Answerer>,
    out: Vec<Action>,
    answered: Vec<AnsweredQ>,
    /// The permission step asks twice: 0 = a file write (`tui_permission`), 1 = a command run (`tui_permission_command`).
    perm_phase: u8,
    /// The agent ran the command and showed no question.
    runs_without_asking: bool,
    /// The resize measurement after the reply (SPEC 11.17).
    probe: Probe,
    resize_result: Option<ResizeProbe>,
    /// "Finish without me": the AI look called this picture (by fingerprint) a trust or one-time approval question the
    /// classifier did not recognise. The safe answerer then looks at it as a dialog of that flavour (SPEC 11.19).
    ai_question: Option<(String, Flavour)>,
    /// The run was stopped (a sign-in or payment screen); nothing is typed or pressed until the step is retried.
    halted: Option<String>,
    /// The latest screen with its cell attributes, when the host has them (SPEC 11.20).
    styled: Option<StyledScreen>,
    /// The answerer could not READ the question that is up (no menu, no highlighted row): an AI look may name the option.
    menu_unreadable: bool,
}

/// How long after a resize the agent gets to redraw before the picture is judged.
const RESIZE_SETTLE_MS: u64 = 1500;
/// Narrower by this many columns for the measurement.
const RESIZE_SHRINK_COLS: u16 = 20;
/// The measurement gives up (nothing is recorded) after this long without a settled picture.
const RESIZE_GIVE_UP_MS: u64 = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Probe {
    /// The reply step is not finished yet.
    Off,
    /// Waiting for a settled screen and the viewer's history to count the word in.
    Measure { note: String },
    /// The pane was made narrower at `at`; waiting for the agent to redraw.
    Shrunk { at: u64, before: HistoryCounts, from: (u16, u16), to: (u16, u16), note: String },
    /// Back at the original size; waiting for it to settle.
    Restore { at: u64, note: String },
    Done,
}

/// The cleaned text of a frame (`exec::clean`: ANSI gone, secrets masked, user folder scrubbed).
fn clean(text: &str) -> String {
    super::super::exec::clean(text)
}

fn dialog_message(flavour: Flavour) -> &'static str {
    match flavour {
        Flavour::Trust => "The agent is asking whether you trust this folder. Answer it in the pane yourself. The wizard presses nothing.",
        Flavour::Login => "The agent wants you to sign in. Do that in the pane. The wizard presses nothing.",
        Flavour::Permission => "The agent is asking for permission. Press your answer in the pane. The wizard answers nothing.",
        Flavour::Question => "The agent is asking a question. Answer it in the pane yourself. The wizard presses nothing.",
    }
}

/// The task of the message step: random word, masked everywhere an AI or the preview sees it.
pub fn message_task(words: &TestWords) -> String {
    message_task_for(&words.one)
}

/// The same task for another word (the visible test uses the second session word).
pub fn message_task_for(word: &str) -> String {
    format!("Reply with exactly the word: {word}")
}

/// The sentence in front of every task that makes the agent use a tool: the wizard's pane is in its own isolated temporary
/// folder and the agent must stay in it (the wizard answers its one-time questions only while they stay inside it).
pub const STAY_IN_FOLDER: &str = "Stay in this folder.";

/// The task of the work step: the agent must think and use one harmless tool (listing the folder), then answer with a
/// sentence that holds the third random word. The reply is that sentence and nothing else; the screens before it show the
/// rows an agent prints while it works (thinking, tool calls, a summary line).
pub fn work_task(words: &TestWords) -> String {
    format!("{STAY_IN_FOLDER} List the files here, then reply with exactly: {}", work_sentence(words))
}

/// The answer the work task asks for: a sentence that holds the third random word.
pub fn work_sentence(words: &TestWords) -> String {
    format!("folder listed, code {}.", words.sky)
}

/// The task of the permission step: a file write (`tui_permission`).
pub fn permission_task(words: &TestWords) -> String {
    format!("{STAY_IN_FOLDER} Create a file named hello.txt in the current folder. It must contain the word {}.", words.sky)
}

/// The second task of the permission step: running a command (`tui_permission_command`). A command that only prints.
pub fn command_task(words: &TestWords) -> String {
    format!("{STAY_IN_FOLDER} Run this shell command in the current folder and tell me what it printed: echo {}", words.sky)
}

impl Capture {
    pub fn new(words: TestWords, grid: (u16, u16), cfg: Config) -> Self {
        let steps = StepId::ALL
            .iter()
            .map(|id| {
                let off = *id == StepId::Work && !cfg.work_step;
                Step {
                    id: *id,
                    status: if off { StepStatus::Skipped } else { StepStatus::Pending },
                    note: if off { "Not asked.".into() } else { String::new() },
                    frames: 0,
                    started_ms: 0,
                }
            })
            .collect();
        Self {
            cfg,
            words,
            grid,
            steps,
            cur: 0,
            stab: Stability::new(),
            kind: ScreenKind::Blank,
            screen: String::new(),
            alive: true,
            ready_for: None,
            deliveries: 0,
            in_flight: false,
            frames: Vec::new(),
            turn: Turn::new(),
            needs: None,
            startup_dialogs: Vec::new(),
            hello_written: false,
            exit_requested: false,
            exit_asked_at: None,
            exit_method: "none",
            exit_screen: None,
            partial: Vec::new(),
            notes: Vec::new(),
            began_ms: None,
            resized: false,
            frozen: false,
            writes_without_asking: false,
            unrecognised: None,
            speculative_permission: false,
            debug: VecDeque::new(),
            last_debug_text: String::new(),
            last_debug_ms: 0,
            seen_kind: ScreenKind::Blank,
            last_now: 0,
            answerer: None,
            out: Vec::new(),
            answered: Vec::new(),
            perm_phase: 0,
            runs_without_asking: false,
            probe: Probe::Off,
            resize_result: None,
            ai_question: None,
            halted: None,
            styled: None,
            menu_unreadable: false,
        }
    }

    /// Let the engine answer the trust and one-time approval questions of this pane (SPEC 11.16).
    pub fn with_auto_answer(mut self, on: bool) -> Self {
        self.answerer = on.then(|| Answerer::new("capture", AnswerCfg::default()));
        self
    }

    /// The wizard's own folder the pane lives in: what a question may name without being refused (`boundary`).
    pub fn with_answer_folder(mut self, folder: Option<std::path::PathBuf>) -> Self {
        self.answerer = self.answerer.take().map(|a| a.with_folder(folder));
        self
    }

    /// The questions the wizard answered for the person so far.
    pub fn answered(&self) -> &[AnsweredQ] {
        &self.answered
    }

    /// What the answerer read and decided for each question (`Answerer::decisions`).
    pub fn decisions(&self) -> Vec<Value> {
        self.answerer.as_ref().map(|a| a.decisions().to_vec()).unwrap_or_default()
    }

    pub fn auto_answer_on(&self) -> bool {
        self.answerer.is_some()
    }

    /// The runner could not press the keys (the pane is not the wizard's own, or the write failed): stop
    /// answering and leave the question to the person.
    pub fn on_answer_refused(&mut self, why: &str) {
        self.answerer = None;
        self.set_note(format!("The wizard did not answer the question: {why}"));
    }

    /// The last looks, oldest first, for "why did it stop" (cleaned, real words in them).
    /// The newest is always the screen as it is right now, with the engine's current reading of it.
    pub fn debug_frames(&self) -> Vec<DebugFrame> {
        let mut frames: Vec<DebugFrame> = self.debug.iter().cloned().collect();
        if self.began_ms.is_some() && frames.last().map_or(true, |f| f.ms != self.last_now) {
            frames.push(self.snapshot_frame(self.last_now));
            if frames.len() > DEBUG_FRAMES {
                frames.remove(0);
            }
        }
        frames
    }

    fn snapshot_frame(&self, now: u64) -> DebugFrame {
        let step = self.current();
        DebugFrame {
            ms: now,
            step: step.as_str(),
            step_status: self.steps[step.index()].status.as_str(),
            kind: self.seen_kind.as_str(),
            reading: if self.frozen { "frozen" } else { self.kind.as_str() },
            stable_ms: self.stab.stable_for(now),
            quiet_ms: self.stab.quiet_for(now),
            needs: self.needs.as_ref().map(|n| n.kind.clone()),
            text: clean(&self.screen),
            highlight: self.emphasised_rows().iter().map(|r| clean(r)).collect(),
            menu: if self.kind.is_dialog() || self.needs.is_some() {
                super::menu::parse_menu_ex(&self.screen, self.styled.as_ref()).map(|m| super::menu::describe_menu(&m))
            } else {
                None
            },
        }
    }

    fn record_debug(&mut self, now: u64) {
        let frame = self.snapshot_frame(now);
        // a new picture, or a heartbeat every 5 s so a frozen screen still shows how long it was frozen
        if frame.text == self.last_debug_text && now.saturating_sub(self.last_debug_ms) < 5_000 && !self.debug.is_empty() {
            return;
        }
        self.last_debug_text.clone_from(&frame.text);
        self.last_debug_ms = now;
        if self.debug.len() >= DEBUG_FRAMES {
            self.debug.pop_front();
        }
        self.debug.push_back(frame);
    }

    pub fn current(&self) -> StepId {
        self.steps[self.cur.min(self.steps.len() - 1)].id
    }

    pub fn step(&self, id: StepId) -> &Step {
        &self.steps[id.index()]
    }

    pub fn needs_you(&self) -> Option<&NeedsYou> {
        self.needs.as_ref()
    }

    pub fn kind(&self) -> ScreenKind {
        self.kind
    }

    pub fn grid(&self) -> (u16, u16) {
        self.grid
    }

    pub fn deliveries(&self) -> u32 {
        self.deliveries
    }

    /// Every step is Done, Skipped, Stuck or Failed.
    pub fn settled(&self) -> bool {
        self.steps.iter().all(|s| s.status.finished())
    }

    pub fn stable_for(&self, now: u64) -> u64 {
        self.stab.stable_for(now)
    }

    pub fn frame(&self, name: &str) -> Option<&CapFrame> {
        self.frames.iter().find(|f| f.name == name)
    }

    fn set_note(&mut self, text: impl Into<String>) {
        let i = self.cur;
        self.steps[i].note = text.into();
    }

    fn set_status(&mut self, status: StepStatus) {
        let i = self.cur;
        self.steps[i].status = status;
    }

    fn store(&mut self, name: &str, text: &str, variants: Vec<String>) {
        let frame = CapFrame { name: name.to_string(), text: clean(text), variants: variants.iter().map(|v| clean(v)).collect() };
        if let Some(existing) = self.frames.iter_mut().find(|f| f.name == name) {
            *existing = frame;
        } else {
            self.frames.push(frame);
        }
        let i = self.cur;
        self.steps[i].frames += 1;
    }

    fn gate(&self, now: u64) -> Gate<'_> {
        Gate {
            alive: self.alive,
            stable_ms: self.stab.stable_for(now),
            need_ms: self.cfg.stable_ms,
            kind: self.kind,
            in_flight: self.in_flight,
            deliveries: self.deliveries,
            ready_for: self.ready_for.as_deref(),
            fingerprint: "",
        }
    }

    /// `gate` with the fingerprint filled in (it is an owned string, so it cannot live in `gate`).
    fn may_type(&self, now: u64) -> Result<(), Refusal> {
        let fingerprint = self.stab.fingerprint();
        let mut gate = self.gate(now);
        gate.fingerprint = &fingerprint;
        // Once the Ready step has kept the picture of the empty input box, THAT shape is what counts: a row
        // that starts with `>` (an echoed task) does not make a screen an input box, and a screen with the
        // old shape and a stale "Working..." line above it still is one.
        match (gate.kind, self.ready_shape()) {
            (ScreenKind::Input, Some(false)) => gate.kind = ScreenKind::Unknown,
            (ScreenKind::Unknown, Some(true)) => gate.kind = ScreenKind::Input,
            _ => {}
        }
        may_type(&gate)
    }

    fn begin_step(&mut self, now: u64) {
        let i = self.cur;
        if self.steps[i].status == StepStatus::Pending {
            self.steps[i].status = StepStatus::Running;
            self.steps[i].started_ms = now;
        }
    }

    fn advance(&mut self, now: u64) {
        if matches!(self.probe, Probe::Measure { .. } | Probe::Shrunk { .. } | Probe::Restore { .. }) {
            self.probe = Probe::Done;
        }
        self.needs = None;
        self.turn = Turn::new();
        while self.cur + 1 < self.steps.len() {
            self.cur += 1;
            if !self.steps[self.cur].status.finished() {
                break;
            }
        }
        if !self.steps[self.cur].status.finished() {
            self.steps[self.cur].status = StepStatus::Running;
            self.steps[self.cur].started_ms = now;
        }
    }

    fn finish_step(&mut self, status: StepStatus, note: impl Into<String>, now: u64) {
        self.set_status(status);
        self.set_note(note);
        if status == StepStatus::Stuck || status == StepStatus::Failed {
            let id = self.current().as_str();
            if !self.partial.contains(&id) {
                self.partial.push(id);
            }
        }
        self.advance(now);
    }

    fn ask(&mut self, kind: &str, message: impl Into<String>) {
        let step = self.current();
        let message = message.into();
        self.needs = Some(NeedsYou { step, kind: kind.to_string(), message: message.clone() });
        self.set_status(StepStatus::NeedsYou);
        self.set_note(message);
    }

    fn clear_ask(&mut self, kinds: &[&str]) {
        if self.needs.as_ref().is_some_and(|n| kinds.contains(&n.kind.as_str())) {
            self.needs = None;
            if self.steps[self.cur].status == StepStatus::NeedsYou {
                self.steps[self.cur].status = StepStatus::Running;
            }
        }
    }

    // ---- the clock ------------------------------------------------------------------------------

    /// One look at the pane. Returns what the runner must do (at most one delivery).
    pub fn tick(&mut self, look: &Look, now: u64) -> Vec<Action> {
        let mut actions = self.tick_inner(look, now);
        actions.append(&mut self.out);
        actions
    }

    fn tick_inner(&mut self, look: &Look, now: u64) -> Vec<Action> {
        self.began_ms.get_or_insert(now);
        self.alive = look.alive;
        self.hello_written |= look.hello_written;
        if let Some(grid) = look.grid.filter(|g| *g != self.grid && matches!(self.probe, Probe::Off | Probe::Done)) {
            // The viewer grid must equal the PTY grid: a pane that changed size mid-capture says so. (The wizard's
            // own resize for the reprint measurement is not a surprise.)
            if !self.resized {
                self.resized = true;
                self.notes.push(format!(
                    "The pane changed size from {}x{} to {}x{} while it was captured; the screens may not all have the same width.",
                    self.grid.0, self.grid.1, grid.0, grid.1
                ));
            }
        }
        self.screen = look.screen.clone();
        self.styled = look.styled.clone();
        self.stab.observe(&look.screen, now);
        let seen_kind = classify(&look.screen);
        self.kind = seen_kind;
        // "Working" needs the screen to MOVE: a spinner, a timer, streaming text. Words like "thinking..."
        // on a picture that has not changed by one character for `frozen_ms` are not work. The engine then
        // reads the screen as unknown: it types nothing (that needs the person's "It is ready") and the
        // step's own rules for a settled screen apply.
        self.frozen = self.kind == ScreenKind::Working && self.stab.quiet_for(now) >= self.cfg.frozen_ms;
        if self.frozen {
            self.kind = ScreenKind::Unknown;
        }
        // The AI look called this picture a trust / one-time approval question: the answerer reads it as one.
        if let Some((fp, flavour)) = self.ai_question.clone() {
            // the question's identity folds the cursor marker, so moving the highlight is still the same question
            if fp != super::answer::fingerprint(&self.screen) {
                self.ai_question = None;
            } else if matches!(self.kind, ScreenKind::Unknown | ScreenKind::Input) {
                self.kind = ScreenKind::Dialog(flavour);
            }
        }
        self.seen_kind = seen_kind;
        self.last_now = now;
        self.record_debug(now);
        if self.halted.is_some() {
            return Vec::new();
        }
        // the unrecognised question belongs to one picture
        if self.unrecognised.as_deref().is_some_and(|fp| fp != self.stab.fingerprint()) {
            self.unrecognised = None;
            self.speculative_permission = false;
            self.clear_ask(&["question"]);
        }
        if self.settled() {
            return Vec::new();
        }
        self.begin_step(now);
        let step = self.current();
        if !self.alive && step != StepId::Exit {
            let note = if step == StepId::Start {
                "The agent quit by itself before it asked for anything. Check the command and its arguments, then retry."
            } else {
                "The agent quit by itself. What was captured so far is kept."
            };
            self.agent_quit(note, now);
            return Vec::new();
        }
        if step == StepId::Message && self.probe != Probe::Off && self.probe != Probe::Done {
            return self.tick_probe(look, now);
        }
        match step {
            StepId::Start => self.tick_start(now),
            StepId::Ready => self.tick_ready(now),
            StepId::Message | StepId::Work | StepId::Permission => return self.tick_turn(step, look, now),
            StepId::Exit => return self.tick_exit(now),
        }
        Vec::new()
    }

    /// The agent ended before the exit step: the running step failed, the steps after it did not run, and
    /// the exit step is next (it is done as soon as it sees the process gone).
    fn agent_quit(&mut self, note: &str, now: u64) {
        self.set_status(StepStatus::Failed);
        self.set_note(note);
        let id = self.current().as_str();
        if !self.partial.contains(&id) {
            self.partial.push(id);
        }
        let exit = StepId::Exit.index();
        for i in self.cur + 1..exit {
            if !self.steps[i].status.finished() {
                self.steps[i].status = StepStatus::Skipped;
                self.steps[i].note = "Not run: the agent quit.".into();
            }
        }
        self.needs = None;
        self.cur = exit;
        self.steps[exit].status = StepStatus::Running;
        self.steps[exit].started_ms = now;
    }

    /// A stable dialog is recorded and shown to the person; nothing is typed. Returns true when the
    /// screen is a dialog (the caller then does nothing else this tick).
    fn dialog(&mut self, now: u64) -> bool {
        let ScreenKind::Dialog(flavour) = self.kind else {
            if self.unrecognised.is_some() {
                self.clear_ask(&["trust", "login", "permission"]);
            } else {
                self.clear_ask(&["trust", "login", "permission", "question"]);
            }
            self.auto_gone();
            self.menu_unreadable = false;
            return false;
        };
        let stable = self.stab.stable_for(now);
        let step = self.current();
        let name = match (flavour, step) {
            (Flavour::Trust, _) => "tui_trust",
            (Flavour::Permission | Flavour::Question, StepId::Permission | StepId::Message) => self.permission_name(),
            // a question the listing task raised: answered, not kept as a permission capture (that is the next step's)
            (Flavour::Permission | Flavour::Question, StepId::Work) => "tui_work_question",
            _ => "tui_firstrun",
        };
        if name.starts_with("tui_permission") && self.speculative_permission {
            // a real, recognised question replaces the guess
            self.speculative_permission = false;
            self.frames.retain(|f| f.name != name);
        }
        let settled = stable >= self.cfg.stable_ms;
        if settled {
            if self.frame(name).is_none() {
                if let Some((text, variants)) = self.stab.best() {
                    self.store(name, &text, variants);
                }
                if !name.starts_with("tui_permission") {
                    self.startup_dialogs.push(flavour.as_str().to_string());
                }
            }
            if step == StepId::Permission || name.starts_with("tui_permission") {
                self.turn.dialog_recorded = true;
            }
        }
        // The wizard answers a trust or one-time approval question in its own pane, but only after the
        // picture of the question was kept (above) and has held still.
        let kept = self.frame(name).is_some();
        match self.auto_step(flavour, if kept { stable } else { 0 }, now) {
            Auto::Handled => return true,
            Auto::Declined(reason) => {
                if settled && self.needs.as_ref().map(|n| n.kind.as_str()) != Some(flavour.as_str()) {
                    self.ask(flavour.as_str(), format!("{} The wizard did not answer it for you: {reason}.", dialog_message(flavour)));
                }
                return true;
            }
            Auto::NotMine => {}
        }
        if !settled {
            self.set_note("Something is changing on the screen.");
            return true;
        }
        if self.needs.as_ref().map(|n| n.kind.as_str()) != Some(flavour.as_str()) {
            self.ask(flavour.as_str(), dialog_message(flavour));
        }
        true
    }

    /// One look of the answerer at the question on screen.
    fn auto_step(&mut self, flavour: Flavour, stable_ms: u64, now: u64) -> Auto {
        let Some(answerer) = self.answerer.as_mut() else { return Auto::NotMine };
        let input = AnswerInput { screen: &self.screen, styled: self.styled.as_ref(), flavour: Some(flavour), stable_ms, now, hint: None };
        match answerer.step(&input) {
            AnswerStep::Nothing => Auto::NotMine,
            AnswerStep::Waiting(note) => {
                self.set_note(note);
                Auto::Handled
            }
            AnswerStep::Press { keys, note } => {
                self.set_note(note.clone());
                self.out.push(Action::Answer { keys, note });
                Auto::Handled
            }
            AnswerStep::Answered(done) => {
                self.answered.push(done);
                Auto::Handled
            }
            AnswerStep::Declined(reason) => {
                self.menu_unreadable = super::answer::can_look(&reason);
                Auto::Declined(reason)
            }
        }
    }

    /// The question is gone: if the wizard pressed keys for it, it was answered.
    fn auto_gone(&mut self) {
        let Some(answerer) = self.answerer.as_mut() else { return };
        let input = AnswerInput { screen: &self.screen, styled: None, flavour: None, stable_ms: 0, now: 0, hint: None };
        if let AnswerStep::Answered(done) = answerer.step(&input) {
            self.answered.push(done);
        }
    }

    fn who_answered(&self) -> &'static str {
        if self.answered.is_empty() {
            "you answered it in the pane"
        } else {
            "the wizard answered it for you (its own isolated folder)"
        }
    }

    fn slow(&mut self, now: u64, limit_ms: u64, message: &str) {
        let i = self.cur;
        let started = self.steps[i].started_ms;
        if now.saturating_sub(started) > limit_ms && self.steps[i].status == StepStatus::Running {
            self.ask("slow", message);
        }
    }

    fn tick_start(&mut self, now: u64) {
        if self.dialog(now) {
            return;
        }
        if self.kind == ScreenKind::Shell {
            self.finish_step(StepStatus::Failed, "The pane shows a shell prompt: the agent is not running.", now);
            return;
        }
        if self.kind == ScreenKind::Blank {
            self.set_note("Waiting for the agent to draw something.");
        } else if self.stab.stable_for(now) < self.cfg.stable_ms {
            self.set_note("Waiting for the screen to hold still.");
        } else if self.kind != ScreenKind::Working {
            let note = if self.startup_dialogs.is_empty() {
                "The agent started.".to_string()
            } else {
                format!("The agent asked a question ({}); {}.", self.startup_dialogs.join(", "), self.who_answered())
            };
            self.finish_step(StepStatus::Done, note, now);
            return;
        }
        self.slow(now, self.cfg.start_limit_ms, "This is taking long. Wait, or press \"It looks stuck\" to go on.");
    }

    fn tick_ready(&mut self, now: u64) {
        if self.dialog(now) {
            return;
        }
        let stable = self.stab.stable_for(now);
        let fp = self.stab.fingerprint();
        let said_ready = self.ready_for.as_deref() == Some(fp.as_str());
        if stable >= self.cfg.stable_ms && self.stab.samples() >= 3 && !matches!(self.kind, ScreenKind::Working | ScreenKind::Blank | ScreenKind::Shell)
            && (self.kind == ScreenKind::Input || said_ready)
        {
            if let Some((text, variants)) = self.stab.best() {
                self.store("tui_idle", &text, variants);
            }
            self.exit_screen = Some(clean(&self.screen));
            self.finish_step(StepStatus::Done, "The agent is waiting for a task.", now);
            return;
        }
        if stable >= self.cfg.unsure_ms && self.kind != ScreenKind::Working && self.kind != ScreenKind::Input {
            if self.needs.is_none() {
                self.ask(
                    "unsure",
                    "I cannot tell whether the agent is waiting for a task. If it is, press \"It is ready\". If it is asking something, answer it in the pane.",
                );
            }
        } else {
            self.clear_ask(&["unsure"]);
            self.set_note(if self.kind == ScreenKind::Working { "The agent is still starting." } else { "Waiting for the screen to hold still." });
        }
        self.slow(now, self.cfg.start_limit_ms, "This is taking long. Wait, or press \"It looks stuck\" to go on.");
    }

    fn task_for(&self, step: StepId) -> String {
        match (step, self.perm_phase) {
            (StepId::Message, _) => message_task(&self.words),
            (StepId::Work, _) => work_task(&self.words),
            (_, 0) => permission_task(&self.words),
            _ => command_task(&self.words),
        }
    }

    /// The name of the permission capture this phase makes.
    fn permission_name(&self) -> &'static str {
        if self.current() == StepId::Permission && self.perm_phase == 1 { "tui_permission_command" } else { "tui_permission" }
    }

    /// One permission question is done. The first one (a file write) is followed by the second (a command run), each in
    /// its own turn; the step is done after the second.
    fn after_permission(&mut self, note: &str, now: u64) {
        if self.perm_phase == 0 {
            self.perm_phase = 1;
            self.turn = Turn::new();
            self.needs = None;
            self.speculative_permission = false;
            self.unrecognised = None;
            self.ready_for = None;
            if let Some(a) = self.answerer.as_mut() {
                a.reset();
            }
            if self.steps[self.cur].status == StepStatus::NeedsYou {
                self.steps[self.cur].status = StepStatus::Running;
            }
            self.set_note("The file question is captured. Now the same for running a command.");
        } else {
            self.finish_step(StepStatus::Done, note, now);
        }
    }

    fn tick_turn(&mut self, step: StepId, look: &Look, now: u64) -> Vec<Action> {
        let dialog_up = self.dialog(now);
        if dialog_up {
            // A dialog during the turn is recorded; the turn goes on once the person has answered.
            self.turn.last_dialog_ms = now;
            if self.turn.phase == TurnPhase::Submitted {
                self.turn.saw_activity = true;
            }
            return Vec::new();
        }
        match self.turn.phase {
            TurnPhase::InFlight => Vec::new(),
            TurnPhase::Waiting if self.turn.failed_deliveries >= 2 => Vec::new(),
            TurnPhase::Waiting => {
                match self.may_type(now) {
                    Ok(()) => {
                        let fingerprint_rows = frame_key(&self.screen);
                        self.turn.baseline = fingerprint_rows;
                        self.in_flight = true;
                        self.deliveries += 1;
                        self.turn.phase = TurnPhase::InFlight;
                        self.clear_ask(&["unsure", "typing", "slow"]);
                        self.set_note("Typing the test message.");
                        vec![Action::Deliver { text: self.task_for(step) }]
                    }
                    Err(refusal) => {
                        if self.needs.as_ref().map(|n| n.kind.as_str()) != Some("typing") {
                            self.set_note(refusal.say());
                        }
                        // A settled screen that is not clearly an input box: ask, once.
                        if refusal == Refusal::NotInput && self.stab.stable_for(now) >= self.cfg.unsure_ms && self.needs.is_none() {
                            self.ask("unsure", refusal.say());
                        }
                        Vec::new()
                    }
                }
            }
            TurnPhase::Submitted => {
                self.watch_turn(step, look, now);
                Vec::new()
            }
        }
    }

    /// The runner reports what the delivery did.
    pub fn on_delivered(&mut self, submitted: bool, why: &str, now: u64) {
        self.in_flight = false;
        if submitted {
            self.turn.phase = TurnPhase::Submitted;
            self.turn.typed_at = now;
            self.turn.saw_activity = false;
            self.turn.working.clear();
            self.set_note("Message sent. Watching the agent work.");
        } else {
            self.turn.phase = TurnPhase::Waiting;
            self.turn.failed_deliveries += 1;
            let note = format!("The message was not typed: {why}");
            if self.turn.failed_deliveries >= 2 {
                self.ask("typing", format!("{note} Skip this step, or press \"It looks stuck\" to go on."));
            } else {
                self.set_note(note);
            }
        }
    }

    fn watch_turn(&mut self, step: StepId, _look: &Look, now: u64) {
        let key = frame_key(&self.screen);
        let changed = self.turn.baseline.is_empty() || key != self.turn.baseline;
        if self.kind == ScreenKind::Working || changed {
            self.turn.saw_activity = self.turn.saw_activity || changed || self.kind == ScreenKind::Working;
        }
        let stable = self.stab.stable_for(now);
        if self.kind == ScreenKind::Working || stable < self.cfg.stable_ms {
            // The picture is moving: this is the working screen.
            if self.turn.saw_activity {
                self.turn.working.add(&self.screen);
            }
            self.set_note("The agent is working.");
            if step == StepId::Permission {
                self.permission_deadline(now);
            } else {
                self.clear_ask(&["unsure_reply", "slow"]);
                self.slow(now, self.cfg.message_limit_ms, "This is taking long. Wait, or press \"It looks stuck\" to go on.");
            }
            return;
        }
        let fp = self.stab.fingerprint();
        let said = self.ready_for.as_deref() == Some(fp.as_str());
        match step {
            StepId::Message => self.finish_message(said, now),
            StepId::Work => self.finish_work(said, now),
            _ => self.finish_permission(said, now),
        }
    }

    /// The reply was captured. Before the step is done, the pane is resized once (and put back) to see whether the
    /// agent reprints its conversation: `[features] reprints_on_resize` is written from this measurement, never guessed.
    fn after_reply(&mut self, note: &str, now: u64) {
        if self.probe == Probe::Off {
            self.probe = Probe::Measure { note: note.to_string() };
            self.set_note("Checking what the agent does when the pane is resized.");
        } else {
            self.finish_step(StepStatus::Done, note, now);
        }
    }

    /// Does the viewer need to hand the engine its scrollback this tick?
    pub fn wants_history(&self) -> bool {
        matches!(self.probe, Probe::Measure { .. } | Probe::Shrunk { .. })
    }

    /// The host refused to resize the pane: the measurement is skipped, the capture goes on.
    pub fn on_resize_refused(&mut self, why: &str, now: u64) {
        let note = match std::mem::replace(&mut self.probe, Probe::Done) {
            Probe::Measure { note } | Probe::Shrunk { note, .. } | Probe::Restore { note, .. } => note,
            _ => return,
        };
        self.notes.push(format!("The pane could not be resized, so it is not known whether the agent reprints its conversation on a resize: {why}"));
        self.finish_step(StepStatus::Done, &note, now);
    }

    fn counts(&self, history: &str) -> HistoryCounts {
        HistoryCounts::of(history, &self.words.one)
    }

    fn tick_probe(&mut self, look: &Look, now: u64) -> Vec<Action> {
        match self.probe.clone() {
            Probe::Measure { note } => {
                if look.history.trim().is_empty() {
                    // The host cannot show the scrollback: nothing to count.
                    self.probe = Probe::Done;
                    self.finish_step(StepStatus::Done, &note, now);
                    return Vec::new();
                }
                if self.stab.stable_for(now) < self.cfg.stable_ms {
                    return Vec::new();
                }
                let from = self.grid;
                let to = (from.0.saturating_sub(RESIZE_SHRINK_COLS).max(40), from.1);
                let before = self.counts(&look.history);
                if to == from || before.word_rows.is_empty() {
                    self.probe = Probe::Done;
                    self.finish_step(StepStatus::Done, &note, now);
                    return Vec::new();
                }
                self.probe = Probe::Shrunk { at: now, before, from, to, note };
                vec![Action::Resize { cols: to.0, rows: to.1 }]
            }
            Probe::Shrunk { at, before, from, to, note } => {
                let settled = look.grid == Some(to) && now.saturating_sub(at) >= RESIZE_SETTLE_MS && self.stab.stable_for(now) >= self.cfg.stable_ms.min(RESIZE_SETTLE_MS);
                if settled && !look.history.trim().is_empty() {
                    let after = self.counts(&look.history);
                    self.resize_result =
                        Some(ResizeProbe { reprints: before.grew_to(&after), from, to, before: before.evidence(), after: after.evidence() });
                } else if now.saturating_sub(at) > RESIZE_GIVE_UP_MS {
                    self.notes.push("The pane was resized but its screen did not settle, so it is not known whether the agent reprints its conversation on a resize.".into());
                } else {
                    return Vec::new();
                }
                self.probe = Probe::Restore { at: now, note };
                vec![Action::Resize { cols: from.0, rows: from.1 }]
            }
            Probe::Restore { at, note } => {
                let back = look.grid == Some(self.grid) && now.saturating_sub(at) >= 800 && self.stab.stable_for(now) >= 500;
                if back || now.saturating_sub(at) > RESIZE_GIVE_UP_MS {
                    self.probe = Probe::Done;
                    self.finish_step(StepStatus::Done, &note, now);
                }
                Vec::new()
            }
            Probe::Off | Probe::Done => Vec::new(),
        }
    }

    fn finish_message(&mut self, said: bool, now: u64) {
        let word = self.words.one.clone();
        let screen = self.screen.clone();
        let reply_rows = reply_rows(&screen, &word);
        let typed_for = now.saturating_sub(self.turn.typed_at);
        if !self.turn.saw_activity {
            if typed_for > self.cfg.no_reaction_ms {
                self.ask("slow", "The message was typed but nothing changed on the screen. Retry, or press \"It looks stuck\" to go on.");
            }
            return;
        }
        if reply_rows >= 1 || said {
            let Some((reply, reply_variants)) = self.stab.best() else { return };
            let idle_key = self.frame("tui_idle").map(|f| frame_key(&f.text));
            let mut exclude = vec![frame_key(&reply)];
            exclude.extend(idle_key);
            let working = self.turn.working.best_excluding(&exclude);
            self.store("tui_reply", &reply, reply_variants);
            match working {
                Some((text, variants)) => {
                    self.store("tui_working", &text, variants);
                    self.after_reply("The agent answered.", now);
                }
                None => {
                    self.notes.push("No screen of the agent at work was caught; the working state has no capture.".into());
                    self.partial.push("message");
                    self.after_reply("The agent answered, but no screen of it at work was caught.", now);
                }
            }
            return;
        }
        if self.stab.stable_for(now) >= self.cfg.reply_unsure_ms && self.needs.is_none() {
            self.ask("unsure_reply", "I cannot see an answer yet. If the agent has finished, press \"It is ready\" to use this screen. Otherwise wait.");
        }
    }

    /// The work step's reply: the sentence row is on a settled screen. The screens are kept as `tui_work_working` (the
    /// agent thinking and using the tool) and `tui_work_reply`. There is no resize measurement here (the message step did it).
    fn finish_work(&mut self, said: bool, now: u64) {
        let word = self.words.sky.clone();
        let screen = self.screen.clone();
        let reply_rows = reply_rows(&screen, &word);
        let typed_for = now.saturating_sub(self.turn.typed_at);
        if !self.turn.saw_activity {
            if typed_for > self.cfg.no_reaction_ms {
                self.ask("slow", "The message was typed but nothing changed on the screen. Retry, or press \"It looks stuck\" to go on.");
            }
            return;
        }
        if reply_rows >= 1 || said {
            let Some((reply, reply_variants)) = self.stab.best() else { return };
            let mut exclude = vec![frame_key(&reply)];
            exclude.extend(self.frame("tui_idle").map(|f| frame_key(&f.text)));
            exclude.extend(self.frame("tui_reply").map(|f| frame_key(&f.text)));
            let working = self.turn.working.best_excluding(&exclude);
            self.store("tui_work_reply", &reply, reply_variants);
            match working {
                Some((text, variants)) => {
                    self.store("tui_work_working", &text, variants);
                    self.finish_step(StepStatus::Done, "The agent listed the folder and answered.", now);
                }
                None => {
                    self.notes.push("No screen of the agent at work on the listing task was caught.".into());
                    self.partial.push("work");
                    self.finish_step(StepStatus::Done, "The agent answered, but no screen of it at work was caught.", now);
                }
            }
            return;
        }
        if self.stab.stable_for(now) >= self.cfg.reply_unsure_ms && self.needs.is_none() {
            self.ask("unsure_reply", "I cannot see an answer yet. If the agent has finished, press \"It is ready\" to use this screen. Otherwise wait.");
        }
    }

    /// Does the screen have the shape of the Ready step (its bottom rows)? `None` before that step kept one.
    fn ready_shape(&self) -> Option<bool> {
        let screen = clean(&self.screen);
        self.frame("tui_idle").map(|f| ready_shape(&f.text, &screen))
    }

    /// The agent is waiting for a task again: the Ready step's shape, or (with none kept) an input box.
    fn at_ready(&self) -> bool {
        self.ready_shape().unwrap_or(self.kind == ScreenKind::Input)
    }

    /// The permission step has its own deadline: after `permission_wait_ms` without a question on screen
    /// the person is asked to skip or retry. Never typed, never answered.
    fn permission_deadline(&mut self, now: u64) {
        let since = self.turn.typed_at.max(self.turn.last_dialog_ms);
        if now.saturating_sub(since) > self.cfg.permission_wait_ms && self.needs.as_ref().map(|n| n.kind.as_str()) != Some("stuck") {
            self.ask("stuck", "This step has taken a long time and is not finished. Press Skip to go on without it, or Retry to try it again.");
        }
    }

    /// A settled screen that is not the agent's input box and not clearly working: keep it as the
    /// permission picture (a guess) and ask the person what it is.
    fn raise_question(&mut self) {
        if let Some((text, variants)) = self.stab.best() {
            let name = self.permission_name();
            self.store(name, &text, variants);
        }
        self.speculative_permission = true;
        self.turn.dialog_recorded = true;
        self.unrecognised = Some(self.stab.fingerprint());
        self.ask(
            "question",
            "The screen has stopped changing and it is not the agent's empty input box. If the agent is asking something, answer it in the pane yourself: the wizard presses nothing. If the agent has finished, press \"It is done\".",
        );
    }

    fn note_writes_without_asking(&mut self) {
        self.writes_without_asking = true;
        self.hello_written = true;
        let note = "This agent did not ask before writing a file: hello.txt appeared and no question was shown.";
        if !self.notes.iter().any(|n| n == note) {
            self.notes.push(note.to_string());
        }
    }

    fn finish_permission(&mut self, said: bool, now: u64) {
        let typed_for = now.saturating_sub(self.turn.typed_at);
        let quiet = matches!(self.kind, ScreenKind::Input | ScreenKind::Unknown);
        let idle = said || self.at_ready();
        if self.turn.dialog_recorded {
            // The person has answered (the question is gone) and the agent is back at its input box, or the
            // file the question was about has been written.
            if quiet && (idle || self.hello_written) {
                self.speculative_permission = false;
                let note = if self.answered.iter().any(|a| a.flavour == "permission") { "The wizard answered the permission question for you." } else { "You answered the permission question in the pane." };
                self.after_permission(note, now);
            } else {
                self.permission_deadline(now);
            }
            return;
        }
        if !self.turn.saw_activity {
            if typed_for > self.cfg.no_reaction_ms {
                self.ask("slow", "The message was typed but nothing changed on the screen. Retry, or press \"It looks stuck\" to go on.");
            }
            return;
        }
        // (a) the agent finished without a question: hello.txt is there, or the screen is back at the
        // shape of the Ready step and has held still
        if self.perm_phase == 0 && self.hello_written && quiet {
            self.note_writes_without_asking();
            self.after_permission("No permission question appeared (the agent did not ask before writing the file).", now);
            return;
        }
        let stable = self.stab.stable_for(now);
        if quiet && idle && stable >= self.cfg.idle_done_ms {
            if self.perm_phase == 0 {
                self.notes.push("The agent finished the permission task without asking and without writing hello.txt; no permission screen was caught.".into());
                if !self.partial.contains(&"permission") {
                    self.partial.push("permission");
                }
                self.after_permission("The agent finished without asking and without writing the file.", now);
            } else {
                // It ran the command and showed no question: a fact, not a failure.
                self.runs_without_asking = true;
                let note = "This agent did not ask before running a command: the command ran and no question was shown.";
                if !self.notes.iter().any(|n| n == note) {
                    self.notes.push(note.to_string());
                }
                self.after_permission("No permission question appeared for the command (the agent did not ask before running it).", now);
            }
            return;
        }
        // (b) a picture that holds still and is neither the input box nor clearly working: ask the person
        if quiet && !idle && stable >= self.cfg.question_ms && self.unrecognised.is_none() {
            self.raise_question();
            return;
        }
        // (c) the deadline
        self.permission_deadline(now);
    }

    fn tick_exit(&mut self, now: u64) -> Vec<Action> {
        if !self.alive {
            if self.exit_method == "none" {
                self.exit_method = "person";
            }
            self.finish_step(StepStatus::Done, "The agent has ended.", now);
            return Vec::new();
        }
        if self.dialog(now) {
            return Vec::new();
        }
        if self.exit_requested {
            if let Some(at) = self.exit_asked_at {
                if now.saturating_sub(at) > self.cfg.exit_wait_ms && self.needs.as_ref().map(|n| n.kind.as_str()) != Some("slow") {
                    self.ask("slow", "/exit did not end the agent. Close it yourself, or skip this step.");
                }
                return Vec::new();
            }
            match self.may_type(now) {
                Ok(()) => {
                    self.in_flight = true;
                    self.deliveries += 1;
                    self.exit_asked_at = Some(now);
                    self.exit_method = "slash_exit";
                    self.set_note("Typing /exit.");
                    return vec![Action::Deliver { text: "/exit".to_string() }];
                }
                Err(refusal) => {
                    self.set_note(refusal.say());
                    // a settled screen that is not clearly the empty input box: the person decides
                    if refusal == Refusal::NotInput && self.stab.stable_for(now) >= self.cfg.unsure_ms && self.needs.is_none() {
                        self.ask("unsure", format!("{} Or close the agent yourself and press \"I closed it myself\".", refusal.say()));
                    }
                }
            }
        } else if self.steps[self.cur].note.is_empty() {
            self.set_note("Optional: press \"Try /exit\", or close the agent yourself. You can also skip this.");
        }
        Vec::new()
    }

    // ---- the person's buttons -------------------------------------------------------------------

    pub fn act(&mut self, step: Option<StepId>, action: CaptureAction, now: u64) -> Result<(), String> {
        if self.in_flight {
            return Err("The wizard is typing a message right now. Wait a moment.".into());
        }
        let target = step.unwrap_or_else(|| self.current());
        match action {
            CaptureAction::Skip => {
                let i = target.index();
                if self.steps[i].status.finished() && i != self.cur {
                    return Err("that step is already finished".into());
                }
                let is_current = i == self.cur;
                self.steps[i].status = StepStatus::Skipped;
                self.steps[i].note = "Skipped.".into();
                if is_current {
                    self.advance(now);
                }
                Ok(())
            }
            CaptureAction::Retry => {
                let from = target.index();
                for i in from..self.steps.len() {
                    if self.steps[i].id == StepId::Work && !self.cfg.work_step {
                        continue;
                    }
                    self.steps[i].status = StepStatus::Pending;
                    self.steps[i].note.clear();
                    self.steps[i].frames = 0;
                }
                let names: &[&str] = match target {
                    StepId::Start => &["tui_trust", "tui_firstrun", "tui_idle", "tui_working", "tui_reply", "tui_work_working", "tui_work_reply", "tui_work_question", "tui_permission", "tui_permission_command"],
                    StepId::Ready => &["tui_idle", "tui_working", "tui_reply", "tui_work_working", "tui_work_reply", "tui_work_question", "tui_permission", "tui_permission_command"],
                    StepId::Message => &["tui_working", "tui_reply", "tui_work_working", "tui_work_reply", "tui_work_question", "tui_permission", "tui_permission_command"],
                    StepId::Work => &["tui_work_working", "tui_work_reply", "tui_work_question", "tui_permission", "tui_permission_command"],
                    StepId::Permission => &["tui_permission", "tui_permission_command"],
                    StepId::Exit => &[],
                };
                self.frames.retain(|f| !names.contains(&f.name.as_str()));
                self.partial.retain(|p| StepId::parse(p).map_or(true, |s| s.index() < from));
                if from <= StepId::Permission.index() {
                    // the file of an earlier try is gone (the runner deletes it), so it is not evidence now
                    self.hello_written = false;
                    self.writes_without_asking = false;
                    self.runs_without_asking = false;
                    self.perm_phase = 0;
                    self.notes.retain(|n| !n.starts_with("This agent did not ask") && !n.starts_with("The agent finished the permission task"));
                }
                self.unrecognised = None;
                self.speculative_permission = false;
                self.ai_question = None;
                self.halted = None;
                if let Some(a) = self.answerer.as_mut() {
                    a.reset();
                }
                self.cur = from;
                self.needs = None;
                self.ready_for = None;
                self.turn = Turn::new();
                self.exit_requested = false;
                self.exit_asked_at = None;
                self.begin_step(now);
                Ok(())
            }
            CaptureAction::Stuck => {
                if target != self.current() {
                    return Err("only the step that is running can be marked stuck".into());
                }
                self.finish_step(StepStatus::Stuck, "You said it looks stuck. What was captured so far is kept.", now);
                Ok(())
            }
            CaptureAction::Ready => {
                if target != self.current() {
                    return Err("only the step that is running can take this".into());
                }
                if self.kind.is_dialog() {
                    return Err("The agent is asking a question. Answer it in the pane first.".into());
                }
                if matches!(self.kind, ScreenKind::Blank | ScreenKind::Shell) {
                    return Err("There is nothing to use yet.".into());
                }
                self.ready_for = Some(self.stab.fingerprint());
                self.clear_ask(&["unsure", "unsure_reply"]);
                Ok(())
            }
            CaptureAction::TryExit => {
                if self.current() != StepId::Exit {
                    return Err("that button belongs to the exit step".into());
                }
                self.exit_requested = true;
                self.clear_ask(&["slow"]);
                Ok(())
            }
            CaptureAction::Closed => {
                if self.current() != StepId::Exit {
                    return Err("that button belongs to the exit step".into());
                }
                if self.exit_method == "none" {
                    self.exit_method = "person";
                }
                self.finish_step(StepStatus::Done, "You closed the agent yourself.", now);
                Ok(())
            }
            CaptureAction::Done | CaptureAction::Question => {
                if target != self.current() || self.current() != StepId::Permission {
                    return Err("that button belongs to the permission step while it is running".into());
                }
                if self.kind.is_dialog() {
                    return Err("The agent is asking a question. Answer it in the pane first.".into());
                }
                if self.turn.phase != TurnPhase::Submitted {
                    return Err("The permission task has not been sent yet.".into());
                }
                if action == CaptureAction::Done {
                    // the screen is the finished agent: whatever was kept as a question was a guess
                    if self.speculative_permission {
                        let name = self.permission_name();
                        self.frames.retain(|f| f.name != name);
                        self.speculative_permission = false;
                        self.turn.dialog_recorded = false;
                    }
                    self.unrecognised = None;
                    let asked = self.turn.dialog_recorded;
                    if self.hello_written && !asked {
                        self.note_writes_without_asking();
                    }
                    self.after_permission("You said the agent had finished.", now);
                } else {
                    // the screen is a question the classifier did not know: keep it as the permission picture
                    let name = self.permission_name();
                    if self.speculative_permission || self.frame(name).is_none() {
                        if let Some((text, variants)) = self.stab.best() {
                            self.store(name, &text, variants);
                        }
                    }
                    self.speculative_permission = false;
                    self.turn.dialog_recorded = true;
                    self.turn.last_dialog_ms = now;
                    self.unrecognised = None;
                    self.needs = None;
                    if self.steps[self.cur].status == StepStatus::NeedsYou {
                        self.steps[self.cur].status = StepStatus::Running;
                    }
                    self.set_note("Kept as the permission question. Answer it in the pane yourself.");
                }
                Ok(())
            }
        }
    }

    /// The drafter AI's one-word look at the current screen (SPEC 11.19). Deterministic: the verdict can only press
    /// "It is ready" (bound to this screen), hand a question to the safe answerer, wait, stop the run or keep what
    /// was captured. It never types and never chooses keys.
    pub fn apply_verdict(&mut self, verdict: Verdict, now: u64) -> Decision {
        if self.settled() {
            return Decision::Refused("the capture is already finished".into());
        }
        match verdict {
            Verdict::ReadyForInput => match self.act(None, CaptureAction::Ready, now) {
                Ok(()) => Decision::PressedReady,
                Err(why) => Decision::Refused(why),
            },
            Verdict::Working => Decision::Wait,
            Verdict::QuestionOther | Verdict::Unknown => Decision::WaitThenSkip,
            Verdict::QuestionTrustOrOnce => {
                if self.answerer.is_none() {
                    return Decision::Refused("the wizard may not answer setup questions in this session".into());
                }
                if matches!(self.kind, ScreenKind::Blank | ScreenKind::Shell) {
                    return Decision::Refused("there is no question on the screen".into());
                }
                if !asks_trust_or_permission(&self.screen) {
                    return Decision::Refused("the screen does not talk about trusting a folder or about a permission".into());
                }
                self.ai_question = Some((super::answer::fingerprint(&self.screen), flavour_of(&self.screen)));
                if let ScreenKind::Dialog(_) = self.kind {
                    // already read as a dialog by the classifier: the answerer has it
                } else {
                    self.set_note("The AI look reads this as a trust or one-time approval question. Only the plain yes is pressed.");
                }
                Decision::SafeAnswerer
            }
            Verdict::SignInOrPayment => {
                self.halted = Some("sign_in".into());
                self.needs = None;
                self.set_note("The agent needs a sign-in or a payment step. The wizard stopped and typed nothing.");
                Decision::Stop { code: super::verdict::SIGN_IN_CODE }
            }
            Verdict::Error => {
                let shown = clean(&self.screen);
                // An account or status row shown since startup (an email, a plan, a quota in a parenthesis) is not an
                // error, and neither is a row the captured frames already showed before this step: the look may not
                // stop the run for it. A screen with a real new error row, or with no such row to judge, is the AI's call.
                let mut baseline = super::super::agent_error::Baseline::new();
                for f in &self.frames {
                    baseline.add_screen(&f.text);
                }
                if super::super::agent_error::only_status_rows_look_wrong(&shown, &baseline) {
                    return Decision::Refused("the only rows that read like a problem are account or status rows that were on the screen since startup".into());
                }
                let last: Vec<&str> = shown.lines().map(str::trim).filter(|l| !l.is_empty()).rev().take(3).collect();
                let tail: Vec<&str> = last.into_iter().rev().collect();
                let masked = super::super::livetest::mask_words(&tail.join(" / "), &self.words);
                let step = self.current();
                self.notes.push(format!("The agent showed an error during the {} step: {masked}", step.as_str()));
                self.finish_step(StepStatus::Stuck, "The agent shows an error. What was captured so far is kept.", now);
                Decision::MarkedStuck
            }
        }
    }

    /// The AI look named the option (and the keys) of a question the safe answerer could not read (SPEC 11.20). Only the
    /// answerer acts on it, after `ai_menu::validate` checked the option and the keys against the screen: it presses them
    /// one at a time, reads the screen after each and stops unless the question is gone within its deadline.
    pub fn menu_choice(&mut self, fp: &str, option: &str, keys: &[String]) -> Result<String, String> {
        if self.settled() || self.halted.is_some() {
            return Err("the capture is not waiting for an answer".into());
        }
        let Some(needs) = self.needs.clone() else {
            return Err("no question is waiting".into());
        };
        if !matches!(needs.kind.as_str(), "trust" | "permission") || !self.menu_unreadable {
            return Err("the wizard did not ask for a look at this question".into());
        }
        let Some(answerer) = self.answerer.as_mut() else {
            return Err("the wizard may not answer setup questions in this session".into());
        };
        let label = answerer.offer_ai(&self.screen, fp, option, keys)?;
        self.menu_unreadable = false;
        self.clear_ask(&["trust", "permission"]);
        self.set_note(format!("Answering the question with the AI look's choice: {label}"));
        Ok(label)
    }

    /// Rows of the screen drawn highlighted (a filled band, inverse video, bold): a hint for the AI look when the text
    /// alone shows no cursor.
    pub fn emphasised_rows(&self) -> Vec<String> {
        let Some(styled) = &self.styled else { return Vec::new() };
        let from = styled.rows.len().saturating_sub(24);
        styled.rows[from..]
            .iter()
            .filter(|r| !r.text.trim().is_empty())
            .filter(|r| {
                let style = r.style_of(0, r.text.chars().count());
                style.is_band() || style.bold
            })
            .map(|r| r.text.trim().to_string())
            .take(8)
            .collect()
    }

    // ---- reading the result ---------------------------------------------------------------------

    /// What was learned. Steps that never ran are listed as skipped.
    pub fn into_set(&self) -> CaptureSet {
        CaptureSet {
            frames: self.frames.clone(),
            grid: self.grid,
            exit_method: self.exit_method.to_string(),
            partial: self.partial.iter().map(|s| s.to_string()).collect(),
            skipped: self
                .steps
                .iter()
                .filter(|s| matches!(s.status, StepStatus::Skipped | StepStatus::Pending))
                .filter(|s| s.id != StepId::Work || self.cfg.work_step)
                .map(|s| s.id.as_str().to_string())
                .collect(),
            hello_written: self.hello_written,
            writes_without_asking: self.writes_without_asking,
            runs_without_asking: self.runs_without_asking,
            startup_dialogs: dedupe(&self.startup_dialogs),
            exit_screen: self.exit_screen.clone(),
            notes: self.notes.clone(),
            answered: self.answered.clone(),
            resize: self.resize_result.clone(),
        }
    }

    pub fn status_json(&self, now: u64) -> Value {
        let steps: Vec<Value> = self
            .steps
            .iter()
            .filter(|s| s.id != StepId::Work || self.cfg.work_step)
            .map(|s| json!({"id": s.id.as_str(), "label": s.id.label(), "status": s.status.as_str(), "note": s.note, "frames": s.frames}))
            .collect();
        let step = self.current();
        let mut can: Vec<&str> = Vec::new();
        if !self.settled() {
            can.extend(["skip", "retry", "stuck"]);
            if matches!(step, StepId::Start | StepId::Ready | StepId::Message | StepId::Work | StepId::Permission | StepId::Exit) && !self.kind.is_dialog() {
                can.push("ready");
            }
            if step == StepId::Exit {
                can.extend(["try_exit", "closed"]);
            }
            if step == StepId::Permission && self.turn.phase == TurnPhase::Submitted && !self.kind.is_dialog() {
                can.extend(["done", "question"]);
            }
        } else {
            can.push("retry");
        }
        json!({
            "step": step.as_str(),
            // The permission step asks twice (a file write, then a command run): 0 or 1.
            "permission_phase": self.perm_phase,
            "steps": steps,
            "screen_kind": self.kind.as_str(),
            "auto_answer": self.answerer.is_some(),
            "answered": self.answered.iter().map(|a| json!({"place": a.place, "flavour": a.flavour, "label": a.label, "keys": a.keys, "via": a.via, "menu": a.read, "denied": a.denied})).collect::<Vec<_>>(),
            "frozen": self.frozen,
            "halted": self.halted,
            "fingerprint": self.stab.fingerprint(),
            "stable_ms": self.stab.stable_for(now),
            "alive": self.alive,
            "settled": self.settled(),
            "needs_you": self.needs.as_ref().map(|n| json!({"step": n.step.as_str(), "kind": n.kind, "message": n.message, "unreadable": self.menu_unreadable && matches!(n.kind.as_str(), "trust" | "permission")})),
            "question_fp": self.answerer.as_ref().and_then(|a| a.question_fp()).map(str::to_string),
            "menu_read": self.answerer.as_ref().and_then(|a| a.menu_read()).map(str::to_string),
            "can": can,
        })
    }
}

fn dedupe(list: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    list.iter().filter(|s| seen.insert((*s).clone())).cloned().collect()
}

/// Rows that hold the reply word but are not the echo of the instruction.
pub fn reply_rows(screen: &str, word: &str) -> usize {
    let word = word.to_lowercase();
    screen
        .lines()
        .map(str::to_lowercase)
        .filter(|row| row.contains(&word) && !row.contains("reply with") && !row.contains("exactly the word"))
        .count()
}
