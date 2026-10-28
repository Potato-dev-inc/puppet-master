//! Text for the saved chat page: redacted, home folder and folder paths cut down, HTML-escaped.

use crate::redact::{self, RedactContext};

/// Most lines of tool output, and of each side of a diff, the page shows.
pub const MAX_LINES: usize = 40;

/// Redacted, path-cut and escaped text, ready to sit in the page. Redaction runs first so a secret inside a path is
/// masked before the path is cut.
pub fn clean(text: &str, ctx: &RedactContext) -> String {
    escape(&strip_paths(redact::text_with(text, ctx).as_str(), ctx.home.as_deref()))
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

/// The folder name a page shows for `cwd`: its last name after the home folder became `~`. A cwd that is the home
/// folder itself shows as `~`, never as the user's name.
pub fn folder_of(cwd: &str, home: Option<&str>) -> String {
    last_name(&home_to_tilde(cwd, home))
}

/// Cuts every path in `text` down to its last name. The home folder is first replaced by `~` (see `home_to_tilde`),
/// so `C:\Users\Default User\proj\a.rs` becomes `a.rs` and a bare `~` stays `~`. A path needs at least two names after
/// its root, so `/api` stays as it is. Paths end at whitespace or a quote; sentence punctuation after one is kept.
pub fn strip_paths(text: &str, home: Option<&str>) -> String {
    let text = home_to_tilde(text, home);
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

/// Replaces the user's home folder by `~`: first any `C:\Users\<name>`, `/home/<name>` or `/Users/<name>` (either
/// slash, any case), where the name runs up to the next separator, spaces included; then the OS home folder itself
/// (`home`, any case, either slash) wherever it is written out. The separator after a replaced folder is kept.
pub fn home_to_tilde(text: &str, home: Option<&str>) -> String {
    let home = home.map(home_key).filter(|key| !key.is_empty());
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some(prefix) = user_folder_prefix(&chars, i) {
            let mut end = i + prefix;
            while end < chars.len() && !is_name_end(chars[end]) {
                end += 1;
            }
            if end > i + prefix {
                out.push('~');
                i = end;
                continue;
            }
        }
        if let Some(len) = home.as_deref().and_then(|key| home_match_len(&chars, i, key)) {
            out.push('~');
            i += len;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The OS home folder in comparison form: separators as `\`, lower case, no trailing separator.
fn home_key(home: &str) -> Vec<char> {
    let normalized: Vec<char> = home.chars().map(|c| if is_sep(c) { '\\' } else { lower(c) }).collect();
    let trimmed = normalized.iter().rposition(|c| *c != '\\').map_or(&normalized[..0], |last| &normalized[..=last]);
    trimmed.to_vec()
}

/// Length of the `C:\Users\` / `/home/` / `/Users/` part starting at `i` (up to and including its separator), when
/// one starts there.
fn user_folder_prefix(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && chars[i - 1].is_alphanumeric() {
        return None;
    }
    if chars[i].is_ascii_alphabetic() && chars.get(i + 1) == Some(&':') && chars.get(i + 2).is_some_and(|c| is_sep(*c)) {
        return word_then_sep(chars, i + 3, "users").map(|end| end - i);
    }
    if is_sep(chars[i]) {
        return ["home", "users"].iter().find_map(|word| word_then_sep(chars, i + 1, word)).map(|end| end - i);
    }
    None
}

/// The index after `word` and the separator that follows it, when both are at `at` (any case).
fn word_then_sep(chars: &[char], at: usize, word: &str) -> Option<usize> {
    let len = word.chars().count();
    let matches = chars.get(at..at + len)?.iter().zip(word.chars()).all(|(c, w)| lower(*c) == w);
    (matches && chars.get(at + len).is_some_and(|c| is_sep(*c))).then_some(at + len + 1)
}

/// Length of the OS home folder (in comparison form `key`) written at `i`, when it ends at a non-letter.
fn home_match_len(chars: &[char], i: usize, key: &[char]) -> Option<usize> {
    if i > 0 && chars[i - 1].is_alphanumeric() {
        return None;
    }
    let same = chars.get(i..i + key.len())?.iter().zip(key).all(|(c, k)| (if is_sep(*c) { '\\' } else { lower(*c) }) == *k);
    let ends = chars.get(i + key.len()).map_or(true, |c| !c.is_alphanumeric());
    (same && ends).then_some(key.len())
}

/// The name of a user folder ends at a separator, or at a quote, an angle bracket, a pipe, a wildcard or a line end.
fn is_name_end(c: char) -> bool {
    is_sep(c) || matches!(c, '"' | '\'' | '<' | '>' | '|' | '*' | '?' | '\r' | '\n')
}

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Where the absolute path (or `~` path) starting at `i` ends, when one starts there.
fn path_end(chars: &[char], i: usize) -> Option<usize> {
    let before = if i == 0 { None } else { Some(chars[i - 1]) };
    let drive = chars[i].is_ascii_alphabetic()
        && chars.get(i + 1) == Some(&':')
        && matches!(chars.get(i + 2), Some('\\') | Some('/'))
        && !before.is_some_and(|c| c.is_alphanumeric());
    let boundary = before.map_or(true, |c| c.is_whitespace() || matches!(c, '"' | '\'' | '(' | '=' | '`' | '[' | '<'));
    let rooted = chars[i] == '/' && chars.get(i + 1).is_some_and(|c| *c != '/' && !c.is_whitespace()) && boundary;
    let home = chars[i] == '~' && chars.get(i + 1).is_some_and(|c| is_sep(*c)) && boundary;
    if !drive && !rooted && !home {
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
