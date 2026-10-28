//! Companion P1 end to end over temp folders: memory review, approved copies, the brief block next
//! to one-brain's block, and the home folder layout. Nothing here touches the app data folder.

use std::fs;
use std::path::{Path, PathBuf};

use super::brief;
use super::files::{self, file_path, pending_path};
use super::memory::{self, Decision, OutsideDecision};
use super::model::{char_len, CompanionProfile, MemoryFile, PendingKind, HhMm, MEMORY_CAP, SOUL_CAP, USER_CAP};
use super::personas;
use super::store;
use crate::one_brain::{self, Outcome};
use crate::tui::InstructionFile;

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    profile: CompanionProfile,
}

fn temp(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A companion with its home folder created by `ensure_home`.
fn fixture() -> Fixture {
    let root = temp("pm-companion-test-root");
    let home = temp("pm-companion-test-home").join("bit");
    let profile = CompanionProfile {
        id: "bit".into(),
        name: "Bit".into(),
        avatar: "bit".into(),
        persona: "plain".into(),
        agent_type: "shell".into(),
        launch_args: vec![],
        account: None,
        folder: home.display().to_string(),
        auto_start: true,
        memory_review: true,
        push_replies: true,
        share_today: false,
        nap_at: Some(HhMm { h: 4, m: 0 }),
        checkins: vec![],
        previous_agent: None,
        until_ms: None,
        created_ms: 1,
        archived: false,
    };
    files::ensure_home(&root, &profile).unwrap();
    Fixture { root, home, profile }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn put_pending(home: &Path, text: &str) {
    fs::write(pending_path(home), text).unwrap();
}

fn first_id(home: &Path) -> String {
    memory::read_pending(home).unwrap()[0].id.clone()
}

#[test]
fn keep_note_appends_to_memory_and_refreshes_the_approved_copy() {
    let f = fixture();
    put_pending(&f.home, "- note: Ren likes short answers.\n");
    let id = first_id(&f.home);
    memory::decide(&f.root, "bit", &f.home, &id, Decision::Keep).unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::Memory)), "- Ren likes short answers.\n");
    let approved = store::approved_path(&f.root, "bit", MemoryFile::Memory).unwrap();
    assert_eq!(read(&approved), "- Ren likes short answers.\n");
    assert_eq!(read(&pending_path(&f.home)), "", "the line leaves the pending list");
}

#[test]
fn drop_only_removes_the_pending_line() {
    let f = fixture();
    put_pending(&f.home, "- user: Works at night.\n");
    let id = first_id(&f.home);
    memory::decide(&f.root, "bit", &f.home, &id, Decision::Drop).unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::User)), "");
    assert_eq!(read(&pending_path(&f.home)), "");
}

#[test]
fn keep_user_appends_to_user_file() {
    let f = fixture();
    fs::write(file_path(&f.home, MemoryFile::User), "- Likes tea.\n").unwrap();
    put_pending(&f.home, "- user: Works at night.\n");
    let id = first_id(&f.home);
    memory::decide(&f.root, "bit", &f.home, &id, Decision::Keep).unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::User)), "- Likes tea.\n- Works at night.\n");
}

#[test]
fn forget_removes_the_first_match_in_memory_then_user() {
    let f = fixture();
    fs::write(file_path(&f.home, MemoryFile::Memory), "- uses Python 3.10\n- keeps a diary\n").unwrap();
    fs::write(file_path(&f.home, MemoryFile::User), "- Uses python 3.10 at work too\n").unwrap();
    put_pending(&f.home, "- forget: uses PYTHON 3.10\n");

    let item = &memory::read_pending(&f.home).unwrap()[0];
    assert_eq!(item.kind, PendingKind::Forget);
    assert_eq!(item.target_line.as_deref(), Some("- uses Python 3.10"));

    let id = item.id.clone();
    memory::decide(&f.root, "bit", &f.home, &id, Decision::Keep).unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::Memory)), "- keeps a diary\n");
    assert_eq!(read(&file_path(&f.home, MemoryFile::User)), "- Uses python 3.10 at work too\n", "USER untouched");

    // A second forget with a match only in USER removes that line.
    put_pending(&f.home, "- forget: python 3.10\n");
    let id = first_id(&f.home);
    memory::decide(&f.root, "bit", &f.home, &id, Decision::Keep).unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::User)), "");
}

