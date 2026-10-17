//! Shortened paths in a reply, put back from the hyperlinks behind them.
//!
//! A TUI that prints `.../x.py` for a long path often draws it as an OSC 8 hyperlink whose target is the whole file
//! (`file:///C:/proj/src/deep/x.py`). The words on screen are all a screen reader sees, and an orchestrator that gets
//! the reply cannot open `.../x.py`. The pane keeps each link's visible text with its target (`pty::pane_facts`); this
//! puts the target's path where its shortened text stands in the reply.
//!
//! Safe by construction: only a link whose visible text is an ellipsised form of its own target path is used, only the
//! words that were the link's text are replaced, and a text that two different links showed this turn is left alone.
//! A path is never made up from anything but a link target.

use crate::pty::pane_facts::Link;

const ELLIPSES: [&str; 2] = ["\u{2026}", "..."];

/// `reply` with each shortened link text replaced by the path of its target.
pub fn restore_link_paths(reply: &str, links: &[Link]) -> String {
    let mut out = reply.to_string();
    let mut done: Vec<&str> = Vec::new();
    for link in links.iter().rev() {
        if done.contains(&link.text.as_str()) || !out.contains(&link.text) {
            continue;
        }
        done.push(link.text.as_str());
        // the same words pointing at two different files this turn: which one is this? Neither.
        let targets: std::collections::BTreeSet<&str> =
            links.iter().filter(|other| other.text == link.text).map(|other| other.target.as_str()).collect();
        if targets.len() != 1 {
            continue;
        }
        let Some(path) = file_path(&link.target) else { continue };
        if is_shortened_form(&link.text, &path) {
            out = out.replace(&link.text, &display_path(&path));
        }
    }
    out
}

/// The local path a `file://` URL names, with `/` separators (a Windows drive keeps its letter). `None` for any other
/// URL, a remote host, or a path that is not absolute.
pub fn file_path(target: &str) -> Option<String> {
    let rest = target.trim().strip_prefix("file://")?;
    let (host, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => return None,
    };
    if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
        return None;
    }
    let decoded = percent_decode(path)?;
    // `/C:/Users/you/x.py` is a drive path
    let drive = decoded.as_bytes();
    let path = if drive.len() >= 3 && drive[0] == b'/' && drive[1].is_ascii_alphabetic() && drive[2] == b':' {
        decoded[1..].to_string()
    } else {
        decoded
    };
    (!path.contains('\0') && path.len() >= 2).then_some(path)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A drive path is written the way Windows writes it; any other path as it is.
fn display_path(path: &str) -> String {
    let b = path.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/' {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// `text` is `path` with its middle (or start) replaced by one ellipsis: what stands before the ellipsis is the path's
/// start or a stretch of it, what stands after is the path's end, and the path is the longer of the two.
pub fn is_shortened_form(text: &str, path: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/");
    let (text, path) = (norm(text.trim()), norm(path));
    let Some((marker, at)) = ELLIPSES.iter().filter_map(|m| text.find(m).map(|at| (*m, at))).min_by_key(|(_, at)| *at) else {
        return false;
    };
    let (head, tail) = (&text[..at], &text[at + marker.len()..]);
    if ELLIPSES.iter().any(|m| tail.contains(m) || head.contains(m)) {
        return false;
    }
    let tail_chars = tail.trim_start_matches('/').chars().count();
    if tail_chars < 2 || path.chars().count() <= text.chars().count() {
        return false;
    }
    if !path.to_lowercase().ends_with(&tail.to_lowercase()) {
        return false;
    }
    let head = head.trim();
    head.is_empty() || head.starts_with('~') || path.to_lowercase().contains(&head.trim_start_matches("./").to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(text: &str, target: &str) -> Link {
        Link { text: text.into(), target: target.into() }
    }

    #[test]
    fn a_file_url_is_a_local_path_and_nothing_else_is() {
        assert_eq!(file_path("file:///C:/Users/you/p/x.py").as_deref(), Some("C:/Users/you/p/x.py"));
        assert_eq!(file_path("file:///home/you/my%20dir/x.py").as_deref(), Some("/home/you/my dir/x.py"));
        assert_eq!(file_path("file://localhost/tmp/a.txt").as_deref(), Some("/tmp/a.txt"));
        assert_eq!(file_path("file://server/share/a.txt"), None, "a remote host is not a local path");
        assert_eq!(file_path("https://example.com/a.txt"), None);
        assert_eq!(file_path("file:///bad%zz/x"), None);
        assert_eq!(file_path("file://"), None);
    }

    #[test]
    fn only_an_ellipsised_form_of_the_path_counts_as_shortened() {
        let path = "C:/Users/you/proj/src/deep/x.py";
        for text in [".../x.py", "\u{2026}/deep/x.py", "C:/Users/\u{2026}/x.py", "~/\u{2026}/x.py", "src/\u{2026}/x.py", "...\\deep\\x.py"] {
            assert!(is_shortened_form(text, path), "{text}");
        }
        for text in ["x.py", "src/deep/x.py", ".../y.py", ".../x.pyc", "other/\u{2026}/x.py", "...", ".../x", "a...b...x.py", "C:/Users/you/proj/src/deep/x.py"] {
            assert!(!is_shortened_form(text, path), "{text}");
        }
    }

    #[test]
    fn the_target_path_replaces_the_shortened_words_and_nothing_else() {
        let links = [link(".../x.py", "file:///C:/Users/you/proj/src/deep/x.py")];
        assert_eq!(
            restore_link_paths("Edited .../x.py (2 lines), then .../x.py again; y.py is untouched", &links),
            r"Edited C:\Users\you\proj\src\deep\x.py (2 lines), then C:\Users\you\proj\src\deep\x.py again; y.py is untouched"
        );
        let unix = [link("\u{2026}/deep/x.py", "file:///home/you/proj/deep/x.py")];
        assert_eq!(restore_link_paths("see \u{2026}/deep/x.py", &unix), "see /home/you/proj/deep/x.py");
    }

    #[test]
    fn a_path_is_never_invented() {
        let reply = "Edited .../y.py and README.md";
        // the link points elsewhere than its words say
        assert_eq!(restore_link_paths(reply, &[link(".../y.py", "file:///C:/p/src/x.py")]), reply);
        // not a file URL
        assert_eq!(restore_link_paths(reply, &[link(".../y.py", "https://example.com/y.py")]), reply);
        // a text that is not shortened is not touched, even with a longer target
        assert_eq!(restore_link_paths(reply, &[link("README.md", "file:///C:/p/README.md")]), reply);
        // the same words for two different files this turn: neither is chosen
        let two = [link(".../y.py", "file:///C:/p/a/y.py"), link(".../y.py", "file:///C:/p/b/y.py")];
        assert_eq!(restore_link_paths(reply, &two), reply);
        // a link whose words are not in the reply changes nothing
        assert_eq!(restore_link_paths("nothing here", &[link(".../y.py", "file:///C:/p/a/y.py")]), "nothing here");
    }
}
