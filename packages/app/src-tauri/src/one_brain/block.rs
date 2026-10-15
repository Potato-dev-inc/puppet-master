//! Managed marker block handling for one-brain memory files.
//!
//! Everything works on raw bytes so that content outside the managed block
//! (user text, BOM, line endings) is preserved exactly.

pub const START: &str = "<!-- puppet-master:memory:start -->";
pub const END: &str = "<!-- puppet-master:memory:end -->";
pub const NOTE: &str = "<!-- Managed by Puppet Master from .puppet-master/memory.md. Edits inside this block are overwritten. -->";

const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const CRLF: &[u8] = b"\r\n";

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    NoChange,
    Write(Vec<u8>),
    Delete,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BlockError {
    /// START without a following END, or END without a START.
    Malformed,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BlockState {
    Absent,
    Present,
    Malformed,
}

/// Reports whether the managed block is absent, present, or malformed.
pub fn block_state(existing: &[u8]) -> BlockState {
    match locate(strip_bom(existing).1) {
        Ok(Some(_)) => BlockState::Present,
        Ok(None) => BlockState::Absent,
        Err(_) => BlockState::Malformed,
    }
}

/// Computes the new file bytes for `memory` inside the managed block.
///
/// `existing` is None when the file does not exist yet. `header` is only used
/// when a new file is created. An empty `memory` removes the block.
pub fn render(existing: Option<&[u8]>, header: Option<&str>, memory: &str) -> Result<Outcome, BlockError> {
    let (bom, body): (bool, &[u8]) = match existing {
        Some(bytes) => strip_bom(bytes),
        None => (false, &[]),
    };
    let eol = if existing.map_or(false, |b| contains(b, CRLF)) { "\r\n" } else { "\n" };
    let span = locate(body)?;

    if memory.trim().is_empty() {
        return Ok(match span {
            Some((s, e)) => remove_block(body, s, e, bom, eol, header),
            None => Outcome::NoChange,
        });
    }

    let memory_trim = trim_memory(&normalize(memory, eol));
    let block = build_block(eol, &memory_trim);
    let eol_b = eol.as_bytes();
    let header_text = header
        .map(|h| normalize(h, eol).trim_end().to_string())
        .filter(|h| !h.is_empty());

    let out: Vec<u8> = match (existing, span) {
        (_, Some((s, e))) => {
            let mut v = body[..s].to_vec();
            v.extend_from_slice(&block);
            v.extend_from_slice(&body[e..]);
            with_bom(bom, v)
        }
        (None, None) => {
            let mut v = Vec::new();
            if let Some(h) = header_text {
                v.extend_from_slice(h.as_bytes());
                v.extend_from_slice(eol_b);
                v.extend_from_slice(eol_b);
            }
            v.extend_from_slice(&block);
            v.extend_from_slice(eol_b);
            v
        }
        (Some(_), None) if is_blank(body) => {
            // Existing but effectively empty: write the block as a new file,
            // keep the BOM, and do not add the header.
            let mut v = block.clone();
            v.extend_from_slice(eol_b);
            with_bom(bom, v)
        }
        (Some(_), None) => {
            let mut v = body.to_vec();
            if !v.ends_with(eol_b) {
                v.extend_from_slice(eol_b);
            }
            v.extend_from_slice(eol_b);
            v.extend_from_slice(&block);
            v.extend_from_slice(eol_b);
            with_bom(bom, v)
        }
    };

    match existing {
        Some(orig) if orig == out.as_slice() => Ok(Outcome::NoChange),
        _ => Ok(Outcome::Write(out)),
    }
}

/// Removes the block spanning `[s, e)` of `body` and undoes the separator
/// added on insert. Returns Delete when nothing meaningful remains.
fn remove_block(body: &[u8], s: usize, e: usize, bom: bool, eol: &str, header: Option<&str>) -> Outcome {
    let eol_b = eol.as_bytes();
    let mut tail = e;
    if body[tail..].starts_with(eol_b) {
        tail += eol_b.len();
    }
    let mut rest: Vec<u8> = body[..s].to_vec();
    if ends_with_blank_line(&rest, eol) {
        rest.truncate(rest.len() - eol_b.len());
    }
    rest.extend_from_slice(&body[tail..]);

    if is_blank(&rest) || equals_header(&rest, header, eol) {
        Outcome::Delete
    } else {
        Outcome::Write(with_bom(bom, rest))
    }
}

/// Finds the managed block. Ok(Some((start, end_exclusive))) when present,
/// Ok(None) when absent, Err when malformed.
fn locate(body: &[u8]) -> Result<Option<(usize, usize)>, BlockError> {
    match find_from(body, START.as_bytes(), 0) {
        Some(s) => match find_from(body, END.as_bytes(), s + START.len()) {
            Some(e) => Ok(Some((s, e + END.len()))),
            None => Err(BlockError::Malformed),
        },
        None if find_from(body, END.as_bytes(), 0).is_some() => Err(BlockError::Malformed),
        None => Ok(None),
    }
}

fn build_block(eol: &str, memory_trim: &str) -> Vec<u8> {
    [START, eol, NOTE, eol, memory_trim, eol, END].concat().into_bytes()
}

/// Splits off a leading UTF-8 BOM, reporting whether it was present.
fn strip_bom(bytes: &[u8]) -> (bool, &[u8]) {
    if bytes.starts_with(BOM) {
        (true, &bytes[BOM.len()..])
    } else {
        (false, bytes)
    }
}

fn with_bom(bom: bool, body: Vec<u8>) -> Vec<u8> {
    if !bom {
        return body;
    }
    let mut v = Vec::with_capacity(BOM.len() + body.len());
    v.extend_from_slice(BOM);
    v.extend_from_slice(&body);
    v
}

fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() || hay.len() - from < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    find_from(hay, needle, 0).is_some()
}

