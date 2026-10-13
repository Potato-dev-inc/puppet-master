//! One brain: the renderer against temp projects, the profile/TOML declarations and the spawn hook.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::tui::declarative::loader::load_text;
use crate::tui::declarative::sources::Origin;
use crate::tui::declarative::validate::Source;
use crate::tui::declarative::DeclarativeProfile;

const KILO_TOML: &str = include_str!("../../tests/agent-definitions/kilo.toml");

fn project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-one-brain-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn target(path: &str) -> Target {
    Target { path: path.into(), header: None, agents: vec!["x".into()] }
}

fn put_memory(dir: &Path, text: &str) {
    write_memory(dir, text).unwrap();
}

fn read(dir: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(dir.join(rel)).unwrap()
}

fn find(hay: &[u8], needle: &str) -> usize {
    hay.windows(needle.len()).position(|w| w == needle.as_bytes()).expect("marker present")
}

fn declarative(toml: &str) -> Arc<dyn TuiProfile> {
    let file = load_text(Path::new("x.toml"), toml, toml.len(), Origin::new(Source::User), None, &["claude", "codex"]);
    assert!(file.errors.is_empty(), "{:?}", file.errors);
    Arc::new(DeclarativeProfile::new(file.definition.unwrap()).unwrap())
}

#[test]
fn insert_keeps_user_content_above_and_below_byte_for_byte() {
    let dir = project();
    let user = "# My project\n\nUse tabs.\n";
    std::fs::write(dir.join("CLAUDE.md"), user).unwrap();
    put_memory(&dir, "We use Postgres.");
    let report = sync(&dir, &[target("CLAUDE.md")], true);
    assert_eq!(report.written, vec!["CLAUDE.md"]);
    let inserted = read(&dir, "CLAUDE.md");
    assert!(inserted.starts_with(user.as_bytes()), "user text above is untouched");
    assert!(String::from_utf8_lossy(&inserted).contains("We use Postgres."));

    // the user then writes below the block; an update must leave both sides identical
    let mut with_tail = inserted.clone();
    with_tail.extend_from_slice(b"\n## Notes\nkeep me\n");
    std::fs::write(dir.join("CLAUDE.md"), &with_tail).unwrap();
    put_memory(&dir, "We use Postgres 16.\nAnd Redis.");
    sync(&dir, &[target("CLAUDE.md")], true);
    let updated = read(&dir, "CLAUDE.md");
    let (s0, e0) = (find(&with_tail, START), find(&with_tail, END) + END.len());
    let (s1, e1) = (find(&updated, START), find(&updated, END) + END.len());
    assert_eq!(&with_tail[..s0], &updated[..s1], "bytes above the block");
    assert_eq!(&with_tail[e0..], &updated[e1..], "bytes below the block");
    assert!(String::from_utf8_lossy(&updated).contains("Postgres 16."));
    assert_eq!(updated.windows(START.len()).filter(|w| *w == START.as_bytes()).count(), 1);
}

#[test]
fn second_sync_is_a_no_write() {
    let dir = project();
    put_memory(&dir, "facts");
    let t = [target("AGENTS.md")];
    assert_eq!(sync(&dir, &t, true).written.len(), 1);
    let before = std::fs::metadata(dir.join("AGENTS.md")).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let again = sync(&dir, &t, true);
    assert!(again.written.is_empty() && again.removed.is_empty() && again.errors.is_empty());
    assert_eq!(std::fs::metadata(dir.join("AGENTS.md")).unwrap().modified().unwrap(), before);
}

#[test]
fn empty_memory_restores_the_original_bytes_and_deletes_a_file_it_created() {
    let dir = project();
    let original = "# Mine\nline\n";
    std::fs::write(dir.join("CLAUDE.md"), original).unwrap();
    let t = [target("CLAUDE.md"), target("AGENTS.md")];
    put_memory(&dir, "facts");
    sync(&dir, &t, true);
    assert!(dir.join("AGENTS.md").exists());
    put_memory(&dir, "  \n");
    let report = sync(&dir, &t, true);
    assert_eq!(read(&dir, "CLAUDE.md"), original.as_bytes(), "the user's file is back as it was");
    assert!(!dir.join("AGENTS.md").exists(), "a file only we created is removed");
    assert_eq!(report.removed.len(), 2);
}

#[test]
fn missing_file_is_created_with_only_the_block_and_missing_memory_creates_nothing() {
    let dir = project();
    let t = [target("AGENTS.md")];
    assert!(sync(&dir, &t, true).written.is_empty(), "no memory, no file");
    assert!(!dir.join("AGENTS.md").exists());
    put_memory(&dir, "facts");
    sync(&dir, &t, true);
    let text = String::from_utf8(read(&dir, "AGENTS.md")).unwrap();
    assert!(text.starts_with(START) && text.trim_end().ends_with(END), "{text}");
    assert!(text.contains("facts"));
}

