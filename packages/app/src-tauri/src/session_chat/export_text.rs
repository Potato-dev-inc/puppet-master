//! Text for the saved chat page: redacted, folder paths cut to the folder name, HTML-escaped.

use crate::redact::{self, RedactContext};

/// Most lines of tool output, and of each side of a diff, the page shows.
pub const MAX_LINES: usize = 40;

/// Redacted, path-cut and escaped text, ready to sit in the page. Redaction runs first so a secret inside a path is
/// masked before the path is cut.
pub fn clean(text: &str, ctx: &RedactContext) -> String {
    escape(&strip_paths(redact::text_with(text, ctx).as_str()))
}

/// The first `max` lines of `text`, cleaned, and how many lines were left out.
pub fn lines(text: &str, ctx: &RedactContext, max: usize) -> (Vec<String>, usize) {
    let all: Vec<&str> = text.lines().collect();
    let shown = all.iter().take(max).map(|line| clean(line, ctx)).collect();
    (shown, all.len().saturating_sub(max))
}

/// Escapes the characters that would let text act as markup.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// The last name of a path (`C:\Users\you\work\app` gives `app`); empty when there is none.
pub fn last_name(path: &str) -> String {
    segments(path).last().map(|name| name.to_string()).unwrap_or_default()
}

/// Replaces every absolute path (`C:\...\x`, `/home/.../x`) in `text` by its last name. A path needs at least two
/// names, so `/api` stays as it is. Paths end at whitespace or a quote; sentence punctuation after one is kept.
pub fn strip_paths(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        match path_end(&chars, i) {
            Some(end) => {
                let token: String = chars[i..end].iter().collect();
                out.push_str(&last_name(&token));
                i = end;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

/// Where the absolute path starting at `i` ends, when one starts there.
fn path_end(chars: &[char], i: usize) -> Option<usize> {
    let before = if i == 0 { None } else { Some(chars[i - 1]) };
    let drive = chars[i].is_ascii_alphabetic()
        && chars.get(i + 1) == Some(&':')
        && matches!(chars.get(i + 2), Some('\\') | Some('/'))
        && !before.is_some_and(|c| c.is_alphanumeric());
    let boundary = before.map_or(true, |c| c.is_whitespace() || matches!(c, '"' | '\'' | '(' | '=' | '`' | '[' | '<'));
    let rooted = chars[i] == '/' && chars.get(i + 1).is_some_and(|c| *c != '/' && !c.is_whitespace()) && boundary;
    if !drive && !rooted {
        return None;
    }
    let mut end = i;
    while end < chars.len() && !is_path_char_end(chars[end]) {
        end += 1;
    }
    while end > i && matches!(chars[end - 1], '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
        end -= 1;
    }
    let token: String = chars[i..end].iter().collect();
    (segments(&token).len() >= 2).then_some(end)
}

fn is_path_char_end(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '|' | '*' | '?' | '`')
}

fn segments(path: &str) -> Vec<&str> {
    path.split(['\\', '/']).filter(|part| !part.is_empty()).collect()
}
