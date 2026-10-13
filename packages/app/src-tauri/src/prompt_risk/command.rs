//! Splitting a command line into segments and words for bash, PowerShell and cmd. POSIX words go
//! through the `shlex` crate (MIT OR Apache-2.0); Windows-style lines keep their backslashes, which
//! `shlex` would read as escapes and destroy (`C:\Users` -> `C:Users`).

/// What follows a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sep {
    End,
    /// `|`: the next segment reads this one's output.
    Pipe,
    /// `;`, `&&`, `||`, `&` or a newline.
    Seq,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub sep: Sep,
}

const MAX_CHARS: usize = 4000;
const MAX_SEGMENTS: usize = 40;

/// Split on unquoted `;`, `&&`, `||`, `|`, `&` and newlines. A here-document keeps only the line
/// that starts it (the body is data, not commands).
pub fn split_segments(command: &str) -> Vec<Segment> {
    let mut text: String = command.chars().take(MAX_CHARS).collect();
    if let Some(at) = text.find("<<") {
        if let Some(nl) = text[at..].find('\n') {
            text.truncate(at + nl);
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut i = 0;
    let push = |cur: &mut String, sep: Sep, out: &mut Vec<Segment>| {
        let t = cur.trim();
        if !t.is_empty() && out.len() < MAX_SEGMENTS {
            out.push(Segment { text: t.to_string(), sep });
        }
        cur.clear();
    };
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            cur.push(c);
            if c == '\\' && q == '"' && i + 1 < chars.len() && chars[i + 1] == '"' {
                cur.push('"');
                i += 1;
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            '\n' | ';' => push(&mut cur, Sep::Seq, &mut out),
            '|' => {
                if chars.get(i + 1) == Some(&'|') {
                    i += 1;
                    push(&mut cur, Sep::Seq, &mut out);
                } else {
                    push(&mut cur, Sep::Pipe, &mut out);
                }
            }
            '&' => {
                let redirect = chars.get(i + 1) == Some(&'>')
                    || (i > 0 && matches!(chars[i - 1], '>' | '<'))
                    || chars.get(i + 1).is_some_and(|n| n.is_ascii_digit()) && i > 0 && chars[i - 1] == '>';
                if redirect {
                    cur.push(c);
                } else {
                    if chars.get(i + 1) == Some(&'&') {
                        i += 1;
                    }
                    push(&mut cur, Sep::Seq, &mut out);
                }
            }
            _ => cur.push(c),
        }
        i += 1;
    }
    push(&mut cur, Sep::End, &mut out);
    // The last segment ends the line even when it was closed by a separator.
    if let Some(last) = out.last_mut() {
        last.sep = Sep::End;
    }
    out
}

/// Words that mark a segment as PowerShell or cmd rather than a POSIX shell.
const WINDOWS_WORDS: &[&str] = &[
    "del", "erase", "rmdir", "rd", "copy", "xcopy", "robocopy", "move", "ren", "rename", "type", "dir", "cls", "attrib",
    "reg", "taskkill", "runas", "setx", "powershell", "pwsh", "cmd", "iwr", "irm", "iex", "gci", "gc", "ri", "sc", "ac",
    "ni", "sls", "start-process", "call", "where", "findstr", "schtasks", "netsh", "sc.exe", "icacls", "takeown",
];

pub fn windows_style(segment: &str) -> bool {
    let first = segment.split_whitespace().next().unwrap_or_default();
    let lower = first.trim_matches(|c| c == '&' || c == '.' || c == '\\').to_ascii_lowercase();
    let lower = lower.trim_end_matches(".exe").to_string();
    if WINDOWS_WORDS.contains(&lower.as_str()) || first.starts_with('$') || first.starts_with('&') || first == "." {
        return true;
    }
    // Verb-Noun cmdlet: Remove-Item, Get-Content, Invoke-WebRequest.
    let mut parts = lower.split('-');
    if let (Some(verb), Some(noun), None) = (parts.next(), parts.next(), parts.next()) {
        if matches!(
            verb,
            "get" | "set" | "remove" | "new" | "invoke" | "copy" | "move" | "rename" | "start" | "stop" | "out" | "add"
                | "clear" | "select" | "test" | "install" | "import" | "export" | "write" | "read" | "restart" | "disable"
                | "enable" | "register" | "unregister" | "expand" | "compress"
        ) && !noun.is_empty()
        {
            return true;
        }
    }
    // A drive path or a backslash path anywhere on the line.
    let bytes = segment.as_bytes();
    segment.contains(":\\")
        || segment.contains("\\\\")
        || bytes.windows(3).any(|w| w[0].is_ascii_alphanumeric() && w[1] == b'\\' && w[2].is_ascii_alphanumeric())
}

/// Quote-aware split on whitespace that keeps backslashes (PowerShell, cmd).
fn windows_words(segment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in segment.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            None => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Words of one segment.
pub fn words(segment: &str, windows: bool) -> Vec<String> {
    if windows {
        return windows_words(segment);
    }
    shlex::split(segment).unwrap_or_else(|| windows_words(segment))
}

/// Redirection targets (`> file`, `>> file`, `2> file`, `>file`) taken out of the word list.
/// Targets that discard output (`/dev/null`, `nul`, `$null`, `&1`) are dropped.
pub fn take_redirects(words: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut kept = Vec::new();
    let mut targets = Vec::new();
    let mut iter = words.into_iter().peekable();
    while let Some(w) = iter.next() {
        let stripped = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        if let Some(rest) = stripped.strip_prefix(">>").or_else(|| stripped.strip_prefix('>')) {
            let target = if rest.is_empty() { iter.next().unwrap_or_default() } else { rest.to_string() };
            let lower = target.to_ascii_lowercase();
            if !target.is_empty() && !matches!(lower.as_str(), "/dev/null" | "nul" | "$null" | "&1" | "&2") && !target.starts_with('&') {
                targets.push(target);
            }
        } else if w == "<" {
            iter.next();
        } else {
            kept.push(w);
        }
    }
    (kept, targets)
}
