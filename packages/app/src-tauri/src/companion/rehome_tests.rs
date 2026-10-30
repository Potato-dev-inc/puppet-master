//! A companion whose home is the Desktop: it is flagged, `Move to its own folder` repairs it, and a failed
//! move changes nothing. Temp folders only; the fake host answers `places()` with a fake account home.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::broad::Places;
use super::brief;
use super::files;
use super::keeper::{Pane, Status};
use super::model::{CompanionProfile, CompanionState};
use super::store;
use super::test_host::{draft, service_with_one, start_ms, temp_dir, FakeHost};
use super::Service;
use crate::tui::InstructionFile;

struct Leak {
    service: Service,
    host: Arc<FakeHost>,
    /// The fake account home.
    home: PathBuf,
    /// `<home>/Desktop`, the companion's home.
    desk: PathBuf,
}

fn profile(folder: &Path) -> CompanionProfile {
    CompanionProfile {
        id: "mochi".into(),
        name: "Mochi".into(),
        avatar: "mochi".into(),
        persona: "calm".into(),
        agent_type: "claude".into(),
        launch_args: vec![],
        account: None,
        folder: folder.display().to_string(),
        auto_start: true,
        memory_review: true,
        push_replies: true,
        share_today: false,
        nap_at: None,
        checkins: vec![],
        previous_agent: None,
        until_ms: None,
        created_ms: 1,
        archived: false,
        pane_ids: vec!["companion-mochi".into()],
    }
}

/// A saved companion that lives on the Desktop of a fake account, already started once.
/// `desk_agents`: the text the Desktop's `AGENTS.md` had before the companion came (None = no such file).
fn leaky(desk_agents: Option<&str>) -> Leak {
    let root = temp_dir("pm-rehome-root");
    let home = temp_dir("pm-rehome-home");
    let desk = home.join("Desktop");
    fs::create_dir_all(&desk).unwrap();
    if let Some(text) = desk_agents {
        fs::write(desk.join("AGENTS.md"), text).unwrap();
    }
    fs::write(desk.join("notes.txt"), "the person's own file").unwrap();
    store::save(&root, &[profile(&desk)]).unwrap();
    let host = FakeHost::new(start_ms());
    *host.places.lock() = Places::for_home(&home);
    let service = Service::new(host.clone(), root);
    service.tick();
    assert_eq!(host.spawn_count(), 1, "it started once, in the Desktop");
    Leak { service, host, home, desk }
}

