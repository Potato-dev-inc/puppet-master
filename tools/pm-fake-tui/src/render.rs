//! Draws the screens. Each function returns the rows of one layout (rows are strings that may carry
//! SGR escapes and never exceed the terminal width in visible characters).
//!
//! The wording, glyphs and row structure are copied from live captures kept in the app repository
//! (`packages/app/src-tauri/src/tui/{claude,codex}/fixtures/`): Claude Code 2.1.299/2.1.300 (banner,
//! rules, `❯` composer with a no-break space, dimmed suggestion, the `Bash command` / `Edit file`
//! dialogs, the folder-trust dialog, the `✻ Churned for 3s · done 15:27` summary) and Codex 0.162.1
//! (`>_ OpenAI Codex` banner, `›` composer with the dimmed "Ask Codex to do anything", the update
//! menu, `• Working (0s • esc to interrupt)`, `• Failed (exit -1)`, the `■` error line).
//!
//! The app's own tests read every one of these screens back through the real `TuiProfile`s, so a
//! change here that the profiles no longer recognise fails there.

use crate::script::{ApprovalKind, GateKind, Layout};

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const NO_DIM: &str = "\x1b[22m";
const GRAY: &str = "\x1b[38;2;153;153;153m";
const INVERSE: &str = "\x1b[7m";
const NO_INVERSE: &str = "\x1b[27m";
const RULE_GRAY: &str = "\x1b[38;2;136;136;136m";
const ORANGE: &str = "\x1b[38;2;215;119;87m";
const YELLOW: &str = "\x1b[38;2;255;193;7m";
const BOLD: &str = "\x1b[1m";

const SPINNER: [char; 5] = ['\u{2722}', '\u{2733}', '\u{2736}', '\u{273b}', '\u{273d}'];

/// One thing in the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// What the user submitted.
    User(String),
    /// A paragraph of the agent's reply.
    Reply(String),
    /// A tool call: its label, an optional result line, and an optional failure exit code.
    Tool { label: String, detail: Option<String>, failed: Option<i32> },
    /// A status line (interrupted, rejected, ...).
    Note(String),
    /// The finished-turn row: `✻ Churned for 3s · done 15:27` / `Worked for 3s • 3:27 PM`.
    Summary(String),
    /// A vendor error line (`■ You've hit your usage limit ...`).
    Error(String),
    /// Literal rows.
    Raw(Vec<String>),
}

/// Nimbus display options taken from the script.
#[derive(Debug, Clone, Default)]
pub struct Opts {
    pub backlog_rows: usize,
    /// Literal rows under the banner row.
    pub banner_rows: Vec<String>,
    pub status_model: Option<String>,
    pub status_model_busy_only: bool,
    pub footer_right: Option<String>,
    /// A status row under the footer, shown from startup on (the account and its plan).
    pub account_row: Option<String>,
    /// The model name in the status line is cut to this many characters (a narrow status bar clips it).
    pub status_model_clip: Option<usize>,
    /// Codex layout: the glyph that starts the composer row, instead of the real `\u{203a}`. A CLI update
    /// that redraws its input box differently (the canary's "changed screens" case).
    pub composer_glyph: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Banner {
    pub version: String,
    pub model: String,
    pub cwd: String,
}

/// How a nimbus permission question is drawn (script `approvals`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NimbusMenu {
    /// `Allow nimbus to ...? [y/n]`
    #[default]
    YesNo,
    /// A command: numbered rows with a cursor.
    Command,
    /// A file write: arrow rows, no numbers.
    File,
    /// Arrow rows, the highlighted one in inverse video, no marker.
    Inverse,
    /// The arrow menu inside a box.
    Boxed,
    /// One row of buttons, the selected one in inverse video; Tab moves.
    Buttons,
    /// One row of buttons; Left and Right move.
    ButtonsArrows,
    /// `[Y]es / [N]o` on the question's line.
    Inline,
    /// The inverse menu whose only yes is an "always" option.
    AlwaysOnly,
    /// Arrow rows with no visible cursor at all.
    Blind,
    /// A numbered command menu with other wording: `Run command` / `Always run` / `Cancel`.
    RunNumbered,
    /// An arrow menu whose yes row is drawn at another indent than the other rows.
    YesMissed,
    /// The numbered `>` menu with long options under the status bullets of earlier tool calls.
    BulletsNumbered,
}

/// The permission dialog being shown.
#[derive(Debug, Clone)]
pub struct Dialog {
    /// Nimbus only: which question this is.
    pub menu: NimbusMenu,
    pub kind: ApprovalKind,
    pub command: String,
    pub path: String,
    pub diff: Vec<String>,
    pub auto_mode_option: bool,
    /// 0-based option the pointer is on.
    pub selected: usize,
}

