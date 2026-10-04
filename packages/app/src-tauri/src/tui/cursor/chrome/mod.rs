//! Screen lines that are Cursor TUI furniture, not part of a reply. Inputs are trimmed and
//! lowercased by the caller.





/// Text that only Cursor's modal dialogs (workspace trust, command approval) print.
const DIALOG_MARKERS: &[&str] = &[
    "workspace trust required",
    "do you trust the contents",
    "trust this workspace",
    "trusting workspace",
    "run this command",
    "add to allowlist",
    "run everything",
    "skip (esc or n)",
];

/// Drop every boxed dialog (a `╭…╰` block containing dialog text, or a loose line of dialog text)
/// from screen text. A dialog is never part of a reply, and after it was answered its box stays in
/// the scrollback with the TUI redrawn below. Other boxed content (a table in a reply) is kept.
pub(super) fn strip_dialog_boxes(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut keep = vec![true; lines.len()];
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if trimmed.starts_with('╭') {
            if let Some(rel) = lines[index..].iter().position(|l| l.trim_start().starts_with('╰')) {
                let end = index + rel;
                let is_dialog = lines[index..=end].iter().any(|l| {
                    let lower = l.to_ascii_lowercase();
                    DIALOG_MARKERS.iter().any(|m| lower.contains(m))
                });
                if is_dialog {
                    keep[index..=end].iter_mut().for_each(|k| *k = false);
                }
                index = end + 1;
                continue;
            }
        }
        // The same dialog with its top/bottom border rows already removed (screen text that went
        // through box-art compaction): a run of `│` rows that contains dialog text.
        if trimmed.starts_with('│') {
            let end = lines[index..]
                .iter()
                .position(|l| !l.trim_start().starts_with('│'))
                .map_or(lines.len(), |rel| index + rel);
            let is_dialog = lines[index..end].iter().any(|l| {
                let lower = l.to_ascii_lowercase();
                DIALOG_MARKERS.iter().any(|m| lower.contains(m))
            });
            if is_dialog {
                keep[index..end].iter_mut().for_each(|k| *k = false);
            }
            index = end;
            continue;
        }
        index += 1;
    }
    lines
        .iter()
        .zip(keep)
        .filter(|(_, keep)| *keep)
        .map(|(line, _)| *line)
        .collect::<Vec<_>>()
        .join("
")
}

/// Drop the working-directory rows under the model footer. Cursor prints `Composer 2.5 · 3%` and
/// then the cwd (`~\AppData\...`), hard-wrapped at the pane width, so a long path becomes several
/// rows and only the first starts with `~\`. The footer is the last block on screen, so once a
/// model row is followed by a path-like row, that row and every row after it up to the next blank
/// row are footer. A path-looking line elsewhere (a reply) is never touched.
pub(super) fn strip_footer_cwd(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let is_model_row = |line: &str| {
        let lower = line.trim().to_ascii_lowercase();
        !lower.is_empty() && (is_status_line(&lower) || is_idle_chrome(&lower))
    };
    let starts_path = |line: &str| {
        let t = line.trim();
        t.starts_with("~\\") || t.starts_with("~/") || t.starts_with('/') || {
            let b = t.as_bytes();
            b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/')
        }
    };
    let mut drop = vec![false; lines.len()];
    for index in 0..lines.len() {
        if !is_model_row(lines[index]) || !lines.get(index + 1).is_some_and(|l| starts_path(l)) {
            continue;
        }
        for (offset, line) in lines[index + 1..].iter().enumerate() {
            if line.trim().is_empty() {
                break;
            }
            drop[index + 1 + offset] = true;
        }
    }
    if !drop.contains(&true) {
        return text.to_string();
    }
    lines
        .iter()
        .zip(drop)
        .filter(|(_, drop)| !*drop)
        .map(|(line, _)| *line)
        .collect::<Vec<_>>()
        .join("
")
}

/// A wrapped header's first row is at least this wide (trimmed); the pane is ~115 columns.
const WRAPPED_HEADER_MIN: usize = 80;

/// Rejoin a shell header the terminal hard-wrapped. A long `$ cd "<path>" && cmd 5.7s` header
/// breaks at the pane width: its first row carries no duration (so it reads as quoted text, not a
/// header) and the rest starts at column 0 with the duration at its end. Merging the pieces lets
/// the header be recognised as the tool row it is. Only a `$ ` row, followed within two rows by
/// continuation rows that end in a duration, is merged.
pub(super) fn join_wrapped_shell_headers(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let lower = line.trim().to_ascii_lowercase();
        // Only a header long enough to have been wrapped: a short `$ cmd` row is quoted text.
        if lower.starts_with("$ ") && !is_tool_line(&lower) && lower.len() >= WRAPPED_HEADER_MIN {
            let mut merged = line.to_string();
            let mut joined = None;
            for offset in 1..=2 {
                let Some(next) = lines.get(index + offset) else { break };
                // Cursor continues the command on the next row, indented past the `$ `.
                if next.trim().is_empty() {
                    break;
                }
                merged.push_str(next.trim_start());
                if is_tool_line(&merged.trim().to_ascii_lowercase()) {
                    joined = Some(offset);
                    break;
                }
            }
            if let Some(offset) = joined {
                out.push(merged);
                index += offset + 1;
                continue;
            }
        }
        out.push(line.to_string());
        index += 1;
    }
    out.join("\n")
}

