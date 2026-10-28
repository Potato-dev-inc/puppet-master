//! The user's home folder as `~`: shared by the saved chat page and the search index, so neither keeps the account name.

/// Replaces the user's home folder by `~`: first any `C:\Users\<name>`, `/home/<name>` or `/Users/<name>` (either
/// slash, any case), `/mnt/<letter>/Users/<name>` (WSL's view of a Windows user folder) and
/// `\\wsl.localhost\<distro>\home\<name>` or `\\wsl$\<distro>\home\<name>` (either slash, any case), where the name
/// runs up to the next separator, spaces included; then the OS home folder itself (`home`, any case, either slash)
/// wherever it is written out. The separator after a replaced folder is kept.
pub fn home_to_tilde(text: &str, home: Option<&str>) -> String {
    let home = home.map(home_key).filter(|key| !key.is_empty());
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let prefix = user_folder_prefix(&chars, i).or_else(|| mnt_user_prefix(&chars, i)).or_else(|| wsl_home_prefix(&chars, i));
        if let Some(prefix) = prefix {
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

/// Length of the `/mnt/<letter>/Users/` part starting at `i` (WSL's view of a Windows user folder), when one starts
/// there. Only a single drive letter counts, so `/mnt/cc/Users/x` is left alone.
fn mnt_user_prefix(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && chars[i - 1].is_alphanumeric() {
        return None;
    }
    if !is_sep(chars[i]) {
        return None;
    }
    let letter = word_then_sep(chars, i + 1, "mnt")?;
    if !chars.get(letter)?.is_ascii_alphabetic() || !is_sep(*chars.get(letter + 1)?) {
        return None;
    }
    word_then_sep(chars, letter + 2, "users").map(|end| end - i)
}

/// Length of the `\\wsl.localhost\<distro>\home\` / `\\wsl$\<distro>\home\` part starting at `i` (two separators,
/// either slash, any case), when one starts there. The distro name runs to the next separator.
fn wsl_home_prefix(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && chars[i - 1].is_alphanumeric() {
        return None;
    }
    if !is_sep(chars[i]) || !chars.get(i + 1).is_some_and(|c| is_sep(*c)) {
        return None;
    }
    let distro = ["wsl.localhost", "wsl$"].iter().find_map(|host| word_then_sep(chars, i + 2, host))?;
    let mut sep = distro;
    while sep < chars.len() && !is_sep(chars[sep]) {
        sep += 1;
    }
    if sep == distro || sep >= chars.len() {
        return None;
    }
    word_then_sep(chars, sep + 1, "home").map(|end| end - i)
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

#[cfg(test)]
#[path = "home_path_tests.rs"]
mod tests;