impl Dialog {
    /// The option labels, in the order drawn. The last one is always the refusal.
    pub fn options(&self, layout: Layout) -> Vec<String> {
        match (layout, self.kind) {
            (Layout::Claude, ApprovalKind::Bash) => {
                let first = self.command.split_whitespace().next().unwrap_or("command");
                let mut options = vec!["Yes".to_string(), format!("Yes, and don\u{2019}t ask again for: {first} *")];
                if self.auto_mode_option {
                    options.push("Yes, and switch to auto mode \u{b7} auto mode handles these prompts for you".into());
                }
                options.push("No".into());
                options
            }
            (Layout::Claude, _) => vec![
                "Yes".into(),
                "Yes, and switch to accept edits (auto-approve file edits and common file commands) for this session (shift+tab)".into(),
                "No".into(),
            ],
            (Layout::Codex, ApprovalKind::Bash) => vec![
                "Yes, proceed (y)".into(),
                "Yes, and don't ask again for this command (a)".into(),
                "No, and tell Codex what to do differently (esc)".into(),
            ],
            (Layout::Codex, _) => vec![
                "Yes, proceed (y)".into(),
                "Yes, and don't ask again for these files (a)".into(),
                "No, and tell Codex what to do differently (esc)".into(),
            ],
            (Layout::Nimbus, _) => match self.menu {
                NimbusMenu::YesNo => vec!["Yes".into(), "No".into()],
                NimbusMenu::Command => vec!["Allow once".into(), "Always allow in this project".into(), "Deny".into()],
                NimbusMenu::File => vec!["Yes, write it".into(), "Yes, and allow all edits this session".into(), "No, keep it as it is".into()],
                NimbusMenu::Inverse | NimbusMenu::Boxed | NimbusMenu::Buttons | NimbusMenu::ButtonsArrows | NimbusMenu::Blind => {
                    vec!["Allow once".into(), "Always allow in this project".into(), "Deny".into()]
                }
                NimbusMenu::Inline => vec!["Yes".into(), "No".into()],
                NimbusMenu::AlwaysOnly => vec!["Always allow in this project".into(), "Deny".into()],
                NimbusMenu::RunNumbered => vec!["Run command".into(), "Always run".into(), "Cancel".into()],
                NimbusMenu::YesMissed => vec!["Yes, run command".into(), "Always run in this project".into(), "No".into()],
                NimbusMenu::BulletsNumbered => {
                    let cmd = self.command.lines().next().unwrap_or_default();
                    vec![
                        "Yes, run command".into(),
                        format!("Yes, and always allow in this conversation for commands that start with '{cmd}'"),
                        format!("Yes, and always allow for commands that start with '{cmd}' (Persist to settings.json)"),
                        "No, cancel".into(),
                    ]
                }
            },
        }
    }
}

/// A startup screen being shown.
#[derive(Debug, Clone)]
pub struct GateView {
    pub kind: GateKind,
    pub selected: usize,
    pub style: crate::script::TrustStyle,
    pub animate: bool,
}

/// What the screen shows right now.
#[derive(Debug, Clone)]
pub enum Mode {
    /// The agent is still starting: the banner, no composer (Codex: the composer under `loading`).
    Loading,
    /// A composer that takes input.
    Ready,
    /// A startup screen (trust, update menu) is up and the composer is not.
    Gate(GateView),
    Approval(Dialog),
    /// Rows only: no composer, nothing moving (nimbus).
    Hold,
}

pub struct View<'a> {
    pub layout: Layout,
    pub cols: usize,
    pub rows: usize,
    pub banner: &'a Banner,
    pub blocks: &'a [Block],
    pub mode: Mode,
    pub composer: &'a str,
    pub suggestion: Option<&'a str>,
    /// No turn has run yet: Claude shows its rotating `Try "..."` hint in an empty composer.
    pub fresh: bool,
    /// `(verb, elapsed_ms)` while a turn runs.
    pub busy: Option<(&'a str, u64)>,
    /// Replaces the footer's hint (`Press Ctrl-C again to exit`, the limit wait line).
    pub notice: Option<&'a str>,
    /// Animation phase: changes every ~200 ms while busy.
    pub tick: u64,
    pub opts: &'a Opts,
}

pub struct Frame {
    pub rows: Vec<String>,
    /// 0-based `(row, col)` of the cursor, when the composer is showing.
    pub cursor: Option<(usize, usize)>,
}

pub fn frame(view: &View) -> Frame {
    frame_with(view, true)
}

/// Every row of the conversation, clipped to the width only: what an inline TUI prints into the scrollback.
pub fn frame_all(view: &View) -> Frame {
    frame_with(view, false)
}

fn frame_with(view: &View, fit_height: bool) -> Frame {
    let mut rows = match (&view.mode, view.layout) {
        (Mode::Gate(gate), Layout::Claude) => (gate_rows(view, gate), None),
        (Mode::Gate(gate), Layout::Codex) => (gate_rows(view, gate), None),
        (Mode::Gate(gate), Layout::Nimbus) => (gate_rows(view, gate), None),
        (_, Layout::Claude) => claude_rows(view),
        (_, Layout::Codex) => codex_rows(view),
        (_, Layout::Nimbus) => nimbus_rows(view),
    };
    // Fit the viewport: keep the bottom rows (the composer), clip long rows.
    let height = if fit_height { view.rows.max(1) } else { usize::MAX };
    let overflow = rows.0.len().saturating_sub(height);
    let mut out: Vec<String> = rows.0.drain(overflow..).map(|row| clip(&row, view.cols)).collect();
    let cursor = rows.1.and_then(|(r, c)| r.checked_sub(overflow).map(|r| (r, c.min(view.cols.saturating_sub(1)))));
    out.truncate(height);
    Frame { rows: out, cursor }
}

// ---- helpers ------------------------------------------------------------------------------------

