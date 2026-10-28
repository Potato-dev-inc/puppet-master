//! The search text in and out: what the person typed becomes an FTS5 query, and the snippet FTS5 marks becomes
//! plain text plus the ranges to highlight.

/// Most words one query uses.
const MAX_TERMS: usize = 12;
/// FTS5 puts these around the matched words in a snippet (control characters never occur in an entry's text).
pub const MARK_START: char = '\u{1}';
pub const MARK_END: char = '\u{2}';

/// The FTS5 query for typed text: every word must occur (in any order), each as a prefix, so the list narrows while
/// the person types. Words are quoted, so punctuation and FTS5 operators typed by the person are only text.
/// `None` when nothing searchable was typed.
pub fn match_expr(typed: &str) -> Option<String> {
    let terms: Vec<String> = typed
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .take(MAX_TERMS)
        .map(|word| format!("\"{}\"*", word.replace('"', "")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// A snippet marked by FTS5 as text on one line and the ranges to highlight, as `[start, end)` in UTF-16 code
/// units (what a JavaScript string counts in).
pub fn marks_of(marked: &str) -> (String, Vec<[u32; 2]>) {
    let mut text = String::with_capacity(marked.len());
    let mut marks = Vec::new();
    let mut at = 0u32;
    let mut start = None;
    for c in marked.chars() {
        match c {
            MARK_START => start = Some(at),
            MARK_END => {
                if let Some(from) = start.take() {
                    if at > from {
                        marks.push([from, at]);
                    }
                }
            }
            _ => {
                // One line: a line break is one character, replaced by one space.
                text.push(if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c });
                at += c.len_utf16() as u32;
            }
        }
    }
    (text, marks)
}
