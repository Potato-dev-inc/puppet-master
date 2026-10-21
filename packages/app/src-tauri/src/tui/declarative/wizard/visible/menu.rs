//! Reading a question's options from the screen (SPEC 11.16, 11.20): the text of the last rows and, when the caller has
//! it, the cell attributes of those rows. Pure: strings and a `StyledScreen` in, a `Menu` out.
//!
//! The shapes it reads (nothing here names an agent):
//!
//! - numbered rows (`1. Yes`, `1) Yes`, `[1] Yes`, `(1) Yes`, `1: Yes`, `1 Yes`), with or without a cursor marker;
//! - arrow menus: rows one under the other at the same column, the cursor drawn as a marker (`>`, `❯`, a bullet) OR as
//!   an inverse / background / bold / colour difference of the highlighted row (no marker at all);
//! - menus drawn inside a box (`│ ❯ Yes │`, `| Yes |`): the border is cut first;
//! - radio rows (`(•) Yes` / `( ) No`) and square bullets;
//! - one row of options (`Allow once   Always allow   Deny`, `[Y]es / [N]o`, `[ Allow ] [ Deny ]`): `MenuKind::Row`;
//! - one-line questions with letters (`[y/n]`, `(y/n/a)`, `y/n`).
//!
//! Which option is highlighted comes from a marker when there is one, else from the attributes
//! (`style_highlight`: the one filled band, else the one with the strongest emphasis). Any doubt is `None`.

use once_cell::sync::Lazy;
use regex::Regex;

use super::screen::rows;
use super::styled::{CellStyle, StyledScreen};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuOption {
    pub text: String,
    pub number: Option<u32>,
    /// The cursor is on this row (a marker, or the way the row is drawn).
    pub highlighted: bool,
    /// The letter that answers it directly (`[Y]es`, `Yes, proceed (y)`).
    pub hotkey: Option<char>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuKind {
    /// Rows moved with Up and Down, Enter confirms.
    Arrow,
    /// Rows with numbers: the number selects (Enter may be needed).
    Numbered,
    /// A one-line `[y/n]` question.
    YesNo,
    /// One row of options (buttons): moved with Left / Right or Tab, Enter confirms; a letter may answer directly.
    Row,
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

/// The longest numbered option row that is still read as an option.
const MAX_NUMBERED_CHARS: usize = 200;

const MARKERS: &str = "\u{276F}\u{203A}\u{25B6}\u{2192}\u{25B8}\u{27A4}\u{25CF}\u{25C9}\u{25CB}\u{25EF}\u{00BB}\u{25A0}\u{25A1}\u{25C6}\u{25C7}\u{25AA}";

static ROW: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?P<indent>\s*)(?:(?P<mark>[>\u{276F}\u{203A}\u{25B6}\u{2192}\u{25B8}\u{27A4}\u{25CF}\u{25C9}\u{25CB}\u{25EF}\u{00BB}\u{25A0}\u{25A1}\u{25C6}\u{25C7}\u{25AA}])\s+)?(?:(?P<num>\d{1,2})(?:[.):]\s+|\s+))?(?P<text>\S.*?)\s*$",
    )
    .expect("menu row regex")
});

static HINT_ROW: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)navigate|\benter\b.*(?:confirm|select)|\besc\b|\u{2191}|\u{2193}|arrow keys|tab to").expect("hint row regex"));

/// `[y/n]`, `(y/n)`, `[Y/n/a]`, `(yes/no)`, a bare `y/n`.
static YESNO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)[\[(]\s*y(?:es)?\s*/\s*n(?:o)?(?:\s*/\s*\w+)*\s*[\])]|\by/n\b").expect("yes/no regex"));

/// A row made only of box-drawing, rule or border characters.
static BORDER_ROW: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\s\u{2500}-\u{257F}\u{2580}-\u{259F}+\-=_|*~.]*$").expect("border row regex"));

