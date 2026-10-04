//! Verified prompt delivery for visible TUI panes.
//!
//! Typing a prompt and pressing Enter blind lost turns on Cursor: the keystrokes raced the TUI,
//! the box kept (or regained) a stale prompt, and the run was later reported against the wrong
//! text. The protocol here, for profiles that opt in (`TuiProfile::verifies_prompt_delivery`):
//!
//! 1. clear the input box, type the prompt WITHOUT Enter;
//! 2. verify the prompt (or a distinctive prefix) is visible in the input area, otherwise clear
//!    and type it again once; if it still is not there, fail with `PROMPT_NOT_SUBMITTED` instead
//!    of pressing Enter over an unknown input box;
//! 3. press Enter and verify a new turn started (the pane left idle, or the prompt now appears
//!    in the transcript region); Enter is repeated once if nothing changed.
//!
//! The pane I/O is behind [`DeliveryIo`] so the sequences are unit-tested without a PTY.

use std::time::Duration;

/// Longest prompt prefix compared against the screen.
const HEAD_CHARS: usize = 40;
const TAIL_CHARS: usize = 30;
/// Prompts longer than this may scroll inside the box, so only their opening is compared.
const TAIL_CHECK_MAX: usize = 300;
/// Shortest prefix that is distinctive enough to trust.
const MIN_HEAD_CHARS: usize = 8;
/// Bottom screen rows that count as "the input area" (box, hint row, footer).
const INPUT_AREA_ROWS: usize = 12;

const VISIBLE_POLLS: u32 = 8;
const STARTED_POLLS: u32 = 12;
const RESUBMIT_AT_POLL: u32 = 6;
const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub trait DeliveryIo {
    fn write(&mut self, text: &str, enter: bool) -> Result<(), String>;
    fn screen(&mut self) -> String;
    /// True while the pane reports it is busy (a turn is running).
    fn busy(&mut self) -> bool;
    fn pause(&mut self, duration: Duration);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// The prompt was typed, verified, submitted, and a new turn was observed starting.
    Started,
    /// Typed, verified and submitted, but no sign of a new turn yet; the turn watch keeps looking.
    SubmittedUnconfirmed,
    /// The prompt never appeared in the input box, so Enter was not pressed.
    NotVisible,
    WriteFailed(String),
}

/// What a message should say when delivery did not happen.
pub const NOT_VISIBLE_MESSAGE: &str = "the prompt text never appeared in the pane's input box after two attempts, so it was not submitted; the input box was cleared, retrying is safe";

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn head_of(prompt: &str) -> Option<String> {
    let head: String = normalize(prompt).chars().take(HEAD_CHARS).collect();
    let head = head.trim_end().to_string();
    (head.chars().count() >= MIN_HEAD_CHARS).then_some(head)
}

fn bottom_rows(screen: &str, rows: usize) -> String {
    let lines: Vec<&str> = screen.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(rows)..].join("\n")
}

/// Whether the prompt (a distinctive prefix of it) is visible in the input area at the bottom of
/// the screen. A long or multi-line paste may be shown as a `[Pasted text ...]` placeholder, which
/// also counts. Prompts too short to be distinctive are accepted by a non-empty input area check
/// being impossible, so they fall back to "the whole prompt is visible".
pub fn prompt_visible_in_input(screen: &str, prompt: &str) -> bool {
    let area = normalize(&bottom_rows(screen, INPUT_AREA_ROWS));
    if let Some(head) = head_of(prompt) {
        if prompt.contains('\n') && area.contains("pasted text") {
            return true;
        }
        // A follow-up often starts like the previous prompt, so for prompts that fit the box
        // also require the end of the text, not just the shared opening.
        let norm = normalize(prompt);
        let len = norm.chars().count();
        let tail_ok = len <= HEAD_CHARS || len > TAIL_CHECK_MAX || {
            let tail: String = norm.chars().skip(len - TAIL_CHARS).collect();
            area.contains(tail.trim_start())
        };
        return area.contains(&head) && tail_ok;
    }
    let whole = normalize(prompt);
    !whole.is_empty() && area.contains(&whole)
}

fn occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}

