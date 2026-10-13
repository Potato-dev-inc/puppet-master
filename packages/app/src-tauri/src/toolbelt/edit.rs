//! Format-preserving edits of ONE table inside an agent's MCP config (JSON / TOML / YAML).
//!
//! The toolbelt only ever adds, replaces or removes members of a single parent table
//! (`mcpServers`, `mcp_servers`, `mcp`, ...). Everything else in the file is left byte for byte:
//! JSON is edited by splicing the text (no re-serialising, so key order, indentation and unrelated
//! content survive), TOML and YAML by line ranges. A change that would not leave a valid file is
//! refused and the file stays untouched.

use serde_json::{json, Value};

use crate::mcp_install::declarative::edit_yaml;

/// A change to one member of the parent table.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Set { key: String, value: Value },
    Remove { key: String },
}

/// Which format a host file uses (mirrors `McpHostFormat`).
pub use crate::tui::declarative::schema::McpHostFormat as Format;

/// Members of the parent table as JSON values (YAML members are read from flow mappings only).
pub fn members(format: Format, text: &str, parent: &[&str]) -> Result<Vec<(String, Value)>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let doc: Value = match format {
        Format::Json => serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?,
        Format::Toml => {
            let t: toml::Table = text.parse().map_err(|e| format!("not valid TOML: {e}"))?;
            serde_json::to_value(t).map_err(|e| e.to_string())?
        }
        Format::Yaml => return yaml_members(text, parent),
    };
    let mut cur = &doc;
    for seg in parent {
        match cur.get(*seg) {
            Some(next) => cur = next,
            None => return Ok(Vec::new()),
        }
    }
    Ok(cur.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
}

