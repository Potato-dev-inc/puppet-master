//! What counts as "the program's own error" (SPEC 4.4 and 11.9): one place for the live test and the
//! visible test.
//!
//! An agent-side error is a stop: no new draft is asked for. So it needs real evidence, never "the first
//! line the program printed" (on an inline TUI that is the banner: a block-character logo, the name and
//! the version). The evidence is a row that says something went wrong with the program's own setup
//! (sign-in, quota, provider, network, an HTTP 4xx/5xx ...) and that is NOT chrome: not a logo or rule
//! made of block, braille or box-drawing characters, not a lone `<Name> CLI 1.4.2` or `v1.4.2` banner row,
//! not the input box, not a footer or key-hint row, not a row the profile's own chrome tables name, not
//! a status row that says all is well (`Signed in as ...`), not an echo of the test's own task.
//!
//! No agent names here: everything is generic shapes plus the profile's tables.

use once_cell::sync::Lazy;
use regex::Regex;

use super::exec;
use super::livetest::EXTERNAL;
use crate::tui::TuiProfile;

/// The row that says what went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The row, framed text stripped, at most 300 characters. Not masked.
    pub message: String,
    /// Its text points at sign-in, quota, provider or network: no draft can fix it.
    pub external: bool,
}

/// Rows that look like a problem but say all is well, or that are about the working state, not a fault.
static HEALTHY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:signed|logged)[ -]?in (?:as|with|via|to|using|successfully)\b|\bauthenticated (?:as|with|via)\b|\blogin successful\b|\bwelcome back\b|(?:using|connected to) (?:the )?(?:\w+ )?provider|\bno errors?\b",
    )
    .expect("healthy row regex")
});

/// A key-hint or footer row: `ctrl+c to stop`, `esc to interrupt`, `? for shortcuts`, `shift+tab to cycle`.
static FOOTER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:ctrl|alt|shift|cmd|control)\s*\+\s*\w|\besc(?:ape)?\b.{0,24}\b(?:to|for)\b|\?\s*for shortcuts|\btab to (?:cycle|complete|accept)\b|\bpress \S+ again to (?:exit|quit)\b")
        .expect("footer row regex")
});

/// A lone version banner: `<Name> CLI 1.4.2`, `Some Agent v2.0.1 (build 3)`, `v1.4.2`, `Name 1.4.2-beta`. At most
/// four words in front of the version, nothing after it but a closing parenthesis or a full stop.
static BANNER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(?:[a-z][\w.+-]*\s+){0,4}(?:cli\s+|code\s+|agent\s+|tui\s+)?v?\d+\.\d+(?:\.\d+)?(?:[-+][\w.]+)?\s*(?:\([^)]*\))?\.?$")
        .expect("banner row regex")
});

/// A program that was started with a wrong argument says so: the profile's `argv` is at fault, not the program.
static USAGE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bunexpected argument\b|\bunknown (?:option|flag|argument|command|arg)\b|\bunrecognized (?:option|flag|argument|arguments|command)\b|\binvalid (?:option|flag|argument)\b|^usage:|\brequired arguments? (?:were|was) not provided\b|\bmissing required (?:argument|option|flag)\b")
        .expect("usage row regex")
});

/// A row that starts with an input glyph (`> text`, `› text`, `❯ text`, `$ cmd`): the input box, or the echo of what was typed.
static INPUT_ROW: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[>\u{203a}\u{276f}\u{25b8}$]\s").expect("input row regex"));

fn is_frame_char(c: char) -> bool {
    matches!(c,
        '\u{2500}'..='\u{257f}'   // box drawing
        | '\u{2580}'..='\u{259f}' // block elements
        | '\u{25a0}'..='\u{25ff}' // geometric shapes
        | '\u{2800}'..='\u{28ff}' // braille
        | '|' | '+' | '=' | '_' | '~' | '#'
    )
}

/// The row without its frame: a leading and trailing run of border, block or braille characters and spaces removed.
pub fn unframed(row: &str) -> String {
    let t = row.trim_matches(|c: char| c.is_whitespace() || is_frame_char(c) || c == '-');
    t.to_string()
}

