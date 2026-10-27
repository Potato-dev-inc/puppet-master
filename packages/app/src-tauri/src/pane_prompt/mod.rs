//! Structured view of the approval prompt currently blocking a TUI pane, and the answer path for
//! it. Nothing here answers on its own: a caller must name the prompt id it read, and broad
//! approvals still need an explicit opt-in.

use crate::operations::OperationError;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

pub mod answers;
mod generic_menu;

/// Phrases that open an approval dialog. The prompt text is taken from the last such line, so
/// output scrolling above the dialog cannot change its id while the same question is showing.
const PROMPT_ANCHORS: &[&str] = &[
    "run this command",
    "allow once",
    "approve this command",
    "trust this workspace",
    "trust the authors",
    "allow all",
    "always allow",
    "do you want to",
];
/// Lines kept above the anchor: the command or file being approved is usually shown there.
/// Wide enough for a wrapped multi-line command; the prompt id does not depend on these lines
/// when the command itself was extracted.
const CONTEXT_ABOVE: usize = 14;
const FALLBACK_TAIL_LINES: usize = 25;

fn prompt_region(screen: &str) -> String {
    let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    let anchor = lines.iter().rposition(|line| {
        let lower = line.to_ascii_lowercase();
        PROMPT_ANCHORS.iter().any(|phrase| lower.contains(phrase))
    });
    let start = match anchor {
        Some(index) => index.saturating_sub(CONTEXT_ABOVE),
        None => lines.len().saturating_sub(FALLBACK_TAIL_LINES),
    };
    lines[start..].join(
        "
",
    )
}

/// Prompt currently shown by `pane_id`, with a stable `prompt_id`, or `None` when the pane has
/// no supported prompt (shell panes and native OpenCode panes have their own mechanisms).
pub fn detect(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    allow_broad: bool,
) -> Option<Value> {
    detect_at(registry, pane_id, allow_broad).map(|(prompt, _)| prompt)
}

/// [`detect`] plus how much of the pane's output the screen it read includes (`pty::stream_pos`,
/// read under the same emulator lock as the screen text). An answer permit carries that position, so
/// the write is refused when the pane printed anything after the prompt was read.
pub fn detect_at(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    allow_broad: bool,
) -> Option<(Value, Option<u64>)> {
    let (agent_type, screen, pos, workspace, status) = {
        let reg = registry.lock();
        let pane = reg.panes.get(pane_id)?;
        let parser = pane.screen.lock();
        let pos = crate::pty::stream_pos::get(pane_id).map(|p| p.processed.load(std::sync::atomic::Ordering::SeqCst));
        let contents = parser.screen().contents();
        let status = pane.status.lock().as_str();
        (pane.info.agent_type.clone(), contents, pos, pane.info.cwd.clone(), status)
    };
    let mut prompt = detect_in_screen_with_status(&agent_type, &screen, allow_broad, Some(status))?;
    // The pane's folder is known here, so "outside this project" can be judged properly.
    attach_risk(&mut prompt, Some(&workspace));
    Some((prompt, pos))
}

/// Put the plain-language `risk` (see `prompt_risk`) on a detected prompt. Without a workspace,
/// absolute paths cannot be judged and relative ones count as inside.
pub(crate) fn attach_risk(prompt: &mut Value, workspace: Option<&str>) {
    let agent_type = prompt.get("agent_type").and_then(Value::as_str).unwrap_or_default().to_string();
    let text_of = |key: &str| prompt.get(key).and_then(Value::as_str).map(str::to_string);
    let (kind, command, cwd, text, path) =
        (text_of("kind"), text_of("command"), text_of("cwd"), text_of("text"), text_of("path"));
    let extra = crate::tui::profile_for(&agent_type).risk_rules();
    let risk = crate::prompt_risk::assess(
        &crate::prompt_risk::RiskInput {
            kind: kind.as_deref().unwrap_or_default(),
            command: command.as_deref(),
            cwd: cwd.as_deref(),
            path: path.as_deref(),
            workspace: workspace.filter(|w| !w.is_empty()),
            text: text.as_deref().unwrap_or_default(),
        },
        &extra,
    );
    if let Ok(value) = serde_json::to_value(&risk) {
        prompt["risk"] = value;
    }
}

