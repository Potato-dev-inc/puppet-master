//! Reading a pane's screen for the guided capture (SPEC 11.5): what kind of screen it is, and whether it
//! has held still. Everything here is plain text in, plain facts out: no pane, no clock of its own, so
//! it is tested with strings.
//!
//! The classifier is deliberately conservative. `Input` needs positive evidence of an input box; a
//! screen that shows nothing the classifier knows is `Unknown`, and the capture then asks the person
//! instead of guessing. A dialog is recognised from its choices (`[y/n]`, numbered yes/no rows, Allow /
//! Deny, "Press Enter to continue") in the last rows of the screen.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

/// The fixed grid every wizard pane is spawned at (the viewer grid must equal the PTY grid).
pub const GRID_COLS: u16 = 120;
pub const GRID_ROWS: u16 = 40;

/// What a dialog is about (decides how the wizard words the request to the person).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    /// "Do you trust the files in this folder?"
    Trust,
    /// Sign-in, API key, device code.
    Login,
    /// "Allow ... to run / create / edit ...?"
    Permission,
    /// Any other question with choices (update notice, tips, onboarding).
    Question,
}

impl Flavour {
    pub fn as_str(self) -> &'static str {
        match self {
            Flavour::Trust => "trust",
            Flavour::Login => "login",
            Flavour::Permission => "permission",
            Flavour::Question => "question",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenKind {
    Blank,
    /// A bare host-shell prompt where the agent was: the agent is not running.
    Shell,
    Dialog(Flavour),
    /// A turn is running (a spinner, or a "stop / interrupt" hint).
    Working,
    /// An input box that takes a task.
    Input,
    Unknown,
}

impl ScreenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ScreenKind::Blank => "blank",
            ScreenKind::Shell => "shell",
            ScreenKind::Dialog(_) => "dialog",
            ScreenKind::Working => "working",
            ScreenKind::Input => "input",
            ScreenKind::Unknown => "unknown",
        }
    }

    pub fn is_dialog(self) -> bool {
        matches!(self, ScreenKind::Dialog(_))
    }
}

/// Rows with something on them, trimmed at the end, in order.
pub fn rows(screen: &str) -> Vec<&str> {
    screen.lines().map(str::trim_end).filter(|r| !r.trim().is_empty()).collect()
}

fn last_rows(screen: &str, n: usize) -> Vec<&str> {
    let all = rows(screen);
    let from = all.len().saturating_sub(n);
    all[from..].to_vec()
}

/// A line of choices: `[y/n]`, `(y/n)`, numbered yes/no rows, Allow/Deny, "Press Enter to continue".
static CHOICES: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?im)",
        r"\[y/n\]|\(y/n\)|\by/n\b|\[yes/no\]|\(yes/no\)|\[y/N\]|\[Y/n\]",
        r"|^\W{0,4}\d[.)]\s*(?:yes|no|allow|deny|continue|trust|accept|decline|quit|exit|proceed|reject|cancel)\b",
        r"|^\W{0,4}(?:yes|no)[,.]\s+\w+",
        r"|\bpress\s+enter\s+(?:to\s+)?(?:continue|confirm|accept|proceed)\b",
        r"|\benter\s+to\s+(?:confirm|continue|accept)\b",
        r"|\ballow\b.{0,60}\b(?:deny|reject|decline)\b|\b(?:deny|reject|decline)\b.{0,60}\ballow\b|\ballow\s+(?:once|always)\b",
        r"|^\s*[\u{276F}\u{203A}>\u{25B6}\u{25CF}\u{25CB}\u{25C9}]\s*\d[.)]\s+\S",
        r"|\byes,?\s+(?:i\s+)?(?:trust|proceed|allow)\b|\bno,?\s+(?:exit|quit|cancel)\b",
        r"|\[a\]llow|\[d\]eny|\(a\)llow|\(d\)eny",
    ))
    .expect("choices regex")
});

static TRUST: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:do you trust|trust (?:this|the|these|files|folder|directory|workspace|project)|(?:folder|directory|workspace|project|repo(?:sitory)?)\b.{0,40}\btrust|is this a project you (?:created|trust)|safety check)",
    )
    .expect("trust regex")
});

static LOGIN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:log ?in|sign ?in|sign ?up|authenticate|authorization|api[ _-]?key|device code|verification code|open (?:this )?(?:url|link)|open your browser|paste (?:your|the) (?:key|token|code))\b",
    )
    .expect("login regex")
});

static PERMISSION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:allow|approve|permit|grant|permission|proceed|run(?:ning)?|execute|create|write|edit|delete|apply|overwrite)\b",
    )
    .expect("permission regex")
});