fn new_home(leak: &Leak) -> PathBuf {
    files::default_folder(&leak.home, "mochi")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn view(leak: &Leak) -> super::model::CompanionView {
    leak.service.view("mochi", false).unwrap()
}

#[test]
fn a_companion_on_the_desktop_is_flagged_with_the_place_and_the_new_folder() {
    let leak = leaky(None);
    let problem = view(&leak).home_problem.expect("flagged");
    assert_eq!(problem.place, "your Desktop");
    assert_eq!(problem.suggested_folder, new_home(&leak).display().to_string());
    // The phone never gets it.
    assert!(leak.service.view("mochi", true).unwrap().home_problem.is_none());
    // And the desktop event carries it.
    let published = leak.host.published.lock().last().cloned().unwrap();
    assert!(published[0].home_problem.is_some());
}

#[test]
fn a_companion_in_a_folder_of_its_own_is_not_flagged() {
    let (service, _host, _root) = service_with_one();
    assert!(service.view("mochi", false).unwrap().home_problem.is_none());
}

#[test]
fn the_brief_in_the_desktop_starts_with_the_guard_for_that_exact_folder() {
    let leak = leaky(None);
    let text = read(&leak.desk.join("AGENTS.md"));
    let guard = brief::guard_line("Mochi", &leak.desk.display().to_string());
    let start = text.find(brief::COMPANION_START).unwrap();
    let body = &text[start..];
    let first = body.lines().nth(2).unwrap();
    assert_eq!(first, guard);
    assert!(first.contains("ignore everything in this block"));
}

#[test]
fn moving_copies_its_files_cuts_only_its_block_and_starts_it_in_the_new_folder() {
    let leak = leaky(Some("My own rules.\n"));
    fs::write(leak.desk.join("SOUL.md"), "I am calm and I like tea.").unwrap();
    fs::write(leak.desk.join("memory").join("pending.md"), "- note: likes mornings\n").unwrap();
    fs::write(leak.desk.join("memory").join("journal").join("2026-10-01.md"), "a day").unwrap();
    fs::write(leak.desk.join("skills").join("tea.md"), "# Tea\n\nBoil water.\n").unwrap();
    // A folder of the person's own next to it, which a move must not touch.
    fs::create_dir_all(leak.desk.join("work2").join("shop")).unwrap();
    fs::write(leak.desk.join("work2").join("shop").join("a.txt"), "project").unwrap();

    let moved = leak.service.rehome("mochi").unwrap();
    let new = new_home(&leak);

    // The profile and the view.
    assert_eq!(moved.folder.as_deref(), Some(new.display().to_string().as_str()));
    assert!(moved.home_problem.is_none());
    assert_eq!(store::load(&leak.service.root).unwrap()[0].folder, new.display().to_string());

    // Its files are in the new folder, byte for byte.
    assert_eq!(read(&new.join("SOUL.md")), "I am calm and I like tea.");
    assert_eq!(read(&new.join("memory").join("pending.md")), "- note: likes mornings\n");
    assert_eq!(read(&new.join("memory").join("journal").join("2026-10-01.md")), "a day");
    assert_eq!(read(&new.join("skills").join("tea.md")), "# Tea\n\nBoil water.\n");
    assert!(new.join(files::MARKER_REL).is_file());
    // The brief was written in the new home, with the guard for the new folder.
    let brief_text = read(&new.join("AGENTS.md"));
    assert!(brief_text.contains(&brief::guard_line("Mochi", &new.display().to_string())));

    // The old folder: its own files are gone, the person's things and the other content stay.
    assert!(!leak.desk.join("SOUL.md").exists());
    assert!(!leak.desk.join("USER.md").exists());
    assert!(!leak.desk.join("memory").exists());
    assert!(!leak.desk.join("skills").exists());
    assert!(!leak.desk.join(".puppet-master").exists());
    assert_eq!(read(&leak.desk.join("AGENTS.md")), "My own rules.\n");
    assert_eq!(read(&leak.desk.join("notes.txt")), "the person's own file");
    assert_eq!(read(&leak.desk.join("work2").join("shop").join("a.txt")), "project");
    assert!(leak.desk.join("AGENTS.md.bak").is_file(), "the copy made before the block was added stays");

    // The pane was closed and started again in the new folder.
    assert!(leak.host.killed.lock().contains(&"companion-mochi".to_string()));
    let spawned = leak.host.spawned.lock().clone();
    assert_eq!(spawned.len(), 2);
    assert_eq!(spawned[1].cwd, new.display().to_string());
}

#[test]
fn an_instruction_file_that_held_only_the_block_is_removed() {
    let leak = leaky(None);
    assert!(leak.desk.join("AGENTS.md").is_file());
    leak.service.rehome("mochi").unwrap();
    assert!(!leak.desk.join("AGENTS.md").exists());
    assert!(new_home(&leak).join("AGENTS.md").is_file());
    assert!(leak.desk.join("notes.txt").is_file());
}

#[test]
fn the_memory_the_person_approved_follows_the_files() {
    let leak = leaky(None);
    fs::write(leak.desk.join("MEMORY.md"), "- Likes short answers.\n").unwrap();
    leak.service.rehome("mochi").unwrap();
    let memory = leak.service.memory_view("mochi", false).unwrap();
    assert_eq!(memory.memory, "- Likes short answers.\n");
    // The approved copy in the app data is keyed by the id, so a file that changed outside is still known.
    assert!(memory.outside_changes.iter().all(|c| c.file != super::model::MemoryFile::Soul));
}

#[test]
fn a_new_folder_that_is_not_empty_is_refused_and_nothing_changes() {
    let leak = leaky(Some("My own rules.\n"));
    let new = new_home(&leak);
    fs::create_dir_all(&new).unwrap();
    fs::write(new.join("something.txt"), "already here").unwrap();
    let before = read(&leak.desk.join("AGENTS.md"));

    let err = leak.service.rehome("mochi").unwrap_err();
    assert_eq!(err.code, "COMPANION_REHOME_FAILED");
    assert!(err.message.contains("not empty"), "{}", err.message);
    assert_eq!(read(&new.join("something.txt")), "already here");
    assert_eq!(fs::read_dir(&new).unwrap().count(), 1);
    assert_eq!(read(&leak.desk.join("AGENTS.md")), before);
    assert!(leak.desk.join("SOUL.md").is_file());
    assert_eq!(view(&leak).folder.as_deref(), Some(leak.desk.display().to_string().as_str()));
    assert_eq!(leak.host.spawn_count(), 1, "nothing was stopped or started");
}

#[test]
fn damaged_markers_in_the_old_file_refuse_the_move_before_anything_is_copied() {
    let leak = leaky(Some("Rules.\n"));
    let path = leak.desk.join("AGENTS.md");
    let text = read(&path).replace(brief::COMPANION_END, "");
    fs::write(&path, &text).unwrap();
    let err = leak.service.rehome("mochi").unwrap_err();
    assert_eq!(err.code, "COMPANION_MARKER_DAMAGED");
    assert!(!new_home(&leak).exists());
    assert_eq!(read(&path), text);
    assert!(leak.desk.join("SOUL.md").is_file());
}

#[test]
fn a_failure_while_writing_the_new_brief_puts_everything_back() {
    let leak = leaky(Some("My own rules.\n"));
    fs::write(leak.desk.join("SOUL.md"), "I am calm.").unwrap();
    let agents_before = read(&leak.desk.join("AGENTS.md"));
    // The copy works, then writing the brief in the new home fails.
    *leak.host.files.lock() = vec![InstructionFile::new("../escape.md")];

    let err = leak.service.rehome("mochi").unwrap_err();
    assert_eq!(err.code, "COMPANION_REHOME_FAILED");
    assert!(err.message.contains("instruction file path is not allowed"), "{}", err.message);

    assert!(!new_home(&leak).exists(), "the new folder this move made is gone again");
    assert_eq!(read(&leak.desk.join("SOUL.md")), "I am calm.");
    assert_eq!(read(&leak.desk.join("AGENTS.md")), agents_before);
    assert!(leak.desk.join("memory").join("pending.md").is_file());
    assert_eq!(view(&leak).folder.as_deref(), Some(leak.desk.display().to_string().as_str()));
    assert_eq!(store::load(&leak.service.root).unwrap()[0].folder, leak.desk.display().to_string());
}

#[test]
fn a_failure_when_saving_the_profile_puts_the_old_block_back_and_starts_it_where_it_was() {
    let leak = leaky(Some("My own rules.\n"));
    fs::write(leak.desk.join("SOUL.md"), "I am calm.").unwrap();
    let agents_before = read(&leak.desk.join("AGENTS.md"));
    // The last step before the switch cannot write: companions.json is a folder now.
    let saved = leak.service.root.join("companions.json");
    fs::remove_file(&saved).unwrap();
    fs::create_dir(&saved).unwrap();

    let err = leak.service.rehome("mochi").unwrap_err();
    assert_eq!(err.code, "COMPANION_STORE_INVALID");

    assert!(!new_home(&leak).exists());
    assert_eq!(read(&leak.desk.join("SOUL.md")), "I am calm.");
    assert_eq!(read(&leak.desk.join("AGENTS.md")), agents_before, "the block was cut and is back");
    assert_eq!(view(&leak).folder.as_deref(), Some(leak.desk.display().to_string().as_str()));
    // The pane was closed for the copy and starts again in the old folder.
    let spawned = leak.host.spawned.lock().clone();
    assert_eq!(spawned.len(), 2);
    assert_eq!(spawned[1].cwd, leak.desk.display().to_string());
}

#[test]
fn a_sleeping_companion_stays_asleep_through_the_move() {
    let leak = leaky(None);
    leak.service.sleep("mochi").unwrap();
    let spawns = leak.host.spawn_count();
    let moved = leak.service.rehome("mochi").unwrap();
    assert_eq!(moved.state, CompanionState::Sleeping);
    assert_eq!(leak.host.spawn_count(), spawns);
    assert!(new_home(&leak).join("SOUL.md").is_file());
}

#[test]
fn a_companion_that_already_has_its_own_folder_is_not_moved() {
    let (service, host, _root) = service_with_one();
    let spawns = host.spawn_count();
    let err = service.rehome("mochi").unwrap_err();
    assert_eq!(err.code, "COMPANION_REHOME_FAILED");
    assert!(err.message.contains("already has a folder of its own"), "{}", err.message);
    assert_eq!(host.spawn_count(), spawns);
    assert_eq!(service.rehome("nobody").unwrap_err().code, "COMPANION_NOT_FOUND");
}

#[test]
fn a_project_folder_is_flagged_and_moves_out_too() {
    // The old setup sheet offered the open project as the home.
    let root = temp_dir("pm-rehome-root");
    let home = temp_dir("pm-rehome-home");
    let project = temp_dir("pm-rehome-project");
    fs::create_dir_all(project.join(".git")).unwrap();
    store::save(&root, &[profile(&project)]).unwrap();
    let host = FakeHost::new(start_ms());
    *host.places.lock() = Places::for_home(&home);
    let service = Service::new(host.clone(), root);
    host.set_pane("companion-mochi", Pane::Alive(Status::Idle));
    service.tick();
    let problem = service.view("mochi", false).unwrap().home_problem.unwrap();
    assert_eq!(problem.place, "a folder with other projects in it");
    service.rehome("mochi").unwrap();
    assert!(files::default_folder(&home, "mochi").join("SOUL.md").is_file());
    assert!(project.join(".git").is_dir(), "the project itself is untouched");
}

#[test]
fn creating_or_moving_a_companion_into_the_desktop_is_refused_in_plain_words() {
    let home = temp_dir("pm-rehome-home");
    let desk = home.join("Desktop");
    fs::create_dir_all(&desk).unwrap();
    let root = temp_dir("pm-rehome-root");
    let host = FakeHost::new(start_ms());
    *host.places.lock() = Places::for_home(&home);
    let service = Service::new(host, root);

    let err = service.create(draft("Pipi", &desk)).unwrap_err();
    assert_eq!(err.code, "COMPANION_FOLDER_BROAD");
    assert!(err.message.starts_with("Pick a folder of its own. Your Desktop has other projects in it,"), "{}", err.message);
    assert!(err.message.contains("agents there would read Pipi's notes."));
    assert!(err.message.contains(&files::default_folder(&home, "pipi").display().to_string()));
    assert!(service.views(false).is_empty());

    // A folder of its own, even inside the Desktop, is fine; moving it to the Desktop is not.
    service.create(draft("Pipi", &desk.join("pipi"))).unwrap();
    let patch: super::manage::Patch = serde_json::from_value(serde_json::json!({ "folder": desk.display().to_string() })).unwrap();
    let err = service.update("pipi", patch).unwrap_err();
    assert_eq!(err.code, "COMPANION_FOLDER_BROAD");
    assert_eq!(service.view("pipi", false).unwrap().folder, Some(desk.join("pipi").display().to_string()));
}

#[test]
fn an_original_that_changed_after_the_copy_is_not_removed() {
    let old = temp_dir("pm-rehome-old");
    let new = temp_dir("pm-rehome-new");
    fs::write(old.join("SOUL.md"), "one").unwrap();
    fs::write(old.join("USER.md"), "two").unwrap();
    let copied = super::rehome::copy_home(&old, &new).unwrap();
    assert_eq!(copied.files, vec!["SOUL.md".to_string(), "USER.md".to_string()]);
    fs::write(old.join("USER.md"), "two, edited later").unwrap();
    let stayed = super::rehome::drop_originals(&old, &new, &copied);
    assert_eq!(stayed, 1);
    assert!(!old.join("SOUL.md").exists());
    assert_eq!(read(&old.join("USER.md")), "two, edited later");
}