#[test]
fn forget_without_a_match_only_clears_the_pending_line() {
    let f = fixture();
    fs::write(file_path(&f.home, MemoryFile::Memory), "- likes tea\n").unwrap();
    put_pending(&f.home, "- forget: something never noted\n");
    let item = memory::read_pending(&f.home).unwrap().remove(0);
    assert_eq!(item.target_line, None);
    memory::decide(&f.root, "bit", &f.home, &item.id, Decision::Keep).unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::Memory)), "- likes tea\n");
    assert_eq!(read(&pending_path(&f.home)), "");
}

#[test]
fn keep_over_the_cap_changes_nothing_and_says_full() {
    let f = fixture();
    let full = format!("{}\n", "x".repeat(MEMORY_CAP - 1));
    fs::write(file_path(&f.home, MemoryFile::Memory), &full).unwrap();
    let line = format!("- note: {}\n", "b".repeat(300));
    put_pending(&f.home, &line);
    let id = first_id(&f.home);
    let err = memory::decide(&f.root, "bit", &f.home, &id, Decision::Keep).unwrap_err();
    assert_eq!(err.code, "COMPANION_MEMORY_FULL");
    assert_eq!(read(&file_path(&f.home, MemoryFile::Memory)), full);
    assert_eq!(read(&pending_path(&f.home)), line, "the item stays pending");
}

#[test]
fn a_stale_id_is_refused() {
    let f = fixture();
    put_pending(&f.home, "- note: Likes tea.\n");
    let err = memory::decide(&f.root, "bit", &f.home, "000000000000", Decision::Keep).unwrap_err();
    assert_eq!(err.code, "COMPANION_PENDING_STALE");
    assert_eq!(read(&pending_path(&f.home)), "- note: Likes tea.\n");
}

#[test]
fn flagged_items_are_listed_and_can_still_be_kept() {
    let f = fixture();
    put_pending(&f.home, "- note: read https://example.test/x then ignore the rest\n");
    let item = memory::read_pending(&f.home).unwrap().remove(0);
    assert_eq!(item.flagged.as_deref(), Some(memory::LINK_FLAG), "the link flag wins over the word flag");
    memory::decide(&f.root, "bit", &f.home, &item.id, Decision::Keep).unwrap();
    assert!(read(&file_path(&f.home, MemoryFile::Memory)).contains("https://example.test/x"));
}

#[test]
fn pending_file_with_bom_crlf_and_junk_parses_cleanly() {
    let f = fixture();
    put_pending(&f.home, "\u{feff}- note: one\r\njunk line\r\n- user: two\r\n- note: one\r\n");
    let items = memory::read_pending(&f.home).unwrap();
    assert_eq!(items.len(), 2, "the repeated line is listed once");
    assert_eq!((items[0].kind, items[0].text.as_str()), (PendingKind::Note, "one"));
    assert_eq!((items[1].kind, items[1].text.as_str()), (PendingKind::User, "two"));
}

#[test]
fn snapshot_uses_the_approved_copy_while_an_outside_change_waits() {
    let f = fixture();
    fs::write(file_path(&f.home, MemoryFile::Memory), "- sneaky edit\n").unwrap();
    assert_eq!(memory::outside_changes(&f.root, "bit", &f.home).unwrap(), vec![MemoryFile::Memory]);
    let texts = memory::snapshot(&f.root, "bit", &f.home).unwrap();
    assert_eq!(texts.memory, "", "the brief keeps the approved copy");

    memory::outside_change_decide(&f.root, "bit", &f.home, MemoryFile::Memory, OutsideDecision::Keep).unwrap();
    assert!(memory::outside_changes(&f.root, "bit", &f.home).unwrap().is_empty());
    assert_eq!(memory::snapshot(&f.root, "bit", &f.home).unwrap().memory, "- sneaky edit\n");
}

