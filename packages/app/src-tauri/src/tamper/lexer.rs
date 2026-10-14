//! A line-level lexer that is driven by a rule table's comment and string delimiters.
//!
//! It turns a file into per-line views the patterns run on. `code` has comments and string contents
//! blanked; `nostr` keeps comments and blanks only string contents (silencing markers such as
//! `// @ts-ignore` live in comments). Blanking replaces each character with spaces of the same UTF-8
//! length, so a byte offset found in a view is valid in the raw line.

use super::rules::{LexerSpec, StringSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineView {
    pub raw: String,
    pub code: String,
    pub nostr: String,
}

#[derive(Clone, Copy)]
enum State {
    Normal,
    Block(usize),
    Str(usize),
}

fn blank(out: &mut String, text: &str) {
    for ch in text.chars() {
        for _ in 0..ch.len_utf8() {
            out.push(' ');
        }
    }
}

/// Split on `\n`, dropping a trailing `\r` and the empty piece after a final newline.
pub fn split_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

pub fn lex(spec: &LexerSpec, text: &str) -> Vec<LineView> {
    // Longest opener first so `"""` wins over `"`.
    let mut strings: Vec<&StringSpec> = spec.strings.iter().collect();
    strings.sort_by_key(|s| std::cmp::Reverse(s.open.len()));
    let mut skips: Vec<&String> = spec.skip_tokens.iter().collect();
    skips.sort_by_key(|s| std::cmp::Reverse(s.len()));

    // The characters a comment, string or skipped token can start with; everything else is copied in runs.
    let mut starts: Vec<char> = Vec::new();
    for token in spec
        .line_comments
        .iter()
        .chain(spec.block_comments.iter().map(|(open, _)| open))
        .chain(spec.skip_tokens.iter())
        .chain(spec.strings.iter().map(|s| &s.open))
    {
        if let Some(c) = token.chars().next() {
            if !starts.contains(&c) {
                starts.push(c);
            }
        }
    }

    let mut state = State::Normal;
    let mut views = Vec::new();
    for raw in split_lines(text) {
        let mut code = String::with_capacity(raw.len());
        let mut nostr = String::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            let rest = &raw[i..];
            match state {
                State::Block(idx) => {
                    let end = &spec.block_comments[idx].1;
                    match rest.find(end.as_str()) {
                        Some(at) => {
                            let upto = at + end.len();
                            blank(&mut code, &rest[..upto]);
                            nostr.push_str(&rest[..upto]);
                            i += upto;
                            state = State::Normal;
                        }
                        None => {
                            blank(&mut code, rest);
                            nostr.push_str(rest);
                            i = raw.len();
                        }
                    }
                }
                State::Str(idx) => {
                    let def = strings[idx];
                    if !def.escape.is_empty() && rest.starts_with(def.escape.as_str()) {
                        // The escape and the character after it are part of the string.
                        let mut take = def.escape.len();
                        if let Some(next) = rest[take..].chars().next() {
                            take += next.len_utf8();
                        }
                        blank(&mut code, &rest[..take]);
                        blank(&mut nostr, &rest[..take]);
                        i += take;
                    } else if rest.starts_with(def.close.as_str()) {
                        code.push_str(&def.close);
                        nostr.push_str(&def.close);
                        i += def.close.len();
                        state = State::Normal;
                    } else {
                        let ch = rest.chars().next().unwrap_or(' ');
                        blank(&mut code, &rest[..ch.len_utf8()]);
                        blank(&mut nostr, &rest[..ch.len_utf8()]);
                        i += ch.len_utf8();
                    }
                }
                State::Normal => {
                    let first = rest.chars().next().unwrap_or(' ');
                    if !starts.contains(&first) {
                        let end = rest.find(|c: char| starts.contains(&c)).unwrap_or(rest.len()).max(first.len_utf8());
                        code.push_str(&rest[..end]);
                        nostr.push_str(&rest[..end]);
                        i += end;
                        continue;
                    }
                    if let Some(_c) = spec.line_comments.iter().find(|c| rest.starts_with(c.as_str())) {
                        blank(&mut code, rest);
                        nostr.push_str(rest);
                        i = raw.len();
                    } else if let Some(idx) = spec.block_comments.iter().position(|(open, _)| rest.starts_with(open.as_str())) {
                        let open = &spec.block_comments[idx].0;
                        blank(&mut code, open);
                        nostr.push_str(open);
                        i += open.len();
                        state = State::Block(idx);
                    } else if let Some(token) = skips.iter().find(|t| rest.starts_with(t.as_str())) {
                        blank(&mut code, token);
                        blank(&mut nostr, token);
                        i += token.len();
                    } else if let Some(idx) = strings.iter().position(|s| rest.starts_with(s.open.as_str())) {
                        let open = &strings[idx].open;
                        code.push_str(open);
                        nostr.push_str(open);
                        i += open.len();
                        state = State::Str(idx);
                    } else {
                        let ch = rest.chars().next().unwrap_or(' ');
                        code.push(ch);
                        nostr.push(ch);
                        i += ch.len_utf8();
                    }
                }
            }
        }
        // A string that cannot span lines ends with its line (an unterminated quote must not eat the file).
        if let State::Str(idx) = state {
            if !strings[idx].multiline {
                state = State::Normal;
            }
        }
        debug_assert_eq!(code.len(), raw.len());
        debug_assert_eq!(nostr.len(), raw.len());
        views.push(LineView { raw: raw.to_string(), code, nostr });
    }
    views
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tamper::rules::Rules;

    fn js() -> &'static LexerSpec {
        &Rules::builtin().ecosystem("js").unwrap().lexer
    }

    #[test]
    fn comments_and_strings_are_blanked_with_the_same_length() {
        let views = lex(js(), "it.skip(\"a // b\", () => {}); // it.skip(x)\nconst é = 'x';");
        assert_eq!(views[0].code.len(), views[0].raw.len());
        assert!(views[0].code.starts_with("it.skip(\"      \""));
        assert!(!views[0].code.contains("// it.skip"));
        assert!(views[0].nostr.contains("// it.skip(x)"));
        assert_eq!(views[1].code.len(), views[1].raw.len());
    }

    #[test]
    fn block_comments_and_template_literals_span_lines() {
        let views = lex(js(), "/* it.skip(\n it.only( */ real();\n`it.skip(\n`; after();");
        assert!(views[0].code.trim().is_empty());
        assert!(views[1].code.contains("real();"));
        assert!(!views[1].code.contains("only"));
        assert!(!views[2].code.contains("skip"));
        assert!(views[3].code.contains("after();"));
    }

    #[test]
    fn rust_character_literals_do_not_open_strings() {
        let rust = &Rules::builtin().ecosystem("rust").unwrap().lexer;
        let views = lex(rust, "let q = '\"'; #[ignore]\nlet s = r#\"#[ignore]\"#;");
        assert!(views[0].code.contains("#[ignore]"), "{:?}", views[0].code);
        assert!(!views[1].code.contains("ignore"), "{:?}", views[1].code);
    }

    #[test]
    fn python_docstrings_are_strings() {
        let py = &Rules::builtin().ecosystem("python").unwrap().lexer;
        let views = lex(py, "\"\"\"\n@pytest.mark.skip\n\"\"\"\nx = 1  # note");
        assert!(!views[1].code.contains("skip"));
        assert!(views[3].code.contains("x = 1"));
        assert!(!views[3].code.contains("note"));
    }
}
