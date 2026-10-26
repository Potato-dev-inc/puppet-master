//! Where the wizard's own panes may reach (SPEC 11.16, the founder's rule). The wizard answers a trust or one-time
//! approval question ONLY in a pane whose folder is its own isolated temporary folder, and only while the question stays
//! inside that folder. A question that names a place outside it, says the access is outside the workspace, or asks to
//! inspect or control other processes or the machine is never answered "yes": the answerer picks the plain "No" row, or,
//! when there is none it can find without a doubt, leaves the question to the person.
//!
//! Pure and generic (nothing here names an agent or one agent's wording): the text of the question's own region of the
//! screen (not the transcript above it) goes in, the first thing that reaches outside comes out.

use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

use super::menu::{normalise_row, Menu, MenuKind};
use super::screen::rows;

/// How many rows above the first option the question may reach (the command it asks about, the paths).
const REGION_ROWS: usize = 14;

/// The rows of a question.
#[derive(Debug, Clone, Default)]
pub struct Region {
    /// What the question says and asks about: the rows above its options. Paths are looked for here (the broad options
    /// repeat the command, cut and wrapped at the edge of the pane, so a half path in them says nothing).
    pub above: String,
    /// The same and the options and the footer: wording ("non-workspace access") and verbs are looked for everywhere.
    pub all: String,
}

/// The rows of the question: from the nearest rule above its options (the transcript of earlier turns is above it), at
/// least the two rows above the options (the question and what it asks about), to the end of the screen.
pub fn question_region(screen: &str, menu: Option<&Menu>) -> Region {
    let all = rows(screen);
    if all.is_empty() {
        return Region::default();
    }
    let first_option = menu.filter(|m| m.kind != MenuKind::YesNo).and_then(|m| m.options.first()).and_then(|first| {
        all.iter().rposition(|row| {
            let row = normalise_row(row);
            row.trim_end().ends_with(first.text.as_str()) || (first.text.chars().count() > 30 && row.contains(first.text.as_str()))
        })
    });
    let anchor = first_option.unwrap_or(all.len().saturating_sub(1));
    let floor = anchor.saturating_sub(REGION_ROWS);
    let mut start = floor;
    let mut collected = 0usize;
    for index in (floor..anchor).rev() {
        if is_rule(all[index]) {
            if collected >= 2 {
                start = index + 1;
                break;
            }
            continue;
        }
        collected += 1;
    }
    let above_end = if first_option.is_some() { anchor } else { all.len() };
    Region { above: all[start..above_end.max(start)].join("\n"), all: all[start..].join("\n") }
}

/// A full-width rule or box edge (`────────`, `╭────╮`, `+-----+`).
fn is_rule(row: &str) -> bool {
    let t = row.trim();
    t.chars().count() >= 8 && t.chars().all(|c| c.is_whitespace() || ('\u{2500}'..='\u{257F}').contains(&c) || matches!(c, '-' | '=' | '_' | '~' | '+' | '*'))
}

// ---- what reaches outside ---------------------------------------------------------------------------

/// Commands that inspect or control other processes or the machine (generic shell verbs, any shell).
static MACHINE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?imx)
        \b(?:get|stop|start|restart|wait|debug)-process\b
        | \b(?:get|stop|start|restart|set)-service\b
        | \b(?:get-ciminstance|get-wmiobject|invoke-cimmethod|invoke-wmimethod|get-counter|get-netTCPConnection|get-nettcpconnection)\b
        | \bwin32_(?:process|service|operatingsystem|computersystem)\b
        | \bwmic\b
        | \btasklist\b | \btaskkill\b | \bpkill\b | \bkillall\b | \bpgrep\b | \bnetstat\b | \bsysteminfo\b | \bschtasks\b
        | \bstop-computer\b | \brestart-computer\b | \bshutdown(?:\.exe)?\s+[/-]
        | \bkill\s+(?:-\S+\s+)*(?:\d+|\$\w+|%\w+%)
        | (?:^|[\s|;&(])ps\s+(?:-[a-z]+|aux|ax|ef)\b
        | (?:^|[|;&]\s*)ps\s*(?:$|\|)
        | \breg(?:\.exe)?\s+(?:add|delete|query|import|export)\b
        | \bsc(?:\.exe)?\s+(?:stop|start|delete|query|config|create)\b
        | \bnet\s+(?:user|localgroup|stop|start|use)\b
        | \bset-executionpolicy\b
        ",
    )
    .expect("machine regex")
});

