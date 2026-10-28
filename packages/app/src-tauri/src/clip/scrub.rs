//! Redacts the replayed screens before anything leaves Rust (spec 3.5). This is the privacy core of
//! Clip It: a key, a home folder or an e-mail address that a terminal printed must not reach a frame.
//!
//! Walls, in order, for every frame:
//! 1. each row on its own is cleaned (`plain`: strict secrets, e-mails, paths cut to last names);
//! 2. rows the terminal wrapped are cleaned as one logical line, then wrapped again at `cols`;
//! 3. a secret that crosses a row break (rows joined with no separator) masks each row it touches.
//!
//! A row that changes loses its colours and is drawn as one plain run. That is a design decision:
//! safe over pretty. The known values of this process (env, credential store) are in the
//! `RedactContext` the caller passes, taken once per plan.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use serde::Serialize;

use super::model::{Frame, Run};
use super::screens::{RawFrame, RawRow};
use crate::redact::{find_spans, Mode, RedactContext, MASK};
use crate::session_chat::export_text::plain;

/// The redacted screens of one replay, ready for the plan.
#[derive(Debug, Clone, Serialize)]
pub struct Scrubbed {
    /// Distinct rows. A frame's `rows` are indexes into this.
    pub row_table: Vec<Vec<Run>>,
    pub frames: Vec<Frame>,
    /// First and last row that is not blank in any frame (inclusive).
    pub row_from: u16,
    pub row_to: u16,
    /// Distinct screen rows that redaction changed.
    pub hidden: u32,
}

/// Redacts `frames`, each `cols` cells wide. Every string in the result has passed `plain`.
pub fn scrub(frames: &[RawFrame], cols: u16, ctx: &RedactContext) -> Scrubbed {
    let mut scrubber = Scrubber::new(cols as usize, ctx);
    let mut out = Vec::with_capacity(frames.len());
    let mut ink: Vec<bool> = Vec::new();
    for frame in frames {
        let rows = scrubber.frame(frame);
        if ink.len() < rows.len() {
            ink.resize(rows.len(), false);
        }
        for (r, &id) in rows.iter().enumerate() {
            ink[r] |= has_ink(&scrubber.table[id as usize]);
        }
        out.push(Frame { at_ms: frame.at_ms, rows, cursor: frame.cursor });
    }
    let (row_from, row_to) = match (ink.iter().position(|&b| b), ink.iter().rposition(|&b| b)) {
        (Some(from), Some(to)) => (from as u16, to as u16),
        _ => (0, 0),
    };
    Scrubbed { row_table: scrubber.table, frames: out, row_from, row_to, hidden: scrubber.hidden.len() as u32 }
}

/// One line of plain text for a caption or a file name: redacted, paths cut to last names, e-mails
/// masked, every line break made a space, then cut to `max_chars`. `plain` runs the strict pass first.
pub fn clean_line(text: &str, max_chars: usize, ctx: &RedactContext) -> String {
    let cleaned = plain(text, ctx);
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max_chars).collect()
}

/// Per-plan state. The caches are why an unchanged row is redacted once, not once per frame.
struct Scrubber<'a> {
    ctx: &'a RedactContext,
    cols: usize,
    /// Cleaned text by raw text (single rows and joined lines share it).
    clean: HashMap<String, String>,
    /// Row index by the row's runs, so identical rows share one entry.
    index: HashMap<Vec<(String, u8)>, u32>,
    table: Vec<Vec<Run>>,
    /// Hashes of the distinct raw rows that redaction changed.
    hidden: HashSet<u64>,
}

/// A row as the screen had it: its text, and the same text as runs of one style.
struct RawText {
    text: String,
    runs: Vec<(String, u8)>,
}

impl<'a> Scrubber<'a> {
    fn new(cols: usize, ctx: &'a RedactContext) -> Self {
        Scrubber { ctx, cols, clean: HashMap::new(), index: HashMap::new(), table: Vec::new(), hidden: HashSet::new() }
    }

