//! The wizard answering ITS OWN test panes' setup questions (SPEC 11.16). A narrow exception to "the wizard
//! never answers a trust or permission dialog": only a pane the wizard started itself, in the wizard's own
//! isolated temporary folder, and only a trust-this-folder or a one-time approval question. Everything is
//! decided here from the text on the screen, with no AI: parse the menu, find the one option that plainly
//! means "yes / trust this folder / allow once", refuse anything that says always / don't ask again / all /
//! session / yolo / bypass, and refuse sign-in, payment, model and telemetry questions altogether.
//!
//! Any doubt is a no: the question stays the person's (`Step::Declined`). Pure and clock-free: the caller gives
//! the screen, how long it has held still and the time, and carries out the keys it is handed (after
//! `scope_ok` said the pane is the wizard's own). The tests drive it with scripted screens.

use std::path::{Component, Path};

use serde_json::{json, Value};

use super::screen::{frame_key, rows, Flavour};

/// One key the wizard may send to answer a question. There is nothing else: no free text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerKey {
    Up,
    Down,
    Left,
    Right,
    Tab,
    Enter,
    /// A short literal: `y`, or the number of a numbered option.
    Text(String),
}

impl AnswerKey {
    pub fn say(&self) -> String {
        match self {
            AnswerKey::Up => "Up".into(),
            AnswerKey::Down => "Down".into(),
            AnswerKey::Left => "Left".into(),
            AnswerKey::Right => "Right".into(),
            AnswerKey::Tab => "Tab".into(),
            AnswerKey::Enter => "Enter".into(),
            AnswerKey::Text(t) => t.clone(),
        }
    }

    /// The key a `[[tui.prompt]]` choice names (`enter`, `y`, `down`...), or `None` for anything else.
    pub fn from_name(name: &str) -> Option<AnswerKey> {
        match name.trim().to_ascii_lowercase().as_str() {
            "enter" | "return" => Some(AnswerKey::Enter),
            "down" => Some(AnswerKey::Down),
            "up" => Some(AnswerKey::Up),
            "left" => Some(AnswerKey::Left),
            "right" => Some(AnswerKey::Right),
            "tab" => Some(AnswerKey::Tab),
            other if other.chars().count() == 1 && other.chars().all(|c| c.is_ascii_alphanumeric()) => Some(AnswerKey::Text(other.to_string())),
            _ => None,
        }
    }
}

/// What the answerer did, for the progress list, the Review screen and `<id>.wizard.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsweredQ {
    /// `capture` or `visibletest`.
    pub place: String,
    /// `trust` or `permission`.
    pub flavour: String,
    /// The option chosen, as the screen words it ("Yes, I trust this folder").
    pub label: String,
    /// The keys sent, in words ("Down, Enter").
    pub keys: String,
    /// The question as it was on the screen (cleaned; masked wherever an AI or the UI sees it).
    pub screen: String,
    /// `rules` (the answerer read the menu itself) or `ai_look` (the AI named the option and the keys, Rust checked them).
    pub via: String,
    /// How the answerer read the question's menu when it decided (`rows: "Allow once" (plain yes, highlighted); ...`).
    pub read: String,
}

impl AnsweredQ {
    pub fn to_json(&self, mask: &dyn Fn(&str) -> String) -> Value {
        json!({"place": self.place, "flavour": self.flavour, "label": self.label, "keys": self.keys, "via": self.via, "menu": mask(&self.read), "screen": mask(&self.screen)})
    }
}

// ---- reading the question (menu.rs) ---------------------------------------------------------------

pub use super::menu::{
    classify_option, describe_menu, off_limits, parse_menu, parse_menu_ex, safe_pick, style_highlight, Menu, MenuKind, MenuOption, OptionClass, NO_PLAIN_YES,
};
use super::menu::marker_chars;
use super::styled::StyledScreen;

// ---- the answer plan ---------------------------------------------------------------------------------

/// Why the answerer said no. The first three mean "the menu could not be READ" (an AI look may help); every other reason
/// is a decision (no safe option, a question it never answers, a menu that moved): nobody may help with those.
pub const NO_MENU: &str = "no menu or yes/no question could be read from the screen";
pub const NO_HIGHLIGHT: &str = "could not see which row is highlighted";
pub const LOST_HIGHLIGHT: &str = "could not confirm which row is highlighted";
pub const MENU_CHANGED: &str = "the menu changed while it was being answered";