/// The question says the access is outside the workspace / project / folder.
static OUTSIDE_WORDS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?ix)
        non[-\s]?workspace
        | \boutside\s+(?:of\s+)?(?:the\s+|your\s+|this\s+|current\s+|allowed\s+|working\s+)*(?:workspace|project|folder|director\w+|repo\w*|sandbox|root|work(?:ing)?\s+dir\w*)
        | \bbeyond\s+(?:the\s+|your\s+|this\s+)*(?:workspace|project|working)
        | \bnot\s+(?:in|inside|within)\s+(?:the\s+|your\s+|this\s+)*(?:workspace|project|current\s+(?:folder|director\w+))
        | \b(?:other|external|parent)\s+(?:folders|directories|locations)\b
        ",
    )
    .expect("outside words regex")
});

/// Bounds of one path-like token: whitespace, quotes and the punctuation a shell or a sentence puts around a path.
fn is_cut(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|' | '*' | '?' | '(' | ')' | '=' | ',' | ';' | '[' | ']' | '{' | '}')
}

fn norm(path: &str) -> String {
    let mut s = path.trim().replace('\\', "/").to_lowercase();
    for prefix in ["//?/unc/", "//?/", "file:///", "file://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = if prefix == "//?/unc/" { format!("//{rest}") } else { rest.to_string() };
            break;
        }
    }
    // a UNC path keeps its two leading slashes, every other doubled slash is one
    let unc = s.starts_with("//") && s[2..].chars().next().is_some_and(|c| c.is_alphanumeric());
    let body = if unc { &s[2..] } else { &s[..] };
    let mut collapsed = String::with_capacity(body.len());
    for c in body.chars() {
        if c == '/' && collapsed.ends_with('/') {
            continue;
        }
        collapsed.push(c);
    }
    let mut s = if unc { format!("//{collapsed}") } else { collapsed };
    for prefix in ["/private/var/", "/private/tmp/", "/private/etc/"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = format!("/{}/{rest}", &prefix["/private/".len()..prefix.len() - 1]);
            break;
        }
    }
    s.trim_end_matches('/').to_string()
}

/// `/c/Users/x` (a POSIX shell on Windows) and `/mnt/c/Users/x` (WSL) are `c:/Users/x`.
fn drive_form(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/mnt/").or_else(|| path.strip_prefix('/'))?;
    let mut parts = rest.splitn(2, '/');
    let drive = parts.next()?;
    if drive.len() == 1 && drive.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some(format!("{drive}:/{}", parts.next().unwrap_or("")));
    }
    None
}

/// The components of a normalised path with `.` and `..` resolved. `None` when a `..` climbs above its root.
fn resolve(path: &str) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            other => out.push(other.to_string()),
        }
    }
    Some(out)
}

fn home() -> Option<String> {
    ["USERPROFILE", "HOME"].iter().find_map(|name| std::env::var(name).ok()).filter(|h| !h.trim().is_empty())
}

fn env_dir(var: &str) -> Option<String> {
    let name = var.trim_matches(|c: char| matches!(c, '%' | '$' | '{' | '}')).trim_start_matches("env:").to_ascii_uppercase();
    match name.as_str() {
        "USERPROFILE" | "HOME" | "HOMEPATH" => home(),
        "TEMP" | "TMP" | "TMPDIR" => std::env::var(&name).ok().filter(|v| !v.trim().is_empty()),
        _ => None,
    }
}

/// A relative path (`a/../../b`) that goes above where it starts.
fn climbs_out(path: &str) -> bool {
    let mut depth = 0i32;
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => depth -= 1,
            _ => depth += 1,
        }
        if depth < 0 {
            return true;
        }
    }
    false
}

/// `path` (a normalised absolute path) is `folder` or below it. `rest` is the text of the row from where the path starts,
/// normalised the same way: a folder with a space in its name is not cut at the space.
fn inside(path: &str, rest: &str, folder: &str) -> bool {
    if folder.is_empty() {
        return false;
    }
    if let Some(after) = rest.strip_prefix(folder) {
        if after.is_empty() || after.starts_with('/') || after.starts_with(|c: char| is_cut(c)) {
            let tail: String = after.chars().take_while(|c| !is_cut(*c)).collect();
            // what follows the folder must not climb out of it
            return !climbs_out(&tail);
        }
    }
    let (Some(p), Some(f)) = (resolve(path), resolve(folder)) else { return false };
    !f.is_empty() && p.len() >= f.len() && p[..f.len()] == f[..]
}

