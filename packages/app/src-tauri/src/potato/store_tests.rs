//! The files of Potato Bot: bots, settings, conversations (redacted, capped, rotated), reads, the persona
//! seeding, "forget talk memory". A missing file reads as empty; a damaged file is an error and is never
//! overwritten.

use std::path::PathBuf;

use super::store::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn text(at: i64, from: FromKind, body: &str) -> Message {
    Message::new(at, from, None, MsgKind::Text, body)
}

#[test]
fn a_missing_file_reads_as_empty_and_a_damaged_one_is_an_error_that_is_never_overwritten() {
    let root = temp("damaged");
    assert!(load_bots(&root).unwrap().is_empty());
    assert_eq!(load_settings(&root).unwrap(), Settings::default());
    assert!(load_reads(&root).unwrap().is_empty());
    assert!(read_messages(&root, "app:dm:nimbus", 10, None).unwrap().is_empty());

    std::fs::write(bots_path(&root), "{ damaged").unwrap();
    assert_eq!(load_bots(&root).unwrap_err().code, "POTATO_STORE_INVALID");
    assert!(add_bot(&root, "nimbus", "fish", 1).is_err());
    assert_eq!(std::fs::read_to_string(bots_path(&root)).unwrap(), "{ damaged", "never overwritten");

    std::fs::write(settings_path(&root), "nope").unwrap();
    assert!(load_settings(&root).is_err());

    let path = conversation_path(&root, "app:dm:nimbus").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not a message\n").unwrap();
    assert!(read_messages(&root, "app:dm:nimbus", 10, None).is_err());
    assert!(append_message(&root, "app:dm:nimbus", text(1, FromKind::You, "hi"), None).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not a message\n");
}

#[test]
fn a_file_from_a_newer_version_is_refused() {
    let root = temp("newer");
    std::fs::write(bots_path(&root), r#"{"v":99,"bots":[]}"#).unwrap();
    assert!(load_bots(&root).is_err());
}

#[test]
fn bots_are_companion_ids_with_a_talk_persona_and_at_most_three() {
    let root = temp("bots");
    let bot = add_bot(&root, "nimbus", "fish", 1_700).unwrap();
    assert_eq!(bot, Bot { id: "nimbus".into(), talk_persona: "fish".into(), created_ms: 1_700 });
    let on_disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(bots_path(&root)).unwrap()).unwrap();
    assert_eq!(on_disk["v"], 1);
    assert_eq!(on_disk["bots"][0]["talkPersona"], "fish", "named talkPersona: a companion's `persona` already means cheerful, calm ...");
    assert_eq!(on_disk["bots"][0]["createdMs"], 1_700);
    assert_eq!(add_bot(&root, "nimbus", "fish", 2).unwrap_err().code, "POTATO_BOT_EXISTS");
    assert_eq!(add_bot(&root, "Bad Id", "fish", 2).unwrap_err().code, "POTATO_BOT_INVALID");
    assert_eq!(add_bot(&root, "../x", "fish", 2).unwrap_err().code, "POTATO_BOT_INVALID");
    assert_eq!(add_bot(&root, "mochi", "wizard", 2).unwrap_err().code, "POTATO_PERSONA_INVALID");
    add_bot(&root, "mochi", "potato", 3).unwrap();
    add_bot(&root, "tofu", "fish", 4).unwrap();
    assert_eq!(add_bot(&root, "kit", "fish", 5).unwrap_err().code, "POTATO_BOT_LIMIT");
    assert_eq!(load_bots(&root).unwrap().len(), 3);
}

#[test]
fn the_persona_switch_changes_only_the_voice() {
    let root = temp("persona");
    add_bot(&root, "nimbus", "fish", 1).unwrap();
    let after = set_talk_persona(&root, "nimbus", "potato").unwrap();
    assert_eq!((after.id.as_str(), after.talk_persona.as_str(), after.created_ms), ("nimbus", "potato", 1));
    assert_eq!(load_bots(&root).unwrap()[0].talk_persona, "potato");
    assert!(set_talk_persona(&root, "nimbus", "wizard").is_err());
    assert_eq!(set_talk_persona(&root, "ghost", "fish").unwrap_err().code, "POTATO_BOT_NOT_FOUND");
}

#[test]
fn settings_default_to_deepseek_fish_and_50_cents_and_are_kept_in_range() {
    let root = temp("settings");
    let s = load_settings(&root).unwrap();
    assert_eq!((s.brain, s.default_persona.as_str(), s.daily_limit_cents), (BrainKind::Deepseek, "fish", 50));
    save_settings(&root, &Settings { daily_limit_cents: 1, brain: BrainKind::Go, ..Settings::default() }).unwrap();
    let s = load_settings(&root).unwrap();
    assert_eq!((s.brain, s.daily_limit_cents), (BrainKind::Go, 5), "from $0.05");
    save_settings(&root, &Settings { daily_limit_cents: 999_999, ..Settings::default() }).unwrap();
    assert_eq!(load_settings(&root).unwrap().daily_limit_cents, 2_000, "to $20");
    assert!(save_settings(&root, &Settings { default_persona: "wizard".into(), ..Settings::default() }).is_err());
    // the wire words
    let json = serde_json::to_value(Settings { brain: BrainKind::Agents, ..Settings::default() }).unwrap();
    assert_eq!(json["brain"], "agents");
    assert_eq!(json["dailyLimitCents"], 50);
    assert_eq!(serde_json::to_value(BrainKind::Deepseek).unwrap(), "deepseek");
    assert_eq!(serde_json::to_value(BrainKind::Go).unwrap(), "go");
}

#[test]
fn a_conversation_id_has_its_surface_in_front_and_names_a_safe_file() {
    assert_eq!(surface_of("app:dm:nimbus"), Some("app"));
    assert_eq!(surface_of("outer:dm:nimbus:12345"), Some("outer"));
    assert_eq!(surface_of("outer:ch:nimbus:999"), Some("outer"));
    for bad in ["", "nimbus", "app:dm", "app::x", "App:dm:x", "app:dm:../x", "app:dm:a/b", "app:dm:a\\b", "app:dm:a b", "app:dm:\u{0}"] {
        assert_eq!(surface_of(bad), None, "{bad:?}");
    }
    assert_eq!(surface_of(&format!("app:dm:{}", "a".repeat(250))), None, "too long");
    assert_eq!(app_bot_of("app:dm:nimbus"), Some("nimbus"));
    assert_eq!(app_bot_of("app:dm:Nimbus"), None);
    assert_eq!(app_bot_of("outer:dm:nimbus"), None);
    assert_eq!(app_bot_of("app:dm:nimbus:extra"), None);
    assert_eq!(app_conversation("nimbus"), "app:dm:nimbus");

    // the colon is %3A: valid on every file system, and two ids never share a name
    assert_eq!(file_stem("app:dm:nimbus"), "app%3Adm%3Animbus");
    let root = temp("paths");
    let path = conversation_path(&root, "app:dm:nimbus").unwrap();
    assert_eq!(path, root.join("conversations").join("app").join("app%3Adm%3Animbus.jsonl"));
    assert_eq!(conversation_path(&root, "../../etc:dm:x").unwrap_err().code, "POTATO_CONVERSATION_INVALID");
    assert_ne!(file_stem("a:b:c"), file_stem("a_b_c"));
}

#[test]
fn a_message_is_redacted_when_it_is_written_and_again_when_it_is_shown() {
    let root = temp("redact");
    let secret = "sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij";
    let saved = append_message(&root, "app:dm:nimbus", text(1, FromKind::You, &format!("my key is {secret} ok")), None).unwrap();
    assert!(!saved.text.contains(secret), "{}", saved.text);
    let raw = std::fs::read_to_string(conversation_path(&root, "app:dm:nimbus").unwrap()).unwrap();
    assert!(!raw.contains(secret), "the secret is not on disk");
    // a line written by something else, with a secret in it, is still cleaned on its way out
    let path = conversation_path(&root, "app:dm:nimbus").unwrap();
    let dirty = serde_json::to_string(&text(2, FromKind::Bot, &format!("here {secret}"))).unwrap();
    std::fs::write(&path, format!("{raw}{dirty}\n")).unwrap();
    let shown = read_messages(&root, "app:dm:nimbus", 10, None).unwrap();
    assert_eq!(shown.len(), 2);
    assert!(shown.iter().all(|m| !m.text.contains(secret)));
}

#[test]
fn the_home_folder_is_taken_out_of_a_message() {
    let root = temp("home");
    let home = "C:\\Users\\someone\\PuppetMaster\\companions\\nimbus";
    let saved = append_message(&root, "app:dm:nimbus", text(1, FromKind::Bot, &format!("I looked in {home} and in {}", home.replace('\\', "/"))), Some(home)).unwrap();
    assert!(!saved.text.contains("someone"), "{}", saved.text);
    assert!(saved.text.contains("its folder"));
}

#[test]
fn a_message_is_cut_to_4000_characters() {
    let root = temp("cut");
    let saved = append_message(&root, "app:dm:nimbus", text(1, FromKind::You, &"é".repeat(9_000)), None).unwrap();
    assert_eq!(saved.text.chars().count(), MAX_MESSAGE_CHARS);
}

#[test]
fn messages_come_back_oldest_first_with_their_fields() {
    let root = temp("order");
    let mut card = Message::new(3, FromKind::Bot, Some("Fish"), MsgKind::HelperCard, "add a readme");
    card.request_id = Some("h-1".into());
    card.project = Some("demo-a".into());
    for m in [text(1, FromKind::You, "one"), Message::new(2, FromKind::Note, None, MsgKind::Note, "Now talking as Potato."), card] {
        append_message(&root, "app:dm:nimbus", m, None).unwrap();
    }
    let all = read_messages(&root, "app:dm:nimbus", 100, None).unwrap();
    assert_eq!(all.iter().map(|m| m.at).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(all[2].request_id.as_deref(), Some("h-1"));
    assert_eq!(all[2].project.as_deref(), Some("demo-a"));
    assert_eq!(all[2].from.name.as_deref(), Some("Fish"));
    let last_two = read_messages(&root, "app:dm:nimbus", 2, None).unwrap();
    assert_eq!(last_two.iter().map(|m| m.at).collect::<Vec<_>>(), [2, 3]);
    // the wire shape of one line
    let line = std::fs::read_to_string(conversation_path(&root, "app:dm:nimbus").unwrap()).unwrap();
    let first: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(first["from"], serde_json::json!({ "kind": "you" }));
    assert_eq!(first["kind"], "text");
    assert!(first.get("mood").is_none() && first.get("requestId").is_none());
}

#[test]
fn a_thread_rotates_at_5000_lines_and_keeps_one_old_file() {
    let root = temp("rotate");
    for i in 0..MAX_FILE_LINES as i64 {
        append_message(&root, "app:dm:nimbus", text(i, FromKind::You, "x"), None).unwrap();
    }
    let path = conversation_path(&root, "app:dm:nimbus").unwrap();
    assert!(!path.with_extension("1.jsonl").exists(), "exactly 5000 lines still fit");
    append_message(&root, "app:dm:nimbus", text(99_999, FromKind::You, "the 5001st"), None).unwrap();
    assert!(path.with_extension("1.jsonl").exists(), "rotated to .1.jsonl");
    let current = std::fs::read_to_string(&path).unwrap();
    assert_eq!(current.lines().count(), 1, "the new file starts with the new message");
    // reading joins the old file and the current one
    let shown = read_messages(&root, "app:dm:nimbus", 10, None).unwrap();
    assert_eq!(shown.last().unwrap().text, "the 5001st");
    assert_eq!(shown.len(), 10);
    // a second rotation replaces the old file: only one old file is kept
    for i in 0..MAX_FILE_LINES as i64 {
        append_message(&root, "app:dm:nimbus", text(i, FromKind::You, "y"), None).unwrap();
    }
    let files: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files.len(), 2, "{files:?}");
}

#[test]
fn a_thread_rotates_at_two_megabytes_too() {
    let root = temp("rotate-bytes");
    let big = "z".repeat(MAX_MESSAGE_CHARS);
    for i in 0..((MAX_FILE_BYTES as usize / MAX_MESSAGE_CHARS) + 3) {
        append_message(&root, "app:dm:nimbus", text(i as i64, FromKind::You, &big), None).unwrap();
    }
    let path = conversation_path(&root, "app:dm:nimbus").unwrap();
    assert!(path.with_extension("1.jsonl").exists());
    assert!(std::fs::metadata(&path).unwrap().len() <= MAX_FILE_BYTES);
}

#[test]
fn a_shared_channel_thread_keeps_only_its_newest_500_lines() {
    let root = temp("channel");
    for i in 0..(CHANNEL_KEEP as i64 + 25) {
        append_message(&root, "outer:ch:nimbus:777", text(i, FromKind::Person, &format!("m{i}")), None).unwrap();
    }
    let path = conversation_path(&root, "outer:ch:nimbus:777").unwrap();
    assert!(!path.with_extension("1.jsonl").exists(), "it trims, it does not rotate");
    let shown = read_messages(&root, "outer:ch:nimbus:777", 1_000, None).unwrap();
    assert_eq!(shown.len(), CHANNEL_KEEP);
    assert_eq!(shown.last().unwrap().text, format!("m{}", CHANNEL_KEEP + 24));
    assert_eq!(shown.first().unwrap().text, "m25");
}

#[test]
fn reads_remember_when_the_person_last_looked() {
    let root = temp("reads");
    mark_read(&root, "nimbus", 500).unwrap();
    mark_read(&root, "mochi", 700).unwrap();
    mark_read(&root, "nimbus", 900).unwrap();
    let reads = load_reads(&root).unwrap();
    assert_eq!((reads["nimbus"], reads["mochi"]), (900, 700));
}

fn resources(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = temp(name);
    for (rel, body) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

#[test]
fn the_persona_is_seeded_once_and_updated_only_where_the_person_never_edited_it() {
    let root = temp("seed");
    let v1 = resources("seed-res-1", &[("protocol.md", "protocol v1"), ("packs/fish/core.md", "fish v1"), ("packs/potato/core.md", "potato v1")]);
    let first = seed_persona(&root, "nimbus", &v1).unwrap();
    assert_eq!(first.copied.len(), 3);
    let dir = persona_dir(&root, "nimbus").unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("packs/fish/core.md")).unwrap(), "fish v1");

    // the person edits one file (the persona-note tool writes here too)
    std::fs::write(dir.join("packs/fish/core.md"), "fish, edited by the person").unwrap();
    // the app ships a new version of all three
    let v2 = resources("seed-res-2", &[("protocol.md", "protocol v2"), ("packs/fish/core.md", "fish v2"), ("packs/potato/core.md", "potato v2"), ("packs/potato/rules.md", "new file")]);
    let second = seed_persona(&root, "nimbus", &v2).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("packs/fish/core.md")).unwrap(), "fish, edited by the person", "an edited file is kept");
    assert_eq!(std::fs::read_to_string(dir.join("protocol.md")).unwrap(), "protocol v2", "an untouched file is updated");
    assert_eq!(std::fs::read_to_string(dir.join("packs/potato/core.md")).unwrap(), "potato v2");
    assert_eq!(std::fs::read_to_string(dir.join("packs/potato/rules.md")).unwrap(), "new file", "a new file is added");
    assert_eq!(second.kept_edited, ["packs/fish/core.md"]);
    assert_eq!(second.updated.len(), 2);
    assert_eq!(second.copied, ["packs/potato/rules.md"]);

    // the same version again changes nothing
    let third = seed_persona(&root, "nimbus", &v2).unwrap();
    assert!(third.copied.is_empty() && third.updated.is_empty());
    // the folder is writable for the persona-note tool
    assert!(!std::fs::metadata(&dir).unwrap().permissions().readonly());
    assert!(!std::fs::metadata(dir.join("protocol.md")).unwrap().permissions().readonly());
}

