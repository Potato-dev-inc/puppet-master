//! Opt-in, per-run approval policy for Cursor shell prompts.
//!
//! A run may name commands (`allowed_commands`) it pre-authorises. A visible Cursor pane that stops
//! on a command approval is then answered with allow-once, and only when every command on the
//! screen is covered by the list and nothing shell-like is mixed in. Anything else stays a normal
//! `manual_input_required` stop. Never allow-always.
//!
//! `allowed_commands` entries are either
//! - a bare program name (`python`): any arguments for that program, or
//! - an argv pattern (`python todo.py add *`): same words, in order. `*` as the last word matches
//!   zero or more remaining arguments, `*` as another whole word matches exactly one argument, and
//!   `*`/`?` inside a word match within that one argument. Program names compare
//!   case-insensitively (a trailing `.exe` is ignored); arguments compare exactly.
//!
//! When the screen shows the full command line, `approved_command` judges the real command: a
//! leading `cd <dir> &&` is stripped (the directory must stay inside the project), the rest may be
//! several `&&`-joined commands and every one must match an entry. Everything else is refused:
//! `;` `||` `|` `&`, redirections, command substitution (`$`, backtick, parentheses), `%`, `^`,
//! newlines, truncated lines, `sudo`/`runas`, and recursive or forced deletes, even if listed.

pub const MAX_ALLOWED_COMMANDS: usize = 10;
const MARKER: &str = "not in allowlist:";
const MAX_PATTERN_LEN: usize = 200;
const MAX_COMMAND_LEN: usize = 1000;

fn is_program_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 64
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

/// Request-time validation: a bare program name, or an argv pattern starting with one. No paths
/// as the program and no shell syntax anywhere.
pub fn validate(allowed: &[String]) -> Result<(), String> {
    if allowed.len() > MAX_ALLOWED_COMMANDS {
        return Err(format!("at most {MAX_ALLOWED_COMMANDS} allowed_commands"));
    }
    for item in allowed {
        let item = item.trim();
        if is_program_token(item) {
            if hard_denied(&[item.to_string()]) {
                return Err(format!("allowed_commands entry `{item}` is never auto-approved"));
            }
            continue;
        }
        let bad = || {
            format!(
                "allowed_commands entries must be a bare program name (letters, digits, . _ -) or an argv pattern like `python todo.py add *` without shell syntax; got `{item}`"
            )
        };
        if item.len() > MAX_PATTERN_LEN {
            return Err(bad());
        }
        let segments = tokenize(item).ok_or_else(bad)?;
        let [argv] = segments.as_slice() else {
            return Err(bad());
        };
        if !is_program_token(&argv[0]) || hard_denied(argv) {
            return Err(bad());
        }
    }
    Ok(())
}

/// Split a command line into `&&`-separated argv lists. `None` means refuse: any syntax the
/// policy does not fully understand ends here, so it can never be judged wrongly.
fn tokenize(line: &str) -> Option<Vec<Vec<String>>> {
    let chars: Vec<char> = line.trim().chars().collect();
    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut argv: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '"' => {
                in_word = true;
                i += 1;
                loop {
                    let c = *chars.get(i)?;
                    if c == '"' {
                        break;
                    }
                    if (c == '\\' && chars.get(i + 1) == Some(&'"'))
                        || matches!(c, '$' | '`' | '%' | '\u{2026}')
                        || c.is_control()
                    {
                        return None;
                    }
                    word.push(c);
                    i += 1;
                }
            }
            '\'' => {
                // cmd.exe does not treat ' as a quote, so shell operators inside are never safe.
                in_word = true;
                i += 1;
                loop {
                    let c = *chars.get(i)?;
                    if c == '\'' {
                        break;
                    }
                    if matches!(
                        c,
                        '$' | '`' | '%' | '\u{2026}' | '&' | '|' | ';' | '<' | '>' | '(' | ')'
                            | '"' | '^'
                    ) || c.is_control()
                    {
                        return None;
                    }
                    word.push(c);
                    i += 1;
                }
            }
            '&' => {
                if chars.get(i + 1) != Some(&'&') {
                    return None;
                }
                if in_word {
                    argv.push(std::mem::take(&mut word));
                    in_word = false;
                }
                if argv.is_empty() {
                    return None;
                }
                segments.push(std::mem::take(&mut argv));
                i += 1;
            }
            ' ' | '\t' => {
                if in_word {
                    argv.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            ';' | '|' | '<' | '>' | '`' | '$' | '(' | ')' | '%' | '^' | '{' | '}'
            | '\u{2026}' => return None,
            c if c.is_control() => return None,
            c => {
                in_word = true;
                word.push(c);
            }
        }
        i += 1;
    }
    if in_word {
        argv.push(word);
    }
    if argv.is_empty() {
        return None;
    }
    segments.push(argv);
    Some(segments)
}