/// A path token that reaches outside `folder`, as the screen shows it; `None` when it stays inside (or is not a path).
fn token_outside(token: &str, rest: &str, folder: Option<&str>) -> Option<String> {
    let shown = |t: &str| t.trim_end_matches(['.', ':']).to_string();
    if token.is_empty() {
        return None;
    }
    let folder_n = folder.map(norm);
    let check_abs = |abs: &str, rest: &str| -> Option<String> {
        let n = norm(abs);
        match folder_n.as_deref() {
            Some(f) if inside(&n, &norm(rest), f) => None,
            _ => Some(shown(token)),
        }
    };
    let slashed = token.replace('\\', "/");
    let lower = slashed.to_lowercase();
    // a drive path (`C:/x`, `\\?\C:\x`, `file:///C:/x`)
    let body = lower.strip_prefix("//?/").or_else(|| lower.strip_prefix("file:///")).unwrap_or(&lower);
    let bytes = body.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        return check_abs(body, rest);
    }
    // a UNC path
    if lower.starts_with("//") && lower[2..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '.') {
        return check_abs(&lower, rest);
    }
    // home and environment variables
    let first = slashed.split('/').next().unwrap_or("");
    let upper = first.to_ascii_uppercase();
    let is_env = (first.len() > 2 && first.starts_with('%') && first.ends_with('%'))
        || upper.starts_with("$ENV:")
        || ["$HOME", "${HOME}", "$USERPROFILE", "$TMPDIR", "$TEMP", "$TMP"].contains(&upper.as_str());
    if first == "~" || is_env {
        let tail = slashed[first.len()..].trim_start_matches('/');
        let base = if first == "~" { home() } else { env_dir(first) };
        if let Some(base) = base {
            let abs = if tail.is_empty() { base.clone() } else { format!("{}/{tail}", base.trim_end_matches(['/', '\\'])) };
            return check_abs(&abs, &abs);
        }
        // the folder as `~/…` shows it when the home folder is not known here: only the tail of the real folder
        if first == "~" && !tail.is_empty() {
            if let Some(f) = folder_n.as_deref() {
                let n = norm(tail);
                if resolve(&n).is_some() && (f.ends_with(&format!("/{n}")) || f.contains(&format!("/{n}/"))) {
                    return None;
                }
            }
        }
        return Some(shown(token));
    }
    // a POSIX absolute path: `/usr/bin/x`, `/c/Users/x`; a lone `/word` is a slash command, not a path
    if slashed.starts_with('/') && !slashed.starts_with("//") {
        const ROOTS: [&str; 22] = [
            "home", "usr", "etc", "var", "tmp", "opt", "mnt", "root", "bin", "sbin", "proc", "sys", "dev", "users", "private", "volumes", "library",
            "applications", "windows", "programdata", "srv", "run",
        ];
        let components = slashed.split('/').filter(|c| !c.is_empty()).count();
        let root = slashed.trim_start_matches('/').split('/').next().unwrap_or("").to_ascii_lowercase();
        if components >= 2 || ROOTS.contains(&root.as_str()) {
            if let Some(drive) = drive_form(&norm(&slashed)) {
                return check_abs(&drive, &drive);
            }
            return check_abs(&slashed, rest);
        }
        return None;
    }
    // a relative path that climbs out of the folder it is in
    if slashed.split('/').any(|part| part == "..") && climbs_out(&slashed) {
        return Some(shown(token));
    }
    None
}

