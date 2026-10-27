//! Choosing Claude Code's session id at launch (`claude --session-id <uuid>`), so a pane's session file is
//! `<project folder>/<id>.jsonl` from the first second, even with several panes in one folder.

use crate::tui::LaunchSession;

/// Arguments that resume, continue or fork a session whose id this app cannot know before the CLI starts.
const OPEN_FLAGS: [&str; 6] = ["--continue", "-c", "--fork-session", "--from-pr", "--teleport", "--remote"];

pub(super) fn launch_session(user_args: &[String], fresh_id: &str) -> LaunchSession {
    let mut named: Option<String> = None;
    let mut args = user_args.iter().map(String::as_str).peekable();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (arg, None),
        };
        match flag {
            "--session-id" | "--resume" | "-r" => {
                let value = inline.or_else(|| args.next_if(|next| !next.starts_with('-')));
                match value {
                    Some(value) if looks_like_id(value) => named = Some(value.to_string()),
                    // `--resume` alone opens a picker; `--resume <words>` searches by title: not known up front.
                    _ => return LaunchSession::Unmanaged,
                }
            }
            flag if OPEN_FLAGS.contains(&flag) => return LaunchSession::Unmanaged,
            _ => {}
        }
    }
    match named {
        Some(id) => LaunchSession::Named(id),
        None => LaunchSession::Chosen { id: fresh_id.to_string(), args: vec!["--session-id".into(), fresh_id.to_string()] },
    }
}

/// A session id is a UUID: 36 characters, hex digits and dashes.
fn looks_like_id(value: &str) -> bool {
    value.len() == 36 && value.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRESH: &str = "11111111-2222-4333-8444-555555555555";
    const OTHER: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn plan(args: &[&str]) -> LaunchSession {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        launch_session(&args, FRESH)
    }

    #[test]
    fn a_plain_launch_gets_a_fresh_id() {
        assert_eq!(
            plan(&[]),
            LaunchSession::Chosen { id: FRESH.into(), args: vec!["--session-id".into(), FRESH.into()] }
        );
        assert!(matches!(plan(&["--model", "opus", "--permission-mode", "plan"]), LaunchSession::Chosen { .. }));
    }

    #[test]
    fn resume_with_an_id_names_that_session() {
        assert_eq!(plan(&["--resume", OTHER]), LaunchSession::Named(OTHER.into()));
        assert_eq!(plan(&["-r", OTHER, "--model", "opus"]), LaunchSession::Named(OTHER.into()));
        assert_eq!(plan(&[&format!("--resume={OTHER}")]), LaunchSession::Named(OTHER.into()));
    }

    #[test]
    fn a_session_id_the_person_gave_is_kept() {
        assert_eq!(plan(&["--session-id", OTHER]), LaunchSession::Named(OTHER.into()));
    }

    #[test]
    fn resume_without_an_id_or_with_words_leaves_it_open() {
        assert_eq!(plan(&["--resume"]), LaunchSession::Unmanaged);
        assert_eq!(plan(&["-r", "--model", "opus"]), LaunchSession::Unmanaged);
        assert_eq!(plan(&["--resume", "fix the login bug"]), LaunchSession::Unmanaged);
    }

    #[test]
    fn continue_and_fork_leave_it_open() {
        for flag in ["--continue", "-c", "--fork-session", "--from-pr", "--teleport"] {
            assert_eq!(plan(&[flag]), LaunchSession::Unmanaged, "{flag}");
        }
        // A fork of a named session gets a new id nobody knows yet.
        assert_eq!(plan(&["--resume", OTHER, "--fork-session"]), LaunchSession::Unmanaged);
    }
}
