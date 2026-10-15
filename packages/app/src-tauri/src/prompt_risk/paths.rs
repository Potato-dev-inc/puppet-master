//! Lexical path handling for the risk assessment. Nothing here touches the file system: the prompt
//! shows a command that has not run yet, so the answer to "is this inside the project?" has to come
//! from the text alone.

/// Where a path lands relative to the pane's workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    Inside,
    Outside,
    /// Cannot tell (no workspace known for an absolute path, or an unexpanded variable).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Norm {
    /// `""` for relative, `"/"` for posix absolute, `"c:"` for a drive, `"unc"` for `\host`.
    root: String,
    parts: Vec<String>,
    /// A relative path that climbs above its starting point (`../x`).
    escapes: bool,
    /// Compare case-insensitively (Windows-style path).
    ci: bool,
}

fn home_prefix(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    path == "~"
        || path.starts_with("~/")
        || path.starts_with("~\\")
        || [
            "$home",
            "${home}",
            "$env:home",
            "$env:userprofile",
            "%userprofile%",
            "%homepath%",
            "%homedrive%",
            "$userprofile",
        ]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// A variable that expands to somewhere outside any project (temp dirs, app data, system roots).
fn outside_var(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        "$tmpdir", "$tmp", "$temp", "$env:temp", "$env:tmp", "%temp%", "%tmp%", "%appdata%", "%localappdata%",
        "$env:appdata", "$env:localappdata", "%programfiles%", "%systemroot%", "%windir%", "$env:windir",
        "$env:systemroot", "$env:programfiles", "%programdata%", "$env:programdata",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
}

fn has_variable(path: &str) -> bool {
    path.contains('$') || path.contains('%') || path.contains('`')
}

fn norm(raw: &str) -> Norm {
    let ci = raw.contains('\\') || raw.chars().nth(1) == Some(':');
    let s = raw.replace('\\', "/");
    let (root, rest) = if s.len() >= 2 && s.as_bytes()[1] == b':' && s.as_bytes()[0].is_ascii_alphabetic() {
        (s[..2].to_ascii_lowercase(), &s[2..])
    } else if s.starts_with("//") {
        ("unc".to_string(), &s[2..])
    } else if s.starts_with('/') {
        ("/".to_string(), &s[1..])
    } else {
        (String::new(), &s[..])
    };
    let mut parts: Vec<String> = Vec::new();
    let mut escapes = false;
    for comp in rest.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() && root.is_empty() {
                    escapes = true;
                }
            }
            c => parts.push(if ci { c.to_ascii_lowercase() } else { c.to_string() }),
        }
    }
    Norm { root, parts, escapes, ci }
}

/// Git Bash / MSYS spell `C:\x` as `/c/x`. Rewrite that shape to a drive when the workspace is a drive path.
fn msys_to_drive(path: Norm, workspace: &Norm) -> Norm {
    if path.root == "/" && workspace.root.ends_with(':') && path.parts.first().is_some_and(|p| p.len() == 1) {
        let mut parts = path.parts.clone();
        let drive = parts.remove(0).to_ascii_lowercase();
        parts.iter_mut().for_each(|p| *p = p.to_ascii_lowercase());
        return Norm { root: format!("{drive}:"), parts, escapes: false, ci: true };
    }
    path
}

fn is_inside(path: &Norm, ws: &Norm) -> bool {
    path.root == ws.root && path.parts.len() >= ws.parts.len() && path.parts[..ws.parts.len()] == ws.parts[..]
}

/// Classify `path` against `workspace`; relative paths resolve against `cwd` (else the workspace).
pub fn locate(path: &str, cwd: Option<&str>, workspace: Option<&str>) -> Where {
    let path = path.trim().trim_matches(|c| c == '"' || c == '\'');
    if path.is_empty() {
        return Where::Inside;
    }
    if home_prefix(path) || outside_var(path) {
        return Where::Outside;
    }
    if has_variable(path) {
        return Where::Unknown;
    }
    let n = norm(path);
    let base = cwd.or(workspace);
    let ws = workspace.or(cwd).map(norm);
    if n.root.is_empty() {
        let Some(base) = base else {
            return if n.escapes { Where::Outside } else { Where::Inside };
        };
        let base_n = norm(base);
        // Resolve against the base, then compare with the workspace.
        let mut parts = base_n.parts.clone();
        let mut escaped = false;
        for comp in path.replace('\\', "/").split('/') {
            match comp {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        escaped = true;
                    }
                }
                c => parts.push(if base_n.ci { c.to_ascii_lowercase() } else { c.to_string() }),
            }
        }
        if escaped {
            return Where::Outside;
        }
        let resolved = Norm { root: base_n.root, parts, escapes: false, ci: base_n.ci };
        return match ws {
            Some(ws) if is_inside(&resolved, &ws) => Where::Inside,
            Some(_) => Where::Outside,
            None => Where::Inside,
        };
    }
    let Some(ws) = ws else { return Where::Unknown };
    let n = msys_to_drive(n, &ws);
    if is_inside(&n, &ws) {
        Where::Inside
    } else {
        Where::Outside
    }
}

/// `/`, `C:\`, `~`: a target so broad that deleting it wipes a drive or a home folder.
pub fn is_broad_root(path: &str) -> bool {
    let trimmed = path.trim().trim_matches(|c| c == '"' || c == '\'');
    let bare = trimmed.trim_end_matches(['/', '\\', '*']);
    if bare.is_empty() && !trimmed.is_empty() {
        return true; // "/", "/*", "\"
    }
    if home_prefix(bare) && !bare.contains(['/', '\\']) {
        return true; // "~", "$HOME", "%USERPROFILE%"
    }
    let n = norm(bare);
    !n.root.is_empty() && n.parts.is_empty()
}

/// Does the word look like a path (as opposed to a flag, a pattern or a URL)?
pub fn looks_like_path(word: &str) -> bool {
    if word.starts_with('-') || word.contains("://") || word.starts_with('@') || word.contains('=') && !word.contains(['/', '\\']) {
        return false;
    }
    word.contains('/')
        || word.contains('\\')
        || word.starts_with('~')
        || word.starts_with('.')
        || word.starts_with("$")
        || word.starts_with('%')
        || (word.len() >= 2 && word.as_bytes()[1] == b':' && word.as_bytes()[0].is_ascii_alphabetic())
}
