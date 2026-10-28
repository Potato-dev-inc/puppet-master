//! The user's home folder as `~`: shared by the saved chat page and the search index, so neither keeps the account name.

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

pub(super) fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

pub(super) fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}