/// `(•) Yes` / `[x] Yes` (chosen) and `( ) No` / `[ ] No` (not chosen), at the start of a row.
static RADIO: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?P<ind>\s*)(?:\((?P<a>[\u{2022}*]| )\)|\[(?P<b>[xX*]| )\])(?P<rest>\s+\S.*)$").expect("radio regex"));

/// `[1] Yes` / `(1) Yes`: a number in brackets.
static BRACKET_NUM: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?P<pre>\s*(?:[>\u{276F}\u{203A}\u{25B6}\u{2192}\u{25B8}\u{27A4}\u{25CF}\u{25C9}\u{25CB}\u{00BB}]\s+)?)[\[(](?P<n>\d{1,2})[\])]\s*(?P<rest>\S.*)$")
        .expect("bracket number regex")
});

fn is_vbar(c: char) -> bool {
    matches!(c, '\u{2502}' | '\u{2503}' | '\u{2551}' | '|')
}

/// One row with a box border cut (`│ ❯ Yes │` becomes `❯ Yes`), a border-only row emptied, and radio / bracketed-number
/// rows rewritten as the plain marker / `N.` forms the parser reads.
pub fn normalise_row(row: &str) -> String {
    let t = row.trim_end();
    if t.trim().is_empty() || BORDER_ROW.is_match(t) {
        return String::new();
    }
    let mut chars: Vec<char> = t.chars().collect();
    let lead = chars.iter().take_while(|c| c.is_whitespace()).count();
    if chars.get(lead).is_some_and(|c| is_vbar(*c)) {
        let mut cut = lead + 1;
        if chars.get(cut) == Some(&' ') {
            cut += 1;
        }
        chars.drain(..cut);
    }
    if let Some(end) = chars.iter().rposition(|c| !c.is_whitespace()) {
        if end > 0 && is_vbar(chars[end]) {
            chars.truncate(end);
        }
    }
    let mut out: String = chars.into_iter().collect::<String>().trim_end().to_string();
    if let Some(c) = RADIO.captures(&out) {
        let chosen = c.name("a").is_some_and(|m| m.as_str() != " ") || c.name("b").is_some_and(|m| m.as_str() != " ");
        out = format!("{}{}{}", &c["ind"], if chosen { " \u{25CF}" } else { " \u{25CB}" }, &c["rest"]);
    }
    if let Some(c) = BRACKET_NUM.captures(&out) {
        out = format!("{}{}. {}", &c["pre"], &c["n"], &c["rest"]);
    }
    out
}

/// A marker that is a cursor (`>`, a chevron, an arrow), not a bullet (`●`, `○`, `■`...).
fn is_cursor_marker(c: char) -> bool {
    matches!(c, '>' | '\u{276F}' | '\u{203A}' | '\u{25B6}' | '\u{2192}' | '\u{25B8}' | '\u{27A4}' | '\u{00BB}')
}

/// Rows (not hints) that may follow a menu drawn with bullets only.
const MAX_ROWS_AFTER_BULLETS: usize = 3;

fn is_selected_marker(c: char) -> bool {
    !matches!(c, '\u{25CB}' | '\u{25EF}' | '\u{25A1}' | '\u{25C7}')
}

#[derive(Clone)]
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

// ---- attributes ----------------------------------------------------------------------------------------

fn accent(style: &CellStyle) -> i32 {
    use vt100::Color;
    match style.fg {
        Color::Default => 0,
        // red is a deny tint, not a selection colour; grey is the unselected look
        Color::Idx(1) | Color::Idx(9) => 0,
        Color::Idx(7) | Color::Idx(8) => -1,
        Color::Idx(_) => 1,
        Color::Rgb(r, g, b) => {
            let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
            if hi.saturating_sub(lo) < 24 {
                -1
            } else if r > g.saturating_add(60) && r > b.saturating_add(60) {
                0
            } else {
                1
            }
        }
    }
}

fn emphasis(style: &CellStyle) -> i32 {
    let mut score = accent(style);
    if style.bold {
        score += 2;
    }
    if style.underline {
        score += 1;
    }
    if style.dim {
        score -= 2;
    }
    score
}

