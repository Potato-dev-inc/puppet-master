//! Declared tests (SPEC n32 section 3.3). The person declares, per project, the command that proves a
//! task passed. Puppet Master never guesses it: `suggestions` only offers what `missions::detect`
//! found, and nothing runs from it until the person declares it.
//!
//! A command is an argv, never a shell string. `parse_command` refuses shell operators, so the check
//! runs exactly the words the person typed: no pipes, redirects, `&&` or substitutions.

use super::store::Store;
use crate::agent_runs::checks::{Check, MAX_COMMAND_TIMEOUT_MS};
use crate::missions::detect;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Most words in one declared command, the program included (S4).
pub const MAX_ARGS: usize = 12;
/// Longest name a declared command can carry, in characters.
pub const MAX_NAME: usize = 60;
/// Shell operators a command may not contain. Checked as plain text before the words are split.
const SHELL_MARKS: [&str; 7] = ["&", "|", ">", "<", ";", "`", "$("];

/// One declared check for a project. Stored in the app data folder (`projects.json`), keyed by project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declared {
    pub name: String,
    pub argv: Vec<String>,
    pub timeout_min: u32,
    /// When the person declared it (unix ms). `declare` stamps it when it is 0.
    pub declared_ms: u64,
}

/// A check Puppet Master found in the project. Only a suggestion: it is not used until declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    pub name: String,
    pub argv: Vec<String>,
    pub source: String,
}

/// What `missions::detect` found in the project. Never declared by itself, so it is never used by
/// a night run until the person presses Use and `declare` saves it.
pub fn suggestions(project: &Path) -> Vec<Suggestion> {
    detect::detect(project)
        .into_iter()
        .map(|found| Suggestion { name: found.name, argv: found.command, source: found.source })
        .collect()
}

/// Turn the text the person typed into an argv. Refuses shell operators, newlines, an unclosed
/// quote, an empty command and more than `MAX_ARGS` words. Quotes group one word; no escapes.
pub fn parse_command(text: &str) -> Result<Vec<String>, String> {
    if text.contains(['\n', '\r']) {
        return Err("A check must be one line.".to_string());
    }
    if let Some(mark) = SHELL_MARKS.iter().find(|mark| text.contains(**mark)) {
        return Err(format!("A check cannot use `{mark}`. Write one plain command, like `npm test`."));
    }
    let argv = split_words(text)?;
    if argv.is_empty() {
        return Err("The check is empty.".to_string());
    }
    if argv.len() > MAX_ARGS {
        return Err(format!("A check can have at most {MAX_ARGS} words."));
    }
    Ok(argv)
}

fn split_words(text: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for ch in text.chars() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                in_word = true;
            }
            None if ch.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            None => {
                current.push(ch);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("A quote is not closed.".to_string());
    }
    if in_word {
        words.push(current);
    }
    Ok(words)
}

/// The project's declared checks, read from the app data folder. Anything unreadable, or that no
/// longer passes `validate`, reads as none: nothing is guessed in its place.
pub fn declared(store: &Store, project: &str) -> Vec<Declared> {
    let list: Vec<Declared> = store
        .declared_raw(&project_key(project))
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    if validate(project, &list).is_ok() {
        list
    } else {
        Vec::new()
    }
}

/// Save the project's declared checks (replaces the whole list). Refuses a list that does not pass
/// `validate`, so a bad check is never written.
pub fn declare(store: &Store, project: &str, list: Vec<Declared>) -> Result<(), String> {
    validate(project, &list)?;
    let stamped: Vec<Declared> = list
        .into_iter()
        .map(|mut check| {
            if check.declared_ms == 0 {
                check.declared_ms = now_ms();
            }
            check
        })
        .collect();
    let value = serde_json::to_value(&stamped).map_err(|err| err.to_string())?;
    store.put_declared_raw(&project_key(project), value)
}