fn is_blank(bytes: &[u8]) -> bool {
    bytes.iter().all(|c| c.is_ascii_whitespace())
}

/// True when `bytes` ends with two consecutive line endings.
fn ends_with_blank_line(bytes: &[u8], eol: &str) -> bool {
    let e = eol.as_bytes();
    bytes.len() >= 2 * e.len() && bytes.ends_with(e) && bytes[..bytes.len() - e.len()].ends_with(e)
}

fn equals_header(rest: &[u8], header: Option<&str>, eol: &str) -> bool {
    match (header, std::str::from_utf8(rest)) {
        (Some(h), Ok(text)) => normalize(h, eol).trim() == text.trim(),
        _ => false,
    }
}

/// Converts every line ending in `text` to `eol`.
fn normalize(text: &str, eol: &str) -> String {
    let lf = text.replace("\r\n", "\n");
    if eol == "\n" {
        lf
    } else {
        lf.replace('\n', eol)
    }
}

/// Drops leading blank lines and trailing whitespace.
fn trim_memory(text: &str) -> String {
    let mut rest = text;
    while let Some(i) = rest.find('\n') {
        if !rest[..i].trim().is_empty() {
            break;
        }
        rest = &rest[i + 1..];
    }
    rest.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(eol: &str, mem: &str) -> String {
        format!("{}{}{}{}{}{}{}", START, eol, NOTE, eol, mem, eol, END)
    }

    fn written(out: Result<Outcome, BlockError>) -> Vec<u8> {
        match out {
            Ok(Outcome::Write(v)) => v,
            other => panic!("expected Write, got {:?}", other),
        }
    }

    fn no_lone_lf(b: &[u8]) -> bool {
        b.iter().enumerate().all(|(i, &c)| c != b'\n' || (i > 0 && b[i - 1] == b'\r'))
    }

    #[test]
    fn insert_appends_below_user_text() {
        let orig: &[u8] = b"# Notes\n\nUser text.\n";
        let out = written(render(Some(orig), None, "mem"));
        let expected = format!("# Notes\n\nUser text.\n\n{}\n", block("\n", "mem"));
        assert_eq!(out, expected.as_bytes());
    }

    #[test]
    fn user_text_above_and_below_survives_update() {
        let first = written(render(Some(b"above\n".as_slice()), None, "old"));
        let mut edited = first.clone();
        edited.extend_from_slice(b"user below\n");
        let out = written(render(Some(edited.as_slice()), None, "new"));
        let expected = format!("above\n\n{}\nuser below\n", block("\n", "new"));
        assert_eq!(out, expected.as_bytes());
        // Bytes outside the block are identical slices of the input.
        assert!(out.starts_with(b"above\n\n"));
        assert!(out.ends_with(b"\nuser below\n"));
    }

    #[test]
    fn update_replaces_only_block() {
        let text = format!("pre\n{}\npost\n", block("\n", "old"));
        let out = written(render(Some(text.as_bytes()), None, "new"));
        assert_eq!(out, format!("pre\n{}\npost\n", block("\n", "new")).as_bytes());
    }

    #[test]
    fn idempotent_render_is_nochange() {
        let out = written(render(Some(b"x\n".as_slice()), None, "m"));
        assert_eq!(render(Some(out.as_slice()), None, "m"), Ok(Outcome::NoChange));
    }

    #[test]
    fn remove_restores_original_exactly() {
        let orig: &[u8] = b"# Notes\n\nUser text.\n";
        let ins = written(render(Some(orig), None, "m"));
        let r = render(Some(ins.as_slice()), None, "  \n ");
        assert_eq!(r, Ok(Outcome::Write(orig.to_vec())));
    }

    #[test]
    fn remove_absent_is_nochange() {
        assert_eq!(render(Some(b"abc\n".as_slice()), None, ""), Ok(Outcome::NoChange));
        assert_eq!(render(None, None, " \n"), Ok(Outcome::NoChange));
    }

    #[test]
    fn remove_deletes_created_file() {
        let created = written(render(None, Some("# Header"), "m"));
        assert_eq!(created, format!("# Header\n\n{}\n", block("\n", "m")).as_bytes());
        assert_eq!(render(Some(created.as_slice()), Some("# Header"), ""), Ok(Outcome::Delete));

        let bare = written(render(None, None, "m"));
        assert_eq!(render(Some(bare.as_slice()), None, ""), Ok(Outcome::Delete));
    }

    #[test]
    fn create_missing_with_header() {
        let out = written(render(None, Some("# Title  \n\n"), "mem"));
        assert_eq!(out, format!("# Title\n\n{}\n", block("\n", "mem")).as_bytes());
    }

    #[test]
    fn empty_existing_gets_block_only() {
        let out = written(render(Some(b"".as_slice()), None, "m"));
        assert_eq!(out, format!("{}\n", block("\n", "m")).as_bytes());

        let bom_only: &[u8] = &[0xEF, 0xBB, 0xBF];
        let out = written(render(Some(bom_only), None, "m"));
        let mut expected = BOM.to_vec();
        expected.extend_from_slice(format!("{}\n", block("\n", "m")).as_bytes());
        assert_eq!(out, expected);
    }

    #[test]
    fn crlf_preserved() {
        let orig: &[u8] = b"line one\r\nline two\r\n";
        let out = written(render(Some(orig), None, "a\nb"));
        assert_eq!(
            out,
            format!("line one\r\nline two\r\n\r\n{}\r\n", block("\r\n", "a\r\nb")).as_bytes()
        );
        assert!(no_lone_lf(&out));
        assert_eq!(
            render(Some(out.as_slice()), None, ""),
            Ok(Outcome::Write(orig.to_vec()))
        );
    }

    #[test]
    fn bom_preserved_on_insert_and_remove() {
        let mut orig = BOM.to_vec();
        orig.extend_from_slice(b"hi\n");
        let out = written(render(Some(orig.as_slice()), None, "m"));
        assert!(out.starts_with(BOM));
        let expected = [BOM, format!("hi\n\n{}\n", block("\n", "m")).as_bytes()].concat();
        assert_eq!(out, expected);
        assert_eq!(
            render(Some(out.as_slice()), None, ""),
            Ok(Outcome::Write(orig.clone()))
        );
    }

    #[test]
    fn bom_not_added_to_new_file() {
        let out = written(render(None, None, "m"));
        assert!(!out.starts_with(BOM));
    }

    #[test]
    fn malformed_never_modifies() {
        let start_only: &[u8] = START.as_bytes();
        assert_eq!(render(Some(start_only), None, "m"), Err(BlockError::Malformed));
        assert_eq!(render(Some(start_only), None, ""), Err(BlockError::Malformed));

        let end_only: &[u8] = END.as_bytes();
        assert_eq!(render(Some(end_only), None, "m"), Err(BlockError::Malformed));
    }

    #[test]
    fn block_state_variants() {
        assert_eq!(block_state(b"plain"), BlockState::Absent);
        let full = format!("x\n{}\n", block("\n", "m"));
        assert_eq!(block_state(full.as_bytes()), BlockState::Present);
        let mut with_bom = BOM.to_vec();
        with_bom.extend_from_slice(full.as_bytes());
        assert_eq!(block_state(&with_bom), BlockState::Present);
        assert_eq!(block_state(START.as_bytes()), BlockState::Malformed);
        assert_eq!(block_state(END.as_bytes()), BlockState::Malformed);
    }

    #[test]
    fn second_start_after_end_is_ignored() {
        let text = format!("{}\nmid\n{}", block("\n", "old"), START);
        let out = written(render(Some(text.as_bytes()), None, "new"));
        assert_eq!(out, format!("{}\nmid\n{}", block("\n", "new"), START).as_bytes());
    }

    #[test]
    fn memory_is_trimmed() {
        let out = written(render(None, None, "\n\n  line one  \n\n"));
        assert_eq!(out, format!("{}\n", block("\n", "  line one")).as_bytes());
    }
}