/// The first place in `region` that is outside `folder` (the wizard's own isolated folder; `None` = unknown, so every
/// absolute path is outside), as the screen shows it.
pub fn path_outside(region: &str, folder: Option<&Path>) -> Option<String> {
    let folder = folder.map(|f| f.to_string_lossy().to_string());
    let lines: Vec<&str> = region.lines().collect();
    for (at, line) in lines.iter().enumerate() {
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            if is_cut(chars[i].1) {
                i += 1;
                continue;
            }
            let start = chars[i].0;
            let mut j = i;
            while j < chars.len() && !is_cut(chars[j].1) {
                j += 1;
            }
            let end = chars.get(j).map_or(line.len(), |(b, _)| *b);
            let token = &line[start..end];
            // a token like `C:\Users\x` is the start of a path that may hold spaces: the folder is looked for in the rest of the row
            let rest = &line[start..];
            if let Some(found) = token_outside(token, rest, folder.as_deref()) {
                // a path the pane wrapped at its edge goes on in the next row
                let wrapped = lines.get(at + 1).map(|next| format!("{rest}{}", next.trim_start()));
                if !wrapped.is_some_and(|joined| token_outside(token, &joined, folder.as_deref()).is_none()) {
                    return Some(found);
                }
            }
            i = j;
        }
    }
    None
}

/// What the question asks that reaches outside the wizard's folder or at the machine, in words; `None` = nothing.
pub fn breach(region: &Region, folder: Option<&Path>) -> Option<String> {
    if let Some(m) = MACHINE.find(&region.all) {
        return Some(format!("the agent asked to inspect or control other processes or the machine: {}", m.as_str().trim()));
    }
    if let Some(path) = path_outside(&region.above, folder) {
        return Some(format!("the agent asked for access outside the wizard's temp folder: {path}"));
    }
    if let Some(m) = OUTSIDE_WORDS.find(&region.all) {
        return Some(format!("the agent asked for access outside the wizard's temp folder (the question says \"{}\")", m.as_str().trim()));
    }
    None
}

// ---- the plain "No" ---------------------------------------------------------------------------------

/// A refusal that only refuses: No, deny, cancel, reject, decline... not one that quits the agent, remembers the refusal
/// for good, or asks for something else (typing a reason).
fn plain_refusal(text: &str) -> bool {
    use super::menu::{classify_option, is_deny, OptionClass};
    let t = text.trim().to_ascii_lowercase();
    if !is_deny(&t) || classify_option(&t) == OptionClass::Risky {
        return false;
    }
    const NOT_PLAIN: [&str; 12] = ["exit", "quit", "abort", "never", "esc", "tell", "instead", "differently", "feedback", "reason", "type", "explain"];
    !NOT_PLAIN.iter().any(|w| t.split(|c: char| !c.is_alphanumeric()).any(|word| word == *w))
}