/// Drop the result row Cursor prints under a search header (`Grepped "def " in .` then
/// `Found 14 matches`). Positional on purpose: the same words standing alone are an answer.
pub(super) fn strip_search_results(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut previous_is_search = false;
    for line in text.lines() {
        let lower = line.trim().to_ascii_lowercase();
        if previous_is_search && is_search_result(&lower) {
            continue;
        }
        if !lower.is_empty() {
            previous_is_search = is_search_header(&lower);
        }
        out.push(line);
    }
    out.join("\n")
}

/// `Found 14 matches`, `Found 2 files`, `No matches found`.
fn is_search_result(lower: &str) -> bool {
    let words: Vec<&str> = lower.split_whitespace().collect();
    match words.as_slice() {
        ["found", n, noun] | ["found", n, noun, ..] if words.len() <= 4 => {
            n.chars().all(|ch| ch.is_ascii_digit())
                && matches!(*noun, "match" | "matches" | "file" | "files" | "result" | "results" | "line" | "lines")
        }
        ["no", "matches", ..] | ["no", "results", ..] | ["no", "files", ..] => words.len() <= 4,
        _ => false,
    }
}

/// Banner, footer, input placeholder or tool-activity row.
pub(super) fn is_chrome_line(lower: &str) -> bool {
    is_status_line(lower)
        || is_header_line(lower)
        || is_tool_line(lower)
        || is_idle_chrome(lower)
        || is_diff_line(lower)
}

/// A row of Cursor's inline diff view. The gutter is a left-block glyph (`▎`) followed by a
/// `+`/`-` marker (or a context row), e.g. `▎+ import os`. Diff text is file content the agent
/// edited, never the reply, so it must not leak into a captured result.
fn is_diff_line(lower: &str) -> bool {
    lower.starts_with(['\u{258d}', '\u{258e}', '\u{258f}'])
}

/// `… truncated (14 more lines)`: Cursor's cut-off notice under a long tool output or diff.
fn is_truncation_notice(lower: &str) -> bool {
    let text = lower.trim_start_matches(['…', '.', ' ']);
    text.starts_with("truncated")
        && (text.contains("more line") || text.contains("lines)") || text.contains("ctrl+o"))
}

/// `Edited todo.py +3 -1` / `Created notes.md +12`: the file-change header above a diff.
fn is_edit_header(lower: &str) -> bool {
    let mut words = lower.split_whitespace();
    if !matches!(
        words.next(),
        Some("edited" | "created" | "deleted" | "wrote" | "updated")
    ) {
        return false;
    }
    let rest: Vec<&str> = words.collect();
    let is_count = |word: &&str| {
        word.strip_prefix(['+', '-'])
            .is_some_and(|n| !n.is_empty() && n.chars().all(|ch| ch.is_ascii_digit()))
    };
    (2..=3).contains(&rest.len())
        && rest[0].chars().any(|ch| matches!(ch, '.' | '/' | '\\'))
        && rest[1..].iter().all(is_count)
}