/// Whether a new turn started after Enter: the pane is busy, or the prompt text now appears more
/// often on screen than before Enter (the TUI echoed it into the transcript), or the input area
/// no longer holds it while the screen changed.
pub fn turn_started(before: &str, now: &str, prompt: &str, busy: bool) -> bool {
    // Nothing on screen moved since Enter: the pane's "busy" flag is left over from typing the
    // prompt (any output flips an idle pane to running) or from the previous turn repainting.
    if before == now {
        return false;
    }
    if busy {
        return true;
    }
    let Some(head) = head_of(prompt) else {
        return normalize(before) != normalize(now);
    };
    let (before_n, now_n) = (normalize(before), normalize(now));
    if occurrences(&now_n, &head) > occurrences(&before_n, &head) {
        return true;
    }
    !prompt_visible_in_input(now, prompt) && before_n != now_n
}

/// Whether the supervisor must deliver the turn's prompt itself: the profile verifies delivery,
/// the run is already `running`, no dispatch closure is typing right now, the stage log shows the
/// dispatch never submitted the prompt, and this has not been tried yet.
pub fn needs_redelivery(
    profile_verifies: bool,
    running: bool,
    dispatch_in_flight: bool,
    prompt_dispatched: bool,
    already_redelivered: bool,
) -> bool {
    profile_verifies && running && !dispatch_in_flight && !prompt_dispatched && !already_redelivered
}

pub fn deliver_prompt(io: &mut dyn DeliveryIo, clear: Option<&str>, prompt: &str) -> Delivery {
    deliver_prompt_traced(io, clear, prompt, &mut Vec::new())
}

/// The few non-empty rows at the bottom of the screen, for a delivery trace line.
fn trace_rows(screen: &str) -> String {
    bottom_rows(screen, 5)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// [`deliver_prompt`] that also records what it saw, so a delivery that claimed success without a
/// turn can be diagnosed from the operation transcript.
pub fn deliver_prompt_traced(
    io: &mut dyn DeliveryIo,
    clear: Option<&str>,
    prompt: &str,
    trace: &mut Vec<String>,
) -> Delivery {
    let mut visible = false;
    for attempt in 0..2 {
        if let Some(clear) = clear {
            if let Err(error) = io.write(clear, false) {
                return Delivery::WriteFailed(error);
            }
            io.pause(Duration::from_millis(250));
        }
        trace.push(format!("attempt {attempt}: typing; busy={} rows={}", io.busy(), trace_rows(&io.screen())));
        if let Err(error) = io.write(prompt, false) {
            return Delivery::WriteFailed(error);
        }
        for poll in 0..VISIBLE_POLLS {
            io.pause(POLL_INTERVAL);
            let screen = io.screen();
            let seen = prompt_visible_in_input(&screen, prompt);
            trace.push(format!("typed poll {poll}: visible={seen} busy={} rows={}", io.busy(), trace_rows(&screen)));
            if seen {
                visible = true;
                break;
            }
        }
        if visible {
            break;
        }
    }
    if !visible {
        if let Some(clear) = clear {
            let _ = io.write(clear, false);
        }
        return Delivery::NotVisible;
    }
    let before = io.screen();
    if let Err(error) = io.write("", true) {
        return Delivery::WriteFailed(error);
    }
    let mut resubmitted = false;
    for poll in 0..STARTED_POLLS {
        io.pause(POLL_INTERVAL);
        let busy = io.busy();
        let screen = io.screen();
        let started = turn_started(&before, &screen, prompt, busy);
        trace.push(format!("enter poll {poll}: started={started} busy={busy} rows={}", trace_rows(&screen)));
        if started {
            return Delivery::Started;
        }
        if poll == RESUBMIT_AT_POLL && !resubmitted {
            // The typed text is verified present, so a second Enter cannot send anything else.
            resubmitted = true;
            let _ = io.write("", true);
        }
    }
    Delivery::SubmittedUnconfirmed
}

/// [`DeliveryIo`] over a live pane.
pub struct PaneIo<'a> {
    pub registry: &'a std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
    pub app: &'a tauri::AppHandle,
    pub pane_id: &'a str,
}

impl DeliveryIo for PaneIo<'_> {
    fn write(&mut self, text: &str, enter: bool) -> Result<(), String> {
        crate::pty::registry_write_input(self.registry, self.app, self.pane_id, text, enter, false, None)
    }

    fn screen(&mut self) -> String {
        crate::pty::registry_read_snapshot(self.registry.as_ref(), self.pane_id).unwrap_or_default()
    }

    fn busy(&mut self) -> bool {
        self.registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == self.pane_id)
            .is_some_and(|pane| pane.status != "idle")
    }

    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// The text typed into the pane for a task: the task, plus the worker's completion-reporting
