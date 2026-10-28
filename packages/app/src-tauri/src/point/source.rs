//! Source hints from the page: resolve them to a real file inside a pane folder (PF-3).
//!
//! The page can say where a component comes from (a dev-server URL, a Vite `/@fs/` path, a webpack id,
//! a `file:` URL). We name a file only when it really exists inside one of the given folders.
//! `..` is refused, and symlinks that leave the folder are refused (we compare real paths).

use super::{SourceHint, SourceHow, SourceRef};
use std::path::{Path, PathBuf};

/// Longest hint we look at. Page text is untrusted, so anything longer is dropped.
const MAX_HINT: usize = 1024;

/// A hint split into an optional Windows drive (`C:`) and its clean path parts.
struct Parsed {
    drive: Option<String>,
    parts: Vec<String>,
}

/// Resolve a page hint to a file that really exists inside one of `roots`.
/// Returns the root the file was found under (as given) and the file relative to it.
/// A `Stack` hint keeps the file only (no line), because a stack line is not a source line.
pub fn resolve(hint: &SourceHint, roots: &[&Path]) -> Option<(PathBuf, SourceRef)> {
    let parsed = parse(&hint.file)?;
    let line = match hint.how {
        SourceHow::Stack => None,
        _ => hint.line.filter(|line| *line > 0),
    };
    roots.iter().find_map(|root| {
        let rel_path = locate(root, &parsed)?;
        Some((root.to_path_buf(), SourceRef { rel_path, line }))
    })
}

/// Turn the page's text into a drive (optional) and clean parts. None when it is not a plain path.
fn parse(raw: &str) -> Option<Parsed> {
    let text = raw.trim();
    if text.is_empty() || text.len() > MAX_HINT || text.contains('\0') {
        return None;
    }
    // A query (`?t=123`) and a fragment are never part of the file name.
    let text = text.split(['?', '#']).next().unwrap_or_default();
    let mut webpack_layer = false;
    let text = match text.split_once("://") {
        Some((scheme, rest)) => {
            let scheme = scheme.to_ascii_lowercase();
            if !matches!(scheme.as_str(), "http" | "https" | "file" | "webpack-internal" | "webpack") {
                return None;
            }
            webpack_layer = scheme == "webpack-internal";
            // Drop the host (`localhost:5173`); `file:///C:/x` and `webpack-internal:///x` have none.
            let path = if starts_with_drive(rest) {
                rest
            } else {
                rest.find('/').map_or("", |at| &rest[at..])
            };
            percent_decode(path)?
        }
        None => text.to_string(),
    };
    // Vite serves files outside the project as `/@fs/C:/...`.
    let text = if text.starts_with("/@fs/") { text[4..].to_string() } else { text };
    let trimmed = text.trim_start_matches(['/', '\\']);
    let (drive, rest) = if starts_with_drive(trimmed) {
        (Some(trimmed[..2].to_string()), &trimmed[2..])
    } else {
        (None, trimmed)
    };
    let mut parts = Vec::new();
    for segment in rest.split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".") {
        // The webpack layer folder, e.g. `(app-pages-browser)`, is not part of the path.
        if webpack_layer && parts.is_empty() && segment.starts_with('(') && segment.ends_with(')') {
            continue;
        }
        // `..` escapes; `:` is never valid in a name here (Windows streams like `x.tsx:stream`).
        if segment == ".." || segment.contains(':') {
            return None;
        }
        parts.push(segment.to_string());
    }
    if parts.is_empty() {
        return None;
    }
    Some(Parsed { drive, parts })
}

/// True for `C:` or `C:/...` (a drive letter, then a separator or the end).
fn starts_with_drive(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/' || bytes[2] == b'\\')
}

/// Decode `%XX` in a URL path. None for a broken escape or bytes that are not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The real file for `parsed` under `root` (or the absolute drive path), as a `/` separated path
/// relative to `root`. None when it is missing, is not a file, or is not inside `root`.
fn locate(root: &Path, parsed: &Parsed) -> Option<String> {
    let candidate = match &parsed.drive {
        Some(drive) => PathBuf::from(format!("{drive}\\{}", parsed.parts.join("\\"))),
        None => parsed.parts.iter().fold(root.to_path_buf(), |path, part| path.join(part)),
    };
    let real_root = root.canonicalize().ok()?;
    let real_file = candidate.canonicalize().ok()?;
    if !real_file.is_file() {
        return None;
    }
    relative_under(&real_root, &real_file)
}

/// `file` relative to `root` with `/` separators, or None when `file` is not under `root`.
/// Comparison ignores case on Windows, where drive letters and folder case can differ.
fn relative_under(root: &Path, file: &Path) -> Option<String> {
    let sep = if cfg!(windows) { '\\' } else { '/' };
    let root_text = plain(root);
    let root_text = root_text.trim_end_matches(sep);
    let file_text = plain(file);
    let root_key = fold(root_text);
    let file_key = fold(&file_text);
    if !file_key.starts_with(&root_key) || file_key.as_bytes().get(root_key.len()) != Some(&(sep as u8)) {
        return None;
    }
    let rest = file_text.get(root_key.len() + 1..)?;
    if rest.is_empty() {
        return None;
    }
    Some(if cfg!(windows) { rest.replace('\\', "/") } else { rest.to_string() })
}

/// The path as text, without the Windows `\\?\` prefix, with `\` separators on Windows.
fn plain(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    let text = text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text);
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text
    }
}

/// Lower-case on Windows (ASCII only, so byte positions stay the same), unchanged elsewhere.
fn fold(text: &str) -> String {
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text.to_string()
    }
}