/// The reason is "I could not read the menu" (not "I read it and the answer is no").
pub fn is_unreadable(reason: &str) -> bool {
    [NO_MENU, NO_HIGHLIGHT, LOST_HIGHLIGHT].contains(&reason)
}

/// The checked AI look may be asked about this question: the menu could not be READ, or it was read and no option is a plain
/// yes (the parser may have missed or merged the row that is). Rust still holds the look's answer to the plain-yes rule
/// (`ai_menu::validate`), so a menu that only has broad options is never pressed. A question the answerer will never answer
/// (sign-in, payment, model, telemetry), a menu with several plain yeses and a menu that moved are nobody's to help with.
pub fn can_look(reason: &str) -> bool {
    is_unreadable(reason) || reason == NO_PLAIN_YES
}

/// A choice a draft's `[[tui.prompt]]` already knows (`allow_once`): the keys it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub keys: Vec<AnswerKey>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Plan {
    Hint(Vec<AnswerKey>),
    /// Rows (Up / Down) or one row of buttons (`horizontal`: Right / Left, else Tab) walked until the highlight is on the target.
    Arrow { target: String, horizontal: bool },
    Number { number: u32, target: String },
    YesNo,
    /// The letter of the option (`[Y]es`).
    Letter { letter: char, target: String },
    /// The keys an AI look named; checked by `ai_menu::validate`, pressed one at a time with the screen read after each.
    Ai { target: String, keys: Vec<AnswerKey> },
}

#[derive(Debug, Clone)]
pub struct AnswerCfg {
    /// The question must have held still this long before any key.
    pub min_stable_ms: u64,
    /// Between two keys.
    pub key_gap_ms: u64,
    /// The same question still there this long after the answer: give up and ask the person.
    pub persist_ms: u64,
    pub max_keys: u32,
}

impl Default for AnswerCfg {
    fn default() -> Self {
        Self { min_stable_ms: 1_000, key_gap_ms: 500, persist_ms: 5_000, max_keys: 8 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Not a question the answerer has anything to do with.
    Nothing,
    /// A question it will answer, not yet.
    Waiting(String),
    /// Send these keys now.
    Press { keys: Vec<AnswerKey>, note: String },
    /// The question went away after the answer.
    Answered(AnsweredQ),
    /// It will not answer this one. The person does. The text says why.
    Declined(String),
}

#[derive(Debug, Clone)]
struct Occ {
    fp: String,
    plan: Option<Plan>,
    declined: Option<String>,
    sent: Vec<AnswerKey>,
    last_key_ms: u64,
    first_key_ms: u64,
    last_highlight: Option<usize>,
    final_sent: bool,
    /// A row of buttons that ignores Left / Right is tried with Tab.
    tab_mode: bool,
    /// The screen (text and attributes) as it was when the last key went in: a navigation key must change it.
    sig: Option<String>,
    via: &'static str,
    label: String,
    flavour: Flavour,
    first_screen: String,
    /// How the menu was read when this question was first looked at (`describe_menu`, or why nothing was read).
    read: String,
}

pub struct Answerer {
    cfg: AnswerCfg,
    place: &'static str,
    cur: Option<Occ>,
    /// What it read and decided for each question, oldest first (the debug file keeps it).
    log: Vec<Value>,
}

pub struct Input<'a> {
    pub screen: &'a str,
    /// The same screen with its cell attributes (inverse, colours, bold), when the host has them: a menu that marks its
    /// highlighted row only by how it is drawn.
    pub styled: Option<&'a StyledScreen>,
    /// What the capture classifier calls the screen (a dialog's flavour), `None` when it is not a dialog.
    pub flavour: Option<Flavour>,
    pub stable_ms: u64,
    pub now: u64,
    pub hint: Option<&'a Hint>,
}

/// The question's identity: its rows with the cursor marker and digits folded, so moving the cursor is
/// still the same question.
pub fn fingerprint(screen: &str) -> String {
    // a box border is not part of the question (`│ ❯ Yes │` and `│   Yes │` are the same row)
    let all: Vec<String> = rows(screen).into_iter().map(super::menu::normalise_row).filter(|r| !r.trim().is_empty()).collect();
    let from = all.len().saturating_sub(16);
    let marks = marker_chars();
    let folded: Vec<String> = all[from..].iter().map(|r| r.trim_start_matches(|c: char| c.is_whitespace() || marks.contains(c)).to_string()).collect();
    frame_key(&folded.join("\n")).join("\n")
}

/// The screen as one string, attributes included: two looks are equal only when nothing on the screen moved.
fn signature(screen: &str, styled: Option<&StyledScreen>) -> String {
    match styled {
        Some(s) => {
            let from = s.rows.len().saturating_sub(24);
            format!("{screen}\n{:?}", &s.rows[from..])
        }
        None => screen.to_string(),
    }
}

impl Answerer {
    pub fn new(place: &'static str, cfg: AnswerCfg) -> Self {
        Self { cfg, place, cur: None, log: Vec::new() }
    }