/// After an ESC that `chars` just yielded: consumes the rest of that escape sequence and returns it (CSI `[...final`,
/// OSC `]...BEL` or `]...ESC \`, a two-character escape).
pub fn take_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut seq = String::from('\x1b');
    match chars.peek() {
        Some('[') => {
            seq.push(chars.next().unwrap_or('['));
            for c in chars.by_ref() {
                seq.push(c);
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
        Some(']') => {
            seq.push(chars.next().unwrap_or(']'));
            while let Some(c) = chars.next() {
                seq.push(c);
                if c == '\x07' {
                    break;
                }
                if c == '\x1b' {
                    if let Some(end) = chars.next() {
                        seq.push(end);
                    }
                    break;
                }
            }
        }
        _ => {}
    }
    seq
}

/// Visible length of a row (escape sequences do not count).
pub fn visible_len(row: &str) -> usize {
    let mut count = 0;
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            take_escape(&mut chars);
        } else {
            count += 1;
        }
    }
    count
}

/// The row cut to `width` visible characters (escape sequences are kept whole).
pub fn clip(row: &str, width: usize) -> String {
    let mut out = String::with_capacity(row.len());
    let mut count = 0;
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            out.push_str(&take_escape(&mut chars));
            continue;
        }
        if count >= width {
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

/// Word-wrap `text` into rows of at most `width` characters; embedded newlines start a row.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut rows = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut len = 0;
        for word in paragraph.split(' ') {
            let word_len = word.chars().count();
            if len > 0 && len + 1 + word_len > width {
                rows.push(std::mem::take(&mut line));
                len = 0;
            }
            if len > 0 {
                line.push(' ');
                len += 1;
            }
            // A word longer than a row is cut.
            let mut rest = word;
            while rest.chars().count() > width {
                let cut: usize = rest.char_indices().nth(width).map_or(rest.len(), |(at, _)| at);
                if !line.is_empty() {
                    rows.push(std::mem::take(&mut line));
                }
                rows.push(rest[..cut].to_string());
                rest = &rest[cut..];
                len = 0;
            }
            line.push_str(rest);
            len += rest.chars().count();
        }
        rows.push(line);
    }
    rows
}

fn rule(view: &View, ch: char) -> String {
    format!("{RULE_GRAY}{}{RESET}", ch.to_string().repeat(view.cols))
}

fn right_align(left: String, right: &str, cols: usize) -> String {
    let used = visible_len(&left);
    let right_len = right.chars().count();
    if used + 2 + right_len + 2 <= cols {
        format!("{left}{}{right}", " ".repeat(cols - used - right_len - 2))
    } else {
        left
    }
}

// ---- Claude -------------------------------------------------------------------------------------

fn claude_banner(view: &View) -> Vec<String> {
    let b = view.banner;
    vec![
        format!("{ORANGE} \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259b}\u{2588}{RESET}   {BOLD}Claude Code{RESET} {GRAY}v{}{RESET}", b.version),
        format!("{ORANGE}\u{259d}\u{259c}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2580}{RESET}  {GRAY}{}{RESET}", b.model),
        format!("{ORANGE} \u{259d}\u{259d}   \u{259d}\u{259d}   {GRAY}{}{RESET}", b.cwd),
    ]
}

fn claude_block(block: &Block, cols: usize) -> Vec<String> {
    let inner = cols.saturating_sub(2);
    let prefixed = |first: &str, text: &str| -> Vec<String> {
        wrap(text, inner).into_iter().enumerate().map(|(i, row)| if i == 0 { format!("{first}{row}") } else { format!("  {row}") }).collect()
    };
    match block {
        Block::User(text) => prefixed("\u{276f} ", text),
        Block::Reply(text) => prefixed("\u{25cf} ", text),
        Block::Tool { label, detail, failed } => {
            let mut rows = vec![format!("\u{25cf} {label}")];
            if let Some(code) = failed {
                rows.push(format!("{GRAY}  \u{23bf} \u{a0}{RESET}Error: exit code {code}"));
            }
            if let Some(detail) = detail {
                rows.push(format!("{GRAY}  \u{23bf} \u{a0}{RESET}{detail}"));
            }
            rows
        }
        Block::Note(text) => vec![format!("{GRAY}  \u{23bf} \u{a0}{RESET}{text}")],
        Block::Summary(text) => vec![format!("{GRAY}{text}{RESET}")],
        Block::Error(text) => prefixed("  ", text),
        Block::Raw(rows) => rows.clone(),
    }
}