/// A row made mostly of logo, rule or border characters (a banner logo, a box edge, a spinner cell).
pub fn is_logo_row(row: &str) -> bool {
    let cells: Vec<char> = row.chars().filter(|c| !c.is_whitespace()).collect();
    if cells.is_empty() {
        return true;
    }
    let frame = cells.iter().filter(|c| is_frame_char(**c) || **c == '-').count();
    frame * 2 >= cells.len()
}

/// A lone `<Name> CLI 1.4.2` or `v1.4.2` banner row (the logo glyphs in front of it do not matter).
pub fn is_banner_row(row: &str) -> bool {
    let core = unframed(row);
    if crate::loop_guard::default_error_line(&core) {
        return false;
    }
    BANNER.is_match(&core)
}

/// A row that is never the text of an error: blank, a logo or rule, a banner, the input box, a key-hint footer.
pub fn is_chrome_row(row: &str) -> bool {
    let core = unframed(row);
    if core.is_empty() || is_logo_row(row) || core.chars().filter(|c| c.is_alphabetic()).count() < 3 {
        return true;
    }
    if is_banner_row(row) || INPUT_ROW.is_match(&core) {
        return true;
    }
    FOOTER.is_match(&core)
}

fn profile_chrome(profile: Option<&dyn TuiProfile>, core: &str) -> bool {
    let Some(p) = profile else { return false };
    let lower = core.to_lowercase();
    p.is_chrome_line(&lower) || p.is_thinking_line(&lower) || p.is_summary_line(&lower) || p.is_tool_activity_line(&lower)
}

/// The rows of `text` that may hold an error: everything chrome is dropped, and so is a row that quotes one of
/// `skip` (the test's own task and random words).
fn candidate_rows<'a>(text: &'a str, profile: Option<&'a dyn TuiProfile>, skip: &'a [String]) -> impl Iterator<Item = String> + 'a {
    text.lines().filter_map(move |raw| {
        let row = raw.trim();
        if row.is_empty() || row.starts_with('{') || row.starts_with('[') || is_chrome_row(row) {
            return None;
        }
        let core = unframed(row);
        if profile_chrome(profile, &core) || HEALTHY.is_match(&core) {
            return None;
        }
        let lower = core.to_lowercase();
        if skip.iter().any(|s| !s.is_empty() && lower.contains(&s.to_lowercase())) {
            return None;
        }
        Some(core)
    })
}

/// The program's own error in `text` (a screen, or what a program printed), or `None` when there is no real
/// evidence. A row that points at sign-in, quota, provider or network, an HTTP 4xx/5xx and the like is
/// `external`. With `plain`, a row that merely reads like an error (`Error: ...`, `cannot ...`) counts too, but a
/// usage error (a wrong argument: the profile's) never does.
pub fn find(text: &str, profile: Option<&dyn TuiProfile>, skip: &[String], plain: bool) -> Option<Found> {
    let rows: Vec<String> = candidate_rows(text, profile, skip).collect();
    if let Some(row) = rows.iter().find(|r| EXTERNAL.is_match(r) && !USAGE.is_match(r)) {
        return Some(Found { message: exec::cut_chars(row, 300), external: true });
    }
    if !plain {
        return None;
    }
    rows.iter()
        .find(|r| !USAGE.is_match(r) && (crate::loop_guard::default_error_line(r) || profile.is_some_and(|p| p.is_error_line(r))))
        .map(|row| Found { message: exec::cut_chars(row, 300), external: false })
}

/// The first row of an error text the program itself gave (its own error event) that says something: not a
/// logo, a rule or a banner.
pub fn first_text_row(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|r| !r.is_empty() && !is_chrome_row(r)).map(|r| exec::cut_chars(&unframed(r), 300))
}

/// True when `message` can be shown to the person as "the program returned an error": it holds words and is
/// not a logo, a rule or a banner row.
pub fn is_real_message(message: &str) -> bool {
    let m = message.trim();
    !m.is_empty() && !is_logo_row(m) && !is_banner_row(m) && m.chars().filter(|c| c.is_alphabetic()).count() >= 3
}
