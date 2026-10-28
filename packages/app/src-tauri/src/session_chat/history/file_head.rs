//! The first lines of a chat file: where the id and the folder of a chat are written. Read at most 50 lines and
//! 64 KB, so a huge file costs little to look at.

use std::io::{BufRead, Read};
use std::path::Path;

use crate::session_chat::tail::open_lines;

/// Lines read from the start of a file.
const HEAD_LINES: usize = 50;
/// Bytes read from the start of a file.
const HEAD_BYTES: usize = 64 * 1024;

/// The first lines of `path` without their line ends. `None` when the file cannot be opened or read.
pub(super) fn head_lines(path: &Path) -> Option<Vec<String>> {
    let mut reader = open_lines(path).ok()?;
    let mut lines = Vec::new();
    let mut used = 0usize;
    while lines.len() < HEAD_LINES && used < HEAD_BYTES {
        let mut buf = Vec::new();
        let left = (HEAD_BYTES - used) as u64;
        let read = (&mut reader).take(left).read_until(b'\n', &mut buf).ok()?;
        if read == 0 {
            break;
        }
        used += read;
        lines.push(String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string());
    }
    Some(lines)
}

/// The first text value at `pointer` (a JSON pointer such as `/session_id`) in the lines, or `None`.
pub(super) fn pointer_text(lines: &[String], pointer: &str) -> Option<String> {
    lines.iter().find_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        value.pointer(pointer)?.as_str().map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_text_finds_the_first_line_that_has_it() {
        let lines = vec!["not json".to_string(), r#"{"a":{"b":"x-1"}}"#.to_string(), r#"{"a":{"b":"y"}}"#.to_string()];
        assert_eq!(pointer_text(&lines, "/a/b").as_deref(), Some("x-1"));
        assert_eq!(pointer_text(&lines, "/missing"), None);
    }
}