/// A spinner glyph or a progress word with an ellipsis, or a stop hint, near the bottom.
static WORKING: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?i)",
        r"[\u{2800}-\u{28FF}]\s+\S",
        r"|[\u{2722}\u{2733}\u{2736}\u{273B}\u{273D}\u{2726}]\s+\S[^\n]{0,60}(?:\u{2026}|\.\.\.)",
        r"|\b(?:esc|ctrl[+-]c|escape|press (?:esc|ctrl[+-]c))\s*(?:to\s+)?(?:interrupt|stop|cancel|abort)\b",
        r"|\b(?:thinking|generating|working|processing|analy[sz]ing|searching|loading|starting)\b.{0,40}(?:\.\.\.|\u{2026})",
    ))
    .expect("working regex")
});

static INPUT_GLYPH: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(?:[\u{2502}\u{2503}|]\s*)?(?:[>\u{276F}\u{203A}\u{00BB}\u{25B6}$]\s?|[>\u{276F}\u{203A}\u{00BB}\u{25B6}$]$)")
        .expect("input glyph regex")
});

static INPUT_WORDS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(?:ask anything|type (?:a|your|the)\b|type here|enter (?:a |your )?(?:task|message|prompt|command)|send a message|your message|how can i help|what (?:would you like|can i)|describe (?:a|the|your)|/help\b|\? for shortcuts|/ for commands|message the|prompt\b.{0,12}\.\.\.)",
    )
    .expect("input words regex")
});

/// Two or more hotkey hints such as `(y)`, `(esc or n)`, `(a)`, `(tab)`: how Cursor and Codex list choices.
static HOTKEYS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\(\s*(?:y|n|a|d|esc|tab|enter|shift\+tab)(?:\s+or\s+\w+)?\s*\)").expect("hotkeys regex")
});

/// "Do you trust ...?" asked as a plain question, with the way out printed as sentences.
static TRUST_QUESTION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:do you trust|trust (?:is )?required|you must trust|trust this (?:folder|directory|workspace|project))")
        .expect("trust question regex")
});

/// Does the end of the screen talk about trusting a folder or ask permission to create, edit or run something? The AI
/// look ("Finish without me") may only hand a screen to the safe answerer when it does, so a hallucinated "this is a
/// trust question" about an update prompt or a tip never presses anything.
pub fn asks_trust_or_permission(screen: &str) -> bool {
    let tail = last_rows(screen, 16).join("\n");
    TRUST.is_match(&tail) || PERMISSION.is_match(&tail)
}

/// Is `screen` a dialog, and about what? Looks at the last 16 rows: an answered dialog that scrolled up
/// is part of the transcript, not a question.
fn dialog(screen: &str) -> Option<Flavour> {
    let tail = last_rows(screen, 16).join("\n");
    if TRUST_QUESTION.is_match(&tail) {
        return Some(Flavour::Trust);
    }
    if !CHOICES.is_match(&tail) && HOTKEYS.find_iter(&tail).count() < 2 {
        return None;
    }
    if TRUST.is_match(&tail) {
        return Some(Flavour::Trust);
    }
    if LOGIN.is_match(&tail) {
        return Some(Flavour::Login);
    }
    if PERMISSION.is_match(&tail) {
        return Some(Flavour::Permission);
    }
    Some(Flavour::Question)
}

fn working(screen: &str) -> bool {
    let tail = last_rows(screen, 8).join("\n");
    WORKING.is_match(&tail)
}

fn input_box(screen: &str) -> bool {
    last_rows(screen, 12)
        .iter()
        .any(|row| INPUT_GLYPH.is_match(row) || (INPUT_WORDS.is_match(row) && row.chars().count() < 160))
}

/// Words that belong to a question or a permission. An input box that shows them is only trusted when
/// the person says so (the guard in `capture::may_type`).
static SUSPECT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:trust(?:ed)?|permissions?|authori[sz]e|approv(?:e|al)|sign[ -]?in|log[ -]?in|password|api[ _-]?key|press enter|are you sure|do you want|would you like)\b|\[y/n\]|\(y/n\)",
    )
    .expect("suspect regex")
});

/// The screen's kind. Order: blank, shell, dialog, working, input, unknown.
pub fn classify(screen: &str) -> ScreenKind {
    if screen.trim().is_empty() {
        return ScreenKind::Blank;
    }
    if crate::tui::shell::host_prompt_visible(screen) {
        return ScreenKind::Shell;
    }
    if let Some(flavour) = dialog(screen) {
        return ScreenKind::Dialog(flavour);
    }
    if working(screen) {
        return ScreenKind::Working;
    }
    if input_box(screen) {
        // An input box next to words that belong to a question is not trusted: the wizard asks the person.
        if SUSPECT.is_match(&last_rows(screen, 24).join("\n")) {
            return ScreenKind::Unknown;
        }
        return ScreenKind::Input;
    }
    ScreenKind::Unknown
}