/// [`detect_in_screen`], and when no profile describes what the pane shows: a menu read by its shape
/// (`generic_menu`), but only for a pane whose observed `status` is `waiting_input`. A pane that is working, idle or
/// unknown is never offered a card for a list that happens to look like a menu.
pub(crate) fn detect_in_screen_with_status(agent_type: &str, screen: &str, allow_broad: bool, status: Option<&str>) -> Option<Value> {
    let found = detect_in_screen(agent_type, screen, allow_broad);
    // A profile's own reading wins. The shared text heuristic (`generic::detect_prompt`: "allow once" somewhere on the
    // screen, answered with `y` / `n`) is a blind guess for a menu: typing `y` into a numbered or arrow menu answers
    // nothing. Where the screen is such a menu, the menu is read as a menu.
    if found.as_ref().is_some_and(|prompt| !is_shared_guess(agent_type, screen, allow_broad, prompt)) {
        return found;
    }
    if matches!(
        crate::tui::profile_for(agent_type).features().kind,
        crate::tui::Kind::NativeProtocol | crate::tui::Kind::Shell
    ) {
        return found;
    }
    let Some(mut card) = generic_menu::unknown_question(agent_type, screen, status) else { return found };
    // A one-line [y/n] question is what the shared guess already answers right.
    if found.is_some() && card.get("menu").and_then(Value::as_str) == Some("yes_no") {
        return found;
    }
    let id = prompt_id_for(&card)?;
    card["prompt_id"] = Value::String(id);
    attach_risk(&mut card, None);
    Some(card)
}

/// Whether `prompt` is nothing but the shared text heuristic's reading of this screen (no table, no identity, no
/// command of the profile's own).
fn is_shared_guess(agent_type: &str, screen: &str, allow_broad: bool, prompt: &Value) -> bool {
    if prompt.get("identity").is_some() || prompt.get("command").is_some() || prompt.get("startup_gate").is_some() {
        return false;
    }
    let Some(shared) = crate::tui::generic::detect_prompt(agent_type, &prompt_region(screen), allow_broad) else { return false };
    shared.get("kind") == prompt.get("kind") && shared.get("choices") == prompt.get("choices") && shared.get("agent_type") == prompt.get("agent_type")
}

/// The reply that picks `choice` for a detected `prompt`: a question read by its shape has its own options; every
/// other prompt is the profile's (`prompt_reply`).
pub(crate) fn reply_for(agent_type: &str, prompt: &Value, choice: &str, allow_broad: bool) -> Result<crate::tui::Reply, String> {
    if prompt.get("kind").and_then(Value::as_str) == Some("unknown_question") {
        return generic_menu::reply(prompt, choice, allow_broad);
    }
    crate::agent_adapters::adapter_for(agent_type).prompt_reply(prompt, choice, allow_broad)
}

pub(crate) fn detect_in_screen(agent_type: &str, screen: &str, allow_broad: bool) -> Option<Value> {
    if matches!(
        crate::tui::profile_for(agent_type).features().kind,
        crate::tui::Kind::NativeProtocol | crate::tui::Kind::Shell
    ) {
        return None;
    }
    let mut prompt = crate::agent_adapters::adapter_for(agent_type)
        .detect_prompt(&prompt_region(screen), allow_broad)
        .or_else(|| gate_prompt(agent_type, screen))?;
    if prompt.get("command").and_then(Value::as_str).is_some() {
        if prompt.get("cwd").is_none() {
            let dir = prompt
                .get("command")
                .and_then(Value::as_str)
                .and_then(crate::agent_runs::approval_policy::leading_cd_dir);
            if let Some(dir) = dir {
                prompt["cwd"] = Value::String(dir);
            }
        }
        prompt["summary"] = Value::String(command_summary(&prompt));
    }
    let id = prompt_id_for(&prompt)?;
    prompt["prompt_id"] = Value::String(id);
    attach_risk(&mut prompt, None);
    Some(prompt)
}

