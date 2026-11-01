// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Hotkey settings, their checks, and the pure matcher the hook thread runs.
//!
//! DevHop took any text, dropped what it could not parse, and let two actions share a chord. Here a
//! change is checked as a whole and refused with a reason per field, so the old keys stay in force.

use std::sync::Arc;

use arc_swap::ArcSwap;
use desk_core::{chord_matches, digit_hid, parse_chord, Chord};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeskAction {
    Next,
    Prev,
    /// 1-based screen number.
    Screen(u8),
    Find,
    Pause,
}

impl DeskAction {
    /// The wire name, e.g. `next`, `screen:3`.
    pub fn name(self) -> String {
        match self {
            DeskAction::Next => "next".into(),
            DeskAction::Prev => "prev".into(),
            DeskAction::Screen(n) => format!("screen:{n}"),
            DeskAction::Find => "find".into(),
            DeskAction::Pause => "pause".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub chord: Chord,
    pub action: DeskAction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub next: String,
    pub prev: String,
    /// Must contain `{n}`; it becomes 1..9.
    pub screen_n: String,
    pub find_pointer: String,
    pub pause: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            next: "ctrl+alt+right".into(),
            prev: "ctrl+alt+left".into(),
            screen_n: "ctrl+alt+{n}".into(),
            find_pointer: "ctrl+alt+f".into(),
            pause: "scrolllock".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// Which setting: `next`, `prev`, `screen_n`, `find_pointer`, `pause`.
    pub field: &'static str,
    pub message: String,
}

/// Every action with its setting name, in a fixed order.
fn entries(cfg: &HotkeyConfig) -> Vec<(&'static str, DeskAction, String)> {
    let mut out = vec![
        ("next", DeskAction::Next, cfg.next.clone()),
        ("prev", DeskAction::Prev, cfg.prev.clone()),
        ("find_pointer", DeskAction::Find, cfg.find_pointer.clone()),
        ("pause", DeskAction::Pause, cfg.pause.clone()),
    ];
    for n in 1..=9u8 {
        out.push(("screen_n", DeskAction::Screen(n), cfg.screen_n.replace("{n}", &n.to_string())));
    }
    out
}

/// The key text for each binding as the page shows it, lower case.
pub fn key_texts(cfg: &HotkeyConfig) -> Vec<(DeskAction, String)> {
    entries(cfg).into_iter().map(|(_, a, t)| (a, t.trim().to_ascii_lowercase())).collect()
}

fn is_function_key(hid: u16) -> bool {
    (0x3A..=0x45).contains(&hid)
}

const HID_SCROLL_LOCK: u16 = 0x47;
const HID_F4: u16 = 0x3D;

/// Why a chord is not allowed, if it is not. A chord that eats ordinary typing or a system shortcut is
/// refused: with the hook installed it would break the person's other programs.
fn chord_problem(chord: &Chord) -> Option<&'static str> {
    if chord.alt && !chord.ctrl && !chord.shift && !chord.gui && chord.hid == HID_F4 {
        return Some("Alt+F4 closes windows, pick another");
    }
    let has_modifier = chord.ctrl || chord.alt || chord.shift || chord.gui;
    if chord.hid == HID_SCROLL_LOCK {
        return None;
    }
    if !has_modifier {
        return Some("Add Ctrl, Alt or Win so it does not catch normal typing");
    }
    if is_function_key(chord.hid) {
        return None;
    }
    if !chord.alt && !chord.gui {
        return Some("Ctrl+key and Shift+key are used by other programs, add Alt or Win");
    }
    None
}

fn note(problems: &mut Vec<Problem>, field: &'static str, message: String) {
    if !problems.iter().any(|p| p.field == field) {
        problems.push(Problem { field, message });
    }
}

/// Check the whole set. On success the bindings the hook will use.
pub fn compile(cfg: &HotkeyConfig) -> Result<Vec<Binding>, Vec<Problem>> {
    let mut problems: Vec<Problem> = Vec::new();
    let mut out: Vec<Binding> = Vec::new();
    let has_n = cfg.screen_n.contains("{n}");
    if !has_n {
        note(&mut problems, "screen_n", "Use {n} where the screen number goes, like ctrl+alt+{n}".into());
    }
    for (field, action, text) in entries(cfg) {
        if field == "screen_n" && !has_n {
            continue;
        }
        let Some(mut chord) = parse_chord(&text) else {
            note(&mut problems, field, format!("\"{}\" is not a key combination", text.trim()));
            continue;
        };
        if let DeskAction::Screen(n) = action {
            if let Some(hid) = digit_hid(n) {
                chord.hid = hid;
            }
        }
        if let Some(why) = chord_problem(&chord) {
            note(&mut problems, field, why.into());
            continue;
        }
        if let Some(other) = out.iter().find(|b| b.chord == chord) {
            note(&mut problems, field, format!("Same keys as \"{}\"", other.action.name()));
            continue;
        }
        out.push(Binding { chord, action });
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

pub fn problems_text(problems: &[Problem]) -> String {
    problems.iter().map(|p| format!("{}: {}", p.field, p.message)).collect::<Vec<_>>().join("; ")
}

// ---- the matcher ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Let the key through to the system.
    Pass,
    /// Eat the key.
    Swallow,
    /// Eat the key and run this action once.
    Fire(DeskAction),
}

/// The table the hook reads. It is swapped as a whole, so the hook never takes a lock for it.
pub type Table = Arc<ArcSwap<Vec<Binding>>>;

pub fn new_table() -> Table {
    Arc::new(ArcSwap::from_pointee(Vec::new()))
}

/// Matches key events against the table. Pure: its state is only the modifiers it has seen go down and the
/// keys it swallowed. DevHop asked `GetAsyncKeyState` for the modifiers inside the hook, which can
/// disagree with the event being decided; this tracks them from the hook's own events.
pub struct HotkeyMatcher {
    table: Table,
    /// Left and right of each: ctrl 0/4, shift 1/5, alt 2/6, gui 3/7 (HID 0xE0..=0xE7).
    held_mods: [bool; 8],
    swallowed: Vec<u16>,
}

impl HotkeyMatcher {
    pub fn new(table: Table) -> Self {
        Self { table, held_mods: [false; 8], swallowed: Vec::new() }
    }

    /// Start from the modifiers actually held when the hook starts (the host reads them once).
    pub fn seed_modifier(&mut self, hid: u16, down: bool) {
        if (0xE0..=0xE7).contains(&hid) {
            self.held_mods[(hid - 0xE0) as usize] = down;
        }
    }

    pub fn reset(&mut self) {
        self.held_mods = [false; 8];
        self.swallowed.clear();
    }

    pub fn on_key(&mut self, hid: u16, down: bool, injected: bool) -> Verdict {
        // Keys sent by programs (including this one) are never hotkeys and never eaten.
        if injected {
            return Verdict::Pass;
        }
        if (0xE0..=0xE7).contains(&hid) {
            self.held_mods[(hid - 0xE0) as usize] = down;
            return Verdict::Pass;
        }
        if !down {
            // DevHop swallowed the press and let the release through, leaving a stray key-up.
            if let Some(i) = self.swallowed.iter().position(|k| *k == hid) {
                self.swallowed.remove(i);
                return Verdict::Swallow;
            }
            return Verdict::Pass;
        }
        if self.swallowed.contains(&hid) {
            // Key repeat of a held hotkey: eaten, but it does not hop again.
            return Verdict::Swallow;
        }
        let m = &self.held_mods;
        let (ctrl, shift, alt, gui) = (m[0] || m[4], m[1] || m[5], m[2] || m[6], m[3] || m[7]);
        let table = self.table.load();
        if let Some(b) = table.iter().find(|b| chord_matches(&b.chord, ctrl, alt, shift, gui, hid)) {
            self.swallowed.push(hid);
            return Verdict::Fire(b.action);
        }
        Verdict::Pass
    }
}