fn claude_rows(view: &View) -> (Vec<String>, Option<(usize, usize)>) {
    let mut rows = claude_banner(view);
    rows.push(String::new());
    if matches!(view.mode, Mode::Loading) {
        return (rows, None);
    }
    for block in view.blocks {
        rows.push(String::new());
        rows.extend(claude_block(block, view.cols));
    }
    if let Some((verb, elapsed)) = view.busy {
        rows.push(String::new());
        let glyph = SPINNER[(view.tick as usize) % SPINNER.len()];
        let secs = elapsed / 1000;
        let timer = if secs >= 3 { format!(" {GRAY}({secs}s){RESET}") } else { String::new() };
        rows.push(format!("{ORANGE}{glyph}{RESET} {verb}\u{2026}{timer}"));
    }
    rows.push(String::new());
    let working = view.busy.is_some();
    if let Mode::Approval(dialog) = &view.mode {
        rows.extend(claude_dialog(view, dialog));
        return (rows, None);
    }
    rows.push(rule(view, '\u{2500}'));
    let inner = view.cols.saturating_sub(2);
    let mut cursor_row = rows.len();
    let mut cursor_col = 2;
    if view.composer.is_empty() {
        let ghost = match (view.suggestion, view.fresh) {
            (Some(text), _) => format!("{DIM}{text}{NO_DIM}"),
            (None, true) => format!("{DIM}Try \"edit <filepath> to...\"{NO_DIM}"),
            (None, false) => String::new(),
        };
        rows.push(format!("\u{276f}\u{a0}{ghost}"));
    } else {
        let wrapped = wrap(view.composer, inner);
        cursor_row = rows.len() + wrapped.len() - 1;
        cursor_col = 2 + wrapped.last().map_or(0, |row| row.chars().count());
        for (i, row) in wrapped.into_iter().enumerate() {
            rows.push(if i == 0 { format!("\u{276f}\u{a0}{row}") } else { format!("  {row}") });
        }
    }
    rows.push(rule(view, '\u{2500}'));
    let left = match view.notice {
        Some(note) => format!("  {GRAY}{note}{RESET}"),
        None if working => format!("  {YELLOW}\u{23f5}\u{23f5} auto mode on {GRAY}(shift+tab to cycle) \u{b7} esc to interrupt{RESET}"),
        None => format!("  {YELLOW}\u{23f5}\u{23f5} auto mode on {GRAY}(shift+tab to cycle){RESET}"),
    };
    rows.push(right_align(left, "\u{25d0} medium \u{b7} /effort", view.cols));
    (rows, Some((cursor_row, cursor_col)))
}

fn claude_dialog(view: &View, dialog: &Dialog) -> Vec<String> {
    let mut rows = vec![rule(view, '\u{2500}')];
    let dashed = format!("{RULE_GRAY}{}{RESET}", "\u{254c}".repeat(view.cols));
    let question = match dialog.kind {
        ApprovalKind::Bash => {
            rows.push(" Bash command".into());
            rows.push(format!(" Run {}", dialog.command.lines().next().unwrap_or_default()));
            rows.push(dashed.clone());
            rows.extend(dialog.command.lines().map(|line| format!(" {line}")));
            rows.push(dashed);
            rows.push(" This command requires approval".into());
            rows.push(String::new());
            " Do you want to proceed?".to_string()
        }
        ApprovalKind::Edit | ApprovalKind::Create => {
            let (title, verb) = if dialog.kind == ApprovalKind::Edit { ("Edit file", "make this edit to") } else { ("Create file", "create") };
            rows.push(format!(" {title}"));
            rows.push(format!(" {}", dialog.path));
            rows.push(dashed.clone());
            rows.extend(dialog.diff.iter().map(|line| format!(" {line}")));
            rows.push(dashed);
            format!(" Do you want to {verb} {}?", dialog.path)
        }
    };
    rows.push(question);
    for (i, option) in dialog.options(Layout::Claude).iter().enumerate() {
        let pointer = if i == dialog.selected { "\u{276f} " } else { "  " };
        rows.push(format!(" {pointer}{}. {option}", i + 1));
    }
    rows.push(String::new());
    rows.push(" Esc to cancel \u{b7} Tab to amend".into());
    rows
}

// ---- Codex --------------------------------------------------------------------------------------

fn codex_banner(view: &View, loading: bool) -> Vec<String> {
    let b = view.banner;
    let second = if loading { "loading".to_string() } else { b.cwd.clone() };
    vec![
        String::new(),
        format!("  {GRAY}>_ {RESET}{BOLD}OpenAI Codex{RESET}{DIM} (v{}){NO_DIM}", b.version),
        format!("     {second}"),
    ]
}

fn codex_block(block: &Block, cols: usize) -> Vec<String> {
    let inner = cols.saturating_sub(2);
    let prefixed = |first: &str, text: &str| -> Vec<String> {
        wrap(text, inner).into_iter().enumerate().map(|(i, row)| if i == 0 { format!("{first}{row}") } else { format!("  {row}") }).collect()
    };
    match block {
        Block::User(text) => prefixed("\u{203a} ", text),
        Block::Reply(text) => prefixed("\u{2022} ", text),
        Block::Tool { label, detail, failed } => match failed {
            Some(code) => {
                let mut rows = vec![format!("\u{2022} Failed (exit {code}) {label}")];
                rows.push(format!("  \u{2514} {}", detail.as_deref().unwrap_or("the command could not be run")));
                rows
            }
            None => {
                let mut rows = vec![format!("\u{2022} {label}")];
                if let Some(detail) = detail {
                    rows.push(format!("  \u{2514} {detail}"));
                }
                rows
            }
        },
        Block::Note(text) => prefixed("\u{25a0} ", text),
        Block::Summary(text) => vec![format!("  {DIM}{text}{NO_DIM}")],
        Block::Error(text) => prefixed("\u{25a0} ", text),
        Block::Raw(rows) => rows.clone(),
    }
}