/// A startup gate only the user may pass (`TuiProfile::startup_gate` without `dismiss`: folder
/// trust, sign-in, onboarding) shown as a prompt card, for a profile that has no answerable prompt
/// of its own for it. The card has no choices (Puppet Master does not know the keys); the user
/// answers in the pane, and the run waits. A profile that does detect the dialog (Claude, Cursor,
/// Codex: trust / deny with keys) wins over this.
fn gate_prompt(agent_type: &str, screen: &str) -> Option<Value> {
    let gate = crate::tui::profile_for(agent_type).startup_gate(screen)?;
    if gate.dismiss.is_some() {
        return None;
    }
    let mut prompt = crate::tui::generic::prompt_json(agent_type, gate.prompt_kind(), gate.summary, Vec::new());
    prompt["summary"] = Value::String(gate.summary.to_string());
    prompt["identity"] = Value::String(gate.id.to_string());
    prompt["startup_gate"] = Value::String(gate.id.to_string());
    Some(prompt)
}

/// Prompt kinds that gate a pane's startup: the agent cannot take a task until the user has
/// answered. They come from a profile's `detect_prompt` (answerable: trust / deny) or from its
/// `startup_gate` (`gate_prompt`). A run parks on such a prompt (`bridge/approvals.rs`), types its
/// task only once the gate has cleared, and fails with `WORKSPACE_TRUST_DECLINED` (trust) or
/// `STARTUP_SCREEN_NOT_COMPLETED` (sign-in, setup) when the agent exits with it showing. Puppet
/// Master never answers one itself. See `tui/README.md`, "Startup gates".
pub(crate) const STARTUP_GATE_KINDS: &[&str] = &["workspace_trust", "startup_screen"];

/// The startup gate the screen shows, if any (a prompt of one of `STARTUP_GATE_KINDS`).
/// Panes whose startup gate the user answered "deny" to. Declining the folder-trust dialog makes the
/// agent exit, but its host shell may stay alive at a prompt, so "the agent exited" cannot be
/// relied on to tell a run it was declined: the decline itself is recorded here and is final.
static DECLINED_GATES: once_cell::sync::Lazy<Mutex<std::collections::HashSet<String>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(std::collections::HashSet::new()));

/// The user declined the startup gate on `pane_id` (see [`gate_was_declined`]).
pub(crate) fn mark_gate_declined(pane_id: &str) {
    DECLINED_GATES.lock().insert(pane_id.to_string());
}

/// Test builds only: panes whose startup gate a run's wait has looked at. A decline seen without the gate
/// having been seen is a different error (the run never knew the folder was untrusted), so a test that
/// declines must first wait for this (see `note_gate_seen` / `gate_was_seen`).
static SEEN_GATES: once_cell::sync::Lazy<Mutex<std::collections::HashSet<String>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(std::collections::HashSet::new()));

/// A run's wait looked at a startup gate on `pane_id`. Recorded in test builds only.
pub(crate) fn note_gate_seen(pane_id: &str) {
    if cfg!(test) {
        SEEN_GATES.lock().insert(pane_id.to_string());
    }
}

/// Test builds: whether a run's wait has looked at a startup gate on `pane_id` yet (see `note_gate_seen`).
#[cfg(test)]
pub(crate) fn gate_was_seen(pane_id: &str) -> bool {
    SEEN_GATES.lock().contains(pane_id)
}

/// Whether the user declined the startup gate on this pane. The decline is final for the run that
/// was waiting on the gate: it fails with `WORKSPACE_TRUST_DECLINED` and nothing is typed.
pub(crate) fn gate_was_declined(pane_id: &str) -> bool {
    DECLINED_GATES.lock().contains(pane_id)
}

/// Records an answer of "deny" to a startup gate (folder trust) as a decline of that pane's gate.
pub(crate) fn note_gate_answer(pane_id: &str, prompt: &Value, choice: &str) {
    let is_gate = prompt.get("kind").and_then(Value::as_str).is_some_and(|kind| STARTUP_GATE_KINDS.contains(&kind));
    if is_gate && choice == "deny" {
        mark_gate_declined(pane_id);
    }
}