/// Apply `ops` in order and return the new text (equal to `text` when nothing changes).
pub fn apply(format: Format, text: &str, parent: &[&str], ops: &[Op]) -> Result<String, String> {
    let mut out = text.to_string();
    for op in ops {
        out = match format {
            Format::Json => json_apply(&out, parent, op)?,
            Format::Toml => toml_apply(&out, parent, op)?,
            Format::Yaml => yaml_apply(&out, parent, op)?,
        };
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// JSON: text splicing
// ---------------------------------------------------------------------------------------------

struct Member {
    key: String,
    start: usize,
    val_start: usize,
    val_end: usize,
}

struct Object {
    open: usize,
    close: usize,
    members: Vec<Member>,
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    i
}

fn string_end(b: &[u8], start: usize) -> Result<usize, String> {
    let mut i = start + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err("unterminated string".into())
}

fn value_end(b: &[u8], start: usize) -> Result<usize, String> {
    match b.get(start) {
        Some(b'"') => string_end(b, start),
        Some(b'{') | Some(b'[') => {
            let mut depth = 0usize;
            let mut i = start;
            while i < b.len() {
                match b[i] {
                    b'"' => i = string_end(b, i)? - 1,
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            Err("unterminated value".into())
        }
        Some(_) => {
            let mut i = start;
            while i < b.len() && !matches!(b[i], b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n') {
                i += 1;
            }
            Ok(i)
        }
        None => Err("unexpected end".into()),
    }
}

fn parse_object(text: &str, open: usize) -> Result<Object, String> {
    let b = text.as_bytes();
    let mut members = Vec::new();
    let mut i = skip_ws(b, open + 1);
    loop {
        match b.get(i) {
            Some(b'}') => return Ok(Object { open, close: i, members }),
            Some(b'"') => {
                let key_end = string_end(b, i)?;
                let key: String = serde_json::from_str(&text[i..key_end]).map_err(|e| e.to_string())?;
                let colon = skip_ws(b, key_end);
                if b.get(colon) != Some(&b':') {
                    return Err("expected `:`".into());
                }
                let val_start = skip_ws(b, colon + 1);
                let val_end = value_end(b, val_start)?;
                members.push(Member { key, start: i, val_start, val_end });
                i = skip_ws(b, val_end);
                if b.get(i) == Some(&b',') {
                    i = skip_ws(b, i + 1);
                }
            }
            _ => return Err("malformed object".into()),
        }
    }
}

/// The object at `path` and how deep we got; `depth < path.len()` means the chain stops early
/// (the remaining segments are missing).
fn find_object(text: &str, path: &[&str]) -> Result<(Object, usize), String> {
    let b = text.as_bytes();
    let root = skip_ws(b, 0);
    if b.get(root) != Some(&b'{') {
        return Err("top level is not an object".into());
    }
    let mut obj = parse_object(text, root)?;
    for (depth, seg) in path.iter().enumerate() {
        let next = obj.members.iter().find(|m| m.key == *seg).map(|m| (m.val_start, b[m.val_start]));
        match next {
            Some((start, b'{')) => obj = parse_object(text, start)?,
            Some(_) => return Err(format!("`{seg}` is not an object")),
            None => return Ok((obj, depth)),
        }
    }
    Ok((obj, path.len()))
}

fn line_indent(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map_or(0, |p| p + 1);
    text[line_start..at].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

fn pretty(value: &Value, indent: &str) -> String {
    let s = serde_json::to_string_pretty(value).unwrap_or_default();
    s.lines()
        .enumerate()
        .map(|(i, l)| if i == 0 { l.to_string() } else { format!("{indent}{l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

fn json_insert(text: &str, obj: &Object, key: &str, value: &Value) -> String {
    let key_json = serde_json::to_string(key).unwrap_or_default();
    let (member_indent, close_indent) = match obj.members.first() {
        Some(m) if text[..m.start].rfind('\n').is_some_and(|nl| text[nl..m.start].trim().is_empty()) => {
            (line_indent(text, m.start), line_indent(text, obj.close))
        }
        _ => {
            let base = line_indent(text, obj.open);
            (format!("{base}  "), base)
        }
    };
    let member = format!("{key_json}: {}", pretty(value, &member_indent));
    let mut out = String::with_capacity(text.len() + member.len() + 8);
    match obj.members.last() {
        Some(last) => {
            out.push_str(&text[..last.val_end]);
            out.push_str(&format!(",\n{member_indent}{member}"));
            out.push_str(&text[last.val_end..]);
        }
        None => {
            out.push_str(&text[..obj.open + 1]);
            out.push_str(&format!("\n{member_indent}{member}\n{close_indent}"));
            out.push_str(&text[obj.close..]);
        }
    }
    out
}

fn json_apply(text: &str, parent: &[&str], op: &Op) -> Result<String, String> {
    if text.trim().is_empty() {
        return match op {
            Op::Remove { .. } => Ok(text.to_string()),
            Op::Set { key, value } => {
                let mut nested = json!({ key.as_str(): value });
                for seg in parent.iter().rev() {
                    nested = json!({ *seg: nested });
                }
                Ok(serde_json::to_string_pretty(&nested).map_err(|e| e.to_string())? + "\n")
            }
        };
    }
    serde_json::from_str::<Value>(text).map_err(|e| format!("existing file is not valid JSON, left untouched: {e}"))?;
    let (obj, depth) = find_object(text, parent)?;
    let out = if depth < parent.len() {
        // Missing parents: add the whole missing chain as one member of the deepest existing object.
        let Op::Set { key, value } = op else { return Ok(text.to_string()) };
        let mut nested = json!({ key.as_str(): value });
        for seg in parent[depth + 1..].iter().rev() {
            nested = json!({ *seg: nested });
        }
        json_insert(text, &obj, parent[depth], &nested)
    } else {
        match op {
            Op::Set { key, value } => match obj.members.iter().find(|m| &m.key == key) {
                Some(m) => {
                    let current: Value = serde_json::from_str(&text[m.val_start..m.val_end]).unwrap_or(Value::Null);
                    if &current == value {
                        return Ok(text.to_string());
                    }
                    let indent = line_indent(text, m.start);
                    format!("{}{}{}", &text[..m.val_start], pretty(value, &indent), &text[m.val_end..])
                }
                None => json_insert(text, &obj, key, value),
            },
            Op::Remove { key } => {
                let Some(pos) = obj.members.iter().position(|m| &m.key == key) else { return Ok(text.to_string()) };
                let m = &obj.members[pos];
                if obj.members.len() == 1 {
                    format!("{}{}", &text[..obj.open + 1], &text[obj.close..])
                } else if pos + 1 < obj.members.len() {
                    format!("{}{}", &text[..m.start], &text[obj.members[pos + 1].start..])
                } else {
                    format!("{}{}", &text[..obj.members[pos - 1].val_end], &text[m.val_end..])
                }
            }
        }
    };
    serde_json::from_str::<Value>(&out).map_err(|e| format!("result would not be valid JSON: {e}"))?;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// TOML: whole sections, in place
// ---------------------------------------------------------------------------------------------

fn toml_header(line: &str) -> Option<String> {
    let t = line.trim();
    let inner = t.strip_prefix('[')?;
    if inner.starts_with('[') {
        return None;
    }
    let inner = inner.split(']').next()?;
    Some(inner.split('.').map(|s| s.trim().trim_matches('"')).collect::<Vec<_>>().join("."))
}

/// Line ranges `[start, end)` of the sections that belong to `key` (the table and its sub-tables).
fn toml_ranges(lines: &[&str], key: &str) -> Vec<(usize, usize)> {
    let sub = format!("{key}.");
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let hit = lines[i].trim_start().starts_with('[')
            && toml_header(lines[i]).is_some_and(|h| h == key || h.starts_with(&sub));
        if !hit {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < lines.len() && !lines[j].trim_start().starts_with('[') {
            j += 1;
        }
        // Blank lines and comments just before the next header belong to what follows.
        while j > i + 1 && (lines[j - 1].trim().is_empty() || lines[j - 1].trim_start().starts_with('#')) {
            j -= 1;
        }
        ranges.push((i, j));
        i = j;
    }
    ranges
}

fn to_toml_block(parent: &[&str], key: &str, value: &Value) -> Result<(String, toml::Table), String> {
    let mut nested = json!({ key: value });
    for seg in parent.iter().rev() {
        nested = json!({ *seg: nested });
    }
    let table: toml::Table =
        serde_json::from_value(nested).map_err(|e| format!("entry is not TOML-representable: {e}"))?;
    let mut block = toml::to_string(&table).map_err(|e| e.to_string())?;
    if !block.ends_with('\n') {
        block.push('\n');
    }
    Ok((block, table))
}

fn toml_apply(text: &str, parent: &[&str], op: &Op) -> Result<String, String> {
    if !text.trim().is_empty() {
        text.parse::<toml::Table>().map_err(|e| format!("existing file is not valid TOML, left untouched: {e}"))?;
    }
    let key = match op {
        Op::Set { key, .. } | Op::Remove { key } => key,
    };
    let full = parent.iter().copied().chain(std::iter::once(key.as_str())).collect::<Vec<_>>().join(".");
    let lines: Vec<&str> = text.lines().collect();
    let ranges = toml_ranges(&lines, &full);
    let inside = |n: usize| ranges.iter().any(|&(s, e)| n >= s && n < e);
    let out = match op {
        Op::Remove { .. } => {
            if ranges.is_empty() {
                return Ok(text.to_string());
            }
            let mut keep: Vec<&str> = Vec::new();
            for (n, line) in lines.iter().enumerate() {
                if inside(n) {
                    continue;
                }
                // Do not leave two blank lines where the section was.
                if line.trim().is_empty() && keep.last().is_some_and(|p| p.trim().is_empty()) {
                    continue;
                }
                keep.push(line);
            }
            while keep.last().is_some_and(|l| l.trim().is_empty()) {
                keep.pop();
            }
            if keep.is_empty() {
                String::new()
            } else {
                keep.join("\n") + "\n"
            }
        }
        Op::Set { value, .. } => {
            let (block, table) = to_toml_block(parent, key, value)?;
            if !ranges.is_empty() {
                let old: String =
                    ranges.iter().flat_map(|&(s, e)| lines[s..e].iter().copied()).collect::<Vec<_>>().join("\n");
                if old.parse::<toml::Table>().ok().as_ref() == Some(&table) {
                    return Ok(text.to_string());
                }
            }
            match ranges.first() {
                Some(&(first_start, _)) => {
                    let mut out: Vec<String> = Vec::new();
                    for (n, line) in lines.iter().enumerate() {
                        if n == first_start {
                            out.push(block.trim_end().to_string());
                        }
                        if !inside(n) {
                            // Two removed sections must not leave two blank lines in a row.
                            if line.trim().is_empty() && out.last().is_some_and(|p| p.trim().is_empty()) {
                                continue;
                            }
                            out.push((*line).to_string());
                        }
                    }
                    out.join("\n") + "\n"
                }
                None => {
                    let mut out = text.trim_end().to_string();
                    if !out.is_empty() {
                        out.push_str("\n\n");
                    }
                    out.push_str(&block);
                    out
                }
            }
        }
    };
    out.parse::<toml::Table>().map_err(|e| {
        format!("result would not be valid TOML (key defined in a form this writer cannot replace): {e}")
    })?;
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// YAML: one-line flow mappings, in place
// ---------------------------------------------------------------------------------------------

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn blank_or_comment(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// Body range `[start, end)` and child indent of the mapping at `path`.
fn yaml_body(lines: &[String], path: &[&str]) -> Option<(usize, usize, usize)> {
    let (mut start, mut end) = (0usize, lines.len());
    for seg in path {
        let child_indent = (start..end).find(|&i| !blank_or_comment(&lines[i])).map(|i| indent_of(&lines[i]))?;
        let prefix = format!("{seg}:");
        let i = (start..end).find(|&i| {
            !blank_or_comment(&lines[i])
                && indent_of(&lines[i]) == child_indent
                && lines[i].trim_start().starts_with(&prefix)
        })?;
        let mut j = i + 1;
        while j < end && (blank_or_comment(&lines[j]) || indent_of(&lines[j]) > child_indent) {
            j += 1;
        }
        start = i + 1;
        end = j;
    }
    let child_indent = (start..end).find(|&i| !blank_or_comment(&lines[i])).map(|i| indent_of(&lines[i]))?;
    Some((start, end, child_indent))
}

fn yaml_members(text: &str, parent: &[&str]) -> Result<Vec<(String, Value)>, String> {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let Some((start, end, indent)) = yaml_body(&lines, parent) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for line in &lines[start..end] {
        if blank_or_comment(line) || indent_of(line) != indent {
            continue;
        }
        if let Some((k, rest)) = line.trim_start().split_once(':') {
            let value = serde_json::from_str(rest.trim()).unwrap_or(Value::Null);
            out.push((k.trim().trim_matches('"').to_string(), value));
        }
    }
    Ok(out)
}

fn yaml_apply(text: &str, parent: &[&str], op: &Op) -> Result<String, String> {
    match op {
        Op::Set { key, value } => {
            let segs: Vec<&str> = parent.iter().copied().chain(std::iter::once(key.as_str())).collect();
            edit_yaml(text, &segs, value)
        }
        Op::Remove { key } => {
            if text.contains('\t') {
                return Err("YAML file contains tabs; left untouched".into());
            }
            let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
            let Some((start, end, indent)) = yaml_body(&lines, parent) else { return Ok(text.to_string()) };
            let prefix = format!("{key}:");
            let Some(i) = (start..end).find(|&i| {
                !blank_or_comment(&lines[i]) && indent_of(&lines[i]) == indent && lines[i].trim_start().starts_with(&prefix)
            }) else {
                return Ok(text.to_string());
            };
            let mut j = i + 1;
            while j < end && (blank_or_comment(&lines[j]) || indent_of(&lines[j]) > indent) {
                j += 1;
            }
            while j > i + 1 && blank_or_comment(&lines[j - 1]) {
                j -= 1;
            }
            lines.drain(i..j);
            Ok(if lines.is_empty() { String::new() } else { lines.join("\n") + "\n" })
        }
    }
}