    /// The row indexes of one frame (into `table`).
    fn frame(&mut self, frame: &RawFrame) -> Vec<u32> {
        let rows: Vec<RawText> = frame.rows.iter().map(raw_text).collect();
        let n = rows.len();
        let mut plain_rows: Vec<Option<String>> = vec![None; n];
        let mut covered = vec![false; n];

        // Wall 1: each row on its own.
        for (r, row) in rows.iter().enumerate() {
            let cleaned = self.clean_text(&row.text);
            if cleaned != row.text {
                plain_rows[r] = Some(cleaned);
            }
        }

        // Wall 2: a line the terminal wrapped is cleaned whole, so a key cut over two rows is seen.
        for (first, last) in logical_lines(&frame.wrapped, n) {
            let joined: String = rows[first..=last].iter().map(|row| row.text.as_str()).collect();
            let cleaned = self.clean_text(&joined);
            if cleaned == joined {
                continue;
            }
            let chunks = wrap(&cleaned, self.cols);
            for r in first..=last {
                plain_rows[r] = Some(chunks.get(r - first).cloned().unwrap_or_default());
                covered[r] = true;
            }
        }

        // Wall 3: a secret that crosses a row break. Rows that wall 2 already re-wrapped are clean.
        let mut bounds: Vec<(usize, usize)> = Vec::with_capacity(n);
        let mut screen = String::new();
        for row in &rows {
            bounds.push((screen.len(), screen.len() + row.text.len()));
            screen.push_str(&row.text);
        }
        for span in find_spans(&screen, self.ctx, Mode::Strict) {
            let touched: Vec<usize> = (0..n).filter(|&r| span.start < bounds[r].1 && span.end > bounds[r].0).collect();
            if touched.len() < 2 || touched.iter().all(|&r| covered[r]) {
                continue;
            }
            for r in touched {
                plain_rows[r] = Some(MASK.to_string());
            }
        }

        let mut ids = Vec::with_capacity(n);
        for (row, changed) in rows.iter().zip(plain_rows) {
            let runs = match changed {
                Some(text) if text != row.text => {
                    self.hidden.insert(hash_of(&row.text));
                    plain_runs(&text)
                }
                _ => row.runs.iter().map(|(t, s)| Run { t: t.clone(), s: *s }).collect(),
            };
            ids.push(self.intern(runs));
        }
        ids
    }

    /// `plain` of `raw`, cached for the plan.
    fn clean_text(&mut self, raw: &str) -> String {
        if let Some(done) = self.clean.get(raw) {
            return done.clone();
        }
        let done = plain(raw, self.ctx);
        self.clean.insert(raw.to_string(), done.clone());
        done
    }

    /// The table index of `runs`, adding it when it is new.
    fn intern(&mut self, runs: Vec<Run>) -> u32 {
        let key: Vec<(String, u8)> = runs.iter().map(|run| (run.t.clone(), run.s)).collect();
        if let Some(&id) = self.index.get(&key) {
            return id;
        }
        let id = self.table.len() as u32;
        self.table.push(runs);
        self.index.insert(key, id);
        id
    }
}

/// The text of a row (wide-character right halves add nothing) and its runs of one style.
fn raw_text(row: &RawRow) -> RawText {
    let mut text = String::new();
    let mut runs: Vec<(String, u8)> = Vec::new();
    for cell in row {
        if cell.ch.is_empty() {
            continue;
        }
        text.push_str(&cell.ch);
        match runs.last_mut() {
            Some((t, s)) if *s == cell.style => t.push_str(&cell.ch),
            _ => runs.push((cell.ch.clone(), cell.style)),
        }
    }
    RawText { text, runs }
}

/// `(first, last)` row ranges of the logical lines: a row whose `wrapped` flag is set continues on the next.
fn logical_lines(wrapped: &[bool], n: usize) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let mut first = 0;
    for r in 0..n {
        let continues = r + 1 < n && wrapped.get(r) == Some(&true);
        if !continues {
            lines.push((first, r));
            first = r + 1;
        }
    }
    lines
}

/// `text` cut into pieces of `cols` characters (one piece when `cols` is 0).
fn wrap(text: &str, cols: usize) -> Vec<String> {
    if cols == 0 {
        return vec![text.to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(cols).map(|piece| piece.iter().collect()).collect()
}

/// Plain text as one run with no style, or no run at all when empty.
fn plain_runs(text: &str) -> Vec<Run> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Run { t: text.to_string(), s: 0 }]
    }
}

/// True when a row shows something (a character other than a space).
fn has_ink(runs: &[Run]) -> bool {
    runs.iter().any(|run| run.t.chars().any(|c| c != ' '))
}

