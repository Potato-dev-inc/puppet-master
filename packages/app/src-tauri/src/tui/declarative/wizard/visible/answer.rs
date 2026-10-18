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

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};

use super::screen::{frame_key, rows, Flavour};

/// One key the wizard may send to answer a question. There is nothing else: no free text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerKey {
    Up,
    Down,
    Enter,
    /// A short literal: `y`, or the number of a numbered option.
    Text(String),
}

impl AnswerKey {
    pub fn say(&self) -> String {
        match self {
            AnswerKey::Up => "Up".into(),
            AnswerKey::Down => "Down".into(),
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
}

impl AnsweredQ {
    pub fn to_json(&self, mask: &dyn Fn(&str) -> String) -> Value {
        json!({"place": self.place, "flavour": self.flavour, "label": self.label, "keys": self.keys, "screen": mask(&self.screen)})
    }
}

// ---- reading the question -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuOption {
    pub text: String,
    pub number: Option<u32>,
    /// The cursor marker is on this row (`>`, a filled bullet...).
    pub highlighted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuKind {
    /// Rows moved with Up and Down, Enter confirms.
    Arrow,
    /// Rows with numbers: the number selects (Enter may be needed).
    Numbered,
    /// A one-line `[y/n]` question.
    YesNo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub kind: MenuKind,
    pub options: Vec<MenuOption>,
}

impl Menu {
    pub fn highlighted(&self) -> Option<usize> {
        self.options.iter().position(|o| o.highlighted)
    }
}

static ROW: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?P<indent>\s*)(?:(?P<mark>[>\u{276F}\u{203A}\u{25B6}\u{2192}\u{25B8}\u{27A4}\u{25CF}\u{25C9}\u{25CB}\u{25EF}])\s+)?(?:(?P<num>\d{1,2})[.)]\s+)?(?P<text>\S.*?)\s*$",
    )
    .expect("menu row regex")
});

static HINT_ROW: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)navigate|\benter\b.*(?:confirm|select)|\besc\b|\u{2191}|\u{2193}|arrow keys|tab to").expect("hint row regex"));

static YESNO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\[y/n\]|\(y/n\)|\[yes/no\]|\(yes/no\)").expect("yes/no regex"));

fn is_selected_marker(c: char) -> bool {
    !matches!(c, '\u{25CB}' | '\u{25EF}')
}

struct Parsed {
    mark: Option<char>,
    num: Option<u32>,
    text_col: usize,
    text: String,
}

fn parse_row(row: &str) -> Option<Parsed> {
    let caps = ROW.captures(row)?;
    let text = caps.name("text")?.as_str().to_string();
    let start = caps.name("text")?.start();
    Some(Parsed {
        mark: caps.name("mark").and_then(|m| m.as_str().chars().next()),
        num: caps.name("num").and_then(|n| n.as_str().parse().ok()),
        text_col: row[..start].chars().count(),
        text,
    })
}

/// The menu in the last rows of `screen`, if there is a plain one.
pub fn parse_menu(screen: &str) -> Option<Menu> {
    let all = rows(screen);
    let from = all.len().saturating_sub(16);
    let tail = &all[from..];
    let parsed: Vec<Option<Parsed>> = tail.iter().map(|r| parse_row(r)).collect();
    let short = |p: &Parsed| p.text.chars().count() <= 80 && !HINT_ROW.is_match(&p.text);

    // numbered: consecutive rows numbered 1, 2, 3...
    let numbered: Vec<usize> = (0..tail.len()).filter(|i| parsed[*i].as_ref().is_some_and(|p| p.num.is_some() && short(p))).collect();
    if numbered.len() >= 2 && numbered.windows(2).all(|w| w[1] == w[0] + 1) {
        let options: Vec<MenuOption> = numbered
            .iter()
            .map(|i| {
                let p = parsed[*i].as_ref().expect("parsed");
                MenuOption { text: p.text.clone(), number: p.num, highlighted: p.mark.is_some_and(is_selected_marker) }
            })
            .collect();
        if options.iter().enumerate().all(|(k, o)| o.number == Some(k as u32 + 1)) {
            return Some(Menu { kind: MenuKind::Numbered, options });
        }
    }
    // arrow: a marker row and the rows around it that start at the same column, ROW AFTER ROW (a blank row between two
    // rows ends the group: the options of a menu are not spaced out, the echo of an earlier task, the bulleted summary
    // row an agent prints after a turn and a quoted line are). The LAST such group is the menu.
    let raw_all: Vec<&str> = screen.lines().map(str::trim_end).collect();
    let end = raw_all.iter().rposition(|r| !r.trim().is_empty()).map_or(0, |i| i + 1);
    let raw = &raw_all[end.saturating_sub(24)..end];
    let raw_parsed: Vec<Option<Parsed>> = raw.iter().map(|r| parse_row(r)).collect();
    let anchors = (0..raw.len()).rev().filter(|i| raw_parsed[*i].as_ref().is_some_and(|p| p.mark.is_some_and(is_selected_marker) && short(p) && p.num.is_none()));
    for anchor in anchors {
        let col = raw_parsed[anchor].as_ref().expect("parsed").text_col;
        let same = |i: usize| raw_parsed[i].as_ref().is_some_and(|p| p.text_col == col && short(p) && p.num.is_none());
        let mut lo = anchor;
        while lo > 0 && same(lo - 1) {
            lo -= 1;
        }
        let mut hi = anchor;
        while hi + 1 < raw.len() && same(hi + 1) {
            hi += 1;
        }
        if hi > lo {
            let options: Vec<MenuOption> = (lo..=hi)
                .map(|i| {
                    let p = raw_parsed[i].as_ref().expect("parsed");
                    MenuOption { text: p.text.clone(), number: None, highlighted: p.mark.is_some_and(is_selected_marker) }
                })
                .collect();
            return Some(Menu { kind: MenuKind::Arrow, options });
        }
    }
    let joined = tail.join("\n");
    if YESNO.is_match(&joined) {
        return Some(Menu { kind: MenuKind::YesNo, options: Vec::new() });
    }
    None
}