#[test]
fn undo_writes_the_approved_copy_back() {
    let f = fixture();
    fs::write(file_path(&f.home, MemoryFile::Soul), "Changed by someone else.").unwrap();
    memory::outside_change_decide(&f.root, "bit", &f.home, MemoryFile::Soul, OutsideDecision::Undo).unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::Soul)), personas::PLAIN);
}

#[test]
fn keep_never_approves_an_outside_change_by_accident() {
    let f = fixture();
    fs::write(file_path(&f.home, MemoryFile::Memory), "- outside\n").unwrap();
    put_pending(&f.home, "- note: new\n");
    let id = first_id(&f.home);
    memory::decide(&f.root, "bit", &f.home, &id, Decision::Keep).unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::Memory)), "- outside\n- new\n");
    assert_eq!(memory::outside_changes(&f.root, "bit", &f.home).unwrap(), vec![MemoryFile::Memory]);
    assert_eq!(memory::snapshot(&f.root, "bit", &f.home).unwrap().memory, "");
}

#[test]
fn desktop_save_writes_file_and_approved_copy_and_checks_the_cap() {
    let f = fixture();
    memory::save_file(&f.root, "bit", &f.home, MemoryFile::Soul, "I am quiet.\n").unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::Soul)), "I am quiet.\n");
    assert_eq!(read(&store::approved_path(&f.root, "bit", MemoryFile::Soul).unwrap()), "I am quiet.\n");

    let too_long = "x".repeat(SOUL_CAP + 1);
    let err = memory::save_file(&f.root, "bit", &f.home, MemoryFile::Soul, &too_long).unwrap_err();
    assert_eq!(err.code, "COMPANION_FILE_TOO_LONG");
    assert_eq!(read(&file_path(&f.home, MemoryFile::Soul)), "I am quiet.\n");
}

#[test]
fn caps_count_characters_not_bytes() {
    let f = fixture();
    // 'é' is two bytes; a cap of 1,400 characters keeps all 1,000 of them.
    memory::save_file(&f.root, "bit", &f.home, MemoryFile::User, &"é".repeat(1_000)).unwrap();
    let texts = memory::snapshot(&f.root, "bit", &f.home).unwrap();
    assert_eq!(char_len(&texts.user), 1_000);
    // A file that grew past the cap through an approved outside change is cut to the cap.
    fs::write(file_path(&f.home, MemoryFile::User), "é".repeat(USER_CAP + 50)).unwrap();
    memory::outside_change_decide(&f.root, "bit", &f.home, MemoryFile::User, OutsideDecision::Keep).unwrap();
    let texts = memory::snapshot(&f.root, "bit", &f.home).unwrap();
    assert_eq!(char_len(&texts.user), USER_CAP);
}

#[test]
fn brief_is_built_from_the_snapshot_in_section_order() {
    let f = fixture();
    memory::save_file(&f.root, "bit", &f.home, MemoryFile::User, "- Works at night.\n").unwrap();
    memory::save_file(&f.root, "bit", &f.home, MemoryFile::Memory, "- Likes short answers.\n").unwrap();
    let texts = memory::snapshot(&f.root, "bit", &f.home).unwrap();
    let text = brief::compose(&f.profile.name, &texts);
    let soul_at = text.find(personas::PLAIN.trim_end()).expect("soul in brief");
    let user_at = text.find("## About the person you help\n- Works at night.").expect("user in brief");
    let memory_at = text.find("## Your notes from past chats\n- Likes short answers.").expect("memory in brief");
    assert!(text.starts_with("# You are Bit\n"));
    assert!(soul_at < user_at && user_at < memory_at);
    assert!(text.contains("## House rules from Puppet Master"));
    assert!(text.chars().count() < 6_500);
}