fn hash_of(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::screens::{RawCell, BOLD, DIM};

    /// A key with the shape of a provider key (`sk-` plus a long tail).
    fn key() -> String {
        format!("sk-ant-api03-{}", "A1".repeat(20))
    }

    /// 64 letters and digits with no token shape: only the long-run rule (strict mode) finds it. The
    /// two halves differ, so the two rows they fill are different rows.
    fn opaque_run() -> String {
        let base = "Zk3Qw9Lp2Xm7Rt4Vb8Nc1Hy6Jd0Fg5Sa";
        let reversed: String = base.chars().rev().collect();
        format!("{base}{reversed}")
    }

    fn cell(ch: char, style: u8) -> RawCell {
        RawCell { ch: ch.to_string(), style }
    }

    fn plain_row(text: &str) -> RawRow {
        text.chars().map(|c| cell(c, 0)).collect()
    }

    fn frame_of(rows: Vec<RawRow>, wrapped: Vec<bool>) -> RawFrame {
        RawFrame { at_ms: 0, rows, cursor: None, wrapped }
    }

    fn no_ctx() -> RedactContext {
        RedactContext::default()
    }

    fn text_at(out: &Scrubbed, frame: usize, row: usize) -> String {
        let id = out.frames[frame].rows[row] as usize;
        out.row_table[id].iter().map(|run| run.t.as_str()).collect()
    }

    fn runs_at(out: &Scrubbed, frame: usize, row: usize) -> Vec<(String, u8)> {
        let id = out.frames[frame].rows[row] as usize;
        out.row_table[id].iter().map(|run| (run.t.clone(), run.s)).collect()
    }

    /// The serialized plan part must not contain any of `secrets`.
    fn assert_absent(out: &Scrubbed, secrets: &[&str]) {
        let json = serde_json::to_string(out).expect("serializes");
        for secret in secrets {
            assert!(!json.contains(secret), "a secret reached the plan: {secret}");
        }
    }

    #[test]
    fn a_key_in_one_row_is_masked() {
        let key = key();
        let rows = vec![plain_row(&format!("token is {key} ok"))];
        let out = scrub(&[frame_of(rows, vec![false])], 80, &no_ctx());
        assert_absent(&out, &[key.as_str()]);
        assert!(text_at(&out, 0, 0).contains(MASK), "{}", text_at(&out, 0, 0));
        assert_eq!(out.hidden, 1);
    }

    #[test]
    fn a_key_with_colour_changes_between_its_letters_is_masked() {
        let key = key();
        let row: RawRow = key.chars().enumerate().map(|(i, c)| cell(c, if i % 2 == 0 { BOLD } else { DIM })).collect();
        let out = scrub(&[frame_of(vec![row], vec![false])], 80, &no_ctx());
        assert_absent(&out, &[key.as_str()]);
        assert_eq!(out.hidden, 1);
    }

    #[test]
    fn a_secret_the_terminal_wrapped_is_masked_on_its_line() {
        let run = opaque_run();
        let rows = vec![plain_row(&run[..32]), plain_row(&run[32..])];
        let out = scrub(&[frame_of(rows, vec![true, false])], 40, &no_ctx());
        assert_absent(&out, &[run.as_str()]);
        assert_eq!(text_at(&out, 0, 0), MASK);
        assert_eq!(text_at(&out, 0, 1), "");
    }

    #[test]
    fn a_key_the_terminal_wrapped_is_masked() {
        let key = key();
        let rows = vec![plain_row(&format!("key {}", &key[..20])), plain_row(&key[20..])];
        let out = scrub(&[frame_of(rows, vec![true, false])], 30, &no_ctx());
        assert_absent(&out, &[key.as_str()]);
    }

    #[test]
    fn a_secret_broken_across_two_hard_rows_is_masked_on_both() {
        let run = opaque_run();
        let rows = vec![plain_row(&run[..32]), plain_row(&run[32..])];
        let out = scrub(&[frame_of(rows, vec![false, false])], 40, &no_ctx());
        assert_absent(&out, &[run.as_str()]);
        assert_eq!(text_at(&out, 0, 0), MASK);
        assert_eq!(text_at(&out, 0, 1), MASK);
        assert_eq!(out.hidden, 2);
    }

    #[test]
    fn a_known_value_is_masked_in_one_row() {
        let ctx = RedactContext { env_values: vec!["hunter2-secret-value".into()], home: None };
        let out = scrub(&[frame_of(vec![plain_row("password hunter2-secret-value here")], vec![false])], 80, &ctx);
        assert_absent(&out, &["hunter2"]);
        assert!(text_at(&out, 0, 0).contains(MASK));
    }

    #[test]
    fn a_known_value_broken_across_two_rows_is_masked() {
        let ctx = RedactContext { env_values: vec!["hunter2-secret-value".into()], home: None };
        let rows = vec![plain_row("login hunter2-secr"), plain_row("et-value ok")];
        let out = scrub(&[frame_of(rows, vec![false, false])], 40, &ctx);
        assert_absent(&out, &["hunter2", "secret"]);
    }

    #[test]
    fn a_windows_user_folder_is_cut_to_the_file_name() {
        let rows = vec![plain_row(r"open C:\Users\you\proj\a.rs now")];
        let out = scrub(&[frame_of(rows, vec![false])], 80, &no_ctx());
        let text = text_at(&out, 0, 0);
        assert!(text.contains("a.rs"), "{text}");
        assert!(!text.contains("Users") && !text.contains("you"), "{text}");
        assert_eq!(out.hidden, 1);
    }

    #[test]
    fn an_e_mail_address_is_masked() {
        let out = scrub(&[frame_of(vec![plain_row("mail maya@example.test now")], vec![false])], 80, &no_ctx());
        let text = text_at(&out, 0, 0);
        assert!(text.contains("<email>"), "{text}");
        assert_absent(&out, &["maya@"]);
        assert_eq!(out.hidden, 1);
    }

    #[test]
    fn a_clean_row_keeps_its_colours() {
        let row: RawRow = "$ ls -la".chars().enumerate().map(|(i, c)| cell(c, if i < 2 { BOLD } else { 0 })).collect();
        let out = scrub(&[frame_of(vec![row], vec![false])], 80, &no_ctx());
        assert_eq!(runs_at(&out, 0, 0), vec![("$ ".to_string(), BOLD), ("ls -la".to_string(), 0)]);
        assert_eq!(out.hidden, 0);
    }

    #[test]
    fn identical_rows_share_one_table_entry() {
        let frame = frame_of(vec![plain_row("build ok"), plain_row("build ok")], vec![false, false]);
        let out = scrub(&[frame.clone(), frame], 80, &no_ctx());
        assert_eq!(out.row_table.len(), 1);
        assert_eq!(out.frames[1].rows, vec![0, 0]);
    }

    #[test]
    fn a_changed_row_is_counted_once_however_many_frames_show_it() {
        let frames = vec![
            frame_of(vec![plain_row("mail maya@example.test")], vec![false]),
            frame_of(vec![plain_row("mail maya@example.test")], vec![false]),
            frame_of(vec![plain_row("mail ann@example.test")], vec![false]),
        ];
        let out = scrub(&frames, 80, &no_ctx());
        assert_eq!(out.hidden, 2);
    }

    #[test]
    fn row_range_skips_blank_margins() {
        let rows = vec![plain_row("   "), plain_row("hi"), plain_row("   ")];
        let out = scrub(&[frame_of(rows, vec![false, false, false])], 80, &no_ctx());
        assert_eq!((out.row_from, out.row_to), (1, 1));
        assert_eq!(out.frames[0].rows.len(), 3);
    }

    #[test]
    fn a_wide_character_adds_its_text_once() {
        let row = vec![cell('中', 0), RawCell { ch: String::new(), style: 0 }, cell('x', 0)];
        let out = scrub(&[frame_of(vec![row], vec![false])], 80, &no_ctx());
        assert_eq!(text_at(&out, 0, 0), "中x");
    }

    #[test]
    fn a_screen_with_no_frames_is_empty() {
        let out = scrub(&[], 80, &no_ctx());
        assert!(out.row_table.is_empty() && out.frames.is_empty());
        assert_eq!((out.row_from, out.row_to, out.hidden), (0, 0, 0));
    }

    #[test]
    fn two_clean_rows_are_not_changed() {
        let out = scrub(&[frame_of(vec![plain_row("hello"), plain_row("world")], vec![false, false])], 80, &no_ctx());
        assert_eq!(text_at(&out, 0, 0), "hello");
        assert_eq!(text_at(&out, 0, 1), "world");
        assert_eq!(out.hidden, 0);
    }

    #[test]
    fn clean_line_masks_flattens_and_cuts() {
        let ctx = RedactContext { env_values: vec!["hunter2-secret-value".into()], home: Some(r"C:\Users\you".into()) };
        let key = key();
        let text = format!(
            "first {key}\nsecond hunter2-secret-value, mail maya@example.test in C:\\Users\\you\\proj\\a.rs"
        );
        let line = clean_line(&text, 140, &ctx);
        assert!(!line.contains('\n'), "{line}");
        for secret in [key.as_str(), "hunter2", "maya", "Users"] {
            assert!(!line.contains(secret), "{line}");
        }
        assert!(line.contains("a.rs"), "{line}");
        assert_eq!(clean_line("abcdefghij", 4, &no_ctx()), "abcd");
    }

    #[test]
    fn wrap_cuts_at_the_width() {
        assert_eq!(wrap("abcdefg", 3), vec!["abc", "def", "g"]);
        assert_eq!(wrap("abc", 0), vec!["abc"]);
    }

    #[test]
    fn logical_lines_follow_the_wrap_flags() {
        assert_eq!(logical_lines(&[true, true, false, false], 4), vec![(0, 2), (3, 3)]);
        assert_eq!(logical_lines(&[true], 1), vec![(0, 0)]);
    }
}