// ---- what is safe -----------------------------------------------------------------------------------

static SAFE_YES: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^(?:yes\b|y\b|allow(?: once| this time| just this once)?\b|approve(?: once)?\b|trust (?:this|the) (?:folder|project|workspace|directory)\b|run (?:once|this)\b|just this once\b)",
    )
    .expect("safe yes regex")
});

static UNSAFE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)always|don.?t ask|do not ask|never ask|ask again|for this session|this session|all tools|all commands|all (?:projects|folders|files)|trust all|yolo|bypass|dangerous|auto-?approve|skip (?:all )?permission|remember|every time|everything|permanently|forever|global|parent (?:folder|directory)|home (?:folder|directory)|full access|unrestricted",
    )
    .expect("unsafe regex")
});

static OFF_LIMITS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:log ?in|sign ?in|sign ?up|authenticat\w*|api[ _-]?keys?|tokens?|passwords?|credentials?|payment|billing|subscri\w*|upgrade|pricing|models?|telemetry|analytics|usage (?:data|statistics)|crash reports?|improve (?:the )?(?:product|service)|share (?:data|anonymous\w*)|survey|feedback|terms of (?:service|use)|privacy policy|license)\b",
    )
    .expect("off limits regex")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionClass {
    SafeYes,
    Risky,
    Other,
}

pub fn classify_option(text: &str) -> OptionClass {
    let t = text.trim();
    if UNSAFE.is_match(t) {
        OptionClass::Risky
    } else if SAFE_YES.is_match(t) {
        OptionClass::SafeYes
    } else {
        OptionClass::Other
    }
}

/// A count of tokens (`298 tokens`, `1.2k tokens`) is a number in a summary row an agent printed after a turn, not a
/// question about an API token.
static TOKEN_COUNT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b\d[\d.,]*\s*[km]?\s*(?:input |output |total |cached )?tokens?\b").expect("token count regex"));

/// Sign-in, payment, model and telemetry questions are never the wizard's to answer.
pub fn off_limits(screen: &str) -> Option<String> {
    let all = rows(screen);
    let from = all.len().saturating_sub(16);
    let tail = all[from..].join("\n");
    let tail = TOKEN_COUNT.replace_all(&tail, " ");
    OFF_LIMITS.find(&tail).map(|m| m.as_str().to_string())
}

/// The one option to choose, or why there is none. Exactly one plain "yes" or nothing.
pub fn safe_pick(menu: &Menu) -> Result<usize, String> {
    if menu.kind == MenuKind::YesNo {
        return Ok(0);
    }
    let safe: Vec<usize> = menu.options.iter().enumerate().filter(|(_, o)| classify_option(&o.text) == OptionClass::SafeYes).map(|(i, _)| i).collect();
    match safe.as_slice() {
        [one] => Ok(*one),
        [] => Err("none of the options plainly means yes, trust this folder or allow once".into()),
        _ => Err("more than one option could be the safe one".into()),
    }
}

// ---- the answer plan ---------------------------------------------------------------------------------

/// A choice a draft's `[[tui.prompt]]` already knows (`allow_once`): the keys it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub keys: Vec<AnswerKey>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Plan {
    Hint(Vec<AnswerKey>),
    Arrow { target: String },
    Number { number: u32, target: String },
    YesNo,
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
    label: String,
    flavour: Flavour,
    first_screen: String,
}