/// instruction when the worker has MCP tools.
pub fn dispatch_prompt(
    task: &str,
    has_mcp_tools: bool,
    task_id: &str,
    agent_id: &str,
    project_path: &str,
) -> String {
    let mut prompt = task.to_string();
    if has_mcp_tools {
        prompt.push_str(&format!("\nWhen complete call complete_task with task_id={task_id}, agent_id={agent_id}, project_path={project_path} and evidence. If blocked call report_task_status with status=blocked and reason."));
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted pane: `on_write` decides what the screen shows after each write.
    struct FakePane {
        screen: String,
        busy: bool,
        writes: Vec<(String, bool)>,
        on_write: Box<dyn FnMut(&mut FakePane, &str, bool)>,
    }

    impl DeliveryIo for FakePane {
        fn write(&mut self, text: &str, enter: bool) -> Result<(), String> {
            self.writes.push((text.to_string(), enter));
            let mut hook = std::mem::replace(&mut self.on_write, Box::new(|_, _, _| {}));
            hook(self, text, enter);
            self.on_write = hook;
            Ok(())
        }
        fn screen(&mut self) -> String {
            self.screen.clone()
        }
        fn busy(&mut self) -> bool {
            self.busy
        }
        fn pause(&mut self, _duration: Duration) {}
    }

    const PROMPT: &str = "Summarize the failing test in tests/login.rs and propose a fix";

    fn pane(on_write: impl FnMut(&mut FakePane, &str, bool) + 'static) -> FakePane {
        FakePane {
            screen: "Composer 2.5\n→ Plan, search, build anything".into(),
            busy: false,
            writes: Vec::new(),
            on_write: Box::new(on_write),
        }
    }

    #[test]
    fn healthy_pane_is_typed_verified_then_submitted() {
        let mut pane = pane(|pane, text, enter| {
            if enter {
                pane.screen = format!("→ {PROMPT}\n\nworking\n→ Add a follow-up");
                pane.busy = true;
            } else if text == PROMPT {
                pane.screen = format!("│ {PROMPT} │\nComposer 2.5");
            }
        });
        let delivery = deliver_prompt(&mut pane, Some("\u{15}"), PROMPT);
        assert_eq!(delivery, Delivery::Started);
        // clear, text without Enter, then a lone Enter.
        assert_eq!(pane.writes[0], ("\u{15}".to_string(), false));
        assert_eq!(pane.writes[1], (PROMPT.to_string(), false));
        assert_eq!(pane.writes[2], (String::new(), true));
    }

    #[test]
    fn enter_is_never_pressed_when_the_prompt_does_not_show_up() {
        // Blank pane: the typed text never renders.
        let mut pane = pane(|pane, _, _| pane.screen = String::new());
        let delivery = deliver_prompt(&mut pane, Some("\u{15}"), PROMPT);
        assert_eq!(delivery, Delivery::NotVisible);
        assert!(
            pane.writes.iter().all(|(_, enter)| !enter),
            "Enter must not be sent over an unverified input box: {:?}",
            pane.writes
        );
        // It typed the prompt twice (one automatic retry) and left the box cleared.
        assert_eq!(pane.writes.iter().filter(|(text, _)| text == PROMPT).count(), 2);
        assert_eq!(pane.writes.last().unwrap().0, "\u{15}");
    }

    #[test]
    fn a_dropped_first_attempt_is_retried_once_automatically() {
        let mut typed = 0;
        let mut pane = pane(move |pane, text, enter| {
            if enter {
                pane.screen = format!("→ {PROMPT}\nworking");
                pane.busy = true;
            } else if text == PROMPT {
                typed += 1;
                if typed >= 2 {
                    pane.screen = format!("│ {PROMPT} │");
                }
            }
        });
        assert_eq!(deliver_prompt(&mut pane, Some("\u{15}"), PROMPT), Delivery::Started);
        assert_eq!(pane.writes.iter().filter(|(text, _)| text == PROMPT).count(), 2);
    }

    #[test]
    fn stale_previous_prompt_in_the_box_is_not_the_new_prompt() {
        // The box keeps showing the previous turn's prompt whatever is typed.
        let previous = "List the files in the repository root directory";
        let mut pane = pane(move |pane, _, _| pane.screen = format!("│ {previous} │"));
        assert_eq!(deliver_prompt(&mut pane, Some("\u{15}"), PROMPT), Delivery::NotVisible);
        assert!(pane.writes.iter().all(|(_, enter)| !enter));
    }

    #[test]
    fn enter_is_repeated_once_when_nothing_changes() {
        let mut pane = pane(|pane, text, enter| {
            if !enter && text == PROMPT {
                pane.screen = format!("│ {PROMPT} │");
            }
        });
        let delivery = deliver_prompt(&mut pane, None, PROMPT);
        assert_eq!(delivery, Delivery::SubmittedUnconfirmed);
        assert_eq!(pane.writes.iter().filter(|(_, enter)| *enter).count(), 2);
    }

    #[test]
    fn multiline_prompt_shown_as_paste_placeholder_is_visible() {
        let prompt = "Do this\nthen that\nand finally the other thing";
        assert!(prompt_visible_in_input("│ [Pasted text #1 +3 lines] │", prompt));
        assert!(!prompt_visible_in_input("│ [Pasted text #1 +3 lines] │", "single line only"));
    }

    #[test]
    fn a_stale_prompt_sharing_the_opening_is_not_the_new_prompt() {
        let previous = "Reply with exactly the word ALPHA and nothing else at all";
        let next = "Reply with exactly the word BRAVO and nothing else at all please";
        assert!(prompt_visible_in_input(&format!("│ {previous} │\nComposer 2.5"), previous));
        assert!(!prompt_visible_in_input(&format!("│ {previous} │\nComposer 2.5"), next));
        assert!(prompt_visible_in_input(&format!("│ {next} │\nComposer 2.5"), next));
    }

    #[test]
    fn prompt_in_the_transcript_region_is_not_input_visibility() {
        let mut screen = format!("→ {PROMPT}\n");
        for n in 0..20 {
            screen.push_str(&format!("reply line {n}\n"));
        }
        screen.push_str("→ Add a follow-up\nComposer 2.5 · 3%");
        assert!(!prompt_visible_in_input(&screen, PROMPT));
    }

    #[test]
    fn redelivery_only_when_a_running_turn_never_had_its_prompt_dispatched() {
        // profile, running, in_flight, dispatched, already
        assert!(needs_redelivery(true, true, false, false, false));
        assert!(!needs_redelivery(true, true, false, true, false), "prompt was dispatched");
        assert!(!needs_redelivery(true, true, true, false, false), "dispatch still typing");
        assert!(!needs_redelivery(true, true, false, false, true), "already retried once");
        assert!(!needs_redelivery(true, false, false, false, false), "not running yet");
        assert!(!needs_redelivery(false, true, false, false, false), "profile cannot verify");
    }

    #[test]
    fn busy_left_over_from_typing_is_not_a_started_turn() {
        // Typing flips an idle pane to running for the next IDLE_AFTER seconds, so "busy" right
        // after Enter says nothing unless the screen also moved.
        let typed = format!("│ {PROMPT} │
Composer 2.5");
        assert!(!turn_started(&typed, &typed, PROMPT, true));
        assert!(turn_started(&typed, "→ Add a follow-up   ctrl+c to stop
Composer 2.5", PROMPT, true));
    }

    #[test]
    fn a_swallowed_enter_with_a_stale_busy_flag_is_not_reported_as_started() {
        let mut pane = pane(|pane, text, enter| {
            if !enter && text == PROMPT {
                pane.screen = format!("│ {PROMPT} │");
                pane.busy = true;
            }
        });
        assert_eq!(deliver_prompt(&mut pane, None, PROMPT), Delivery::SubmittedUnconfirmed);
    }

    #[test]
    fn the_trace_records_what_each_step_saw() {
        let mut pane = pane(|pane, text, enter| {
            if enter {
                pane.screen = format!("→ {PROMPT}
working
→ Add a follow-up");
                pane.busy = true;
            } else if text == PROMPT {
                pane.screen = format!("│ {PROMPT} │");
            }
        });
        let mut trace = Vec::new();
        assert_eq!(
            deliver_prompt_traced(&mut pane, None, PROMPT, &mut trace),
            Delivery::Started
        );
        assert!(trace.iter().any(|line| line.starts_with("typed poll 0: visible=true")));
        assert!(trace.iter().any(|line| line.starts_with("enter poll 0: started=true")));
    }

    #[test]
    fn dispatch_prompt_adds_the_completion_instruction_only_for_mcp_workers() {
        assert_eq!(dispatch_prompt("t", false, "id", "pane", "C:/p"), "t");
        let with = dispatch_prompt("t", true, "id", "pane", "C:/p");
        assert!(with.starts_with("t\nWhen complete call complete_task with task_id=id, agent_id=pane"));
    }
}
