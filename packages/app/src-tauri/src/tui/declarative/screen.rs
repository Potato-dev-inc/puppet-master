//! The `[tui]` table engine (SPEC 5.5, TUI-2..TUI-5): every screen-reading method of `TuiProfile`
//! answered from compiled regex tables. Pure: no I/O, no registration. Real Kilo regexes come from
//! PTY captures (T23); this module only interprets whatever a definition says.
//!
//! Safety limits: every screen text is capped at `SCREEN_CAP` bytes (the tail is kept) before a
//! regex runs, regexes are built with the VAL-2 size limit, and idle windows are clamped (TUI-4).

use std::time::Duration;

use regex::{Regex, RegexBuilder};
use serde_json::Value;

use once_cell::sync::Lazy;
use crate::pty::status::PaneStatus;
use crate::tui::types::{Reply, StandbyScreen};

use super::schema::{DefError, LineRegexTable, PromptTable, TuiTable};
use super::validate::REGEX_SIZE_LIMIT;

/// Screen text longer than this is truncated to its tail before matching.
pub const SCREEN_CAP: usize = 256 * 1024;
/// `busy`, `classify`, `menu` and `ready` regexes look at this many trailing non-empty lines.
const TAIL_LINES: usize = 12;

pub const IDLE_MIN_MS: u64 = 1500;
pub const IDLE_MAX_MS: u64 = 5000;
pub const IDLE_DEFAULT_MS: u64 = 2500;
pub const QUIET_MIN_MS: u64 = 500;

/// Same pattern as `output.rs`: `pty::ansi::strip_ansi` also collapses repeated lines, which would
/// change what a screen regex sees, so the engine strips escapes only.
static ANSI: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\x1B(?:\][^\x07\x1B]*(?:\x07|\x1B\\)|[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])").expect("ansi regex")
});

/// Escapes and carriage returns removed, nothing else.
fn strip_ansi(text: &str) -> String {
    ANSI.replace_all(text, "").replace('\r', "")
}

fn build(pattern: &str, at: &str) -> Result<Regex, DefError> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .multi_line(true)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|err| match err {
            regex::Error::CompiledTooBig(limit) => {
                DefError::new("AGENT_DEF_REGEX_TOO_LARGE", at, format!("compiled regex exceeds {limit} bytes"))
            }
            other => DefError::new("AGENT_DEF_BAD_REGEX", at, other.to_string()),
        })
}

fn build_all(patterns: &[String], at: &str) -> Result<Vec<Regex>, DefError> {
    patterns.iter().enumerate().map(|(i, p)| build(p, &format!("{at}.{i}"))).collect()
}

fn line_table(table: &Option<LineRegexTable>, at: &str) -> Result<Vec<Regex>, DefError> {
    match table {
        Some(t) => build_all(&t.line_regex, at),
        None => Ok(Vec::new()),
    }
}

