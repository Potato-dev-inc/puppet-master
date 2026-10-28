//! Session ids that may reach a command line. An id comes from a file name, a pointer or a database, so it is
//! checked before it is stored or used: Windows launches go through `cmd /C` or a PowerShell string.

/// The one placeholder a `resume_args` template may use.
const SESSION_PLACEHOLDER: &str = "{session}";

/// True for ids of 8 to 80 characters, each a letter, a digit, `_` or `-`. Nothing else.
pub fn valid_session_id(id: &str) -> bool {
    (8..=80).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Fills `{session}` in a resume template. `None` when the id is not valid, when the template has no `{session}`,
/// or when it has any other brace (an unknown placeholder).
pub fn resume_args_from(template: &[String], session: &str) -> Option<Vec<String>> {
    if !valid_session_id(session) || !template.iter().any(|arg| arg.contains(SESSION_PLACEHOLDER)) {
        return None;
    }
    let args: Vec<String> = template.iter().map(|arg| arg.replace(SESSION_PLACEHOLDER, session)).collect();
    // The id has no braces (checked above), so any brace left is a placeholder this code does not know.
    let known = args.iter().all(|arg| !arg.contains('{') && !arg.contains('}'));
    known.then_some(args)
}