fn codex_rows(view: &View) -> (Vec<String>, Option<(usize, usize)>) {
    let glyph = view.opts.composer_glyph.as_deref().unwrap_or("\u{203a}");
    let loading = matches!(view.mode, Mode::Loading);
    let mut top = codex_banner(view, loading);
    for block in view.blocks {
        top.push(String::new());
        top.extend(codex_block(block, view.cols));
    }
    if let Some((_, elapsed)) = view.busy {
        top.push(String::new());
        top.push(format!("\u{2022} Working ({}s \u{2022} esc to interrupt)", elapsed / 1000));
    }
    let mut bottom: Vec<String> = Vec::new();
    let mut cursor: Option<(usize, usize)> = None;
    if let Mode::Approval(dialog) = &view.mode {
        bottom.push(String::new());
        match dialog.kind {
            ApprovalKind::Bash => {
                bottom.push("  Would you like to run the following command?".into());
                bottom.push(String::new());
                bottom.extend(dialog.command.lines().map(|line| format!("    $ {line}")));
            }
            _ => {
                bottom.push("  Would you like to make the following edits?".into());
                bottom.push(String::new());
                bottom.push(format!("    {}", dialog.path));
                bottom.extend(dialog.diff.iter().map(|line| format!("    {line}")));
            }
        }
        bottom.push(String::new());
        for (i, option) in dialog.options(Layout::Codex).iter().enumerate() {
            let pointer = if i == dialog.selected { "\u{203a}" } else { " " };
            bottom.push(format!("{pointer} {}. {option}", i + 1));
        }
        bottom.push(String::new());
        bottom.push("  Press enter to confirm or esc to cancel".into());
    } else {
        bottom.push(String::new());
        let composer_row = bottom.len();
        if view.composer.is_empty() {
            let ghost = match view.suggestion {
                Some(text) => text.to_string(),
                None => "Ask Codex to do anything".to_string(),
            };
            bottom.push(format!("{BOLD}{glyph}{RESET} {DIM}{ghost}{NO_DIM}"));
            cursor = Some((composer_row, 2));
        } else {
            let wrapped = wrap(view.composer, view.cols.saturating_sub(2));
            let last = wrapped.len() - 1;
            let width = wrapped[last].chars().count();
            for (i, row) in wrapped.into_iter().enumerate() {
                bottom.push(if i == 0 { format!("{BOLD}{glyph}{RESET} {row}") } else { format!("  {row}") });
            }
            cursor = Some((composer_row + last, 2 + width));
        }
        bottom.push(String::new());
        let model = if view.banner.model.is_empty() { "GPT-6-Luna medium" } else { view.banner.model.as_str() };
        bottom.push(format!("  {GRAY}{model} \u{b7} {}{RESET}", view.banner.cwd));
        let hint = view.notice.map_or("? for shortcuts".to_string(), str::to_string);
        bottom.push(format!("  {GRAY}{hint}{RESET}"));
    }
    // The composer block sits at the bottom of the terminal; the transcript fills the rows above it.
    let room = view.rows.saturating_sub(bottom.len());
    let dropped = top.len().saturating_sub(room);
    let mut rows: Vec<String> = top.drain(dropped..).collect();
    rows.resize(room.max(rows.len()), String::new());
    let base = rows.len();
    rows.extend(bottom);
    (rows, cursor.map(|(r, c)| (base + r, c)))
}

// ---- Nimbus: an agent no profile knows -------------------------------------------------------------
//
// Made up for the Agent Wizard's end-to-end tests: plain ASCII frames, a boxed `> ` input row with a
// dimmed placeholder, a braille spinner next to `ctrl+c to stop`, `reply:` rows, `Allow ...? [y/n]`
// questions. None of the real agents' profiles recognise any of it, so a wizard that learns this agent
// has to learn it from the screens alone.

const NIMBUS_SPINNER: [char; 10] =
    ['\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283c}', '\u{2834}', '\u{2826}', '\u{2827}', '\u{2807}', '\u{280f}'];

fn nimbus_block(block: &Block, cols: usize) -> Vec<String> {
    let inner = cols.saturating_sub(10);
    let prefixed = |first: &str, text: &str| -> Vec<String> {
        wrap(text, inner).into_iter().enumerate().map(|(i, row)| if i == 0 { format!("{first}{row}") } else { format!("  {row}") }).collect()
    };
    match block {
        Block::User(text) => prefixed("> ", text),
        Block::Reply(text) => prefixed("reply: ", text),
        Block::Tool { label, detail, failed } => {
            let mut rows = vec![format!("tool: {label}")];
            if let Some(code) = failed {
                rows.push(format!("  -> failed with exit code {code}"));
            }
            if let Some(detail) = detail {
                rows.push(format!("  -> {detail}"));
            }
            rows
        }
        Block::Note(text) => vec![format!("note: {text}")],
        Block::Summary(text) => vec![text.clone()],
        Block::Error(text) => prefixed("error: ", text),
        Block::Raw(rows) => rows.clone(),
    }
}