/// Which of these options (their text drawn in these styles, in menu order) is the highlighted one, from how they are
/// drawn alone: the one option on a filled band (inverse video or a background colour), else the one emphasised more
/// than all the others (bold, an accent colour). Anything ambiguous is `None`.
pub fn style_highlight(styles: &[CellStyle]) -> Option<usize> {
    let n = styles.len();
    if n < 2 {
        return None;
    }
    let bands: Vec<usize> = (0..n).filter(|i| styles[*i].is_band()).collect();
    if bands.len() == 1 {
        return Some(bands[0]);
    }
    if bands.len() > 1 && bands.len() < n {
        return None;
    }
    if bands.len() == n {
        let key = |s: &CellStyle| (s.inverse, s.bg);
        if n == 2 {
            return match (styles[0].inverse, styles[1].inverse) {
                (true, false) => Some(0),
                (false, true) => Some(1),
                _ => None,
            };
        }
        let odd: Vec<usize> = (0..n)
            .filter(|i| {
                let others: Vec<_> = (0..n).filter(|j| j != i).map(|j| key(&styles[j])).collect();
                others.iter().all(|k| *k == others[0]) && others[0] != key(&styles[*i])
            })
            .collect();
        return if odd.len() == 1 { Some(odd[0]) } else { None };
    }
    let scores: Vec<i32> = styles.iter().map(emphasis).collect();
    let max = *scores.iter().max()?;
    let at: Vec<usize> = (0..n).filter(|i| scores[*i] == max).collect();
    let second = scores.iter().filter(|s| **s < max).max().copied().unwrap_or(i32::MIN);
    if max >= 1 && at.len() == 1 && (second == i32::MIN || max > second) {
        Some(at[0])
    } else {
        None
    }
}

/// The attributes of the row of the screen whose normalised text is `row_text`, at the option text `needle`.
fn style_of_text(styled: &StyledScreen, row_text: &str, needle: &str) -> Option<CellStyle> {
    let row = styled.rows.iter().rev().find(|r| normalise_row(&r.text) == row_text)?;
    let idx = row.text.find(needle).map(|b| row.text[..b].chars().count());
    Some(match idx {
        Some(from) => row.style_of(from, from + needle.chars().count()),
        None => row.style_of(0, row.text.chars().count()),
    })
}

/// A row that reads like an option or its refusal, not a title.
fn looks_like_option(text: &str) -> bool {
    classify_option(text) != OptionClass::Other || is_deny(text)
}

/// Fill in `highlighted` from the attributes when no option is marked by the text. `rows_text` is each option's
/// normalised row. Only an option that reads like an option may be the highlighted one.
fn highlight_from_styles(options: &mut [MenuOption], rows_text: &[String], styled: Option<&StyledScreen>) {
    if options.iter().any(|o| o.highlighted) {
        return;
    }
    let Some(styled) = styled else { return };
    let styles: Vec<Option<CellStyle>> = options.iter().zip(rows_text).map(|(o, row)| style_of_text(styled, row, &o.text)).collect();
    if styles.iter().any(Option::is_none) {
        return;
    }
    let styles: Vec<CellStyle> = styles.into_iter().flatten().collect();
    if let Some(i) = style_highlight(&styles) {
        if looks_like_option(&options[i].text) {
            options[i].highlighted = true;
        }
    }
}

// ---- the parser ----------------------------------------------------------------------------------------

/// The menu in the last rows of `screen`, if there is a plain one (the text only; one row of buttons is not read).
pub fn parse_menu(screen: &str) -> Option<Menu> {
    parse_menu_ex(screen, None).filter(|m| m.kind != MenuKind::Row)
}