pub struct Answerer {
    cfg: AnswerCfg,
    place: &'static str,
    cur: Option<Occ>,
}

pub struct Input<'a> {
    pub screen: &'a str,
    /// What the capture classifier calls the screen (a dialog's flavour), `None` when it is not a dialog.
    pub flavour: Option<Flavour>,
    pub stable_ms: u64,
    pub now: u64,
    pub hint: Option<&'a Hint>,
}

/// The question's identity: its rows with the cursor marker and digits folded, so moving the cursor is
/// still the same question.
pub fn fingerprint(screen: &str) -> String {
    let all = rows(screen);
    let from = all.len().saturating_sub(16);
    let marks = ">\u{276F}\u{203A}\u{25B6}\u{2192}\u{25B8}\u{27A4}\u{25CF}\u{25C9}\u{25CB}\u{25EF}";
    let folded: Vec<String> = all[from..].iter().map(|r| r.trim_start_matches(|c: char| c.is_whitespace() || marks.contains(c)).to_string()).collect();
    frame_key(&folded.join("\n")).join("\n")
}

impl Answerer {
    pub fn new(place: &'static str, cfg: AnswerCfg) -> Self {
        Self { cfg, place, cur: None }
    }

    /// Forget the question being answered (a retry of the step).
    pub fn reset(&mut self) {
        self.cur = None;
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
                label: String::new(),
                flavour,
                first_screen: input.screen.to_string(),
            };
            match decide(input.screen, input.hint) {
                Ok((plan, label)) => {
                    occ.plan = Some(plan);
                    occ.label = label;
                }
                Err(reason) => occ.declined = Some(reason),
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
        }
    }

    fn drive(&mut self, input: &Input<'_>) -> Step {
        let cfg = self.cfg.clone();
        let occ = self.cur.as_mut().expect("an occurrence");
        if let Some(reason) = &occ.declined {
            return Step::Declined(reason.clone());
        }
        let now = input.now;
        if !occ.sent.is_empty() && now.saturating_sub(occ.first_key_ms) >= cfg.persist_ms {
            let reason = "the question is still on the screen 5 seconds after the answer".to_string();
            occ.declined = Some(reason.clone());
            return Step::Declined(reason);
        }
        if !occ.sent.is_empty() && now.saturating_sub(occ.last_key_ms) < cfg.key_gap_ms {
            return Step::Waiting("Answering the question.".into());
        }
        let plan = occ.plan.clone().expect("a plan");
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
            Plan::Number { number, target } => {
                let menu = parse_menu(input.screen).filter(|m| m.kind == MenuKind::Numbered);
                let Some(menu) = menu.filter(|m| m.options.iter().any(|o| o.number == Some(number) && o.text == target)) else {
                    return decline(occ, "the menu changed while it was being answered");
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
            Plan::Arrow { target } => {
                let Some(menu) = parse_menu(input.screen).filter(|m| m.kind == MenuKind::Arrow) else {
                    return decline(occ, "the menu changed while it was being answered");
                };
                let picked = safe_pick(&menu).ok().filter(|i| menu.options[*i].text == target);
                let (Some(want), Some(here)) = (picked, menu.highlighted()) else {
                    return decline(occ, "could not confirm which row is highlighted");
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
                    if now.saturating_sub(occ.last_key_ms) > 1_500 {
                        return decline(occ, "the highlight did not move when the arrow key was pressed");
                    }
                    return Step::Waiting("Waiting for the highlight to move.".into());
                }
                occ.last_highlight = Some(here);
                let key = if want > here { AnswerKey::Down } else { AnswerKey::Up };
                press(occ, vec![key], false)
            }
        }
    }
}

/// The plan for this screen, and the label of the option it picks.
fn decide(screen: &str, hint: Option<&Hint>) -> Result<(Plan, String), String> {
    if let Some(word) = off_limits(screen) {
        return Err(format!("the question mentions \"{word}\", and sign-in, payment, model and telemetry questions are only yours to answer"));
    }
    let menu = parse_menu(screen);
    // The draft's own `allow_once` keys are used when the screen has no menu to read (a one-line question).
    // A menu that can be read is walked row by row instead: the highlight is verified, so a default cursor
    // on "No" is never confirmed, and an "always" row is never the one pressed.
    if let Some(hint) = hint {
        if menu.as_ref().is_none_or(|m| m.kind == MenuKind::YesNo) {
            return Ok((Plan::Hint(hint.keys.clone()), hint.label.clone()));
        }
    }
    let Some(menu) = menu else {
        return Err("no menu or yes/no question could be read from the screen".into());
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
                return Err("could not see which row is highlighted".into());
            }
            let o = &menu.options[pick];
            Ok((Plan::Arrow { target: o.text.clone() }, o.text.clone()))
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