/// The row of the plain "No" of this menu: the one refusal that only refuses (the shortest when several do). `None` = no
/// row to press without a doubt.
pub fn plain_no(menu: &Menu) -> Option<usize> {
    if menu.kind == MenuKind::YesNo {
        return Some(0);
    }
    menu.options
        .iter()
        .enumerate()
        .filter(|(_, o)| plain_refusal(&o.text))
        .min_by_key(|(i, o)| (o.text.chars().count(), *i))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn folder() -> PathBuf {
        PathBuf::from(r"C:\Users\user\AppData\Local\Temp\pm-wizard-wz_abc123")
    }

    fn region(text: &str) -> Region {
        Region { above: text.to_string(), all: text.to_string() }
    }

    fn outside(text: &str) -> Option<String> {
        breach(&region(text), Some(&folder()))
    }

    #[test]
    fn paths_outside_the_folder_are_found_in_every_shape() {
        for text in [
            r#"Get-ChildItem -Path "C:\Users\user""#,
            r#"Select-String -Path "C:\Users\user\Desktop\work2\x.rs""#,
            r"type D:\data\x.txt",
            r"dir \\server\share\x",
            r"cat C:\\Users\\user\\x",
            "ls /etc/passwd",
            "cat /home/user/.ssh/config",
            "cat ~/notes.txt",
            "cat $HOME/notes.txt",
            r"dir %USERPROFILE%\Desktop",
            r"Get-Content $env:APPDATA\x",
            r"type ..\..\secret.txt",
            r"type C:\Users\user\AppData\Local\Temp\pm-wizard-wz_abc123\..\other\x",
            "cat ../x",
            "cd ..",
        ] {
            assert!(outside(text).is_some(), "{text}");
        }
    }

    #[test]
    fn paths_inside_the_folder_and_plain_words_are_not() {
        for text in [
            "Get-ChildItem",
            "Get-ChildItem -Force",
            r"Get-ChildItem -Path .\sub",
            "ls ./sub/x.txt",
            "cat sub/../x.txt",
            r"Get-Content C:\Users\user\AppData\Local\Temp\pm-wizard-wz_abc123\test-project\hello.txt",
            r#"Get-Content "C:\Users\user\AppData\Local\Temp\pm-wizard-wz_abc123\test-project\hello.txt""#,
            "Do you trust the files in this folder?\n  C:/Users/user/AppData/Local/Temp/pm-wizard-wz_abc123/test-project",
            "Run this command? https://example.com/a/b and/or 1/3 /help for commands (ctrl+o to expand)",
            "... Get-ChildItem ...",
        ] {
            assert_eq!(outside(text), None, "{text}");
        }
    }

    #[test]
    fn a_folder_with_a_space_in_its_name_is_still_inside() {
        let spaced = PathBuf::from(r"C:\Users\Jo Smith\AppData\Local\Temp\pm-wizard-wz_1");
        let text = r#"Read "C:\Users\Jo Smith\AppData\Local\Temp\pm-wizard-wz_1\test-project\a.txt""#;
        assert_eq!(breach(&region(text), Some(&spaced)), None);
        assert!(breach(&region(r#"Read "C:\Users\Jo Smith\Desktop\a.txt""#), Some(&spaced)).is_some());
    }

    #[test]
    fn with_no_known_folder_every_absolute_path_is_outside() {
        assert!(breach(&region(r"dir C:\x\y"), None).is_some());
        assert_eq!(breach(&region("dir .\\x"), None), None);
    }

    #[test]
    fn the_words_of_an_outside_access_and_the_process_verbs_are_found() {
        for text in [
            "Yes, and always allow non-workspace access",
            "access outside the workspace",
            "outside of the project folder",
            "This is outside the current directory",
            "Get-Process | Where-Object { $_.Name -like 'x' }",
            "Stop-Process -Id 4",
            "Get-CimInstance Win32_Process -Filter \"x\"",
            "tasklist /v",
            "taskkill /F /PID 4",
            "ps aux | grep x",
            "pkill -f node",
            "kill -9 1234",
            "wmic process get name",
            "Restart-Computer",
        ] {
            assert!(breach(&region(text), Some(&folder())).is_some(), "{text}");
        }
        for text in ["Get-ChildItem", "Run this command?\n echo hello", "killed 1 task", "the process ended", "the workspace is trusted"] {
            assert_eq!(breach(&region(text), Some(&folder())), None, "{text}");
        }
    }

    #[test]
    fn the_region_is_the_question_not_the_transcript_above_its_rule() {
        let screen = [
            "\u{25CF} Ran 3 commands (Select-String -Path \"C:\\Users\\user\\Desktop\\x\")",
            "",
            "Command",
            &"\u{2500}".repeat(60),
            "Requesting permission for:",
            "   echo hello",
            "",
            "Run this command?",
            "> 1. Yes, run command",
            "  2. No, cancel",
        ]
        .join("\n");
        let menu = super::super::menu::parse_menu(&screen).expect("a menu");
        let region = question_region(&screen, Some(&menu));
        assert!(region.all.contains("echo hello") && !region.all.contains("Desktop") && region.above.contains("echo hello") && !region.above.contains("Yes, run"), "{region:?}");
        assert_eq!(breach(&region, Some(&folder())), None);
    }

    #[test]
    fn the_plain_no_is_the_row_that_only_refuses() {
        let menu = |rows: &[&str]| Menu {
            kind: MenuKind::Numbered,
            options: rows.iter().enumerate().map(|(i, t)| super::super::menu::MenuOption { text: t.to_string(), number: Some(i as u32 + 1), highlighted: i == 0, hotkey: None }).collect(),
        };
        assert_eq!(plain_no(&menu(&["Yes, allow access", "Yes, and always allow non-workspace access", "No, deny access"])), Some(2));
        assert_eq!(plain_no(&menu(&["Yes, run command", "Yes, and always allow", "No, cancel"])), Some(2));
        assert_eq!(plain_no(&menu(&["Yes, I trust this folder", "No, exit"])), None);
        assert_eq!(plain_no(&menu(&["Yes", "No, and tell the agent what to do differently", "No"])), Some(2));
        assert_eq!(plain_no(&menu(&["Allow once", "Deny", "Deny and never ask again"])), Some(1));
    }
}