/// The menu in the last rows of `screen`, with the cell attributes of the screen when the caller has them.
pub fn parse_menu_ex(screen: &str, styled: Option<&StyledScreen>) -> Option<Menu> {
    let lines: Vec<String> = screen.lines().map(normalise_row).collect();
    let nonblank: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let tail: Vec<&String> = nonblank[nonblank.len().saturating_sub(16)..].to_vec();
    let parsed: Vec<Option<Parsed>> = tail.iter().map(|r| parse_row(r)).collect();
    let short = |p: &Parsed| p.text.chars().count() <= 80 && !HINT_ROW.is_match(&p.text);

    // numbered: consecutive rows numbered 1, 2, 3... The LAST such run is the menu (an earlier numbered list in the
    // agent's reply is history). An option may be long ("Yes, and always allow ... for commands that start with ..."), so a
    // numbered row may be longer than a row of an arrow menu.
    let long_ok = |p: &Parsed| p.text.chars().count() <= MAX_NUMBERED_CHARS && !HINT_ROW.is_match(&p.text);
    let numbered: Vec<usize> = (0..tail.len()).filter(|i| parsed[*i].as_ref().is_some_and(|p| p.num.is_some() && long_ok(p))).collect();
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for i in numbered {
        let number = parsed[i].as_ref().and_then(|p| p.num);
        let continues = runs.last().and_then(|r| r.last()).is_some_and(|last| {
            *last + 1 == i && parsed[*last].as_ref().and_then(|p| p.num).zip(number).is_some_and(|(a, b)| b == a + 1)
        });
        match runs.last_mut() {
            Some(run) if continues => run.push(i),
            _ => runs.push(vec![i]),
        }
    }
    if let Some(run) = runs.iter().rev().find(|r| r.len() >= 2 && parsed[r[0]].as_ref().and_then(|p| p.num) == Some(1)) {
        let mut options: Vec<MenuOption> = run
            .iter()
            .map(|i| {
                let p = parsed[*i].as_ref().expect("parsed");
                MenuOption { text: p.text.clone(), number: p.num, highlighted: p.mark.is_some_and(is_selected_marker), hotkey: None }
            })
            .collect();
        // more than one cursor is not a single-selection menu
        if options.iter().filter(|o| o.highlighted).count() <= 1 {
            let rows_text: Vec<String> = run.iter().map(|i| tail[*i].clone()).collect();
            highlight_from_styles(&mut options, &rows_text, styled);
            return Some(Menu { kind: MenuKind::Numbered, options });
        }
    }
    // arrow: a marker row and the rows around it that start at the same column, ROW AFTER ROW (a blank row between two
    // rows ends the group: the options of a menu are not spaced out, the echo of an earlier task, the bulleted summary
    // row an agent prints after a turn and a quoted line are). The LAST such group is the menu.
    let end = lines.iter().rposition(|r| !r.trim().is_empty()).map_or(0, |i| i + 1);
    let raw: Vec<&String> = lines[end.saturating_sub(24)..end].iter().collect();
    let raw_parsed: Vec<Option<Parsed>> = raw.iter().map(|r| parse_row(r)).collect();
    let group_at = |anchor: usize| -> Option<(usize, usize)> {
        let col = raw_parsed[anchor].as_ref()?.text_col;
        let same = |i: usize| raw_parsed[i].as_ref().is_some_and(|p| p.text_col == col && short(p) && p.num.is_none());
        let mut lo = anchor;
        while lo > 0 && same(lo - 1) {
            lo -= 1;
        }
        let mut hi = anchor;
        while hi + 1 < raw.len() && same(hi + 1) {
            hi += 1;
        }
        (hi > lo).then_some((lo, hi))
    };
    let arrow = |lo: usize, hi: usize| -> Option<Menu> {
        // a title row ("Run this command?") glued to the first option is not an option
        let mut lo = lo;
        while lo < hi && raw_parsed[lo].as_ref().is_some_and(|p| p.mark.is_none() && (p.text.ends_with('?') || p.text.ends_with(':'))) {
            lo += 1;
        }
        let mut options: Vec<MenuOption> = (lo..=hi)
            .map(|i| {
                let p = raw_parsed[i].as_ref().expect("parsed");
                MenuOption { text: p.text.clone(), number: None, highlighted: p.mark.is_some_and(is_selected_marker), hotkey: None }
            })
            .collect();
        // a block where two or more rows carry the marker is a list of status bullets (the tool calls an agent printed
        // for an earlier turn), not a menu with one cursor
        if options.iter().filter(|o| o.highlighted).count() > 1 {
            return None;
        }
        let rows_text: Vec<String> = (lo..=hi).map(|i| raw[i].clone()).collect();
        highlight_from_styles(&mut options, &rows_text, styled);
        Some(Menu { kind: MenuKind::Arrow, options })
    };
    // The LAST block with a cursor marker (`>`, a chevron, an arrow) is the menu. A block drawn only with bullets counts
    // when it sits at the bottom: only hints and a footer may follow it (bullets higher up are the transcript).
    let anchors = (0..raw.len()).rev().filter(|i| raw_parsed[*i].as_ref().is_some_and(|p| p.mark.is_some_and(is_selected_marker) && short(p) && p.num.is_none()));
    let mut bullets: Option<Menu> = None;
    for anchor in anchors {
        let Some((lo, hi)) = group_at(anchor) else { continue };
        let Some(menu) = arrow(lo, hi) else { continue };
        let has_cursor = (lo..=hi).any(|i| raw_parsed[i].as_ref().and_then(|p| p.mark).is_some_and(is_cursor_marker));
        if has_cursor {
            return Some(menu);
        }
        let after = raw[hi + 1..].iter().filter(|r| !r.trim().is_empty() && !HINT_ROW.is_match(r)).count();
        if bullets.is_none() && after <= MAX_ROWS_AFTER_BULLETS {
            bullets = Some(menu);
        }
    }
    if let Some(menu) = bullets {
        return Some(menu);
    }
    // no marker anywhere: the highlighted row is drawn as a band (inverse video, a background) or in a stronger style
    if let Some(styled) = styled {
        let mut at = raw.len();
        while at > 0 {
            at -= 1;
            let Some(p) = raw_parsed[at].as_ref().filter(|p| short(p) && p.num.is_none()) else { continue };
            if !looks_like_option(&p.text) {
                continue;
            }
            let Some(style) = style_of_text(styled, raw[at], &p.text) else { continue };
            let Some((lo, hi)) = group_at(at) else { continue };
            // the band row itself, or any group that tells its highlighted row apart by attributes
            if style.is_band() || style.bold || emphasis(&style) >= 1 {
                if let Some(menu) = arrow(lo, hi) {
                    if menu.highlighted().is_some() && menu.options.iter().any(|o| looks_like_option(&o.text)) {
                        return Some(menu);
                    }
                }
            }
        }
    }
    // one row of options: buttons, `[Y]es / [N]o`
    for i in (0..raw.len()).rev().take(8) {
        if let Some(menu) = row_menu(raw[i], styled) {
            return Some(menu);
        }
    }
    let joined = tail.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
    if YESNO.is_match(&joined) {
        return Some(Menu { kind: MenuKind::YesNo, options: Vec::new() });
    }
    None
}