#[test]
fn nested_target_with_a_header_is_created_once() {
    let dir = project();
    put_memory(&dir, "facts");
    let t = [Target {
        path: ".cursor/rules/puppet-master.mdc".into(),
        header: Some("---\nalwaysApply: true\n---".into()),
        agents: vec![],
    }];
    sync(&dir, &t, true);
    let text = String::from_utf8(read(&dir, ".cursor/rules/puppet-master.mdc")).unwrap();
    assert!(text.starts_with("---\nalwaysApply: true\n---\n"), "{text}");
    assert!(text.contains(START));
    // an existing file without front-matter does not get the header
    std::fs::write(dir.join("other.mdc"), "mine\n").unwrap();
    let t2 = [Target { path: "other.mdc".into(), header: Some("---\nx: y\n---".into()), agents: vec![] }];
    sync(&dir, &t2, true);
    assert!(String::from_utf8(read(&dir, "other.mdc")).unwrap().starts_with("mine\n"));
}

#[test]
fn crlf_files_stay_crlf() {
    let dir = project();
    std::fs::write(dir.join("CLAUDE.md"), "# Title\r\nbody\r\n").unwrap();
    put_memory(&dir, "line one\nline two");
    sync(&dir, &[target("CLAUDE.md")], true);
    let bytes = read(&dir, "CLAUDE.md");
    let lone_lf = bytes.iter().enumerate().any(|(i, b)| *b == b'\n' && (i == 0 || bytes[i - 1] != b'\r'));
    assert!(!lone_lf, "no bare LF in a CRLF file");
    assert!(bytes.starts_with(b"# Title\r\nbody\r\n"));
}

#[test]
fn bom_is_kept_and_never_added() {
    let dir = project();
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(b"# Hi\n");
    std::fs::write(dir.join("CLAUDE.md"), &with_bom).unwrap();
    put_memory(&dir, "facts");
    sync(&dir, &[target("CLAUDE.md"), target("AGENTS.md")], true);
    assert!(read(&dir, "CLAUDE.md").starts_with(&[0xEF, 0xBB, 0xBF]));
    assert!(!read(&dir, "AGENTS.md").starts_with(&[0xEF, 0xBB, 0xBF]));
}

#[test]
fn backup_is_made_once_before_the_first_write_to_a_user_file() {
    let dir = project();
    let original = "# precious\n";
    std::fs::write(dir.join("CLAUDE.md"), original).unwrap();
    put_memory(&dir, "one");
    let t = [target("CLAUDE.md")];
    sync(&dir, &t, true);
    assert_eq!(std::fs::read_to_string(dir.join("CLAUDE.md.bak")).unwrap(), original);
    put_memory(&dir, "two");
    sync(&dir, &t, true);
    assert_eq!(std::fs::read_to_string(dir.join("CLAUDE.md.bak")).unwrap(), original, ".bak is not overwritten");
    // a file we created has nothing to back up
    sync(&dir, &[target("AGENTS.md")], true);
    assert!(!dir.join("AGENTS.md.bak").exists());
}

#[test]
fn damaged_markers_are_reported_and_the_file_is_left_alone() {
    let dir = project();
    let broken = format!("keep\n{START}\nhalf a block\n");
    std::fs::write(dir.join("CLAUDE.md"), &broken).unwrap();
    put_memory(&dir, "facts");
    let report = sync(&dir, &[target("CLAUDE.md")], true);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(read(&dir, "CLAUDE.md"), broken.as_bytes());
    assert_eq!(status(&dir, &[target("CLAUDE.md")], true)[0].status, "blocked");
}

#[test]
fn switches_remove_the_blocks() {
    let dir = project();
    put_memory(&dir, "facts");
    let t = [target("AGENTS.md")];
    sync(&dir, &t, true);
    assert!(dir.join("AGENTS.md").exists());
    set_project_enabled(&dir, false).unwrap();
    assert!(!project_enabled(&dir));
    sync(&dir, &t, true);
    assert!(!dir.join("AGENTS.md").exists(), "per-project opt-out");
    set_project_enabled(&dir, true).unwrap();
    sync(&dir, &t, true);
    assert!(dir.join("AGENTS.md").exists());
    sync(&dir, &t, false);
    assert!(!dir.join("AGENTS.md").exists(), "app-wide switch off");
}

#[test]
fn status_names_each_state() {
    let dir = project();
    put_memory(&dir, "facts");
    std::fs::write(dir.join("CLAUDE.md"), "mine\n").unwrap();
    let t = [target("CLAUDE.md"), target("AGENTS.md")];
    let s = status(&dir, &t, true);
    assert_eq!((s[0].status, s[0].user_content), ("will_add", true));
    assert_eq!((s[1].status, s[1].user_content), ("will_create", false));
    sync(&dir, &t, true);
    let s = status(&dir, &t, true);
    assert_eq!((s[0].status, s[0].user_content), ("in_sync", true));
    assert_eq!((s[1].status, s[1].user_content), ("in_sync", false));
    put_memory(&dir, "changed");
    assert_eq!(status(&dir, &t, true)[0].status, "will_update");
    put_memory(&dir, "");
    assert_eq!(status(&dir, &t, true)[0].status, "will_remove");
}