fn nimbus_rows(view: &View) -> (Vec<String>, Option<(usize, usize)>) {
    let b = view.banner;
    let mut rows = vec![format!("NIMBUS {} | {} | {}", b.version, b.model, b.cwd)];
    rows.extend(view.opts.banner_rows.iter().cloned());
    rows.push(String::new());
    if matches!(view.mode, Mode::Loading) {
        rows.push("starting up...".into());
        return (rows, None);
    }
    for n in 1..=view.opts.backlog_rows {
        rows.push(format!("earlier session note {n}"));
    }
    for block in view.blocks {
        rows.push(String::new());
        rows.extend(nimbus_block(block, view.cols));
    }
    if let Some((verb, elapsed)) = view.busy {
        rows.push(String::new());
        let glyph = NIMBUS_SPINNER[(view.tick as usize) % NIMBUS_SPINNER.len()];
        let model = match (&view.opts.status_model, view.opts.status_model_busy_only) {
            (Some(model), true) => format!("  model: {}", status_model_text(model, view.opts.status_model_clip)),
            _ => String::new(),
        };
        rows.push(format!("{glyph} {verb}...  {}s  ctrl+c to stop{model}", elapsed / 1000));
    }
    rows.push(String::new());
    if matches!(view.mode, Mode::Hold) {
        return (rows, None);
    }
    if let Mode::Approval(dialog) = &view.mode {
        let what = match dialog.kind {
            ApprovalKind::Bash => format!("run `{}`", dialog.command.lines().next().unwrap_or_default()),
            ApprovalKind::Edit => format!("edit {}", dialog.path),
            ApprovalKind::Create => format!("create {}", dialog.path),
        };
        match dialog.menu {
            NimbusMenu::YesNo => rows.push(format!("Allow nimbus to {what}? [y/n]")),
            NimbusMenu::Command | NimbusMenu::RunNumbered => {
                rows.push(if dialog.menu == NimbusMenu::RunNumbered { format!("Allow nimbus to {what}?") } else { "Run this command?".into() });
                rows.push(format!("  {}", dialog.command.lines().next().unwrap_or_default()));
                rows.push(String::new());
                for (i, label) in dialog.options(Layout::Nimbus).iter().enumerate() {
                    rows.push(format!("{} {}. {label}", if i == dialog.selected { ">" } else { " " }, i + 1));
                }
            }
            NimbusMenu::Inverse | NimbusMenu::AlwaysOnly => {
                rows.push(format!("Allow nimbus to {what}?"));
                rows.push(String::new());
                for (i, label) in dialog.options(Layout::Nimbus).iter().enumerate() {
                    // the cursor is how the row is drawn: a band of inverse video, and no marker
                    rows.push(if i == dialog.selected { format!("  {INVERSE} {label:<34} {NO_INVERSE}") } else { format!("   {label}") });
                }
                rows.push(String::new());
                rows.push("  up/down to move, enter to confirm".into());
            }
            NimbusMenu::Blind => {
                rows.push(format!("Allow nimbus to {what}?"));
                rows.push(String::new());
                for label in dialog.options(Layout::Nimbus) {
                    rows.push(format!("   {label}"));
                }
                rows.push(String::new());
                rows.push("  up/down to move, enter to confirm".into());
            }
            NimbusMenu::Boxed => {
                let width = 44usize;
                let line = |text: String| format!("\u{2502} {text:<w$} \u{2502}", w = width - 4);
                rows.push(format!("\u{256d}{}\u{256e}", "\u{2500}".repeat(width - 2)));
                rows.push(line(format!("Allow nimbus to {what}?")));
                rows.push(line(String::new()));
                for (i, label) in dialog.options(Layout::Nimbus).iter().enumerate() {
                    rows.push(line(format!("{} {label}", if i == dialog.selected { "\u{276f}" } else { " " })));
                }
                rows.push(format!("\u{2570}{}\u{256f}", "\u{2500}".repeat(width - 2)));
            }
            NimbusMenu::Buttons | NimbusMenu::ButtonsArrows => {
                rows.push(format!("Allow nimbus to {what}?"));
                rows.push(String::new());
                let buttons: Vec<String> = dialog
                    .options(Layout::Nimbus)
                    .iter()
                    .enumerate()
                    .map(|(i, label)| if i == dialog.selected { format!("{INVERSE} {label} {NO_INVERSE}") } else { format!(" {label} ") })
                    .collect();
                rows.push(format!("  {}", buttons.join("  ")));
            }
            NimbusMenu::YesMissed => {
                rows.push(format!("Allow nimbus to {what}?"));
                rows.push(String::new());
                for (i, label) in dialog.options(Layout::Nimbus).iter().enumerate() {
                    let at = i == dialog.selected;
                    // the yes row sits one column left of the others, with its own cursor drawn in front of it
                    rows.push(if i == 0 { format!(" {}{label}", if at { "> " } else { "" }) } else { format!("   {} {label}", if at { ">" } else { " " }) });
                }
            }
            NimbusMenu::BulletsNumbered => {
                let cmd = dialog.command.lines().next().unwrap_or_default();
                let rule = "\u{2500}".repeat(view.cols.saturating_sub(2).clamp(30, 110));
                // what an agent prints for the tool calls of the turns before: status bullets, never a menu
                rows.pop();
                rows.push("\u{25CB} Thought for 3.4s (Determining the correct file path to create the file within the designated directory structu...)".into());
                rows.push("\u{25CF} Edited 1 file (hello.txt)".into());
                rows.push("\u{25CF} Explored 1 file (hello.txt) (ctrl+o to expand)".into());
                rows.push(String::new());
                rows.push(rule.clone());
                rows.push(format!("\u{25CF} Ran ({cmd}) (ctrl+o to expand)"));
                rows.push(String::new());
                rows.push("Command".into());
                rows.push(rule);
                rows.push(String::new());
                rows.push("Requesting permission for:".into());
                rows.push(format!("   {cmd}"));
                rows.push(String::new());
                rows.push("Run this command?".into());
                for (i, label) in dialog.options(Layout::Nimbus).iter().enumerate() {
                    rows.push(format!("{} {}. {label}", if i == dialog.selected { ">" } else { " " }, i + 1));
                }
                rows.push(String::new());
                rows.push("  \u{2191}/\u{2193} Navigate \u{b7} tab Amend \u{b7} ctrl+g edit/expand command".into());
                rows.push("esc to cancel".into());
            }
            NimbusMenu::Inline => rows.push(format!("Allow nimbus to {what}? [Y]es / [N]o")),
            NimbusMenu::File => {
                rows.push(format!("Write {}?", dialog.path));
                rows.push(String::new());
                for (i, label) in dialog.options(Layout::Nimbus).iter().enumerate() {
                    rows.push(format!(" {} {label}", if i == dialog.selected { "\u{276f}" } else { " " }));
                }
                rows.push(String::new());
                rows.push(" up/down to move, enter to confirm".into());
            }
        }
        return (rows, None);
    }
    let width = view.cols.saturating_sub(2).clamp(30, 110);
    let rule = format!("+{}+", "-".repeat(width - 2));
    rows.push(rule.clone());
    let inner = width - 6;
    let box_row = |text: &str, tail: &str, dim: bool| -> String {
        let used = text.chars().count() + tail.chars().count();
        let pad = " ".repeat(inner.saturating_sub(used));
        if dim {
            format!("| > {text}{DIM}{tail}{NO_DIM}{pad} |")
        } else {
            format!("| > {text}{tail}{pad} |")
        }
    };
    let cursor_row = rows.len();
    let mut cursor_col = 4;
    if view.composer.is_empty() {
        let ghost = view.suggestion.unwrap_or("Type a task...");
        rows.push(box_row("", ghost, true));
    } else {
        let wrapped = wrap(view.composer, inner);
        let last = wrapped.len() - 1;
        cursor_col = 4 + wrapped[last].chars().count();
        for (i, row) in wrapped.iter().enumerate() {
            rows.push(if i == 0 { box_row(row, "", false) } else { format!("|   {row}{} |", " ".repeat(inner.saturating_sub(row.chars().count()))) });
        }
        return finish_nimbus(rows, rule, view, Some((cursor_row + last, cursor_col)));
    }
    finish_nimbus(rows, rule, view, Some((cursor_row, cursor_col)))
}