pub(crate) fn startup_gate(agent_type: &str, screen: &str) -> Option<Value> {
    let prompt = detect_in_screen(agent_type, screen, false)?;
    let kind = prompt.get("kind").and_then(Value::as_str)?;
    STARTUP_GATE_KINDS.contains(&kind).then_some(prompt)
}

/// Refusal to type a task into a pane that is showing a prompt. Dialogs act on hotkeys (Cursor's
/// trust dialog takes `a`/`q`), so typing a prompt over one answers it with the prompt's letters
/// and the task is lost. `None` when the pane is free for input.
pub(crate) fn dispatch_blocker(agent_type: &str, screen: &str) -> Option<OperationError> {
    // The agent is gone and its host shell is showing a prompt (the folder-trust dialog was
    // declined, or the agent crashed): a task typed now would run as a shell command.
    if crate::tui::agent_gone_to_host_shell(agent_type, screen) {
        return Some(OperationError::new(
            "AGENT_EXITED",
            "the agent in this pane has exited and the terminal shows a shell prompt; the task was not typed into the shell. Start a new run",
            false,
        ));
    }
    let Some(prompt) = detect_in_screen(agent_type, screen, false) else {
        // Not an approval, but a startup screen (update notice, trust, login): typing a task into
        // it is as lost as typing over a dialog. A notice with its own dismiss keys is not a
        // blocker here: delivery closes it before typing (and refuses if it will not close).
        let gate = crate::tui::profile_for(agent_type).startup_gate(screen)?;
        if gate.dismiss.is_some() {
            return None;
        }
        return Some(gate_blocked_error(gate));
    };
    let mut error = OperationError::new(
        "PANE_BLOCKED_BY_PROMPT",
        "the pane is showing a prompt (workspace trust or command approval); the task was not typed into it. Answer the prompt with answer_prompt, then retry",
        true,
    );
    error.context = json!({"prompt_id": prompt.get("prompt_id"), "kind": prompt.get("kind")});
    Some(error)
}

/// The refusal for a task that was not typed because a startup screen is up.
pub(crate) fn gate_blocked_error(gate: crate::tui::StartupGate) -> OperationError {
    let mut error = OperationError::new(
        "PANE_BLOCKED_BY_PROMPT",
        format!(
            "the pane is showing a startup screen ({}); the task was not typed into it. Deal with it in the pane, then retry",
            gate.summary
        ),
        true,
    );
    error.context = json!({"startup_gate": gate.id});
    error
}

/// `Run command: <command> (in <cwd>)`, so a caller sees what it is approving without the pane.
pub(crate) fn command_summary(prompt: &Value) -> String {
    let command = prompt
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match prompt.get("cwd").and_then(Value::as_str) {
        Some(cwd) if !command.contains(cwd) => format!("Run command: {command} (in {cwd})"),
        _ => format!("Run command: {command}"),
    }
}

/// The id names what is being approved. When the command was read off the screen it is the
/// identity (so redraws, a still-arriving option list or scrolling do not change it); otherwise the
/// whole prompt is hashed.
fn prompt_id_for(prompt: &Value) -> Option<String> {
    if let Some(command) = prompt.get("command").and_then(Value::as_str) {
        // The command as read off the screen breaks wherever the pane's width does (`npm test` on one
        // row at 105 columns, `npm\ntest` at 52), so the same approval read at two widths was two
        // prompts: the phone's grid lease alternated the id and the transcript listed one approval
        // four times (VERIFY-R11 #2). Whitespace is not part of what is being approved's identity.
        let squash = |text: &str| text.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
        return crate::agent_runs::stable_prompt_id(&json!({
            "agent_type": prompt.get("agent_type"),
            "kind": prompt.get("kind"),
            "command": squash(command),
            "cwd": prompt.get("cwd").and_then(Value::as_str).map(squash),
        }));
    }
    // No command to anchor on: hash the semantic identity, never the layout. A profile may name
    // it (`identity`); otherwise it is the prompt text with wrapping, box art and case removed, so
    // a dialog that re-renders at a different width keeps the id that was handed out.
    let identity = prompt
        .get("identity")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| stable_question_text(prompt.get("text").and_then(Value::as_str).unwrap_or_default()));
    // A named identity is the whole id: the choices on offer can differ between reads (a broad
    // answer is only listed when asked for) and must not change it.
    let named = prompt.get("identity").and_then(Value::as_str).is_some();
    crate::agent_runs::stable_prompt_id(&json!({
        "agent_type": prompt.get("agent_type"),
        "kind": prompt.get("kind"),
        "choices": if named { Value::Null } else { prompt.get("choices").cloned().unwrap_or(Value::Null) },
        "identity": identity,
    }))
}

