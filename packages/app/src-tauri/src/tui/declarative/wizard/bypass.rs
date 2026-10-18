//! The deny list of the autonomous wizard ("Finish without me", founder decision 2026-10-18).
//!
//! With the switch on, nobody reads the planned commands before they run. So Rust refuses, before any test
//! pane or one-task run is started, an argv, environment variable or value that turns off an agent's own
//! safety questions: `--yolo`, `--dangerously-skip-permissions`, `--permission-mode bypassPermissions`,
//! `--no-sandbox`, `--trust-all-tools`, `--auto-approve`, `--allow-all-tools`, `--always-allow`, `--force`,
//! `--ask-for-approval never`, `--sandbox danger-full-access` and their like. A refused plan goes back to the
//! drafter as feedback (`refusal_text`); a person who turns the switch off keeps today's behaviour, where
//! they read the command themselves.
//!
//! Pure text matching on normalised tokens (lower case, leading dashes cut, `_` read as `-`), so a spelling
//! change (`--Skip_Permissions`) does not slip through. Nothing here names an agent.

use crate::tui::declarative::schema::Definition;

/// One refused token and the reason in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The token as written (`--yolo`, `--approval-mode=yolo`, `YOLO_MODE=1`).
    pub token: String,
    pub why: &'static str,
}

/// Substrings of a normalised token that are refused wherever they occur (flag name or value).
const DENIED: &[(&str, &str)] = &[
    ("yolo", "it skips every confirmation"),
    ("danger", "it turns off safety checks"),
    ("bypass", "it bypasses the permission questions"),
    ("skip-permission", "it skips the permission questions"),
    ("skip-all-permission", "it skips the permission questions"),
    ("no-sandbox", "it turns the sandbox off"),
    ("trust-all", "it trusts everything without asking"),
    ("trust-every", "it trusts everything without asking"),
    ("auto-approve", "it approves every request without asking"),
    ("autoapprove", "it approves every request without asking"),
    ("approve-all", "it approves every request without asking"),
    ("allow-all", "it allows everything without asking"),
    ("always-allow", "it allows everything without asking"),
    ("always-approve", "it approves every request without asking"),
    ("full-auto", "it lets the agent act without asking"),
    ("no-confirm", "it skips the confirmations"),
    ("skip-confirm", "it skips the confirmations"),
    ("assume-yes", "it answers every question with yes"),
    ("no-approval", "it skips the approval questions"),
    ("dont-ask", "it stops the agent from asking"),
    ("dontask", "it stops the agent from asking"),
    ("unrestricted", "it removes the agent's limits"),
];

/// Flags whose name alone is refused (exact, after normalising), because a bare `--force` style switch
/// overrides a check.
const DENIED_FLAGS: &[&str] = &["force", "yes", "auto", "unsafe"];

/// A flag that takes a value, and the values of it that mean "do not ask / do not limit".
const RISKY_VALUES: &[(&[&str], &[&str])] = &[
    (&["ask-for-approval", "approval-policy", "approval-mode", "permission-mode", "permissions"], &["never", "none", "off", "disabled", "false", "auto", "yes", "all"]),
    (&["sandbox", "sandbox-mode"], &["none", "off", "disabled", "false", "danger-full-access"]),
];

/// `Force-Color` style switches that look like `force` but only change colours.
const HARMLESS_FORCE: &[&str] = &["force-color", "force-colors", "force-colour", "force-tty", "force-terminal"];

fn norm(token: &str) -> String {
    token.trim().trim_start_matches('-').to_ascii_lowercase().replace('_', "-")
}

/// Is `token` a placeholder the template fills in (`<task>`, `{project}`)?
fn is_placeholder(token: &str) -> bool {
    let t = token.trim();
    (t.starts_with('<') && t.ends_with('>')) || (t.starts_with('{') && t.ends_with('}'))
}

fn hit_for(token: &str) -> Option<&'static str> {
    if is_placeholder(token) {
        return None;
    }
    let n = norm(token);
    if n.is_empty() {
        return None;
    }
    let (name, _) = n.split_once('=').unwrap_or((n.as_str(), ""));
    if HARMLESS_FORCE.contains(&name) {
        return None;
    }
    if token.trim_start().starts_with('-') && DENIED_FLAGS.contains(&name) {
        return Some("it overrides the agent's own checks");
    }
    if name.starts_with("force-") && !HARMLESS_FORCE.contains(&name) {
        return Some("it overrides the agent's own checks");
    }
    DENIED.iter().find(|(needle, _)| n.contains(needle)).map(|(_, why)| *why)
}

/// Every refused token of an argv. Looks at each token, at the `--flag=value` form, and at a flag followed
/// by a risky value (`--ask-for-approval never`).
pub fn hits_in_argv(argv: &[String]) -> Vec<Hit> {
    let mut out: Vec<Hit> = Vec::new();
    let mut push = |token: &str, why: &'static str| {
        if !out.iter().any(|h| h.token == token) {
            out.push(Hit { token: token.to_string(), why });
        }
    };
    for (i, token) in argv.iter().enumerate() {
        if let Some(why) = hit_for(token) {
            push(token, why);
            continue;
        }
        let n = norm(token);
        let (name, inline) = match n.split_once('=') {
            Some((name, value)) => (name.to_string(), Some(value.to_string())),
            None => (n.clone(), None),
        };
        if !token.trim_start().starts_with('-') {
            continue;
        }
        for (flags, values) in RISKY_VALUES {
            if !flags.contains(&name.as_str()) {
                continue;
            }
            let value = inline.clone().or_else(|| argv.get(i + 1).filter(|v| !v.starts_with('-')).map(|v| norm(v)));
            if value.as_deref().is_some_and(|v| values.contains(&v)) {
                let shown = match &inline {
                    Some(_) => token.clone(),
                    None => format!("{token} {}", argv.get(i + 1).map(String::as_str).unwrap_or("")),
                };
                push(&shown, "it stops the agent from asking before it acts");
            }
        }
    }
    out
}