// ---- one row of options ------------------------------------------------------------------------------

static SEPARATOR: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s{2,}|\t|\s[/|\u{2022}\u{00B7}\u{2502}]\s").expect("separator regex"));
static HOTKEY_LEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\[(](?P<k>[A-Za-z])[\])](?P<rest>[A-Za-z].*)$").expect("hotkey lead regex"));
static HOTKEY_TAIL: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?P<text>.+?)\s*[\[(](?P<k>[A-Za-z])[\])]$").expect("hotkey tail regex"));
static DENY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(?:no\b|n\b|deny\b|reject\b|decline\b|cancel\b|don.?t allow\b|skip\b|refuse\b|exit\b|quit\b|abort\b|never\b|esc\b)").expect("deny regex"));

/// A refusal: No, Deny, Reject, Cancel...
pub fn is_deny(text: &str) -> bool {
    DENY.is_match(text.trim())
}

struct Seg {
    text: String,
    hotkey: Option<char>,
    /// Drawn with `> text <`, `[ text ]` or `< text >` while the others are not.
    wrapped: bool,
}

fn segments(row: &str) -> Vec<Seg> {
    let t = row.trim();
    let mut out = Vec::new();
    let mut last = 0usize;
    let mut pieces: Vec<&str> = Vec::new();
    for m in SEPARATOR.find_iter(t) {
        pieces.push(&t[last..m.start()]);
        last = m.end();
    }
    pieces.push(&t[last..]);
    for piece in pieces {
        let p = piece.trim();
        if p.is_empty() {
            continue;
        }
        let mut text = p.to_string();
        let mut wrapped = false;
        for (open, close) in [("[ ", " ]"), ("< ", " >"), ("> ", " <"), ("( ", " )")] {
            if text.starts_with(open) && text.ends_with(close) && text.chars().count() > 4 {
                text = text[open.len()..text.len() - close.len()].trim().to_string();
                wrapped = true;
            }
        }
        let mut hotkey = None;
        if let Some(c) = HOTKEY_LEAD.captures(&text) {
            let k = c["k"].chars().next().expect("a letter");
            hotkey = Some(k.to_ascii_lowercase());
            text = format!("{}{}", &c["k"], &c["rest"]);
        } else if let Some(c) = HOTKEY_TAIL.captures(&text) {
            hotkey = c["k"].chars().next().map(|k| k.to_ascii_lowercase());
            text = c["text"].to_string();
        }
        out.push(Seg { text, hotkey, wrapped });
    }
    out
}