/// The part of a prompt's text that is the QUESTION: rows that ask something (`?`, `[y/n]`, an anchor phrase)
/// or offer a choice (`1.`, `[a]`, `(y)`). A spinner row, an elapsed-time counter, a blinking cursor or output that
/// scrolled in below the dialog change on every redraw and must not change the id the caller was handed
/// (a trust question answered with the id just read was refused as stale). When no row looks like either, the whole
/// text is the identity. Letters and digits only, lowercase: wrapping and box art do not count.
fn stable_question_text(text: &str) -> String {
    let alnum = |line: &str| -> String { line.chars().filter(|ch| ch.is_alphanumeric()).flat_map(char::to_lowercase).collect() };
    let asks = |line: &str| {
        let lower = line.to_ascii_lowercase();
        lower.contains('?')
            || lower.contains("[y/n]")
            || lower.contains("(y/n)")
            || PROMPT_ANCHORS.iter().any(|phrase| lower.contains(phrase))
            || is_choice_row(line)
    };
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.iter().position(|line| asks(line));
    let last = lines.iter().rposition(|line| asks(line));
    match (first, last) {
        // From a few rows above the first asking row (what is being approved is usually shown there) to the last
        // asking or choice row. What is drawn below the question (a spinner, a counter, a cursor) and what scrolled
        // in above it are not the question.
        (Some(first), Some(last)) => lines[first.saturating_sub(CONTEXT_ROWS)..=last].iter().map(|line| alnum(line)).collect(),
        _ => alnum(text),
    }
}

/// Rows above the first asking row that still belong to the question's identity (the command or file being approved).
const CONTEXT_ROWS: usize = 4;

/// A row that offers an answer: `1. Yes`, `2) No`, `[a] Trust`, `(y) Run once`, `> 1. Yes`, with box art around it.
fn is_choice_row(line: &str) -> bool {
    let body = line.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '>' | '|' | '*' | '-' | '\u{276f}' | '\u{203a}' | '\u{2502}' | '\u{2551}' | '\u{25cf}' | '\u{25cb}')
    });
    let mut chars = body.chars();
    let Some(first) = chars.next() else { return false };
    match first {
        '[' | '(' => {
            let inner: String = chars.by_ref().take_while(|ch| !matches!(ch, ']' | ')')).collect();
            (1..=3).contains(&inner.chars().count()) && inner.chars().all(char::is_alphanumeric)
        }
        digit if digit.is_ascii_digit() => {
            let rest = body.trim_start_matches(|ch: char| ch.is_ascii_digit());
            rest.starts_with(['.', ')', ':']) && rest.len() > 1
        }
        _ => false,
    }
}