#[test]
fn paths_must_stay_inside_the_project() {
    for ok in ["AGENTS.md", ".cursor/rules/x.mdc", "a\\b.md"] {
        assert!(relative_path_ok(ok), "{ok}");
    }
    for bad in ["", "/etc/passwd", "\\x", "../x", "a/../../x", "C:/x", "a//b", "./x"] {
        assert!(!relative_path_ok(bad), "{bad}");
    }
}

#[test]
fn code_profiles_declare_only_verified_files() {
    let _serial = crate::tui::registry::test_serial();
    let by_agent = |id: &str| {
        crate::tui::profile_for(id).instruction_files().into_iter().map(|f| f.path).collect::<Vec<_>>()
    };
    assert_eq!(by_agent("claude"), ["CLAUDE.md"]);
    assert_eq!(by_agent("codex"), ["AGENTS.md"]);
    assert_eq!(by_agent("opencode"), ["AGENTS.md"]);
    assert_eq!(by_agent("cursor_agent"), [".cursor/rules/puppet-master.mdc"]);
    for undeclared in ["cmd", "powershell", "bash", "no-such-agent"] {
        assert!(by_agent(undeclared).is_empty(), "{undeclared} reads no declared file");
    }
}

#[test]
fn toml_declares_several_files_with_a_header_and_no_table_means_none() {
    let none = KILO_TOML.replace("\r\n", "\n").replace("[instructions]\nfile = \"AGENTS.md\"", "");
    assert!(!none.contains("[instructions]"));
    assert!(declarative(&none).instruction_files().is_empty(), "an agent without a declaration has no files");

    let one = declarative(KILO_TOML);
    assert_eq!(one.instruction_files().into_iter().map(|f| f.path).collect::<Vec<_>>(), ["AGENTS.md"]);

    let many_toml = KILO_TOML.replace(
        "file = \"AGENTS.md\"",
        "files = [\"AGENTS.md\", \".kilo/rules/pm.md\"]\nheader = \"<!-- hdr -->\"",
    );
    let many = declarative(&many_toml);
    let files = many.instruction_files();
    assert_eq!(files.len(), 2);
    assert_eq!(files[1].path, ".kilo/rules/pm.md");
    assert_eq!(files[1].header.as_deref(), Some("<!-- hdr -->"));

    // every declared file is written; the agent without a declaration adds none
    let targets = targets_from(&[many, declarative(&none)]);
    assert_eq!(targets.len(), 2);
    let dir = project();
    put_memory(&dir, "facts");
    sync(&dir, &targets, true);
    assert!(dir.join("AGENTS.md").exists() && dir.join(".kilo/rules/pm.md").exists());
    assert!(String::from_utf8(read(&dir, ".kilo/rules/pm.md")).unwrap().starts_with("<!-- hdr -->"));
}

#[test]
fn a_definition_cannot_declare_a_path_outside_the_project() {
    let bad = KILO_TOML.replace("file = \"AGENTS.md\"", "file = \"../../.bashrc\"");
    let file = load_text(Path::new("x.toml"), &bad, bad.len(), Origin::new(Source::User), None, &["claude", "codex"]);
    assert!(file.errors.iter().any(|e| e.path.starts_with("instructions")), "{:?}", file.errors);
}

#[test]
fn the_spawn_hook_syncs_every_registered_agents_file_in_the_cwd() {
    let _serial = crate::tui::registry::test_serial();
    let dir = project();
    std::fs::write(dir.join("CLAUDE.md"), "# mine\n").unwrap();
    put_memory(&dir, "Spawn-time fact.");
    let report = sync_for_spawn(&dir.display().to_string(), true);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    for rel in ["CLAUDE.md", "AGENTS.md", ".cursor/rules/puppet-master.mdc"] {
        assert!(String::from_utf8_lossy(&read(&dir, rel)).contains("Spawn-time fact."), "{rel}");
    }
    // a project without a memory is not touched by a spawn
    let bare = project();
    sync_for_spawn(&bare.display().to_string(), true);
    assert_eq!(std::fs::read_dir(&bare).unwrap().count(), 0);
    // a missing cwd is ignored
    assert_eq!(sync_for_spawn("Z:/does/not/exist", true), SyncReport::default());
}

/// The hook must stay wired into both spawn paths (a pane and a headless run).
#[test]
fn spawn_paths_call_the_hook() {
    let pane = include_str!("../pty/registry/spawn.rs");
    let headless = include_str!("../agent_runs/headless/process.rs");
    assert!(pane.contains("crate::one_brain::sync_on_spawn(&cwd)"));
    assert!(headless.contains("crate::one_brain::sync_on_spawn(&project)"));
}