/// A row of two or more short options, every one of which reads as a yes, an affirmative with a catch ("Always allow")
/// or a refusal, with at least one refusal and at least one affirmative.
fn row_menu(row: &str, styled: Option<&StyledScreen>) -> Option<Menu> {
    if row.trim().is_empty() || row.chars().count() > 140 {
        return None;
    }
    // the question may share the row with its answers (`Allow it? [Y]es / [N]o`): only what follows its last "? " is the options
    let row = row.rfind("? ").map_or(row, |i| &row[i + 2..]);
    let segs = segments(row);
    if segs.len() < 2 || segs.len() > 6 || segs.iter().any(|s| s.text.chars().count() > 40 || !s.text.chars().any(|c| c.is_alphabetic())) {
        return None;
    }
    let affirmative = |t: &str| classify_option(t) != OptionClass::Other;
    if !segs.iter().all(|s| affirmative(&s.text) || is_deny(&s.text)) {
        return None;
    }
    if !segs.iter().any(|s| is_deny(&s.text)) || !segs.iter().any(|s| affirmative(&s.text) && !is_deny(&s.text)) {
        return None;
    }
    let mut options: Vec<MenuOption> = segs.iter().map(|s| MenuOption { text: s.text.clone(), number: None, highlighted: false, hotkey: s.hotkey }).collect();
    let wrapped: Vec<usize> = (0..segs.len()).filter(|i| segs[*i].wrapped).collect();
    if wrapped.len() == 1 {
        options[wrapped[0]].highlighted = true;
    }
    if !options.iter().any(|o| o.highlighted) {
        if let Some(styled) = styled {
            let styles: Vec<Option<CellStyle>> = options.iter().map(|o| style_of_text(styled, row, &o.text)).collect();
            if styles.iter().all(Option::is_some) {
                let styles: Vec<CellStyle> = styles.into_iter().flatten().collect();
                if let Some(i) = style_highlight(&styles) {
                    options[i].highlighted = true;
                }
            }
        }
    }
    Some(Menu { kind: MenuKind::Row, options })
}

// ---- what is safe -----------------------------------------------------------------------------------

/// A one-time affirmative action: yes, allow (once), run (the command), proceed, continue, approve, accept, OK, confirm,
/// go ahead, "Yes, run command", "Yes, proceed". Anything broad is caught first by `UNSAFE`.
static SAFE_YES: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^(?:yes\b|y\b|ok(?:ay)?\b|allow(?: once| this time| just this once)?\b|approve(?: once)?\b|accept(?: once)?\b|proceed\b|continue\b|confirm\b|go ahead\b|run\b|execute\b|trust (?:this|the) (?:folder|project|workspace|directory)\b|just this once\b)",
    )
    .expect("safe yes regex")
});