fn program_name(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// Programs and flags that are never auto-approved, even when an entry would match them.
fn hard_denied(argv: &[String]) -> bool {
    let Some(first) = argv.first() else {
        return true;
    };
    let program = program_name(first);
    if matches!(program.as_str(), "sudo" | "su" | "doas" | "runas" | "format" | "mkfs" | "dd") {
        return true;
    }
    let deletes = matches!(
        program.as_str(),
        "rm" | "rmdir" | "rd" | "del" | "erase" | "remove-item" | "ri"
    );
    deletes
        && argv[1..].iter().any(|arg| {
            let lower = arg.to_ascii_lowercase();
            (lower.starts_with('-') || lower.starts_with('/'))
                && lower.len() > 1
                && (lower.contains('r') || lower.contains('f') || lower.contains('s'))
        })
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn entry_matches(entry: &str, argv: &[String]) -> bool {
    let entry = entry.trim();
    if !is_program_token(&argv[0]) {
        return false;
    }
    if is_program_token(entry) {
        return program_name(entry) == program_name(&argv[0]);
    }
    let Some(segments) = tokenize(entry) else {
        return false;
    };
    let [pattern] = segments.as_slice() else {
        return false;
    };
    if program_name(&pattern[0]) != program_name(&argv[0]) {
        return false;
    }
    let open_tail = pattern.len() > 1 && pattern.last().map(String::as_str) == Some("*");
    let fixed = if open_tail {
        &pattern[1..pattern.len() - 1]
    } else {
        &pattern[1..]
    };
    let args = &argv[1..];
    let length_ok = if open_tail {
        args.len() >= fixed.len()
    } else {
        args.len() == fixed.len()
    };
    length_ok && fixed.iter().zip(args).all(|(p, a)| glob_match(p, a))
}

fn norm_path(path: &str) -> String {
    let mut text = path.replace('/', "\\");
    if let Some(rest) = text.strip_prefix("\\\\?\\") {
        text = rest.to_string();
    }
    text.trim_end_matches('\\').to_ascii_lowercase()
}

/// `cd <dir>` / `cd /d <dir>` target, when the argv is such a command.
fn cd_target(argv: &[String]) -> Option<&str> {
    if !argv[0].eq_ignore_ascii_case("cd") && !argv[0].eq_ignore_ascii_case("chdir") {
        return None;
    }
    match &argv[1..] {
        [dir] => Some(dir),
        [flag, dir] if flag.eq_ignore_ascii_case("/d") => Some(dir),
        _ => None,
    }
}

fn dir_inside_project(dir: &str, project: &str) -> bool {
    let d = norm_path(dir);
    let p = norm_path(project);
    if d.is_empty()
        || d.split('\\').any(|part| part == "..")
        || d.starts_with('-')
        || d.starts_with('~')
    {
        return false;
    }
    let absolute = d.starts_with('\\') || d.chars().nth(1) == Some(':');
    if !absolute {
        return true;
    }
    !p.is_empty() && (d == p || d.starts_with(&format!("{p}\\")))
}

/// The directory a leading `cd <dir> &&` moves into, if the command starts with one.
pub fn leading_cd_dir(command: &str) -> Option<String> {
    let segments = tokenize(command)?;
    (segments.len() > 1)
        .then(|| cd_target(&segments[0]).map(str::to_string))
        .flatten()
}

/// Whether the whole command line is covered by the run's allow-list. Returns the commands that
/// were judged (for the transcript), after stripping a leading `cd <dir> &&`.
pub fn approved_command(command: &str, allowed: &[String], project: Option<&str>) -> Option<String> {
    if allowed.is_empty() || command.len() > MAX_COMMAND_LEN {
        return None;
    }
    let mut commands = tokenize(command)?;
    if let Some(dir) = cd_target(&commands[0]) {
        if let Some(project) = project {
            if !dir_inside_project(dir, project) {
                return None;
            }
        }
        commands.remove(0);
    }
    if commands.is_empty() {
        return None;
    }
    let all_allowed = commands
        .iter()
        .all(|argv| !hard_denied(argv) && allowed.iter().any(|entry| entry_matches(entry, argv)));
    all_allowed.then(|| {
        commands
            .iter()
            .map(|argv| argv.join(" "))
            .collect::<Vec<_>>()
            .join(" && ")
    })
}

/// What to allow-once for a detected prompt, if the run's policy covers it: the judged command
/// when the prompt carries the command read off the screen (and its option list is drawn),
/// otherwise the program from Cursor's `Not in allowlist:` line.
pub fn decide(
    raw_prompt: &serde_json::Value,
    screen: &str,
    allowed: &[String],
    project: Option<&str>,
) -> Option<String> {
    if raw_prompt.get("kind").and_then(serde_json::Value::as_str) != Some("command_approval") {
        return None;
    }
    match raw_prompt.get("command").and_then(serde_json::Value::as_str) {
        Some(command) => {
            if raw_prompt.get("fully_drawn").and_then(serde_json::Value::as_bool) != Some(true) {
                return None;
            }
            approved_command(command, allowed, project)
        }
        None => approved_program(screen, allowed),
    }
}

/// The program to allow-once for this screen, if the policy covers it. Used only when the screen
/// did not show a full command line: Cursor's `Not in allowlist: <program>` line.
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
    if hard_denied(&[program.to_string()]) {
        return None;
    }
    // Only bare names can be judged from the program alone; a pattern needs the full argv.
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

    const PROJECT: &str = r"C:\x";

    fn ok(command: &str, items: &[&str]) -> bool {
        approved_command(command, &allow(items), Some(PROJECT)).is_some()
    }

    #[test]
    fn pattern_entries_validate() {
        assert!(validate(&allow(&["python todo.py add *", "git status", "python -m pytest -q *"])).is_ok());
        for bad in [
            "python todo.py add x && rm -rf /",
            "python x > out.txt",
            "python $(whoami)",
            "sudo ls",
            "rm -rf build",
            "./python x.py",
            "python x; ls",
        ] {
            assert!(validate(&allow(&[bad])).is_err(), "must reject entry {bad:?}");
        }
    }

    #[test]
    fn cd_prefix_is_stripped_before_judging() {
        let list = ["python todo.py add *"];
        assert!(ok(r#"cd "C:\x" && python todo.py add "buy milk""#, &list));
        assert!(ok(r#"cd /d "C:\x\sub" && python todo.py add a b"#, &list));
        assert!(ok("python todo.py add x", &list));
        assert!(!ok(r#"cd "C:\x""#, &list), "cd alone is not the real command");
        assert!(!ok(r#"cd "C:\other" && python todo.py add x"#, &list));
        assert!(!ok(r#"cd "C:\x\..\other" && python todo.py add x"#, &list));
        assert!(!ok(r#"cd "C:\xylophone" && python todo.py add x"#, &list));
        assert_eq!(
            leading_cd_dir(r#"cd "C:\x" && python a.py"#).as_deref(),
            Some(r"C:\x")
        );
        assert!(leading_cd_dir("python a.py").is_none());
    }

    #[test]
    fn glob_semantics() {
        assert!(ok("python todo.py add", &["python todo.py add *"]));
        assert!(ok("python todo.py add a b c", &["python todo.py add *"]));
        assert!(!ok("python todo.py list", &["python todo.py add *"]));
        assert!(!ok("python todo.py add x", &["python todo.py"]));
        assert!(ok("python todo.py", &["python todo.py"]));
        assert!(ok("python -m pytest tests/test_a.py", &["python -m pytest tests/*"]));
        assert!(!ok("python -m pytest other/test_a.py", &["python -m pytest tests/*"]));
        assert!(ok("Python.exe a.py 1", &["python a.py *"]));
        assert!(ok("git status", &["git"]));
        assert!(ok("git push --force", &["git"]), "a bare name allows any arguments");
    }

    #[test]
    fn chained_segments_must_all_match() {
        let list = ["python todo.py add *"];
        assert!(!ok("python todo.py add x && rm -rf /", &list));
        assert!(!ok(r#"cd "C:\x" && python todo.py add x && rm -rf /"#, &list));
        assert!(ok("python todo.py add x && python todo.py add y", &list));
        assert!(!ok("python todo.py add x && python evil.py", &list));
        assert!(ok("python a.py && git status", &["python a.py", "git"]));
    }

    #[test]
    fn shell_syntax_is_refused() {
        let list = ["python todo.py add *", "python", "git", "rm"];
        for bad in [
            "python todo.py add x; rm -rf /",
            "python todo.py add x || rm y",
            "python todo.py add x | tee log",
            "python todo.py add x > ..\\out.txt",
            "python todo.py add x >> out.txt",
            "python todo.py add x < in.txt",
            "python todo.py add x &",
            "python todo.py add $(whoami)",
            "python todo.py add `whoami`",
            "python todo.py add \"$HOME\"",
            "python todo.py add \"%PATH%\"",
            "python todo.py add 'a && rm -rf /'",
            "python todo.py add x\nrm -rf /",
            "python todo.py add \"unterminated",
            "python todo.py add \"a\\\" && rm x\"",
            "python todo.py add x^&rm",
            "python todo.py add (a)",
            "python todo.py add trunc\u{2026}",
            "sudo python x.py",
            "runas /user:admin python x.py",
            "rm -rf build",
            "rm -r build",
            "rm -fr build",
            "git status && rm /s x",
            "./python x.py",
            "C:\\evil\\python.exe todo.py add x",
        ] {
            assert!(!ok(bad, &list), "must refuse: {bad:?}");
        }
        assert!(!ok("", &list));
        assert!(!ok("   ", &list));
        assert!(!ok("&& python x", &list));
        assert!(!ok("python x.py", &[]));
    }

    #[test]
    fn operators_inside_double_quotes_are_literal() {
        assert!(ok(
            r#"python todo.py add "buy milk && eggs; today""#,
            &["python todo.py add *"]
        ));
    }

    #[test]
    fn without_a_project_the_cd_target_is_not_checked() {
        let list = allow(&["python todo.py add *"]);
        assert!(approved_command(r#"cd "C:\anywhere" && python todo.py add x"#, &list, None).is_some());
    }

    #[test]
    fn marker_path_never_approves_hard_denied_programs() {
        assert!(approved_program("Not in allowlist: sudo", &allow(&["sudo"])).is_none());
    }
}