/// What an idle, untouched Cursor pane shows: the input placeholder (`→ Plan, search, build
/// anything`) and a bare model footer (`Composer 2.5`, before any context usage is printed).
fn is_idle_chrome(lower: &str) -> bool {
    let text = lower.trim_start_matches(['→', '>', ' ']);
    if text.starts_with("plan, search, build anything") {
        return true;
    }
    text.strip_prefix("composer ").is_some_and(|version| {
        !version.is_empty()
            && version.len() <= 8
            && version.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
    })
}

/// Cursor's tool-activity rows shown above a reply: the collapsed-output notice
/// (`… 2 output lines hidden · ctrl+o to expand`), a shell header (`$ cd … 11s`), file reads and
/// searches (`Read todo.py`, `Globbed "**/*.py" in .`, `Read, globbed 1 file, 1 glob`) and the
/// in-flight forms (`Running…`). None of them is part of the reply text.
pub(super) fn is_tool_line(lower: &str) -> bool {
    if lower.contains("ctrl+o to expand")
        || (lower.contains("output line") && lower.contains("hidden"))
        || is_truncation_notice(lower)
        || is_edit_header(lower)
        || is_search_header(lower)
        || is_running_notice(lower)
    {
        return true;
    }
    // A shell call's header is `$ <command>` followed by its duration (or `Waiting for
    // approval...`). A `$ <command>` row with neither is text the agent wrote (a command quoted in
    // the answer), not Cursor's header. When the duration wraps onto the next row, that bare
    // duration row is itself a tool row.
    if let Some(command) = lower.strip_prefix("$ ") {
        return has_trailing_duration(command) || command.contains("waiting for approval");
    }
    if is_duration(lower) {
        return true;
    }
    let target = lower
        .strip_prefix("read ")
        .or_else(|| lower.strip_prefix("reading "));
    target.is_some_and(|target| {
        !target.contains(' ')
            && target.chars().any(|ch| matches!(ch, '.' | '/' | '\\'))
            && !target.ends_with('.')
    })
}

const TOOL_VERBS: &[&str] = &[
    "read", "reading", "globbed", "globbing", "grepped", "grepping", "searched", "searching",
    "listed", "listing", "fetched", "fetching", "ran",
];
const TOOL_NOUNS: &[&str] = &[
    "file", "files", "glob", "globs", "grep", "greps", "search", "searches", "dir", "dirs",
    "directory", "directories", "listing", "listings", "command", "commands", "page", "pages",
];

/// A search/read header: a verb followed by a quoted pattern (`Globbed "**/*.py" in .`,
/// `Grepped "todo" in src`) or the collapsed summary of a group of calls, which is only verbs,
/// counts and nouns (`Read, globbed 1 file, 1 glob`, `Globbed, read 2 globs, 1 file`). Prose that
/// starts with the same word (`Read the docs first.`) has other words in it and does not match.
fn is_search_header(lower: &str) -> bool {
    let words: Vec<&str> = lower.split_whitespace().collect();
    let Some(first) = words.first() else {
        return false;
    };
    let verb = first.trim_end_matches(',');
    if !TOOL_VERBS.contains(&verb) {
        return false;
    }
    if !matches!(verb, "read" | "reading")
        && lower[first.len()..].trim_start().starts_with(['"', '\'', '`'])
    {
        return true;
    }
    let is_summary_word = |word: &&str| {
        let word = word.trim_end_matches(',');
        TOOL_VERBS.contains(&word)
            || TOOL_NOUNS.contains(&word)
            || (!word.is_empty() && word.chars().all(|ch| ch.is_ascii_digit()))
    };
    // A group summary carries counts; `Read files.` or `Ran commands` alone is prose.
    words.len() >= 3
        && words.iter().all(is_summary_word)
        && words
            .iter()
            .any(|word| word.trim_end_matches(',').chars().all(|ch| ch.is_ascii_digit()))
}

/// `Running…` / `Running...`: the in-flight marker of a tool call that has no duration yet.
fn is_running_notice(lower: &str) -> bool {
    matches!(lower.trim(), "running" | "running…" | "running...")
}