/// Anything broader than this one action: always, don't ask again, the session, all, every time, remember, trust all,
/// for this project, permanently... An option with one of these is never picked, whatever else it says.
static UNSAFE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)always|don.?t ask|do not ask|never ask|ask again|for this session|this session|\ball\b|all tools|all commands|all (?:projects|folders|files)|trust all|yolo|bypass|dangerous|auto-?approve|skip (?:all )?permission|remember|every ?time|everything|permanently|forever|global|parent (?:folder|directory)|home (?:folder|directory)|full access|unrestricted|for (?:this|the|all|every) (?:project|repo\w*|workspace|folder|directory|chat|conversation|session)|from now on|\bfuture\b|allow[- ]?list|white[- ]?list|persist|wildcard|without|anyway|\bignore\b|\bany\b|\bevery\b",
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

/// Why `safe_pick` found nothing. The menu was READ and no option is a plain yes. (The checked AI look may still name one
/// the parser missed: `ai_menu::validate` holds it to the same rule.)
pub const NO_PLAIN_YES: &str = "none of the options plainly means yes, trust this folder or allow once";

/// What the answerer thinks of one option, in words: `plain yes`, `broad`, `no` or `other`.
pub fn option_word(text: &str) -> &'static str {
    if is_deny(text) {
        "no"
    } else {
        match classify_option(text) {
            OptionClass::SafeYes => "plain yes",
            OptionClass::Risky => "broad",
            OptionClass::Other => "other",
        }
    }
}

/// The menu as the parser read it, for the decision log: `rows: "Allow once" (plain yes, highlighted); "Deny" (no)`.
pub fn describe_menu(menu: &Menu) -> String {
    let kind = match menu.kind {
        MenuKind::Arrow => "rows",
        MenuKind::Numbered => "numbered rows",
        MenuKind::Row => "one row of buttons",
        MenuKind::YesNo => return "a yes/no question".to_string(),
    };
    let options: Vec<String> = menu
        .options
        .iter()
        .map(|o| {
            let number = o.number.map(|n| format!("{n}. ")).unwrap_or_default();
            format!("\"{number}{}\" ({}{})", o.text, option_word(&o.text), if o.highlighted { ", highlighted" } else { "" })
        })
        .collect();
    format!("{kind}: {}", options.join("; "))
}

/// The one option to choose, or why there is none. Exactly one plain "yes" or nothing.
pub fn safe_pick(menu: &Menu) -> Result<usize, String> {
    if menu.kind == MenuKind::YesNo {
        return Ok(0);
    }
    let safe: Vec<usize> = menu.options.iter().enumerate().filter(|(_, o)| classify_option(&o.text) == OptionClass::SafeYes).map(|(i, _)| i).collect();
    match safe.as_slice() {
        [one] => Ok(*one),
        [] => Err(NO_PLAIN_YES.into()),
        _ => Err("more than one option could be the safe one".into()),
    }
}

/// The marker characters (for the question's identity: moving the cursor is still the same question).
pub fn marker_chars() -> String {
    format!(">{MARKERS}")
}

// ---- every option the screen shows, loosely --------------------------------------------------------------

/// One option as the screen words it, for checking a name somebody else gave (the AI look): the text of every row
/// of the last rows (marker, number and box cut), and of every segment of a row of buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    pub number: Option<u32>,
    pub hotkey: Option<char>,
}