    /// Forget the question being answered (a retry of the step).
    pub fn reset(&mut self) {
        self.cur = None;
    }

    /// The AI look named an option and keys for the question the answerer said it could not read (SPEC 11.20). Rust checks
    /// them (`ai_menu::validate`) and, when they hold, the answerer presses them one at a time. Returns the option's label.
    pub fn offer_ai(&mut self, screen: &str, fp: &str, option: &str, keys: &[String]) -> Result<String, String> {
        let Some(occ) = self.cur.as_mut() else { return Err("no question is being answered".into()) };
        if !occ.declined.as_deref().is_some_and(can_look) {
            return Err("the safe answerer did not ask for a look at this question".into());
        }
        if !occ.sent.is_empty() {
            return Err("keys were already pressed for this question".into());
        }
        if occ.fp != fingerprint(screen) || (!fp.is_empty() && occ.fp != fp) {
            return Err("the question changed since it was looked at".into());
        }
        let read = occ.read.clone();
        let flavour = occ.flavour.as_str();
        let plan = match super::ai_menu::validate(screen, option, keys) {
            Ok(plan) => plan,
            Err(why) => {
                self.log.push(json!({"place": self.place, "flavour": flavour, "menu": read, "decision": format!("AI look named \"{option}\" with keys {keys:?}; Rust refused: {why}")}));
                return Err(why);
            }
        };
        self.log.push(json!({"place": self.place, "flavour": flavour, "menu": read, "decision": format!("AI look named \"{}\"; Rust checked it against the screen", plan.target)}));
        let occ = self.cur.as_mut().expect("the question is still being answered");
        occ.declined = None;
        occ.label = plan.target.clone();
        occ.via = "ai_look";
        occ.plan = Some(Plan::Ai { target: plan.target.clone(), keys: plan.keys });
        Ok(plan.target)
    }

    /// How the menu of the question being looked at was read (`describe_menu`, or why nothing was read).
    pub fn menu_read(&self) -> Option<&str> {
        self.cur.as_ref().map(|o| o.read.as_str())
    }

    /// What it read and decided for every question so far, oldest first.
    pub fn decisions(&self) -> &[Value] {
        &self.log
    }

    /// Why the answerer declined the question it is looking at, when it did.
    pub fn declined(&self) -> Option<&str> {
        self.cur.as_ref().and_then(|o| o.declined.as_deref())
    }

    /// The identity of the question being looked at.
    pub fn question_fp(&self) -> Option<&str> {
        self.cur.as_ref().map(|o| o.fp.as_str())
    }