/// Plain-language notes about one declared check. A relative script path (`./test.sh`) can be
/// edited by an agent, so Puppet Master cannot tell whether the check itself changed.
pub fn warnings(_project: &Path, declared: &Declared) -> Vec<String> {
    match declared.argv.first() {
        Some(program) if is_relative_path(program) => vec![format!(
            "This runs {program} from the project. If an agent edits that file, Puppet Master can't tell."
        )],
        _ => Vec::new(),
    }
}

/// The key a project's checks are stored under: the same key the operations and missions use.
fn project_key(project: &str) -> String {
    crate::project_path::comparable_project_path(Path::new(project))
}

/// Checks the list against the same rules the agent runs use. Each command must have a program, at
/// most `MAX_ARGS` words, a name, and a time limit between 1 minute and the 10 minute cap.
fn validate(project: &str, list: &[Declared]) -> Result<(), String> {
    for check in list {
        if check.name.trim().is_empty() {
            return Err("A check needs a name.".to_string());
        }
        if check.name.chars().count() > MAX_NAME {
            return Err(format!("A check name can be at most {MAX_NAME} characters."));
        }
        if check.argv.first().map_or(true, |program| program.trim().is_empty()) {
            return Err("A check needs a program to run, like `npm`.".to_string());
        }
        if check.argv.len() > MAX_ARGS {
            return Err(format!("A check can have at most {MAX_ARGS} words."));
        }
        if check.timeout_min == 0 {
            return Err("A check needs a time limit.".to_string());
        }
        if u64::from(check.timeout_min) * 60_000 > MAX_COMMAND_TIMEOUT_MS {
            return Err("A check's time limit can be at most 10 minutes.".to_string());
        }
    }
    let checks: Vec<Check> = list
        .iter()
        .map(|check| Check::CommandExits {
            command: check.argv.clone(),
            expect_exit: 0,
            timeout_ms: Some(u64::from(check.timeout_min) * 60_000),
        })
        .collect();
    crate::agent_runs::checks::validate(Path::new(project), &checks)
}