/// The model name as the status line shows it: cut to `clip` characters when the bar is narrow.
fn status_model_text(model: &str, clip: Option<usize>) -> String {
    match clip {
        Some(width) => model.chars().take(width).collect(),
        None => model.to_string(),
    }
}

fn finish_nimbus(mut rows: Vec<String>, rule: String, view: &View, cursor: Option<(usize, usize)>) -> (Vec<String>, Option<(usize, usize)>) {
    rows.push(rule);
    let mut hint = view.notice.map_or("/help for commands | ctrl+c twice to quit".to_string(), str::to_string);
    if let (Some(model), false) = (&view.opts.status_model, view.opts.status_model_busy_only) {
        hint = match view.opts.status_model_clip {
            // A status bar that clips its right end: the model is the last thing on the row.
            Some(_) => format!("{hint} | model: {}", status_model_text(model, view.opts.status_model_clip)),
            None => format!("model: {model} | {hint}"),
        };
    }
    let mut row = format!("  {GRAY}{hint}{RESET}");
    if let Some(right) = &view.opts.footer_right {
        // Flush against the last column of the grid the agent was told it has.
        let used = 2 + hint.chars().count();
        let gap = view.cols.saturating_sub(used + right.chars().count());
        row = format!("  {GRAY}{hint}{}{right}{RESET}", " ".repeat(gap));
    }
    rows.push(row);
    if let Some(account) = &view.opts.account_row {
        rows.push(format!("  {GRAY}{account}{RESET}"));
    }
    (rows, cursor)
}

// ---- startup screens ----------------------------------------------------------------------------