/// `text` limited to `SCREEN_CAP` bytes, keeping the end (the live part of a screen).
pub fn cap(text: &str) -> &str {
    if text.len() <= SCREEN_CAP {
        return text;
    }
    let mut start = text.len() - SCREEN_CAP;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The last `n` non-empty lines of ANSI-stripped `text`.
fn tail_lines(text: &str, n: usize) -> String {
    let clean = strip_ansi(cap(text));
    let mut lines: Vec<&str> = clean.lines().filter(|l| !l.trim().is_empty()).rev().take(n).collect();
    lines.reverse();
    lines.join("\n")
}

struct Choice {
    id: String,
    reply: Reply,
}

struct Prompt {
    kind: String,
    detect: Regex,
    not_when: Option<Regex>,
    broad: bool,
    choices: Vec<Choice>,
}

struct StripBlock {
    start: Regex,
    end: Regex,
}

/// Compiled `[tui]` tables for one definition.
pub struct TuiEngine {
    id: String,
    idle_after: Duration,
    quiet_idle_after: Duration,
    clear_input: Option<&'static str>,
    ready: Option<Regex>,
    not_ready: Vec<Regex>,
    working: Vec<Regex>,
    idle: Vec<Regex>,
    chrome: Vec<Regex>,
    tool_activity: Vec<Regex>,
    activity: Vec<Regex>,
    menu: Vec<Regex>,
    strip_blocks: Vec<StripBlock>,
    dedupe: bool,
    footer_model: Option<Regex>,
    prompts: Vec<Prompt>,
    waiting: Vec<Regex>,
    has_ready: bool,
    delivery: bool,
    sidebar: Option<crate::tui::types::SidebarCrop>,
}

fn compile_prompt(table: &PromptTable, at: &str) -> Result<Prompt, DefError> {
    let mut choices = Vec::new();
    for (j, choice) in table.choices.iter().enumerate() {
        let here = format!("{at}.choices.{j}");
        let reply = match (&choice.key, &choice.text) {
            (Some(key), None) => {
                let sequence = crate::pty::keys::sequence(key)
                    .map_err(|e| DefError::new("AGENT_DEF_INCONSISTENT", &format!("{here}.key"), e))?;
                // The answer routes type a reply and then press Enter themselves, so the bytes
                // here are the sequence without its own trailing Enter: `enter` is the empty
                // reply, `escape` is ESC (then their Enter), `y` is `y`.
                Reply::key(sequence.strip_suffix('\r').unwrap_or(&sequence))
            }
            (None, Some(text)) => Reply { key: None, text: Some(text.clone()) },
            _ => {
                return Err(DefError::new(
                    "AGENT_DEF_INCONSISTENT",
                    &here,
                    "a choice needs exactly one of `key` or `text`",
                ))
            }
        };
        choices.push(Choice { id: choice.id.clone(), reply });
    }
    Ok(Prompt {
        kind: table.kind.clone(),
        detect: build(&table.detect_regex, &format!("{at}.detect_regex"))?,
        not_when: table.not_when_regex.as_deref().map(|p| build(p, &format!("{at}.not_when_regex"))).transpose()?,
        broad: table.broad,
        choices,
    })
}

impl TuiEngine {
    /// `clear_input` is the already interned `clear_input_sequence` (the trait returns `&'static`).
    pub fn compile(id: &str, tui: &TuiTable, clear_input: Option<&'static str>) -> Result<Self, DefError> {
        let idle_ms = tui.idle_after_ms.unwrap_or(IDLE_DEFAULT_MS).clamp(IDLE_MIN_MS, IDLE_MAX_MS);
        let quiet_ms = tui.quiet_idle_after_ms.unwrap_or(idle_ms).clamp(QUIET_MIN_MS, idle_ms);
        let ready = tui.ready.as_ref().and_then(|r| r.regex.as_deref()).map(|p| build(p, "tui.ready.regex")).transpose()?;
        let not_ready = match &tui.ready {
            Some(r) => build_all(&r.not_regex, "tui.ready.not_regex")?,
            None => Vec::new(),
        };
        let (working, idle) = match &tui.busy {
            Some(b) => (build_all(&b.working_regex, "tui.busy.working_regex")?, build_all(&b.idle_regex, "tui.busy.idle_regex")?),
            None => (Vec::new(), Vec::new()),
        };
        let mut strip_blocks = Vec::new();
        for (i, block) in tui.dialogs.iter().flat_map(|d| d.strip_blocks.iter()).enumerate() {
            strip_blocks.push(StripBlock {
                start: build(&block.start_regex, &format!("tui.dialogs.strip_blocks.{i}.start_regex"))?,
                end: build(&block.end_regex, &format!("tui.dialogs.strip_blocks.{i}.end_regex"))?,
            });
        }
        let footer_model = tui.footer_model.as_ref().map(|f| build(&f.regex, "tui.footer_model.regex")).transpose()?;
        let mut prompts = Vec::new();
        for (i, p) in tui.prompt.iter().enumerate() {
            prompts.push(compile_prompt(p, &format!("tui.prompt.{i}"))?);
        }
        Ok(Self {
            id: id.to_string(),
            idle_after: Duration::from_millis(idle_ms),
            quiet_idle_after: Duration::from_millis(quiet_ms),
            clear_input,
            ready,
            not_ready,
            working,
            idle,
            chrome: line_table(&tui.chrome, "tui.chrome.line_regex")?,
            tool_activity: line_table(&tui.tool_activity, "tui.tool_activity.line_regex")?,
            activity: build_all(&tui.activity_regex, "tui.activity_regex")?,
            menu: match &tui.menu {
                Some(m) => build_all(&m.regex, "tui.menu.regex")?,
                None => Vec::new(),
            },
            strip_blocks,
            dedupe: tui.tidy.as_ref().is_some_and(|t| t.dedupe_adjacent_lines),
            footer_model,
            prompts,
            waiting: match &tui.classify {
                Some(c) => build_all(&c.waiting_regex, "tui.classify.waiting_regex")?,
                None => Vec::new(),
            },
            has_ready: tui.ready.is_some(),
            delivery: tui
                .delivery
                .as_ref()
                .is_some_and(|d| d.input_echo_regex.is_some() && d.transcript_echo_regex.is_some()),
            sidebar: tui.layout.as_ref().and_then(|l| {
                Some(crate::tui::types::SidebarCrop { min_cols: l.sidebar_min_cols?, width: l.sidebar_width? })
            }),
        })
    }

    pub fn sidebar_crop(&self) -> Option<crate::tui::types::SidebarCrop> {
        self.sidebar
    }

    pub fn idle_after(&self) -> Duration {
        self.idle_after
    }

    pub fn quiet_idle_after(&self) -> Duration {
        self.quiet_idle_after
    }

    pub fn clear_input_sequence(&self) -> Option<&'static str> {
        self.clear_input
    }

    /// TUI-5: only with both echo regexes.
    pub fn verifies_prompt_delivery(&self) -> bool {
        self.delivery
    }

    /// `Some(true)` on a working marker, `Some(false)` on an idle marker, `None` otherwise or when
    /// the definition has no busy table. Working wins over idle when both show.
    pub fn busy_marker(&self, visible_tail: &str) -> Option<bool> {
        if self.working.is_empty() && self.idle.is_empty() {
            return None;
        }
        let tail = tail_lines(visible_tail, TAIL_LINES);
        if self.working.iter().any(|re| re.is_match(&tail)) {
            Some(true)
        } else if self.idle.iter().any(|re| re.is_match(&tail)) {
            Some(false)
        } else {
            None
        }
    }

    /// `waiting_regex` over the visible tail -> `WaitingInput`. An empty screen is never blocked.
    pub fn classify_screen(&self, visible_tail: &str) -> Option<PaneStatus> {
        let tail = tail_lines(visible_tail, TAIL_LINES);
        if tail.trim().is_empty() {
            return None;
        }
        self.waiting.iter().any(|re| re.is_match(&tail)).then_some(PaneStatus::WaitingInput)
    }

    fn line_matches(table: &[Regex], lower: &str) -> bool {
        let line = cap(lower).trim();
        !line.is_empty() && table.iter().any(|re| re.is_match(line))
    }

    pub fn is_chrome_line(&self, lower: &str) -> bool {
        Self::line_matches(&self.chrome, lower)
    }

    pub fn is_tool_activity_line(&self, lower: &str) -> bool {
        Self::line_matches(&self.tool_activity, lower)
    }

    /// The phrase of the newest visible line an `activity_regex` matches (`text` group, else group
    /// 1, else the whole match), or `None` when the definition declares none or nothing matches.
    pub fn activity_phrase(&self, tail: &str) -> Option<String> {
        if self.activity.is_empty() {
            return None;
        }
        let tail = strip_ansi(cap(tail));
        tail.lines().rev().map(str::trim).filter(|l| !l.is_empty()).take(TAIL_LINES * 2).find_map(|line| {
            self.activity.iter().find_map(|re| {
                let caps = re.captures(line)?;
                let found = caps.name("text").or_else(|| caps.get(1)).or_else(|| caps.get(0))?;
                Some(found.as_str().trim().to_string()).filter(|t| !t.is_empty())
            })
        })
    }

    pub fn looks_like_menu(&self, text: &str) -> bool {
        if self.menu.is_empty() {
            return false;
        }
        let tail = tail_lines(text, TAIL_LINES);
        self.menu.iter().any(|re| re.is_match(&tail))
    }

    /// Removes every line from one matching `start_regex` through the next matching `end_regex`
    /// (inclusive, searching from the start line itself). A block with no end line is left alone:
    /// dropping the rest of a screen would lose reply text, and a still-open dialog is answered
    /// through `detect_prompt` instead.
    pub fn strip_dialogs(&self, text: &str) -> String {
        let text = cap(text);
        if self.strip_blocks.is_empty() {
            return text.to_string();
        }
        let lines: Vec<&str> = text.lines().collect();
        let mut keep = vec![true; lines.len()];
        for block in &self.strip_blocks {
            let mut i = 0;
            while i < lines.len() {
                if keep[i] && block.start.is_match(lines[i]) {
                    if let Some(end) = (i..lines.len()).find(|&j| block.end.is_match(lines[j])) {
                        for flag in &mut keep[i..=end] {
                            *flag = false;
                        }
                        i = end + 1;
                        continue;
                    }
                }
                i += 1;
            }
        }
        let kept: Vec<&str> = lines.iter().zip(&keep).filter(|(_, k)| **k).map(|(l, _)| *l).collect();
        let mut joined = kept.join("\n");
        if text.ends_with('\n') && !kept.is_empty() {
            joined.push('\n');
        }
        joined
    }

    /// `dedupe_adjacent_lines`: a non-blank line equal to the previous line (ignoring surrounding
    /// whitespace) is dropped.
    pub fn tidy_reply(&self, text: &str) -> String {
        let text = cap(text);
        if !self.dedupe {
            return text.to_string();
        }
        let mut out: Vec<&str> = Vec::new();
        for line in text.lines() {
            if !line.trim().is_empty() && out.last().is_some_and(|prev| prev.trim() == line.trim()) {
                continue;
            }
            out.push(line);
        }
        let mut joined = out.join("\n");
        if text.ends_with('\n') {
            joined.push('\n');
        }
        joined
    }

    /// The `model` group of the last `footer_model` match in the buffer.
    pub fn detect_footer_model(&self, buffer: &str) -> Option<String> {
        let re = self.footer_model.as_ref()?;
        let clean = strip_ansi(cap(buffer));
        re.captures_iter(&clean)
            .filter_map(|c| c.name("model").map(|m| m.as_str().trim().to_string()))
            .filter(|m| !m.is_empty())
            .last()
    }

    fn matching_prompt(&self, clean: &str) -> Option<&Prompt> {
        self.prompts
            .iter()
            .find(|p| p.detect.is_match(clean) && !p.not_when.as_ref().is_some_and(|re| re.is_match(clean)))
    }

    /// First matching `[[tui.prompt]]`. `allow_all` is advertised only with `allow_broad` (SEC-7).
    /// `None` means the tables do not recognise the screen (callers fall back to the generic one).
    pub fn detect_prompt(&self, screen: &str, allow_broad: bool) -> Option<Value> {
        let clean = strip_ansi(cap(screen));
        let prompt = self.matching_prompt(&clean)?;
        let choices: Vec<&str> = prompt
            .choices
            .iter()
            .map(|c| c.id.as_str())
            .filter(|id| allow_broad || !(*id == "allow_all" && prompt.broad))
            .collect();
        Some(serde_json::json!({
            "agent_type": self.id,
            "kind": prompt.kind,
            "text": clean.trim(),
            "choices": choices,
        }))
    }

    /// True when a table prompt's `detect_regex` matches even if `not_when_regex` suppresses it: the
    /// screen is the definition's own, so the generic heuristic must not second-guess it.
    pub fn claims(&self, screen: &str) -> bool {
        let clean = strip_ansi(cap(screen));
        self.prompts.iter().any(|p| p.detect.is_match(&clean))
    }

    /// `Some` when the prompt's kind belongs to a table. Same broad rule and message as
    /// `generic::prompt_reply`.
    pub fn prompt_reply(&self, prompt: &Value, choice: &str, allow_broad: bool) -> Option<Result<Reply, String>> {
        let kind = prompt.get("kind").and_then(Value::as_str).unwrap_or_default();
        let mut tables = self.prompts.iter().filter(|p| p.kind == kind).peekable();
        tables.peek()?;
        let tables: Vec<&Prompt> = tables.collect();
        // Several tables may share a kind; the one that has the choice answers it.
        let table = tables.iter().find(|p| p.choices.iter().any(|c| c.id == choice)).unwrap_or(&tables[0]);
        let broad_error = || "broad approval requires explicit allow_broad".to_string();
        Some(match table.choices.iter().find(|c| c.id == choice) {
            Some(c) if c.id == "allow_all" && !(allow_broad && table.broad) => Err(broad_error()),
            Some(c) => Ok(c.reply.clone()),
            None if choice == "allow_all" => Err(broad_error()),
            None => Err("unsupported approval choice".into()),
        })
    }

    /// TUI-3: without `[tui.ready]` this is always `NotReady`. A dialog (a prompt table match or a
    /// `not_regex` hit) is never `Ready`.
    pub fn standby_screen(&self, screen: &str) -> StandbyScreen {
        if !self.has_ready {
            return StandbyScreen::NotReady;
        }
        let clean = strip_ansi(cap(screen));
        if self.not_ready.iter().any(|re| re.is_match(&clean)) || self.matching_prompt(&clean).is_some() {
            return StandbyScreen::Dialog;
        }
        match &self.ready {
            Some(re) if re.is_match(&tail_lines(&clean, TAIL_LINES)) => StandbyScreen::Ready,
            _ => StandbyScreen::NotReady,
        }
    }
}