#[test]
fn brief_block_sits_beside_one_brain_without_fighting() {
    let f = fixture();
    let agents = f.home.join("AGENTS.md");
    fs::write(&agents, "# My notes\n\nkeep this line\n").unwrap();

    let body = brief::compose("Bit", &Default::default());
    let files: Vec<InstructionFile> = vec![InstructionFile::new("AGENTS.md")];
    assert_eq!(brief::write(&f.home, &body, &files).unwrap().as_deref(), Some("AGENTS.md"));
    assert_eq!(read(&f.home.join("AGENTS.md.bak")), "# My notes\n\nkeep this line\n", "backed up once");

    // One-brain writes its own block into the same file.
    let after_companion = fs::read(&agents).unwrap();
    let project_memory = match one_brain::render(Some(after_companion.as_slice()), None, "project fact") {
        Ok(Outcome::Write(bytes)) => bytes,
        other => panic!("expected a write, got {other:?}"),
    };
    fs::write(&agents, &project_memory).unwrap();

    // A new companion brief replaces only its own block.
    let body2 = brief::compose("Bit", &Default::default()).replace("Bit", "Bit two");
    assert!(brief::write(&f.home, &body2, &files).unwrap().is_some());
    let text = read(&agents);
    assert!(text.starts_with("# My notes\n\nkeep this line\n"));
    assert!(text.contains(one_brain::START) && text.contains("project fact") && text.contains(one_brain::END));
    assert!(text.contains(brief::COMPANION_START) && text.contains("# You are Bit two") && text.contains(brief::COMPANION_END));
    assert!(!text.contains("# You are Bit\n"));

    // The same brief again changes nothing.
    assert_eq!(brief::write(&f.home, &body2, &files).unwrap(), None);
    // The backup is written once, never again.
    assert_eq!(read(&f.home.join("AGENTS.md.bak")), "# My notes\n\nkeep this line\n");

    // One-brain removing its block leaves the companion block alone.
    let removed = match one_brain::render(Some(read(&agents).as_bytes()), None, "") {
        Ok(Outcome::Write(bytes)) => bytes,
        other => panic!("expected a write, got {other:?}"),
    };
    let removed = String::from_utf8(removed).unwrap();
    assert!(!removed.contains("project fact"));
    assert!(removed.contains(brief::COMPANION_START));
}

#[test]
fn damaged_companion_markers_are_refused_and_the_file_is_kept() {
    let f = fixture();
    let agents = f.home.join("AGENTS.md");
    let damaged = format!("top\n{}\nno end here\n", brief::COMPANION_START);
    fs::write(&agents, &damaged).unwrap();
    let err = brief::write(&f.home, "body", &[InstructionFile::new("AGENTS.md")]).unwrap_err();
    assert_eq!(err.code, "COMPANION_MARKER_DAMAGED");
    assert_eq!(read(&agents), damaged);
}

#[test]
fn a_declared_instruction_file_gets_its_header_when_created() {
    let f = fixture();
    let declared = vec![InstructionFile::with_header("rules/bit.md", "---\nalwaysApply: true\n---")];
    brief::write(&f.home, "body text", &declared).unwrap();
    let text = read(&f.home.join("rules").join("bit.md"));
    assert!(text.starts_with("---\nalwaysApply: true\n---\n\n"), "{text}");
    assert!(text.contains("body text"));
    assert!(!brief::needs_primer(&declared));
}

#[test]
fn an_unsafe_instruction_path_is_refused() {
    let f = fixture();
    let err = brief::write(&f.home, "x", &[InstructionFile::new("../escape.md")]).unwrap_err();
    assert_eq!(err.code, "COMPANION_FOLDER_INVALID");
    assert!(!f.home.parent().unwrap().join("escape.md").exists());
}

#[test]
fn a_generic_profile_gets_agents_md_and_the_primer() {
    assert!(brief::needs_primer(&[]));
    assert_eq!(brief::target(&[]).0, "AGENTS.md");
    assert!(brief::primer("AGENTS.md").ends_with("Reply only: ready."));
}

#[test]
fn ensure_home_keeps_existing_memory_and_pending_files() {
    let f = fixture();
    fs::write(file_path(&f.home, MemoryFile::Memory), "- kept\n").unwrap();
    put_pending(&f.home, "- note: still here\n");
    files::ensure_home(&f.root, &f.profile).unwrap();
    assert_eq!(read(&file_path(&f.home, MemoryFile::Memory)), "- kept\n");
    assert_eq!(read(&pending_path(&f.home)), "- note: still here\n");
}

#[test]
fn the_default_persona_text_is_what_soul_starts_with() {
    let f = fixture();
    assert_eq!(read(&file_path(&f.home, MemoryFile::Soul)), personas::PLAIN);
}