/// Does `kind` on this screen mean the agent shows its own question (never ours to answer)?
pub fn needs_person(kind: ScreenKind) -> Option<Flavour> {
    match kind {
        ScreenKind::Dialog(f) => Some(f),
        _ => None,
    }
}

/// Folds what animates: digits become `0`, spinner and progress glyphs `*`, trailing spaces and blank
/// rows go. Two frames with the same key are the same picture for the capture.
pub fn frame_key(screen: &str) -> Vec<String> {
    rows(screen)
        .iter()
        .map(|row| {
            let mut out = String::with_capacity(row.len());
            let mut in_digits = false;
            for c in row.chars() {
                let folded = match c {
                    '0'..='9' => '0',
                    '\u{2800}'..='\u{28FF}' => '*',
                    '\u{2722}' | '\u{2733}' | '\u{2736}' | '\u{273B}' | '\u{273D}' | '\u{2726}' => '*',
                    '\u{25D0}' | '\u{25D3}' | '\u{25D1}' | '\u{25D2}' | '\u{25CF}' | '\u{25CB}' => '*',
                    other => other,
                };
                // a run of digits is one digit: "3s" and "12s" are the same picture
                if folded == '0' && in_digits {
                    continue;
                }
                in_digits = folded == '0';
                out.push(folded);
            }
            out
        })
        .collect()
}

fn differing_rows(a: &[String], b: &[String]) -> usize {
    let common = a.len().min(b.len());
    let mut diff = a.len().abs_diff(b.len());
    for i in 0..common {
        if a[i] != b[i] {
            diff += 1;
        }
    }
    diff
}

/// How many differing rows still count as the same picture (an animated row, a clock).
pub const TOLERANCE: usize = 1;
/// Distinct exact frames kept per anchor.
const MAX_DISTINCT: usize = 16;

/// Tracks whether the screen has held still. The anchor is the picture the timer runs for: a new frame
/// within `TOLERANCE` rows of it keeps the anchor and the timer; anything else starts a new anchor.
#[derive(Debug, Default, Clone)]
pub struct Stability {
    anchor: Vec<String>,
    since_ms: u64,
    last_seen_ms: u64,
    /// Exact frames seen under this anchor: (text, times seen, first seen order).
    bag: Vec<(String, u32)>,
    seen: bool,
    /// The last exact frame, and when it last differed from the one before. `frame_key` folds spinner
    /// glyphs and digit runs, so a spinner or a clock keeps the anchor; these two see them move.
    raw_last: String,
    raw_changed_ms: u64,
}

impl Stability {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one frame. Returns true when it started a new anchor (the picture changed).
    pub fn observe(&mut self, screen: &str, now_ms: u64) -> bool {
        let key = frame_key(screen);
        let changed = !self.seen || differing_rows(&key, &self.anchor) > TOLERANCE;
        if changed {
            self.anchor = key;
            self.since_ms = now_ms;
            self.bag.clear();
            self.seen = true;
        }
        self.last_seen_ms = now_ms;
        let text = screen.trim_end().to_string();
        if changed || text != self.raw_last {
            self.raw_last = text.clone();
            self.raw_changed_ms = now_ms;
        }
        if let Some(entry) = self.bag.iter_mut().find(|(t, _)| *t == text) {
            entry.1 += 1;
        } else if self.bag.len() < MAX_DISTINCT {
            self.bag.push((text, 1));
        }
        changed
    }

    pub fn stable_for(&self, now_ms: u64) -> u64 {
        if self.seen {
            now_ms.saturating_sub(self.since_ms)
        } else {
            0
        }
    }

    /// How long not one character of the screen has moved (a spinner, a timer and a blinking row all
    /// count as moving). A screen that is "working" by its words but has been dead still this long is
    /// not working.
    pub fn quiet_for(&self, now_ms: u64) -> u64 {
        if self.seen {
            now_ms.saturating_sub(self.raw_changed_ms)
        } else {
            0
        }
    }

    /// Frames sampled under the current anchor.
    pub fn samples(&self) -> u32 {
        self.bag.iter().map(|(_, n)| n).sum()
    }

    /// A short stable name for the current picture (what "the person said it is ready" is bound to).
    pub fn fingerprint(&self) -> String {
        let joined = self.anchor.join("\n");
        super::super::session::sha256_hex(joined.as_bytes())[..16].to_string()
    }