/// The prompt blocking `pane_id` as a run's `required_action` (with an id, kind, options and a
/// summary naming the command), or `None` when the pane shows no supported prompt. The id is the
/// same one `detect` and `answer_prompt` use.
pub fn required_action(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<Value> {
    let prompt = detect(registry, pane_id, false)?;
    Some(required_action_from_prompt(prompt))
}

pub(crate) fn required_action_from_prompt(prompt: Value) -> Value {
    let id = prompt.get("prompt_id").cloned().unwrap_or(Value::Null);
    let options: Vec<Value> = prompt
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|choice| json!({"id": choice, "label": custom_label(&prompt, choice).unwrap_or_else(|| choice.to_string())}))
        .collect();
    let summary = prompt
        .get("summary")
        .or_else(|| prompt.get("text"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut action = json!({
        "id": id,
        "prompt_id": id,
        "kind": prompt.get("kind"),
        "summary": summary,
        "options": options,
        "raw_prompt": prompt,
    });
    for key in ["command", "cwd", "risk"] {
        if let Some(value) = action["raw_prompt"].get(key).cloned() {
            action[key] = value;
        }
    }
    action
}

pub fn answer(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    prompt_id: &str,
    choice: &str,
    allow_broad: bool,
) -> Result<Value, OperationError> {
    answer_with(registry, app, pane_id, prompt_id, choice, allow_broad, None)
}

/// [`answer`], typed under a person's `AnswerPermit` when one is given (a phone card, a lock-screen
/// button): the key is refused when the pane printed since the prompt was re-read.
pub fn answer_with(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    prompt_id: &str,
    choice: &str,
    allow_broad: bool,
    permit: Option<&crate::agent_runs::readiness::AnswerPermit>,
) -> Result<Value, OperationError> {
    let agent_type = registry
        .lock()
        .panes
        .get(pane_id)
        .map(|pane| pane.info.agent_type.clone())
        .ok_or_else(|| {
            OperationError::new(
                "PANE_NOT_FOUND",
                crate::pty::registry::unknown_pane_message(pane_id),
                false,
            )
        })?;
    let prompt = detect(registry, pane_id, true).ok_or_else(|| {
        OperationError::new(
            "STALE_PROMPT",
            "this pane is not showing a supported prompt; it may already be answered (re-check with wait_for_panes)",
            false,
        )
    })?;
    if prompt.get("prompt_id").and_then(Value::as_str) != Some(prompt_id) {
        return Err(OperationError::new(
            "STALE_PROMPT",
            "the prompt on screen changed since you read it; read the new prompt before answering",
            false,
        ));
    }
    let (key, reply_choice) = reply_key(&agent_type, &prompt, choice, allow_broad)?;
    let origin = match permit {
        Some(permit) => crate::pty::WriteOrigin::Decided(permit),
        None => crate::pty::WriteOrigin::Answer,
    };
    // One key per prompt occurrence: a burst of answers to a dialog that is still drawn (a TUI
    // redraws a moment after the key) presses nothing more (`answers`).
    let slot = answers::answer_once(pane_id, prompt_id, &reply_choice, answers::now_ms(), || {
        crate::pty::registry::write_input(registry, app, pane_id, &key, true, false, None, origin)
            .map_err(|error| OperationError::new("PROMPT_REPLY_FAILED", error, true))
    })?;
    if let answers::Slot::Duplicate(first) = slot {
        return Ok(json!({
            "pane_id": pane_id,
            "prompt_id": prompt_id,
            "kind": prompt["kind"],
            "choice": first.choice,
            "already_applied": true,
            "answered_at_ms": first.at_ms,
        }));
    }
    note_gate_answer(pane_id, &prompt, &reply_choice);
    Ok(
        json!({"pane_id": pane_id, "prompt_id": prompt_id, "kind": prompt["kind"], "choice": reply_choice, "sent_key": key}),
    )
}

/// Whether an answer route's reply says the answer was a repeat (nothing was sent this time): the
/// bare-pane reply's `already_applied`, or a run receipt's `answer.already_applied`.
pub fn reply_was_repeat(reply: &Value) -> bool {
    reply.get("already_applied").and_then(Value::as_bool) == Some(true)
        || reply.pointer("/answer/already_applied").and_then(Value::as_bool) == Some(true)
}

/// Map a caller's choice to the key the agent expects. Broad approvals need `allow_broad`.
fn reply_key(
    agent_type: &str,
    prompt: &Value,
    choice: &str,
    allow_broad: bool,
) -> Result<(String, String), OperationError> {
    if matches!(choice, "allow_all" | "allow_broad" | "always") && !allow_broad {
        return Err(OperationError::new(
            "BROAD_APPROVAL_REQUIRES_OPT_IN",
            "allow_all requires allow_broad=true",
            false,
        ));
    }
    let unknown = prompt.get("kind").and_then(Value::as_str) == Some("unknown_question");
    let choice = match choice {
        // A question read by its shape has no "allow_once": its options are the screen's own, and a broad one is
        // refused inside `reply_for` unless `allow_broad` was given.
        other if unknown => other,
        "once" => "allow_once",
        "reject" => "deny",
        "always" | "allow_broad" => "allow_all",
        other => other,
    };
    let reply = reply_for(agent_type, prompt, choice, allow_broad)
        .map_err(|error| OperationError::new("INVALID_PROMPT_CHOICE", error, false))?;
    let key = reply.key.or(reply.text).ok_or_else(|| {
        OperationError::new(
            "INVALID_PROMPT_CHOICE",
            "adapter did not produce a reply",
            false,
        )
    })?;
    Ok((key, choice.to_string()))
}


/// A pane prompt as the desktop UI shows it (`pane_prompt` command, `pty://prompt` event).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanePrompt {
    pub prompt_id: String,
    pub kind: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub choices: Vec<PromptChoice>,
    /// What saying yes would do and how risky it is. Optional so older UIs keep working.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<crate::prompt_risk::Risk>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PromptChoice {
    pub id: String,
    pub label: String,
    /// "Don't ask again" / "allow all" style answer: the UI keeps it out of the way on risky prompts.
    pub broad: bool,
}

/// A label the profile gave a choice (`labels`: "Don't trust" for `deny` on a trust dialog).
fn custom_label(prompt: &Value, id: &str) -> Option<String> {
    prompt.get("labels")?.get(id)?.as_str().map(str::to_string)
}

fn choice_label(id: &str) -> String {
    let spaced = id.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

impl PanePrompt {
    /// Shape a detected prompt (`detect` / `detect_in_screen`) for the UI; `None` without an id.
    pub fn from_detected(prompt: &Value) -> Option<Self> {
        let text_of = |key: &str| prompt.get(key).and_then(Value::as_str).map(str::to_string);
        let choices = prompt
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|id| {
                let label = custom_label(prompt, id).unwrap_or_else(|| choice_label(id));
                PromptChoice { broad: crate::prompt_risk::choice_is_broad(id, &label), id: id.to_string(), label }
            })
            .collect();
        Some(Self {
            prompt_id: text_of("prompt_id")?,
            kind: text_of("kind").unwrap_or_else(|| "unknown".into()),
            text: text_of("text").unwrap_or_default(),
            summary: text_of("summary"),
            choices,
            risk: prompt.get("risk").and_then(|v| serde_json::from_value(v.clone()).ok()),
        })
    }
}

/// The prompt a pane shows right now, shaped for the UI. Never offers broad approvals.
pub fn detect_for_ui(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<PanePrompt> {
    PanePrompt::from_detected(&detect(registry, pane_id, false)?)
}

/// Per-pane memory behind `pty://prompt`: whether the pane was waiting and the last prompt id sent.
#[derive(Debug, Default)]
pub struct PromptTrack {
    waiting: bool,
    last_id: Option<String>,
}

/// What to emit on `pty://prompt` after a screen update: `Some(prompt)` is an event to send
/// (`Some(None)` = prompt cleared), `None` = nothing to say. Emits on entering `waiting_input`
/// (even with no parsed prompt), on leaving it, and when the detected prompt changes. `detect` is
/// only called while waiting.
pub fn next_prompt_event(
    track: &mut PromptTrack,
    waiting: bool,
    detect: impl FnOnce() -> Option<PanePrompt>,
) -> Option<Option<PanePrompt>> {
    if !waiting {
        let was = std::mem::take(&mut track.waiting);
        track.last_id = None;
        return was.then_some(None);
    }
    let prompt = detect();
    let id = prompt.as_ref().map(|p| p.prompt_id.clone());
    let entered = !std::mem::replace(&mut track.waiting, true);
    if entered || id != track.last_id {
        track.last_id = id;
        return Some(prompt);
    }
    None
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod claude_tests;
