//! Opt-in, per-run approval policy for Cursor shell prompts.
//!
//! A run may name programs (`allowed_commands`) it pre-authorises. A visible Cursor pane that stops
//! on `Not in allowlist: <program>` is then answered with allow-once, and only when the screen
//! names exactly one program, that program is on the run's list, and nothing shell-like follows it.
//! Anything else stays a normal `manual_input_required` stop. Never allow-always.

pub const MAX_ALLOWED_COMMANDS: usize = 10;
const MARKER: &str = "not in allowlist:";

fn is_program_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 64
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

/// Request-time validation: bare program names only, no paths or shell syntax.
pub fn validate(allowed: &[String]) -> Result<(), String> {
    if allowed.len() > MAX_ALLOWED_COMMANDS {
        return Err(format!("at most {MAX_ALLOWED_COMMANDS} allowed_commands"));
    }
    match allowed.iter().find(|item| !is_program_token(item.trim())) {
        Some(bad) => Err(format!(
            "allowed_commands entries must be bare program names (letters, digits, . _ -); got `{bad}`"
        )),
        None => Ok(()),
    }
}

/// The program to allow-once for this screen, if the policy covers it.
pub fn approved_program(screen: &str, allowed: &[String]) -> Option<String> {
    if allowed.is_empty() {
        return None;
    }
    let lower = screen.to_ascii_lowercase();
    let mut hits = lower.match_indices(MARKER);
    let (start, _) = hits.next()?;
    if hits.next().is_some() {
        return None;
    }
    let rest = screen[start + MARKER.len()..].lines().next()?.trim();
    let mut words = rest.split_whitespace();
    let program = words
        .next()?
        .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`');
    if !is_program_token(program) {
        return None;
    }
    let tail = words.collect::<Vec<_>>().join(" ");
    if tail
        .chars()
        .any(|ch| matches!(ch, ';' | '&' | '|' | '<' | '>' | '`' | '$' | '(' | ')'))
    {
        return None;
    }
    allowed
        .iter()
        .any(|item| item.trim().eq_ignore_ascii_case(program))
        .then(|| program.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn approves_only_an_exact_listed_program() {
        let screen = "Run this command?\nNot in allowlist: python\nAllow once (y)";
        assert_eq!(approved_program(screen, &allow(&["python"])).as_deref(), Some("python"));
        assert_eq!(approved_program(screen, &allow(&["Python"])).as_deref(), Some("python"));
        assert!(approved_program(screen, &allow(&["pytest"])).is_none());
        assert!(approved_program(screen, &[]).is_none());
    }

    #[test]
    fn refuses_ambiguous_or_shell_like_prompts() {
        let two = "Not in allowlist: python\nNot in allowlist: rm";
        assert!(approved_program(two, &allow(&["python", "rm"])).is_none());
        let chained = "Not in allowlist: python; rm -rf x";
        assert!(approved_program(chained, &allow(&["python"])).is_none());
        let pathy = "Not in allowlist: ./python";
        assert!(approved_program(pathy, &allow(&["python"])).is_none());
        assert!(approved_program("nothing to see", &allow(&["python"])).is_none());
    }

    #[test]
    fn validation_accepts_bare_names_only() {
        assert!(validate(&allow(&["python", "git", "cargo"])).is_ok());
        assert!(validate(&allow(&["rm -rf"])).is_err());
        assert!(validate(&allow(&["/bin/sh"])).is_err());
        assert!(validate(&allow(&[""])).is_err());
        assert!(validate(&vec!["a".to_string(); 11]).is_err());
    }
}