#[test]
fn forgetting_talk_memory_moves_the_brain_folder_and_deletes_nothing() {
    let root = temp("forget");
    ensure_brain_dirs(&root, "nimbus").unwrap();
    std::fs::write(vault_dir(&root, "nimbus").unwrap().join("journal.md"), "I remember the cat").unwrap();
    // the conversation stays: a thread is the person's, the vault is the bot's
    append_message(&root, "app:dm:nimbus", text(1, FromKind::You, "hi"), None).unwrap();
    let moved = forget_talk_memory(&root, "nimbus", "1700000000").unwrap().expect("moved");
    assert!(!brain_dir(&root, "nimbus").unwrap().exists());
    assert_eq!(std::fs::read_to_string(moved.join("vault").join("journal.md")).unwrap(), "I remember the cat");
    assert!(moved.starts_with(root.join(".removed")));
    assert_eq!(read_messages(&root, "app:dm:nimbus", 10, None).unwrap().len(), 1);
    // nothing to forget is fine, and a bad id is refused
    assert_eq!(forget_talk_memory(&root, "nimbus", "1700000001").unwrap(), None);
    assert!(forget_talk_memory(&root, "../x", "1").is_err());
}

#[test]
fn the_brain_folders_are_made_and_a_bad_bot_id_cannot_escape() {
    let root = temp("dirs");
    let cwd = ensure_brain_dirs(&root, "nimbus").unwrap();
    assert_eq!(cwd, root.join("brain").join("nimbus"));
    for dir in [vault_dir(&root, "nimbus").unwrap(), state_dir(&root, "nimbus").unwrap(), persona_dir(&root, "nimbus").unwrap(), share_dir(&root, "nimbus").unwrap()] {
        assert!(dir.is_dir(), "{}", dir.display());
    }
    assert_eq!(std::fs::read_dir(share_dir(&root, "nimbus").unwrap()).unwrap().count(), 0, "the share folder is empty");
    for bad in ["", "..", "../x", "a/b", "A", "a b"] {
        assert!(brain_dir(&root, bad).is_err(), "{bad:?}");
    }
}