    pub fn step(&mut self, input: &Input<'_>) -> Step {
        let Some(flavour) = input.flavour else {
            // The question is gone. If keys went in for it, it was answered.
            return match self.cur.take() {
                Some(occ) if !occ.sent.is_empty() => Step::Answered(self.done(&occ)),
                _ => Step::Nothing,
            };
        };
        if !matches!(flavour, Flavour::Trust | Flavour::Permission) {
            // Sign-in and any other question: the person's.
            self.cur = None;
            return Step::Declined(format!("it is a {} question, which only you answer", flavour.as_str()));
        }
        let fp = fingerprint(input.screen);
        if self.cur.as_ref().is_some_and(|c| c.fp != fp) {
            // A different question replaced it: the first one was answered if keys went in.
            let old = self.cur.take().expect("checked");
            if !old.sent.is_empty() {
                // report the answer now; the new question is looked at on the next call
                return Step::Answered(self.done(&old));
            }
        }
        if self.cur.is_none() {
            if input.stable_ms < self.cfg.min_stable_ms {
                return Step::Waiting("Waiting for the question to hold still.".into());
            }
            let mut occ = Occ {
                fp,
                plan: None,
                declined: None,
                sent: Vec::new(),
                last_key_ms: 0,
                first_key_ms: 0,
                last_highlight: None,
                final_sent: false,
                tab_mode: false,
                sig: None,
                via: "rules",
                label: String::new(),
                flavour,
                first_screen: input.screen.to_string(),
                read: String::new(),
            };
            let (read, decision) = decide(input.screen, input.styled, input.hint);
            occ.read = read;
            match decision {
                Ok((plan, label)) => {
                    self.log.push(json!({"place": self.place, "flavour": flavour.as_str(), "menu": occ.read, "decision": format!("will answer: {label}")}));
                    occ.plan = Some(plan);
                    occ.label = label;
                }
                Err(reason) => {
                    self.log.push(json!({"place": self.place, "flavour": flavour.as_str(), "menu": occ.read, "decision": format!("declined: {reason}")}));
                    occ.declined = Some(reason);
                }
            }
            self.cur = Some(occ);
        }
        self.drive(input)
    }

    fn done(&self, occ: &Occ) -> AnsweredQ {
        AnsweredQ {
            place: self.place.to_string(),
            flavour: occ.flavour.as_str().to_string(),
            label: occ.label.clone(),
            keys: occ.sent.iter().map(AnswerKey::say).collect::<Vec<_>>().join(", "),
            screen: occ.first_screen.clone(),
            via: occ.via.to_string(),
            read: occ.read.clone(),
        }
    }

