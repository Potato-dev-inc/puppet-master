//! The transcript search: the mapping inferred from three line shapes, a negative, the folders that may and may not be
//! looked at, and the table that comes out.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use super::infer::{infer, needles, Missing};
use super::{add_to_draft, clean_name, find, names_for, Found, Homes, Roots, Search, Snapshot, MARK};

const PROMPT: &str = "Reply with exactly the word: pong-abcdef";
const REPLY: &str = "pong-abcdef";

fn lines(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

const FLAT: [&str; 2] = [
    r#"{"ts":1793178000,"role":"user","content":"Reply with exactly the word: pong-abcdef"}"#,
    r#"{"ts":1793178002,"role":"assistant","model":"acme-1","content":"pong-abcdef"}"#,
];

const NESTED: [&str; 3] = [
    r#"{"type":"queue","sessionId":"s1"}"#,
    r#"{"type":"user","timestamp":"2026-10-28T10:00:00Z","message":{"role":"user","content":"Reply with exactly the word: pong-abcdef"}}"#,
    r#"{"type":"assistant","timestamp":"2026-10-28T10:00:02Z","message":{"role":"assistant","model":"m-1","content":[{"type":"text","text":"pong-abcdef"}]}}"#,
];

const TOOLS: [&str; 5] = [
    r#"{"role":"user","content":"Reply with exactly the word: pong-abcdef"}"#,
    r#"{"role":"assistant","id":"c1","name":"write_file","input":{"path":"a.txt","content":"x"}}"#,
    r#"{"role":"tool","tool_call_id":"c1","output":"wrote a.txt","exit_code":0}"#,
    r#"{"role":"assistant","content":"pong-abcdef"}"#,
    r#"{"role":"user","content":"thanks"}"#,
];

#[test]
fn a_flat_role_and_content_line_gives_the_role_the_text_and_the_extras() {
    let m = infer(&lines(&FLAT), PROMPT, &needles(REPLY, REPLY)).expect("a mapping");
    assert_eq!(m.role, "/role");
    assert_eq!(m.text, ["/content"]);
    assert_eq!((m.user_values.as_slice(), m.assistant_values.as_slice()), (&["user".to_string()][..], &["assistant".to_string()][..]));
    assert_eq!(m.timestamp.as_deref(), Some("/ts"));
    assert_eq!(m.model.as_deref(), Some("/model"));
    assert!(m.tool.is_none());
}

#[test]
fn a_nested_message_with_parts_gives_the_array_as_the_text() {
    let m = infer(&lines(&NESTED), PROMPT, &needles(REPLY, REPLY)).expect("a mapping");
    assert_eq!(m.role, "/message/role");
    assert_eq!(m.text, ["/message/content"], "a list of parts is read as the list, not as its first part");
    assert_eq!(m.timestamp.as_deref(), Some("/timestamp"));
    assert_eq!(m.model.as_deref(), Some("/message/model"));
}

#[test]
fn tool_lines_give_the_call_and_the_result_fields_and_the_role_word_of_the_result() {
    let m = infer(&lines(&TOOLS), PROMPT, &needles(REPLY, REPLY)).expect("a mapping");
    let tool = m.tool.expect("tool fields");
    assert_eq!(tool.name, "/name");
    assert_eq!(tool.input.as_deref(), Some("/input"));
    assert_eq!(tool.id.as_deref(), Some("/id"));
    assert_eq!(tool.result_for.as_deref(), Some("/tool_call_id"));
    assert_eq!(tool.output.as_deref(), Some("/output"));
    assert_eq!(tool.error.as_deref(), Some("/exit_code"));
    assert_eq!(tool.sample_name, "write_file");
    assert_eq!(m.tool_values, ["tool"]);
}

#[test]
fn no_line_with_the_message_or_the_reply_gives_nothing_and_says_how_far_it_got() {
    let other = lines(&[r#"{"role":"user","content":"something else"}"#, r#"{"role":"assistant","content":"unrelated"}"#]);
    assert_eq!(infer(&other, PROMPT, &needles(REPLY, REPLY)), Err(Missing::Message));
    let no_reply = lines(&[FLAT[0], r#"{"role":"assistant","content":"other words"}"#]);
    assert_eq!(infer(&no_reply, PROMPT, &needles(REPLY, REPLY)), Err(Missing::Reply));
    // the same role word on both lines: nothing tells them apart
    let same = lines(&[
        r#"{"who":"x","role":"user","content":"Reply with exactly the word: pong-abcdef"}"#,
        r#"{"who":"x","role":"user","content":"pong-abcdef"}"#,
    ]);
    assert_eq!(infer(&same, PROMPT, &needles(REPLY, REPLY)), Err(Missing::Role));
    assert_eq!(infer(&lines(&["not json", "[1,2]"]), PROMPT, &needles(REPLY, REPLY)), Err(Missing::Json));
    // every reason is a plain sentence
    for why in [Missing::Json, Missing::Message, Missing::Reply, Missing::Role] {
        assert!(!why.why().is_empty() && !why.why().contains("pong"));
    }
}

#[test]
fn a_reply_the_screen_decorated_is_still_found_by_the_text_after_the_glyph_or_by_the_word() {
    assert_eq!(needles("\u{25cf} pong-abcdef", "pong-abcdef"), ["\u{25cf} pong-abcdef", "pong-abcdef"]);
    assert_eq!(needles("reply: pong-abcdef", "pong-abcdef"), ["reply: pong-abcdef", "pong-abcdef"]);
    for shown in ["\u{25cf} pong-abcdef", "reply: pong-abcdef", "pong-abcdef"] {
        let m = infer(&lines(&FLAT), PROMPT, &needles(shown, "pong-abcdef")).unwrap_or_else(|e| panic!("{shown}: {e:?}"));
        assert_eq!(m.text, ["/content"]);
    }
}

#[test]
fn a_reply_word_inside_the_task_line_is_not_taken_for_the_reply() {
    // the task holds the reply word: only the line that does NOT hold the task can be the reply
    let m = infer(&lines(&[FLAT[0]]), PROMPT, &needles(REPLY, REPLY));
    assert_eq!(m, Err(Missing::Reply));
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A scratch tree: `home` (the person's), `proj` (the wizard's temporary folder) and `elsewhere`.
struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new(name: &str) -> Tree {
        let root = std::env::temp_dir().join(format!("pm-transcript-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        for dir in ["home", "proj", "elsewhere"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        Tree { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn proj(&self) -> PathBuf {
        self.root.join("proj")
    }

    fn roots(&self, names: &[&str]) -> Roots {
        Roots {
            cwd: self.proj(),
            homes: Homes { home: Some(self.home()), appdata: Some(self.home().join("AppData").join("Roaming")), localappdata: Some(self.home().join("AppData").join("Local")) },
            names: names.iter().map(|n| n.to_string()).collect(),
        }
    }

    fn write(&self, path: &Path, lines: &[&str]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    fn append(&self, path: &Path, lines: &[&str]) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all((lines.join("\n") + "\n").as_bytes()).unwrap();
    }
}

fn search<'a>(roots: &'a Roots, before: &'a Snapshot) -> Search<'a> {
    Search { roots, before, since: SystemTime::now() - Duration::from_secs(30), prompt: PROMPT, reply: REPLY, word: REPLY }
}

/// The folder name a program that names its folders after the project would make for the wizard's temporary folder.
fn slug_of(path: &Path) -> String {
    crate::session_chat::cwd_placeholders(path).into_iter().find(|(name, _)| *name == "cwd_slug").unwrap().1
}

#[test]
fn a_transcript_in_the_program_folder_under_home_becomes_a_table_with_placeholders() {
    let tree = Tree::new("home");
    let roots = tree.roots(&["acme"]);
    let dir = tree.home().join(".acme").join("sessions").join(slug_of(&tree.proj()));
    // an older session of the same project is already there, with a line that must never be read
    tree.write(&dir.join("old.jsonl"), &[r#"{"role":"user","content":"older private line"}"#]);
    let before = Snapshot::take(&roots);
    tree.write(&dir.join("new.jsonl"), &FLAT);
    tree.append(&dir.join("old.jsonl"), &[r#"{"role":"user","content":"added to an older file"}"#]);
    let found = find(&search(&roots, &before)).expect("found");
    assert_eq!(found.dir, "{home}/.acme/sessions/{cwd_slug}");
    assert!(!found.tool);
    assert!(found.toml.starts_with(MARK), "{}", found.toml);
    assert!(found.toml.contains("role = \"/role\"") && found.toml.contains("text = \"/content\""), "{}", found.toml);
    // nothing of the transcript or of the real folders is in the table
    assert!(!found.toml.contains("pong-") && !found.toml.contains("older") && !found.toml.contains(&tree.root.to_string_lossy().to_string()), "{}", found.toml);
}

#[test]
fn the_other_home_patterns_are_looked_at_and_a_deeper_folder_is_not() {
    for (relative, template) in [
        (".config/acme/logs", "{home}/.config/acme/logs"),
        ("AppData/Roaming/acme/chats", "{home}/AppData/Roaming/acme/chats"),
        ("AppData/Local/acme", "{home}/AppData/Local/acme"),
    ] {
        let tree = Tree::new("patterns");
        let roots = tree.roots(&["acme"]);
        let before = Snapshot::take(&roots);
        tree.write(&tree.home().join(relative).join("t.jsonl"), &NESTED);
        let found = find(&search(&roots, &before)).unwrap_or_else(|e| panic!("{relative}: {}", e.why));
        assert_eq!(found.dir, template);
    }
    // a folder beyond the depth limit is never walked
    let tree = Tree::new("deep");
    let roots = tree.roots(&["acme"]);
    let before = Snapshot::take(&roots);
    tree.write(&tree.home().join(".acme/a/b/c/d/e/f/g/h/deep.jsonl"), &FLAT);
    let err = find(&search(&roots, &before)).unwrap_err();
    assert!(!err.retry && err.why.contains("no .jsonl file"), "{err:?}");
}

#[test]
fn a_transcript_in_the_temporary_folder_itself_is_found_and_uses_cwd() {
    let tree = Tree::new("cwd");
    let roots = tree.roots(&[]);
    let before = Snapshot::take(&roots);
    tree.write(&tree.proj().join("session.jsonl"), &FLAT);
    let found = find(&search(&roots, &before)).expect("found");
    assert_eq!(found.dir, "{cwd}");
    // and in a subfolder
    let tree = Tree::new("cwd-sub");
    let roots = tree.roots(&[]);
    let before = Snapshot::take(&roots);
    tree.write(&tree.proj().join(".state").join("chats").join("s.jsonl"), &NESTED);
    assert_eq!(find(&search(&roots, &before)).expect("found").dir, "{cwd}/.state/chats");
}

#[test]
fn nothing_outside_the_listed_folders_is_read_even_when_it_changes_and_holds_the_message() {
    let tree = Tree::new("outside");
    let roots = tree.roots(&["acme"]);
    let before = Snapshot::take(&roots);
    // a program folder of another name, a plain folder of the home, and a folder outside the home
    tree.write(&tree.home().join(".other").join("s.jsonl"), &FLAT);
    tree.write(&tree.home().join("documents").join("s.jsonl"), &FLAT);
    tree.write(&tree.root.join("elsewhere").join("s.jsonl"), &FLAT);
    let err = find(&search(&roots, &before)).unwrap_err();
    assert!(err.why.contains("no .jsonl file"), "{err:?}");
    // a name is letters, digits, `_` and `-` of the last path part, so a pattern cannot leave its folder
    assert_eq!(clean_name("Acme"), Some("acme".to_string()));
    assert_eq!(clean_name("/usr/bin/acme"), Some("acme".to_string()));
    assert_eq!(clean_name("../.."), None);
    assert_eq!(clean_name("a b"), None);
    assert_eq!(clean_name(""), None);
    assert_eq!(names_for("acme", Path::new("/x/acme.exe"), "acme_two"), ["acme", "acme_two"]);
}

#[test]
fn a_changed_file_without_the_message_says_so_and_asks_to_look_again() {
    let tree = Tree::new("nomatch");
    let roots = tree.roots(&["acme"]);
    let before = Snapshot::take(&roots);
    tree.write(&tree.home().join(".acme").join("log.jsonl"), &[r#"{"event":"start"}"#, r#"{"event":"idle"}"#]);
    let err = find(&search(&roots, &before)).unwrap_err();
    assert!(err.retry && err.why.contains("1 .jsonl file(s) changed") && err.why.contains("holds the message"), "{err:?}");
}

#[test]
fn a_table_whose_mapping_the_reader_cannot_use_is_not_offered() {
    let tree = Tree::new("badmap");
    let roots = tree.roots(&[]);
    let before = Snapshot::take(&roots);
    // the reply sits where the task also is, in a field that is the same on both lines: no role tells them apart
    tree.write(&tree.proj().join("s.jsonl"), &[r#"{"kind":"a","text":"Reply with exactly the word: pong-abcdef"}"#, r#"{"kind":"a","text":"pong-abcdef"}"#]);
    assert!(find(&search(&roots, &before)).is_err());
}

#[test]
fn tool_lines_are_kept_only_when_the_reader_reads_them() {
    let tree = Tree::new("tools");
    let roots = tree.roots(&[]);
    let before = Snapshot::take(&roots);
    tree.write(&tree.proj().join("s.jsonl"), &TOOLS);
    let found = find(&search(&roots, &before)).expect("found");
    assert!(found.tool, "{}", found.toml);
    for key in ["tool_name = \"/name\"", "tool_input = \"/input\"", "tool_id = \"/id\"", "tool_result_for = \"/tool_call_id\"", "tool_output = \"/output\"", "tool_error = \"/exit_code\""] {
        assert!(found.toml.contains(key), "{key} in\n{}", found.toml);
    }
}

#[test]
fn the_table_is_added_to_a_draft_once_and_never_over_the_drafters_own() {
    let draft = crate::tui::declarative::wizard::tests::support::visible_draft("nimbus");
    let found = Found {
        toml: format!("{MARK}\n[session_chat]\nformat = \"jsonl\"\ndir = \"{{cwd}}\"\n\n[session_chat.map]\nrole = \"/role\"\ntext = \"/content\"\n"),
        dir: "{cwd}".into(),
        tool: false,
    };
    let with = add_to_draft(&draft, &found).expect("added");
    assert!(with.contains(MARK) && with.ends_with('\n'));
    let def = crate::tui::declarative::schema::parse_definition(&with).expect("still a definition");
    assert!(def.session_chat.is_some());
    // the table is part of the fingerprint, so the person approves it
    let plain = crate::tui::declarative::schema::parse_definition(&draft).unwrap();
    let print = |d| crate::tui::declarative::fingerprint::fingerprint(d, crate::tui::declarative::validate::Source::User, None, Path::new("nimbus.toml"));
    assert_ne!(print(&def), print(&plain));
    // a draft that has a table of its own keeps it
    assert!(add_to_draft(&with, &found).is_none());
    // a table that is not a valid definition is not added
    let broken = Found { toml: "[session_chat]\nformat = \"jsonl\"\n".into(), ..found };
    assert!(add_to_draft(&draft, &broken).is_none());
}