fn gate_rows(view: &View, gate: &GateView) -> Vec<String> {
    let cwd = &view.banner.cwd;
    match (view.layout, gate.kind) {
        (Layout::Nimbus, GateKind::Trust) if gate.style != crate::script::TrustStyle::Numbered => {
            use crate::script::TrustStyle;
            let pointer = |i: usize| if gate.selected == i { ">" } else { " " };
            let (first, second) = match gate.style {
                TrustStyle::ArrowNoFirst => ("No, exit", "Yes, I trust this folder"),
                TrustStyle::Always => ("Yes, always trust this folder", "No, exit"),
                _ => ("Yes, I trust this folder", "No, exit"),
            };
            vec![
                "Accessing workspace:".into(),
                cwd.to_string(),
                String::new(),
                "Do you trust the files in this folder?".into(),
                "Nimbus requires permission to read, edit, and run files here.".into(),
                String::new(),
                format!("{} {first}", pointer(0)),
                format!("{} {second}", pointer(1)),
                String::new(),
                "↑/↓ Navigate · enter Confirm".into(),
            ]
        }
        (Layout::Nimbus, GateKind::Trust) => {
            let pointer = |i: usize| if gate.selected == i { ">" } else { " " };
            let blink = if gate.animate && view.tick % 2 == 0 { " \u{258c}" } else { "" };
            let mut rows = vec![
                format!("NIMBUS {} | {}", view.banner.version, view.banner.model),
                String::new(),
                format!("Do you trust the files in this folder?{blink}"),
                format!("  {cwd}"),
                String::new(),
                format!("{} 1. Yes, trust this folder", pointer(0)),
                format!("{} 2. No, exit", pointer(1)),
                String::new(),
                "Press Enter to confirm".into(),
            ];
            if gate.animate {
                // Alive and waiting: a spinner and a counter that change on every redraw.
                let glyph = NIMBUS_SPINNER[(view.tick as usize) % NIMBUS_SPINNER.len()];
                rows.push(String::new());
                rows.push(format!("{glyph} Waiting for your answer...  {}s", view.tick / 5));
            }
            rows
        }
        (Layout::Claude, GateKind::Trust) => {
            let pointer = |i: usize| if gate.selected == i { "\u{276f} " } else { "  " };
            let mut rows = vec![
                String::new(),
                rule(view, '\u{2500}'),
                " Accessing workspace:".into(),
                String::new(),
                format!(" {cwd}"),
                String::new(),
            ];
            rows.extend(wrap(
                "Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open source project, or work from your team). If not, take a moment to review what's in this folder first.",
                view.cols.saturating_sub(2),
            ).into_iter().map(|row| format!(" {row}")));
            rows.extend([
                String::new(),
                " Claude Code'll be able to read, edit, and execute files here.".into(),
                String::new(),
                " Security guide".into(),
                String::new(),
                format!(" {}No, exit", pointer(0)),
                format!(" {}Yes, I trust this folder", pointer(1)),
                String::new(),
                " Enter to confirm \u{b7} Esc to cancel".into(),
            ]);
            rows
        }
        (Layout::Codex, GateKind::Trust) => {
            let pointer = |i: usize| if gate.selected == i { "\u{203a}" } else { " " };
            vec![
                String::new(),
                format!("> You are running Codex in {cwd}"),
                String::new(),
                "  Do you trust the contents of this directory? Working with untrusted contents comes with higher risk of prompt injection.".into(),
                String::new(),
                format!("{} 1. Yes, continue", pointer(0)),
                format!("{} 2. No, quit", pointer(1)),
                String::new(),
                "  Press enter to continue".into(),
            ]
        }
        (_, GateKind::UpdateMenu) => {
            let pointer = |i: usize| if gate.selected == i { "\u{203a}" } else { " " };
            vec![
                String::new(),
                "  Update available \u{b7} 0.159.3 \u{2192} 0.162.1".into(),
                "  Release notes: https://github.com/openai/codex/releases/latest".into(),
                String::new(),
                format!("{} 1. Update now (runs `powershell -ExecutionPolicy Bypass -c '$env:CODEX_NON_INTERACTIVE=1; irm https://chatgpt.com/codex/install.ps1 | iex'`)", pointer(0)),
                format!("{} 2. Skip", pointer(1)),
                format!("{} 3. Skip until next version", pointer(2)),
                String::new(),
                "  enter continue \u{b7} esc skip".into(),
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>(layout: Layout, banner: &'a Banner, blocks: &'a [Block], mode: Mode) -> View<'a> {
        static OPTS: Opts = Opts { backlog_rows: 0, banner_rows: Vec::new(), status_model: None, status_model_busy_only: false, footer_right: None, account_row: None, status_model_clip: None, composer_glyph: None };
        View { layout, cols: 80, rows: 24, banner, blocks, mode, composer: "", suggestion: None, fresh: true, busy: None, notice: None, tick: 0, opts: &OPTS }
    }

    #[test]
    fn rows_never_exceed_the_width_and_the_viewport() {
        let banner = Banner { version: "2.1.299".into(), model: "Haiku 5.5".into(), cwd: "D:\\x".into() };
        let blocks: Vec<Block> = (0..40).map(|i| Block::Reply(format!("paragraph {i} with some words in it to wrap around the line"))).collect();
        for layout in [Layout::Claude, Layout::Codex] {
            let f = frame(&view(layout, &banner, &blocks, Mode::Ready));
            assert_eq!(f.rows.len(), 24, "{layout:?}");
            assert!(f.rows.iter().all(|row| visible_len(row) <= 80), "{layout:?}");
            assert!(f.cursor.is_some_and(|(row, _)| row < 24), "{layout:?} {:?}", f.cursor);
        }
    }

    #[test]
    fn the_claude_composer_is_the_row_between_two_rules() {
        let banner = Banner { version: "2.1.299".into(), model: "Haiku 5.5".into(), cwd: "D:\\x".into() };
        let f = frame(&view(Layout::Claude, &banner, &[], Mode::Ready));
        let plain: Vec<String> = f.rows.iter().map(|r| strip(r)).collect();
        let at = plain.iter().position(|r| r.starts_with('\u{276f}')).expect("composer row");
        assert!(plain[at - 1].starts_with('\u{2500}') && plain[at + 1].starts_with('\u{2500}'), "{plain:?}");
        assert!(plain[at].contains("Try \"edit <filepath> to...\""), "{}", plain[at]);
    }

    #[test]
    fn wrapping_breaks_on_words_and_keeps_newlines() {
        assert_eq!(wrap("aaa bbb ccc", 7), vec!["aaa bbb", "ccc"]);
        assert_eq!(wrap("one\ntwo", 20), vec!["one", "two"]);
        assert_eq!(wrap("abcdefghijkl", 8), vec!["abcdefgh", "ijkl"]);
    }

    pub fn strip(row: &str) -> String {
        let mut out = String::new();
        let mut chars = row.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }
}