/// Refused environment entries: the name or the value says so (`YOLO_MODE=1`, `MODE=bypassPermissions`).
pub fn hits_in_env(env: &[(String, String)]) -> Vec<Hit> {
    let mut out = Vec::new();
    for (name, value) in env {
        let why = hit_for(name).or_else(|| hit_for(value));
        if let Some(why) = why {
            out.push(Hit { token: format!("{name}={value}"), why });
        }
    }
    out
}

/// Every argv and environment entry a definition could start the agent with.
pub fn hits_in_definition(def: &Definition) -> Vec<Hit> {
    let mut out: Vec<Hit> = Vec::new();
    let mut add = |hits: Vec<Hit>| {
        for h in hits {
            if !out.iter().any(|o| o.token == h.token) {
                out.push(h);
            }
        }
    };
    if let Some(h) = &def.headless {
        add(hits_in_argv(&h.argv));
        for seg in &h.optional {
            add(hits_in_argv(&seg.args));
        }
    }
    if let Some(t) = &def.tui {
        add(hits_in_argv(&t.argv));
        for seg in &t.optional {
            add(hits_in_argv(&seg.args));
        }
        if let Some(read_only) = &t.read_only_argv {
            add(hits_in_argv(read_only));
        }
    }
    let env: Vec<(String, String)> = def.env.set.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    add(hits_in_env(&env));
    out
}

/// The words the drafter gets back: "remove <flag>; the wizard never runs agents with permission bypasses".
pub fn refusal_text(hits: &[Hit]) -> String {
    let list: Vec<String> = hits.iter().map(|h| format!("`{}` ({})", h.token, h.why)).collect();
    let remove: Vec<String> = hits.iter().map(|h| h.token.clone()).collect();
    format!("remove {}; the wizard never runs agents with permission bypasses. Refused: {}.", remove.join(", "), list.join(", "))
}

/// The message of a refusal at the place a test would start (defense in depth, after `verify`).
pub fn refusal_error(hits: &[Hit]) -> super::report::WizardError {
    super::report::WizardError::new(409, "WIZARD_AUTO_BYPASS_REFUSED", refusal_text(hits))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn tokens(argv: &[&str]) -> Vec<String> {
        hits_in_argv(&v(argv)).into_iter().map(|h| h.token).collect()
    }

    #[test]
    fn bypass_style_flags_are_refused() {
        for flag in [
            "--yolo",
            "--dangerously-skip-permissions",
            "--dangerously-bypass-approvals-and-sandbox",
            "--skip-permissions",
            "--skip_permissions",
            "--Skip-Permissions",
            "--no-sandbox",
            "--trust-all-tools",
            "--auto-approve",
            "--allow-all-tools",
            "--always-allow",
            "--full-auto",
            "--force",
            "--no-confirm",
            "--approval-mode=yolo",
            "--permission-mode=bypassPermissions",
            "--sandbox=danger-full-access",
        ] {
            assert_eq!(tokens(&["chat", flag]), vec![flag.to_string()], "{flag} must be refused");
        }
    }

    #[test]
    fn a_flag_followed_by_a_value_that_turns_asking_off_is_refused() {
        assert_eq!(tokens(&["--ask-for-approval", "never"]), vec!["--ask-for-approval never".to_string()]);
        assert_eq!(tokens(&["--permission-mode", "bypassPermissions"]), vec!["bypassPermissions".to_string()]);
        assert_eq!(tokens(&["--sandbox", "none"]), vec!["--sandbox none".to_string()]);
        assert!(!tokens(&["--approval-mode", "yolo"]).is_empty());
    }

    #[test]
    fn ordinary_arguments_pass() {
        for argv in [
            vec!["chat", "--no-interactive"],
            vec!["exec", "--json", "--model", "<model>"],
            vec!["-p", "<task>", "--output-format", "stream-json"],
            vec!["--permission-mode", "default"],
            vec!["--sandbox", "workspace-write"],
            vec!["--ask-for-approval", "on-request"],
            vec!["--force-color"],
            vec!["--resume", "<session>"],
            vec![],
        ] {
            assert!(tokens(&argv).is_empty(), "{argv:?} must pass");
        }
    }

    #[test]
    fn a_placeholder_is_never_a_hit() {
        assert!(tokens(&["{task}", "<yolo>"]).is_empty());
    }

    #[test]
    fn environment_names_and_values_are_checked() {
        let hits = hits_in_env(&[("YOLO_MODE".into(), "1".into()), ("MODE".into(), "bypassPermissions".into()), ("NO_COLOR".into(), "1".into())]);
        assert_eq!(hits.iter().map(|h| h.token.as_str()).collect::<Vec<_>>(), vec!["YOLO_MODE=1", "MODE=bypassPermissions"]);
    }

    #[test]
    fn the_refusal_tells_the_drafter_what_to_remove() {
        let text = refusal_text(&hits_in_argv(&v(&["--yolo", "--no-sandbox"])));
        assert!(text.starts_with("remove --yolo, --no-sandbox; the wizard never runs agents with permission bypasses"), "{text}");
    }
}