    fn drive(&mut self, input: &Input<'_>) -> Step {
        let cfg = self.cfg.clone();
        let occ = self.cur.as_mut().expect("an occurrence");
        if let Some(reason) = &occ.declined {
            return Step::Declined(reason.clone());
        }
        let now = input.now;
        let plan = occ.plan.clone().expect("a plan");
        // An AI answer may press several keys half a second apart: its deadline runs from the LAST key.
        let since = if matches!(plan, Plan::Ai { .. }) { occ.last_key_ms } else { occ.first_key_ms };
        if !occ.sent.is_empty() && now.saturating_sub(since) >= cfg.persist_ms && (occ.final_sent || !matches!(plan, Plan::Ai { .. })) {
            let reason = "the question is still on the screen 5 seconds after the answer".to_string();
            occ.declined = Some(reason.clone());
            return Step::Declined(reason);
        }
        if !occ.sent.is_empty() && now.saturating_sub(occ.last_key_ms) < cfg.key_gap_ms {
            return Step::Waiting("Answering the question.".into());
        }
        let decline = |occ: &mut Occ, reason: &str| {
            occ.declined = Some(reason.to_string());
            Step::Declined(reason.to_string())
        };
        let press = |occ: &mut Occ, keys: Vec<AnswerKey>, final_key: bool| {
            if occ.sent.is_empty() {
                occ.first_key_ms = now;
            }
            occ.last_key_ms = now;
            occ.sent.extend(keys.iter().cloned());
            occ.final_sent |= final_key;
            occ.sig = Some(signature(input.screen, input.styled));
            Step::Press { keys, note: format!("Answering the {} question: {}", occ.flavour.as_str(), occ.label) }
        };
        let waiting = || Step::Waiting("Answered. Waiting for the question to go.".into());
        match plan {
            Plan::Hint(keys) => {
                if occ.sent.is_empty() {
                    press(occ, keys, true)
                } else {
                    waiting()
                }
            }
            Plan::YesNo => {
                if occ.sent.is_empty() {
                    press(occ, vec![AnswerKey::Text("y".into())], false)
                } else if !occ.final_sent {
                    // some prompts need Enter after the letter; the question is still here
                    press(occ, vec![AnswerKey::Enter], true)
                } else {
                    waiting()
                }
            }
            Plan::Letter { letter, target } => {
                if occ.sent.is_empty() {
                    return press(occ, vec![AnswerKey::Text(letter.to_string())], false);
                }
                if occ.final_sent {
                    return waiting();
                }
                // the letter may only move the highlight: confirm when it is plainly on the target
                let menu = parse_menu_ex(input.screen, input.styled);
                let on_target = menu.as_ref().and_then(|m| m.highlighted().map(|i| m.options[i].text == target)).unwrap_or(false);
                if on_target {
                    press(occ, vec![AnswerKey::Enter], true)
                } else {
                    Step::Waiting("Waiting for the question to react to the letter.".into())
                }
            }
            Plan::Number { number, target } => {
                let menu = parse_menu_ex(input.screen, input.styled).filter(|m| m.kind == MenuKind::Numbered);
                let Some(menu) = menu.filter(|m| m.options.iter().any(|o| o.number == Some(number) && o.text == target)) else {
                    return decline(occ, MENU_CHANGED);
                };
                if occ.sent.is_empty() {
                    return press(occ, vec![AnswerKey::Text(number.to_string())], false);
                }
                if occ.final_sent {
                    return waiting();
                }
                // typing the number may only move the cursor: confirm when it is on the intended row
                if menu.options.iter().any(|o| o.number == Some(number) && o.highlighted) {
                    press(occ, vec![AnswerKey::Enter], true)
                } else {
                    // some agents act on the number at once and take a moment to close the question
                    Step::Waiting("Waiting for the question to react to the number.".into())
                }
            }
            Plan::Arrow { target, horizontal } => {
                let wanted = if horizontal { MenuKind::Row } else { MenuKind::Arrow };
                let Some(menu) = parse_menu_ex(input.screen, input.styled).filter(|m| m.kind == wanted) else {
                    return decline(occ, MENU_CHANGED);
                };
                let picked = safe_pick(&menu).ok().filter(|i| menu.options[*i].text == target);
                let (Some(want), Some(here)) = (picked, menu.highlighted()) else {
                    return decline(occ, LOST_HIGHLIGHT);
                };
                if occ.final_sent {
                    return waiting();
                }
                if here == want {
                    return press(occ, vec![AnswerKey::Enter], true);
                }
                if occ.sent.len() as u32 >= cfg.max_keys {
                    return decline(occ, "the highlight did not reach the right row");
                }
                if occ.last_highlight == Some(here) && !occ.sent.is_empty() {
                    if now.saturating_sub(occ.last_key_ms) <= 1_500 {
                        return Step::Waiting("Waiting for the highlight to move.".into());
                    }
                    // A row of buttons may ignore Left / Right and move with Tab; nothing else is tried.
                    if horizontal && !occ.tab_mode {
                        occ.tab_mode = true;
                        occ.last_highlight = Some(here);
                        return press(occ, vec![AnswerKey::Tab], false);
                    }
                    return decline(occ, "the highlight did not move when the arrow key was pressed");
                }
                occ.last_highlight = Some(here);
                let key = match (horizontal, occ.tab_mode, want > here) {
                    (false, _, true) => AnswerKey::Down,
                    (false, _, false) => AnswerKey::Up,
                    (true, true, _) => AnswerKey::Tab,
                    (true, false, true) => AnswerKey::Right,
                    (true, false, false) => AnswerKey::Left,
                };
                press(occ, vec![key], false)
            }
            Plan::Ai { target, keys } => {
                let idx = occ.sent.len();
                if idx >= keys.len() {
                    return waiting();
                }
                // what the last key did: a navigation key must have changed something on the screen
                let sig = signature(input.screen, input.styled);
                if idx > 0 && super::ai_menu::is_navigation(&keys[idx - 1]) && occ.sig.as_deref() == Some(sig.as_str()) {
                    if now.saturating_sub(occ.last_key_ms) <= 1_500 {
                        return Step::Waiting("Waiting for the highlight to move.".into());
                    }
                    return decline(occ, "a navigation key changed nothing on the screen");
                }
                let next = keys[idx].clone();
                if next == AnswerKey::Enter {
                    // when the highlight can be read at all it must be on the named option
                    let menu = parse_menu_ex(input.screen, input.styled);
                    if let Some((menu, here)) = menu.and_then(|m| m.highlighted().map(|h| (m, h))) {
                        if super::ai_menu::canon(&menu.options[here].text) != super::ai_menu::canon(&target) {
                            return decline(occ, &format!("the highlight is on \"{}\", not on the named option", menu.options[here].text));
                        }
                    }
                }
                let last = idx + 1 == keys.len();
                press(occ, vec![next], last)
            }
        }
    }
}

/// How the menu was read, and the plan for this screen with the label of the option it picks.
fn decide(screen: &str, styled: Option<&StyledScreen>, hint: Option<&Hint>) -> (String, Result<(Plan, String), String>) {
    if let Some(word) = off_limits(screen) {
        return ("not read: it is a sign-in, payment, model or telemetry question".into(), Err(format!("the question mentions \"{word}\", and sign-in, payment, model and telemetry questions are only yours to answer")));
    }
    let menu = parse_menu_ex(screen, styled);
    let read = menu.as_ref().map_or_else(|| "no menu read".to_string(), describe_menu);
    let result = plan_for(menu, hint);
    (read, result)
}

fn plan_for(menu: Option<Menu>, hint: Option<&Hint>) -> Result<(Plan, String), String> {
    // The draft's own `allow_once` keys are used when the screen has no menu to read (a one-line question).
    // A menu that can be read is walked row by row instead: the highlight is verified, so a default cursor
    // on "No" is never confirmed, and an "always" row is never the one pressed.
    if let Some(hint) = hint {
        if menu.as_ref().is_none_or(|m| m.kind == MenuKind::YesNo) {
            return Ok((Plan::Hint(hint.keys.clone()), hint.label.clone()));
        }
    }
    let Some(menu) = menu else {
        return Err(NO_MENU.into());
    };
    let pick = safe_pick(&menu)?;
    match menu.kind {
        MenuKind::YesNo => Ok((Plan::YesNo, "Yes".into())),
        MenuKind::Numbered => {
            let o = &menu.options[pick];
            Ok((Plan::Number { number: o.number.unwrap_or(0), target: o.text.clone() }, o.text.clone()))
        }
        MenuKind::Arrow => {
            if menu.highlighted().is_none() {
                return Err(NO_HIGHLIGHT.into());
            }
            let o = &menu.options[pick];
            Ok((Plan::Arrow { target: o.text.clone(), horizontal: false }, o.text.clone()))
        }
        MenuKind::Row => {
            let o = &menu.options[pick];
            // a letter answers directly and cannot land on another button; else the highlight is walked
            if let Some(letter) = o.hotkey {
                return Ok((Plan::Letter { letter, target: o.text.clone() }, o.text.clone()));
            }
            if menu.highlighted().is_none() {
                return Err(NO_HIGHLIGHT.into());
            }
            Ok((Plan::Arrow { target: o.text.clone(), horizontal: true }, o.text.clone()))
        }
    }
}

// ---- where the wizard may answer ----------------------------------------------------------------------

fn normal(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_ascii_lowercase()),
            Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().trim_start_matches(r"\\?\").to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

/// A path with `..` or `.` in it could climb out of a folder while still starting with its name.
fn has_dots(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}

/// `child` is `parent` or below it (case-insensitive, `\\?\` prefix ignored). A path with `..` or `.`
/// in it is never "below": `normal` drops those parts, so they must be refused before comparing.
pub fn under(child: &Path, parent: &Path) -> bool {
    if has_dots(child) || has_dots(parent) {
        return false;
    }
    let (c, p) = (normal(child), normal(parent));
    !p.is_empty() && c.len() >= p.len() && c[..p.len()] == p[..]
}

/// The enforcement in code (SPEC 11.16): the pane's working folder is the session's own folder, which is
/// inside the wizard's temporary root. (The pane must also be one this wizard session started: the host
/// refuses keys for any other pane.) Everything else keeps the old rule: nothing is answered for the person.
pub fn scope_ok(pane_cwd: Option<&Path>, session_dir: &Path, temp_root: &Path) -> Result<(), String> {
    let Some(cwd) = pane_cwd else {
        return Err("the pane's folder is unknown".into());
    };
    if !under(session_dir, temp_root) {
        return Err("the session folder is not inside the wizard's temporary folder".into());
    }
    if !under(cwd, session_dir) {
        return Err("the pane is not in the wizard's own isolated folder".into());
    }
    Ok(())
}