    /// The most frequent exact frame under the anchor (the earlier one on a tie), and the rows in which
    /// the other frames differ from it.
    pub fn best(&self) -> Option<(String, Vec<String>)> {
        let mut best: Option<&(String, u32)> = None;
        for entry in &self.bag {
            if best.map_or(true, |b| entry.1 > b.1) {
                best = Some(entry);
            }
        }
        let (text, _) = best?;
        Some((text.clone(), variant_rows(text, self.bag.iter().map(|(t, _)| t.as_str()))))
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A bag of frames seen while something was changing (the working screen): the most frequent frame that
/// is not in `exclude`, plus the other rows.
#[derive(Debug, Default, Clone)]
pub struct FrameBag {
    frames: HashMap<String, u32>,
    order: Vec<String>,
}

impl FrameBag {
    pub fn add(&mut self, screen: &str) {
        let text = screen.trim_end().to_string();
        if text.trim().is_empty() {
            return;
        }
        if let Some(n) = self.frames.get_mut(&text) {
            *n += 1;
            return;
        }
        if self.order.len() >= 64 {
            return;
        }
        self.order.push(text.clone());
        self.frames.insert(text, 1);
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.order.clear();
    }

    /// The most frequent frame whose key differs from every key in `exclude`; ties go to the earlier one.
    pub fn best_excluding(&self, exclude: &[Vec<String>]) -> Option<(String, Vec<String>)> {
        let mut best: Option<(&String, u32)> = None;
        for text in &self.order {
            let key = frame_key(text);
            if exclude.iter().any(|e| differing_rows(e, &key) == 0) {
                continue;
            }
            let n = self.frames[text];
            if best.map_or(true, |(_, bn)| n > bn) {
                best = Some((text, n));
            }
        }
        let (text, _) = best?;
        Some((text.clone(), variant_rows(text, self.order.iter().map(String::as_str))))
    }
}

/// Rows of the other frames that are not rows of `best`: what moved between frames. At most 20, no
/// duplicates, taken from at most 5 other frames.
fn variant_rows<'a>(best: &str, frames: impl Iterator<Item = &'a str>) -> Vec<String> {
    let best_rows: Vec<&str> = best.lines().map(str::trim_end).collect();
    let mut variants: Vec<String> = Vec::new();
    for other in frames.filter(|f| *f != best).take(5) {
        for row in other.lines().map(str::trim_end) {
            if !row.trim().is_empty() && !best_rows.contains(&row) && variants.len() < 20 && !variants.iter().any(|v| v == row) {
                variants.push(row.to_string());
            }
        }
    }
    variants
}

/// Does `screen` have the shape of `idle`, the picture kept when the agent first waited for a task?
/// What counts is the EMPTY input box near the bottom: its placeholder words ("Type a task...", "Ask
/// anything") or, for a box that has none, the same bare prompt row. A row that merely starts with `>`
/// (the task that was just typed, a question's answer line) is not an empty input box.
pub fn ready_shape(idle: &str, screen: &str) -> bool {
    let tail = last_rows(screen, 8);
    let idle_tail = last_rows(idle, 12);
    let placeholder = |row: &&str| INPUT_WORDS.is_match(row) && row.chars().count() < 160;
    if idle_tail.iter().any(|r| placeholder(&r)) {
        return tail.iter().any(placeholder);
    }
    let wanted: Vec<Vec<String>> = idle_tail.iter().filter(|r| INPUT_GLYPH.is_match(r)).map(|r| frame_key(r)).collect();
    if !wanted.is_empty() {
        let have: Vec<Vec<String>> = tail.iter().map(|r| frame_key(r)).collect();
        return wanted.iter().all(|w| have.contains(w));
    }
    bottom_matches(idle, screen, 4)
}

/// Do the last `n` rows of `screen` look like the last `n` rows of `idle` (the picture seen when the
/// agent waited for its first task)? One row may differ (a clock, a context counter). Used to tell
/// that a turn is over when the screen is back at the shape of the Ready step.
pub fn bottom_matches(idle: &str, screen: &str, n: usize) -> bool {
    let a = frame_key(idle);
    let b = frame_key(screen);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let n = n.min(a.len()).min(b.len());
    differing_rows(&a[a.len() - n..], &b[b.len() - n..]) <= TOLERANCE
}

/// How many rows of `screen` contain `word` (case-insensitive).
pub fn rows_with(screen: &str, word: &str) -> usize {
    let word = word.to_lowercase();
    screen.lines().filter(|r| r.to_lowercase().contains(&word)).count()
}

/// The first row containing `word`, trimmed.
pub fn row_with<'a>(screen: &'a str, word: &str) -> Option<&'a str> {
    let word = word.to_lowercase();
    screen.lines().map(str::trim).find(|r| r.to_lowercase().contains(&word))
}

/// The last row containing `word`, trimmed (the reply row comes after the echo).
pub fn last_row_with<'a>(screen: &'a str, word: &str) -> Option<&'a str> {
    let word = word.to_lowercase();
    screen.lines().map(str::trim).rev().find(|r| r.to_lowercase().contains(&word))
}
