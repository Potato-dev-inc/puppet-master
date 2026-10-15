//! Argv templating (FMT-3/FMT-4, HDL-1, HDL-7). The program is resolved elsewhere; this renders
//! only the argument list. The task is always exactly one argv element and is never re-scanned for
//! placeholders, so hostile task text cannot add, drop or change other elements.

use crate::tui::types::HeadlessLaunch;

use super::schema::{Definition, HeadlessTable, OptionalSegment, TuiTable};

/// Values a placeholder can take. `None` means the run has no such value.
struct Values<'a> {
    task: Option<&'a str>,
    project: Option<&'a str>,
    model: Option<&'a str>,
    session: Option<&'a str>,
}

impl<'a> Values<'a> {
    fn of(launch: &HeadlessLaunch<'a>) -> Self {
        Self {
            task: Some(launch.task),
            project: Some(launch.project),
            model: launch.model,
            session: launch.resume_session,
        }
    }

    fn get(&self, name: &str) -> Option<Option<&'a str>> {
        match name {
            "task" => Some(self.task),
            "project" => Some(self.project),
            "model" => Some(self.model),
            "session" => Some(self.session),
            // `{reasoning}` is in the closed set but no launch carries one yet: always absent.
            "reasoning" => Some(None),
            _ => None,
        }
    }
}

/// Substitute `{name}` tokens of one argv element in a single left-to-right pass. Substituted text
/// is never rescanned. Returns `None` when a known placeholder has no value (the element is then
/// omitted); unknown `{...}` text stays literal (the validator rejects it at load time).
fn substitute(element: &str, values: &Values<'_>) -> Option<String> {
    let mut out = String::with_capacity(element.len());
    let mut rest = element;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let token = after.find('}').map(|close| &after[..close]).filter(|name| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic() || c == '_')
        });
        match token.and_then(|name| values.get(name).map(|v| (name, v))) {
            Some((name, value)) => {
                out.push_str(value?);
                rest = &after[name.len() + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Some(out)
}

fn segment_active(when: &str, launch: &HeadlessLaunch<'_>) -> bool {
    match when {
        "model" => launch.model.is_some(),
        "resume" => launch.resume_session.is_some(),
        "read_only" => launch.read_only,
        "not_read_only" => !launch.read_only,
        "allow_shell" => launch.allow_shell,
        "trust_workspace" => launch.trust_workspace,
        _ => false,
    }
}

fn push_all(out: &mut Vec<String>, args: &[String], values: &Values<'_>) {
    out.extend(args.iter().filter_map(|arg| substitute(arg, values)));
}

/// The headless argv after the program: base argv, then every active `[[headless.optional]]`
/// segment in declaration order, then the task (HDL-7): `task_flag` + task, nothing for stdin
/// delivery or an explicit `{task}` placeholder, else `task_separator` (if any) + task.
pub fn render(headless: &HeadlessTable, launch: &HeadlessLaunch<'_>) -> Vec<String> {
    let values = Values::of(launch);
    let mut out = Vec::new();
    push_all(&mut out, &headless.argv, &values);
    for OptionalSegment { when, args } in &headless.optional {
        if segment_active(when, launch) {
            push_all(&mut out, args, &values);
        }
    }
    let explicit_task = headless.argv.iter().any(|a| has_task_placeholder(a));
    if let Some(flag) = &headless.task_flag {
        out.push(flag.clone());
        out.push(launch.task.to_string());
    } else if headless.stdin.is_none() && !explicit_task {
        if let Some(separator) = &headless.task_separator {
            out.push(separator.clone());
        }
        out.push(launch.task.to_string());
    }
    out
}

/// `render` for a whole definition; `None` when it has no `[headless]` table.
pub fn render_headless(def: &Definition, launch: &HeadlessLaunch<'_>) -> Option<Vec<String>> {
    def.headless.as_ref().map(|h| render(h, launch))
}

/// The text written to the child's stdin (`[headless.stdin]`), if the definition delivers the task
/// that way.
pub fn render_stdin(headless: &HeadlessTable, launch: &HeadlessLaunch<'_>) -> Option<String> {
    let template = &headless.stdin.as_ref()?.template;
    substitute(template, &Values::of(launch))
}

fn has_task_placeholder(element: &str) -> bool {
    super::validate::placeholders(element).contains(&"task")
}

/// The visible-pane argv after the program (TUI-1): `[tui] argv`, active `[[tui.optional]]`
/// segments (`model`, `resume` when the value is known, `read_only`), then `read_only_argv` for a
/// read-only run. No task: a visible pane is handed its task by typing it.
pub fn render_tui(tui: &TuiTable, project: &str, read_only: bool, model: Option<&str>, session: Option<&str>) -> Vec<String> {
    let values = Values { task: None, project: Some(project), model, session };
    let mut out = Vec::new();
    push_all(&mut out, &tui.argv, &values);
    for OptionalSegment { when, args } in &tui.optional {
        let active = match when.as_str() {
            "model" => model.is_some(),
            "resume" => session.is_some(),
            "read_only" => read_only,
            _ => false,
        };
        if active {
            push_all(&mut out, args, &values);
        }
    }
    if read_only {
        if let Some(extra) = &tui.read_only_argv {
            push_all(&mut out, extra, &values);
        }
    }
    out
}
