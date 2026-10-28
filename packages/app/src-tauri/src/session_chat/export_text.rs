//! Text for the saved chat page: redacted, home folder and folder paths cut down, HTML-escaped.

use super::home_path::{home_to_tilde, is_sep};
use crate::redact::{self, RedactContext};

/// Most lines of tool output, and of each side of a diff, the page shows.
pub const MAX_LINES: usize = 40;

/// Redacted (strict: the page may be sent on), path-cut and escaped text, ready to sit in the page. Redaction runs
/// first so a secret inside a path is masked before the path is cut.
pub fn clean(text: &str, ctx: &RedactContext) -> String {
    escape(&cut_paths(text, ctx))
}

/// The first `max` lines of `text`, cleaned, and how many lines were left out. The whole text is redacted before it
/// is cut into lines: a secret that spans lines (a private key body) is only found as a whole.
pub fn lines(text: &str, ctx: &RedactContext, max: usize) -> (Vec<String>, usize) {
    let cleaned = cut_paths(text, ctx);
    let all: Vec<&str> = cleaned.lines().collect();
    let shown = all.iter().take(max).map(|line| escape(line)).collect();
    (shown, all.len().saturating_sub(max))
}

/// Plain text for a file that is not HTML: redacted in strict mode, then with its paths cut down to their last names.
/// Not HTML-escaped, so the caller must not put it into markup without escaping.
pub fn plain(text: &str, ctx: &RedactContext) -> String {
    cut_paths(text, ctx)
}

/// `text` redacted in strict mode, then with its paths cut down.
fn cut_paths(text: &str, ctx: &RedactContext) -> String {
    strip_paths(redact::strict_with(text, ctx).as_str(), ctx.home.as_deref())
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