fn is_relative_path(word: &str) -> bool {
    !Path::new(word).is_absolute() && (word.contains('/') || word.contains('\\'))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A temp folder for one test, removed at the end.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            Scratch(std::env::temp_dir().join(format!("pm-ns-checks-{}", uuid::Uuid::new_v4().simple())))
        }

        fn store(&self) -> Store {
            Store::at(self.0.join("store"))
        }

        fn project(&self, name: &str) -> String {
            let dir = self.0.join(name);
            fs::create_dir_all(&dir).expect("project dir");
            dir.to_string_lossy().to_string()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn check(name: &str, argv: &[&str], timeout_min: u32) -> Declared {
        Declared {
            name: name.to_string(),
            argv: argv.iter().map(|word| word.to_string()).collect(),
            timeout_min,
            declared_ms: 5,
        }
    }

    #[test]
    fn parse_command_accepts_plain_commands() {
        assert_eq!(parse_command("npm test").unwrap(), vec!["npm", "test"]);
        assert_eq!(parse_command("  cargo   test -p x ").unwrap(), vec!["cargo", "test", "-p", "x"]);
        assert_eq!(parse_command("python -m pytest -q").unwrap(), vec!["python", "-m", "pytest", "-q"]);
        assert_eq!(parse_command("node run.js \"my folder/file name\"").unwrap()[2], "my folder/file name");
    }

    #[test]
    fn parse_command_refuses_shell_operators_and_bad_shapes() {
        for text in ["a && b", "a | b", "a > f", "a < f", "a; b", "echo `x`", "echo $(x)", "a & b"] {
            assert!(parse_command(text).is_err(), "should refuse {text}");
        }
        assert!(parse_command("npm test\nrm -rf x").is_err(), "newline");
        assert!(parse_command("npm test\r").is_err(), "carriage return");
        assert!(parse_command("   ").is_err(), "empty");
        assert!(parse_command("node 'open quote").is_err(), "unclosed quote");
        let twelve = (0..12).map(|i| format!("a{i}")).collect::<Vec<_>>().join(" ");
        assert_eq!(parse_command(&twelve).unwrap().len(), 12);
        let thirteen = format!("{twelve} extra");
        assert!(parse_command(&thirteen).is_err(), "13 words");
    }

    #[test]
    fn suggestions_are_never_used_until_declared() {
        let scratch = Scratch::new();
        let project = scratch.project("app");
        fs::write(Path::new(&project).join("package.json"), r#"{"scripts":{"test":"node --test"}}"#).expect("pkg");
        let found = suggestions(Path::new(&project));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].argv, vec!["npm", "test"]);
        assert_eq!(found[0].source, "package.json");
        assert!(declared(&scratch.store(), &project).is_empty(), "a suggestion is not a declared check");
    }

    #[test]
    fn declare_rejects_an_empty_program_and_a_missing_timeout() {
        let scratch = Scratch::new();
        let store = scratch.store();
        let project = scratch.project("app");
        assert!(declare(&store, &project, vec![check("Tests", &[" "], 5)]).is_err(), "blank program");
        assert!(declare(&store, &project, vec![check("Tests", &[], 5)]).is_err(), "no program");
        assert!(declare(&store, &project, vec![check("Tests", &["npm", "test"], 0)]).is_err(), "no timeout");
        assert!(declare(&store, &project, vec![check("Tests", &["npm", "test"], 11)]).is_err(), "over 10 min");
        assert!(declare(&store, &project, vec![check(" ", &["npm", "test"], 5)]).is_err(), "no name");
        assert!(declared(&store, &project).is_empty());
    }

    #[test]
    fn a_relative_script_gets_a_warning() {
        let scratch = Scratch::new();
        let project = scratch.project("app");
        let script = check("Tests", &["./test.sh"], 5);
        let notes = warnings(Path::new(&project), &script);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("./test.sh") && notes[0].contains("can't tell"));
        assert!(warnings(Path::new(&project), &check("Tests", &["npm", "test"], 5)).is_empty());
    }

    #[test]
    fn declared_checks_survive_a_reload() {
        let scratch = Scratch::new();
        let project = scratch.project("app");
        let list = vec![check("Tests", &["npm", "test"], 5), check("Build", &["cargo", "test", "-p", "x"], 10)];
        declare(&scratch.store(), &project, list.clone()).expect("declare");
        let reloaded = Store::at(scratch.0.join("store"));
        assert_eq!(declared(&reloaded, &project), list);
    }

    #[test]
    fn declare_stamps_a_missing_time() {
        let scratch = Scratch::new();
        let store = scratch.store();
        let project = scratch.project("app");
        let mut fresh = check("Tests", &["npm", "test"], 5);
        fresh.declared_ms = 0;
        declare(&store, &project, vec![fresh]).expect("declare");
        assert!(declared(&store, &project)[0].declared_ms > 0);
    }

    #[test]
    fn a_hand_edited_bad_list_reads_as_none() {
        let scratch = Scratch::new();
        let store = scratch.store();
        let project = scratch.project("app");
        let value = serde_json::json!([{ "name": "Tests", "argv": [], "timeout_min": 5, "declared_ms": 1 }]);
        store.put_declared_raw(&project_key(&project), value).expect("put");
        assert!(declared(&store, &project).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn two_spellings_of_one_project_share_its_checks() {
        let scratch = Scratch::new();
        let store = scratch.store();
        let project = scratch.project("App");
        declare(&store, &project, vec![check("Tests", &["npm", "test"], 5)]).expect("declare");
        let other_spelling = project.replace('\\', "/").to_uppercase();
        assert_eq!(declared(&store, &other_spelling).len(), 1);
        assert_eq!(declared(&store, &format!("{other_spelling}/")).len(), 1);
    }
}