/// A screen-level look at where the cursor is, for an answer (the AI look's Enter) that no read menu can vouch for. Finds
/// the block of rows (no blank row inside it) that holds the named option (`is_target` says whether a row's text is that
/// option). When the named row itself carries a cursor, or no cursor can be seen in the block, it is `None` (nothing
/// against pressing Enter). When ANOTHER option-looking row of the same block carries the cursor (a marker such as `>` or
/// `❯`, else the one row drawn highlighted), it is that row's text.
pub fn cursor_elsewhere(screen: &str, styled: Option<&StyledScreen>, is_target: &dyn Fn(&str) -> bool) -> Option<String> {
    let lines: Vec<String> = screen.lines().map(normalise_row).collect();
    let from = lines.len().saturating_sub(40);
    let lines = &lines[from..];
    let mut start = 0;
    while start < lines.len() {
        if lines[start].trim().is_empty() {
            start += 1;
            continue;
        }
        let mut end = start;
        while end < lines.len() && !lines[end].trim().is_empty() {
            end += 1;
        }
        let block: Vec<(&String, Parsed)> = lines[start..end].iter().filter_map(|l| parse_row(l).map(|p| (l, p))).filter(|(_, p)| !HINT_ROW.is_match(&p.text)).collect();
        start = end;
        let Some(named) = block.iter().position(|(_, p)| is_target(&p.text)) else { continue };
        let option_like = |p: &Parsed| p.num.is_some() || looks_like_option(&p.text);
        let cursor_rows: Vec<usize> = (0..block.len()).filter(|i| block[*i].1.mark.is_some_and(is_cursor_marker) && (*i == named || option_like(&block[*i].1))).collect();
        if cursor_rows.contains(&named) {
            continue;
        }
        if let Some(other) = cursor_rows.first() {
            return Some(block[*other].1.text.clone());
        }
        // no marker: the one row drawn highlighted, when the attributes are known
        let Some(styled) = styled else { continue };
        let candidates: Vec<usize> = (0..block.len()).filter(|i| *i == named || option_like(&block[*i].1)).collect();
        if candidates.len() < 2 {
            continue;
        }
        let styles: Vec<Option<CellStyle>> = candidates.iter().map(|i| style_of_text(styled, block[*i].0, &block[*i].1.text)).collect();
        if styles.iter().any(Option::is_none) {
            continue;
        }
        let styles: Vec<CellStyle> = styles.into_iter().flatten().collect();
        if let Some(at) = style_highlight(&styles) {
            let row = candidates[at];
            if row != named {
                return Some(block[row].1.text.clone());
            }
        }
    }
    None
}

pub fn shown_options(screen: &str) -> Vec<Shown> {
    let lines: Vec<String> = screen.lines().map(normalise_row).filter(|l| !l.trim().is_empty()).collect();
    let tail = &lines[lines.len().saturating_sub(16)..];
    let mut out = Vec::new();
    for line in tail {
        if let Some(p) = parse_row(line) {
            if p.text.chars().count() <= 100 {
                out.push(Shown { text: p.text.clone(), number: p.num, hotkey: None });
            }
        }
        let segs = segments(line);
        if segs.len() >= 2 {
            for s in segs {
                out.push(Shown { text: s.text, number: None, hotkey: s.hotkey });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boxed_row_loses_its_border() {
        assert_eq!(normalise_row("\u{2502} \u{276F} Yes, allow once        \u{2502}"), "\u{276F} Yes, allow once");
        assert_eq!(normalise_row("\u{256D}\u{2500}\u{2500}\u{2500}\u{256E}"), "");
        assert_eq!(normalise_row("| > Type a task...   |"), "> Type a task...");
    }

    #[test]
    fn radio_and_bracketed_numbers_are_read() {
        assert_eq!(normalise_row("(\u{2022}) Yes"), " \u{25CF} Yes");
        assert_eq!(normalise_row("( ) No"), " \u{25CB} No");
        assert_eq!(normalise_row("[2] No, exit"), "2. No, exit");
    }

    #[test]
    fn a_band_wins_over_everything() {
        let band = CellStyle { inverse: true, ..CellStyle::default() };
        let plain = CellStyle::default();
        assert_eq!(style_highlight(&[plain, band, plain]), Some(1));
        assert_eq!(style_highlight(&[plain, plain, plain]), None);
        let bold = CellStyle { bold: true, ..CellStyle::default() };
        assert_eq!(style_highlight(&[plain, bold]), Some(1));
        assert_eq!(style_highlight(&[bold, bold, plain]), None);
    }
}
