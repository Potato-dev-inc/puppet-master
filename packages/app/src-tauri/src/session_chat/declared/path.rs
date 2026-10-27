//! Turns a `[session_chat] dir` template into a folder, and only a folder the person's home or the pane's own
//! folder holds. Nothing outside those two is ever listed or read, whatever a definition says.

use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::table::{env_name, ROOT_PLACEHOLDERS};
use crate::session_chat::plain_path;

/// What a template can name.
pub struct Names<'a> {
    pub home: Option<&'a Path>,
    /// The folder the pane was started in.
    pub cwd: &'a Path,
    /// Reads an environment variable.
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

/// The pane's folder as the agent saw it: no `\\?\` prefix, no trailing separator.
pub fn clean_cwd(cwd: &Path) -> String {
    let text = plain_path(&cwd.to_string_lossy());
    let trimmed = text.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() { text } else { trimmed.to_string() }
}

/// Every character that is not an ASCII letter or digit becomes `-` (`C:\Users\you\proj` is `C--Users-you-proj`).
pub fn slug(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Only the separators and the drive colon become `-` (`/home/you/proj` is `-home-you-proj`).
pub fn dashes(cwd: &str) -> String {
    cwd.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect()
}

/// Percent-encoded: everything but letters, digits and `-_.~` becomes `%XX` per byte.
pub fn url_encoded(cwd: &str) -> String {
    let mut out = String::with_capacity(cwd.len() * 2);
    for byte in cwd.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The last folder's name.
pub fn folder_name(cwd: &str) -> String {
    cwd.rsplit(['/', '\\']).find(|part| !part.is_empty()).unwrap_or("").to_string()
}

/// Lower-case hex SHA-256 of the folder path.
pub fn sha256_hex(cwd: &str) -> String {
    Sha256::digest(cwd.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The segment placeholders and what each one is for `cwd`, the likeliest first. A folder name in a transcript path that
/// equals one of these can be written with the placeholder (`{cwd_slug}`), so the table holds for any project folder.
pub fn cwd_placeholders(cwd: &Path) -> Vec<(&'static str, String)> {
    let text = clean_cwd(cwd);
    vec![
        ("cwd_sha256", sha256_hex(&text)),
        ("cwd_slug", slug(&text)),
        ("cwd_dashes", dashes(&text)),
        ("cwd_url", url_encoded(&text)),
        ("cwd_name", folder_name(&text)),
    ]
}

/// `template` with its placeholders filled in. `None` when one cannot be (no home folder, an unset variable) or
/// when a segment placeholder came out as something that is not one segment.
pub fn expand(template: &str, names: &Names<'_>) -> Option<String> {
    let cwd = clean_cwd(names.cwd);
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('}')?;
        let name = &after[..close];
        let value = match name {
            "home" => plain_path(&names.home?.to_string_lossy()),
            "cwd" => cwd.clone(),
            "cwd_slug" => slug(&cwd),
            "cwd_dashes" => dashes(&cwd),
            "cwd_url" => url_encoded(&cwd),
            "cwd_name" => folder_name(&cwd),
            "cwd_sha256" => sha256_hex(&cwd),
            other => {
                let variable = env_name(other)?;
                (names.env)(variable).filter(|value| !value.trim().is_empty())?
            }
        };
        let is_segment = !ROOT_PLACEHOLDERS.contains(&name) && env_name(name).is_none();
        if is_segment && (value.is_empty() || value.contains(['/', '\\']) || value == "..") {
            return None;
        }
        out.push_str(&value);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The folders a transcript folder may be under: the home folder and the pane's folder, each as it really is on
/// disk. A root that does not exist is left out.
pub fn allowed_roots(names: &Names<'_>) -> Vec<PathBuf> {
    [names.home, Some(names.cwd)]
        .into_iter()
        .flatten()
        .filter_map(|root| std::fs::canonicalize(plain_path(&root.to_string_lossy())).ok())
        .collect()
}

/// `dir` as a real folder under one of `roots`, or `None`: the folder is missing, has a `..` in it, or (once
/// symlinks are followed) is somewhere else. The answer is the real path, so every file listed below it is too.
pub fn checked_dir(dir: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let path = PathBuf::from(dir);
    if path.components().any(|part| matches!(part, Component::ParentDir)) {
        return None;
    }
    let real = std::fs::canonicalize(&path).ok()?;
    (real.is_dir() && under_any(&real, roots)).then_some(real)
}

/// `path` as a real file under one of `roots`, or `None` (a symlink that leads outside gives `None`).
pub fn checked_file(path: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let real = std::fs::canonicalize(path).ok()?;
    (real.is_file() && under_any(&real, roots)).then_some(real)
}

fn under_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

/// Whether `name` matches `glob`, where `*` is any run of characters and `?` any one; letter case is ignored.
pub fn glob_matches(glob: &str, name: &str) -> bool {
    let glob: Vec<char> = glob.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let (mut g, mut n) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while n < name.len() {
        if g < glob.len() && (glob[g] == '?' || glob[g] == name[n]) {
            g += 1;
            n += 1;
        } else if g < glob.len() && glob[g] == '*' {
            backtrack = Some((g, n));
            g += 1;
        } else if let Some((star, matched)) = backtrack {
            g = star + 1;
            n = matched + 1;
            backtrack = Some((star, matched + 1));
        } else {
            return false;
        }
    }
    glob[g..].iter().all(|c| *c == '*')
}