/// The text ends with a duration such as `9.3s`, `213ms` or `1m 27s`.
fn has_trailing_duration(text: &str) -> bool {
    text.split_whitespace().last().is_some_and(|word| {
        let number = word
            .strip_suffix("ms")
            .or_else(|| word.strip_suffix('s'))
            .or_else(|| word.strip_suffix('m'));
        word.len() <= 7
            && number.is_some_and(|n| {
                n.chars().next().is_some_and(|ch| ch.is_ascii_digit())
                    && n.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
            })
    })
}

/// `6.0s`, `14s`, `1m 3s`: how long a tool call took, printed under its header.
fn is_duration(lower: &str) -> bool {
    let text = lower.trim();
    !text.is_empty()
        && text.len() <= 9
        && text.ends_with('s')
        && text.chars().all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | 's' | 'm' | ' '))
        && text.chars().any(|ch| ch.is_ascii_digit())
}

/// Cursor's startup banner and footer rows: `Cursor Agent`, `v2026.10.01-e373342`, `Tip: ...`,
/// and the `~\folder` working-directory line. None of them is ever a reply.
fn is_header_line(lower: &str) -> bool {
    let is_version = lower.strip_prefix('v').is_some_and(|rest| {
        rest.len() >= 8
            && rest.as_bytes()[..4].iter().all(u8::is_ascii_digit)
            && rest.as_bytes()[4] == b'.'
    });
    lower == "cursor agent"
        || lower.starts_with("cursor agent v")
        || is_version
        || lower.starts_with("tip:")
        || ((lower.starts_with("~\\") || lower.starts_with("~/")) && !lower.contains(' '))
}

/// Footer such as `Composer 2.5 · 7.2%` or `Composer 2.5 · 12% · 1 file edited` (model name, then
/// context usage and a change summary). A reply that only mentions the model, or uses ` · ` in a
/// sentence, does not match: every part after the model name must itself be a status figure.
fn is_status_line(lower: &str) -> bool {
    if lower.len() > 120 {
        return false;
    }
    let mut parts = lower.split(" · ").map(str::trim);
    let (Some(model), rest) = (parts.next(), parts.collect::<Vec<_>>()) else {
        return false;
    };
    let model_like = !model.is_empty()
        && model.len() <= 30
        && model.chars().next().is_some_and(char::is_alphabetic)
        && !model.contains([',', ';', ':', '!', '?'])
        && !model.ends_with('.');
    if !model_like || rest.is_empty() {
        return false;
    }
    // Every part after the model must be a status figure; a footer cell this build does not know
    // yet (a short `Auto-run`-style tag) is tolerated only when the row still ends in the context
    // percentage, which a sentence never does after a ` · ` separator.
    rest.iter().all(|part| is_status_figure(part))
        || (rest.last().is_some_and(|part| is_status_figure(part) && part.ends_with('%'))
            && rest.len() <= 3
            && rest.iter().all(|part| part.len() <= 24 && !part.contains(['.', ',']) || is_status_figure(part)))
}

/// `7.2%`, `1 file edited`, `3 files changed`, `+14 -3`: one footer figure.
fn is_status_figure(part: &str) -> bool {
    if part.ends_with('%') {
        return part[..part.len() - 1]
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '.')
            && part.len() > 1;
    }
    let words: Vec<&str> = part.split_whitespace().collect();
    let is_count = |word: &str| word.chars().all(|ch| ch.is_ascii_digit()) && !word.is_empty();
    let is_delta = |word: &str| {
        word.strip_prefix(['+', '-'])
            .is_some_and(|n| is_count(n))
    };
    if !words.is_empty() && words.iter().all(|word| is_delta(word)) {
        return true;
    }
    matches!(
        words.as_slice(),
        [n, "file" | "files", "edited" | "changed" | "created" | "modified" | "deleted"] if is_count(n)
    )
}

/// Drop later copies of a long line. Cursor redraws its commentary (the "I will now ..." lines
/// above a tool call) as it streams, so the same sentence can sit on screen more than once;
/// short lines are data (`ok`, `[]`) and may repeat on purpose.
pub(super) fn tidy_reply(text: &str) -> String {
    const LONG_LINE: usize = 40;
    let mut seen: Vec<&str> = Vec::new();
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        let key = line.trim();
        if key.chars().count() >= LONG_LINE {
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
        }
        out.push(line);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests;
