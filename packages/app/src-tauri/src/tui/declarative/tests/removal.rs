//! Remove / restore of a user definition (removal.rs): files move to `.removed/`, nothing is
//! deleted, the approval is dropped and only comes back for a byte-identical file. Everything runs
//! against a temp tree; the real app dirs are never touched.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

use crate::tui::declarative::approvals::Approvals;
use crate::tui::declarative::loader::{build, load_all, reload_global, DefState, LoadInputs};
use crate::tui::declarative::removal::{list_removed_at, purge_all_at, purge_at, remove_at, restore_at, REMOVED_DIR};
use crate::tui::declarative::service;
use crate::tui::declarative::sources::scan_dir;

const NOW: &str = "2026-10-16T12:34:56Z";
const CODE: [&str; 1] = ["claude"];

struct Tree {
    root: PathBuf,
    agents: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("pm-removal-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let agents = root.join("agents");
        fs::create_dir_all(&agents).unwrap();
        Self { root, agents }
    }
    fn approvals(&self) -> PathBuf {
        self.root.join("approvals.json")
    }
    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
    fn inputs(&self) -> LoadInputs<'_> {
        LoadInputs { bundled: &[], user_dir: Some(&self.agents), code_ids: &CODE, projects: &[], extra: &[] }
    }
    fn list(&self) -> Value {
        service::listing(&self.inputs(), &self.approvals())
    }
    fn state_of(&self, id: &str) -> Option<String> {
        self.list()["definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == id)
            .map(|d| d["state"].as_str().unwrap().to_string())
    }
    /// Write `agents/<id>.toml` and approve it; also a fixtures folder and a wizard record.
    fn approved_agent(&self, id: &str) -> PathBuf {
        let file = self.write(&format!("agents/{id}.toml"), &def(id));
        self.write(&format!("agents/{id}/headless_ok.txt"), "pong");
        self.write(&format!("agents/{id}.wizard.json"), "{\"made_by\":\"wizard\"}");
        let fp = self.list()["definitions"][0]["fingerprint"].as_str().unwrap().to_string();
        service::approve(&self.inputs(), &self.approvals(), &file.to_string_lossy(), &fp).unwrap();
        assert_eq!(self.state_of(id).as_deref(), Some("active"));
        file
    }
    fn remove(&self, file: &str) -> Result<Value, service::ServiceError> {
        remove_at(&self.agents, &self.approvals(), file, &CODE, NOW)
    }
    fn restore(&self, removed_id: &str) -> Result<Value, service::ServiceError> {
        restore_at(&self.agents, &self.approvals(), removed_id, &CODE)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// `cargo` is on PATH while tests run, so the binary resolves without any agent installed.
fn def(id: &str) -> String {
    format!(
        "schema_version = 1\n[agent]\nid = \"{id}\"\nlabel = \"Test {id}\"\n[binary]\nnames = [\"cargo\"]\ninstall_hint = \"get cargo\"\n[headless]\nargv = [\"run\"]\ntask_separator = \"--\"\n[headless.output]\nformat = \"plain_text\"\n"
    )
}

#[test]
fn remove_moves_file_fixtures_and_wizard_record_and_drops_the_approval() {
    let t = Tree::new();
    let file = t.approved_agent("rma_x");
    let out = t.remove(&file.to_string_lossy()).unwrap();
    assert_eq!(out["ok"], true);
    assert_eq!(out["label"], "Test rma_x");
    assert_eq!((out["fixtures"].clone(), out["wizard"].clone()), (true.into(), true.into()));
    let removed_id = out["removed_id"].as_str().unwrap().to_string();
    assert_eq!(removed_id, "rma_x-20261016-123456");

    // gone from the agents folder ...
    assert!(!file.exists());
    assert!(!t.agents.join("rma_x").exists());
    assert!(!t.agents.join("rma_x.wizard.json").exists());
    // ... and kept, not deleted
    let kept = t.agents.join(REMOVED_DIR).join(&removed_id);
    assert_eq!(fs::read_to_string(kept.join("rma_x.toml")).unwrap(), def("rma_x"));
    assert_eq!(fs::read_to_string(kept.join("rma_x").join("headless_ok.txt")).unwrap(), "pong");
    assert!(kept.join("rma_x.wizard.json").is_file());
    assert!(kept.join("removal.json").is_file());
    // the approval is dropped
    assert!(Approvals::load(&t.approvals()).records().is_empty());
    assert_eq!(t.state_of("rma_x"), None);
}

#[test]
fn removed_agent_leaves_the_registry_and_the_agent_listing() {
    let _serial = crate::tui::registry::test_serial();
    let t = Tree::new();
    let file = t.approved_agent("rmb_x");
    reload_global(&t.inputs(), &t.approvals());
    let has = || {
        crate::tui::agent_types_listing()["agents"].as_array().unwrap().iter().any(|a| a["id"] == "rmb_x")
    };
    assert!(has(), "approved agent is listed before removal");
    t.remove(&file.to_string_lossy()).unwrap();
    reload_global(&t.inputs(), &t.approvals());
    assert!(!has(), "a removed agent is not in the agent list any more");
    assert!(crate::tui::declarative::loader::last_entries().iter().all(|e| e.id.as_deref() != Some("rmb_x")));
    reload_global(&LoadInputs { bundled: &[], user_dir: None, code_ids: &CODE, projects: &[], extra: &[] }, &t.approvals());
}

#[test]
fn the_loader_ignores_the_removed_folder() {
    let t = Tree::new();
    // Even a valid definition sitting in `.removed/<x>/` or named like it is never a source.
    t.write(&format!("agents/{REMOVED_DIR}/old-1/old.toml"), &def("rmc_old"));
    t.write(&format!("agents/{REMOVED_DIR}/top.toml"), &def("rmc_top"));
    fs::create_dir_all(t.agents.join("stray.toml")).unwrap();
    let scan = scan_dir(&t.agents);
    assert!(scan.files.is_empty(), "{:?}", scan.files);
    let (files, issues) = load_all(&t.inputs());
    assert!(files.is_empty() && issues.is_empty());
    assert!(build(&t.inputs(), &Approvals::default()).entries.is_empty());

    let file = t.write("agents/rmc_a.toml", &def("rmc_a"));
    t.remove(&file.to_string_lossy()).unwrap();
    assert!(build(&t.inputs(), &Approvals::default()).entries.is_empty(), "removing keeps the files out of the loader");
}

#[test]
fn restore_puts_everything_back_and_the_approval_with_it() {
    let t = Tree::new();
    let file = t.approved_agent("rmd_x");
    let removed_id = t.remove(&file.to_string_lossy()).unwrap()["removed_id"].as_str().unwrap().to_string();
    assert_eq!(list_removed_at(&t.agents)["removed"][0]["removed_id"], removed_id.as_str());

    let back = t.restore(&removed_id).unwrap();
    assert_eq!(back["ok"], true);
    assert_eq!(back["approval_restored"], true);
    assert_eq!(back["needs_approval"], false);
    assert_eq!(fs::read_to_string(&file).unwrap(), def("rmd_x"));
    assert_eq!(fs::read_to_string(t.agents.join("rmd_x").join("headless_ok.txt")).unwrap(), "pong");
    assert!(t.agents.join("rmd_x.wizard.json").is_file());
    assert_eq!(t.state_of("rmd_x").as_deref(), Some("active"), "no new approval needed");
    assert!(list_removed_at(&t.agents)["removed"].as_array().unwrap().is_empty());
    assert!(!t.agents.join(REMOVED_DIR).join(&removed_id).exists());
}

#[test]
fn restore_refuses_when_the_file_name_is_taken_again_and_moves_nothing() {
    let t = Tree::new();
    let file = t.approved_agent("rme_x");
    let removed_id = t.remove(&file.to_string_lossy()).unwrap()["removed_id"].as_str().unwrap().to_string();
    t.write("agents/rme_x.toml", "# someone made a new one\n");

    let denied = t.restore(&removed_id).unwrap_err();
    assert_eq!((denied.status, denied.code), (409, "AGENT_DEF_RESTORE_CLASH"));
    assert!(denied.message.contains("rme_x.toml"), "{}", denied.message);
    assert_eq!(fs::read_to_string(&file).unwrap(), "# someone made a new one\n", "the new file is untouched");
    let kept = t.agents.join(REMOVED_DIR).join(&removed_id);
    assert!(kept.join("rme_x.toml").is_file() && kept.join("rme_x").is_dir() && kept.join("rme_x.wizard.json").is_file());
    assert!(!t.agents.join("rme_x").exists(), "nothing was half-restored");

    // a taken fixtures folder alone is a clash too
    fs::remove_file(&file).unwrap();
    fs::create_dir_all(t.agents.join("rme_x")).unwrap();
    let denied = t.restore(&removed_id).unwrap_err();
    assert_eq!(denied.code, "AGENT_DEF_RESTORE_CLASH");
    assert!(!file.exists() && kept.join("rme_x.toml").is_file());
}

#[test]
fn restoring_a_changed_file_needs_approval_again() {
    let t = Tree::new();
    let file = t.approved_agent("rmf_x");
    let removed_id = t.remove(&file.to_string_lossy()).unwrap()["removed_id"].as_str().unwrap().to_string();
    // someone edits the kept copy (even a change that keeps the fingerprint is not byte-identical)
    let kept = t.agents.join(REMOVED_DIR).join(&removed_id).join("rmf_x.toml");
    fs::write(&kept, format!("# edited\n{}", def("rmf_x"))).unwrap();

    let back = t.restore(&removed_id).unwrap();
    assert_eq!(back["approval_restored"], false);
    assert_eq!(back["needs_approval"], true);
    assert_eq!(t.state_of("rmf_x").as_deref(), Some("needs_approval"));
    assert!(Approvals::load(&t.approvals()).records().is_empty());
}

#[test]
fn a_file_that_was_never_approved_comes_back_unapproved() {
    let t = Tree::new();
    let file = t.write("agents/rmg_x.toml", &def("rmg_x"));
    let id = t.remove(&file.to_string_lossy()).unwrap();
    assert_eq!(id["was_approved"], false);
    let back = t.restore(id["removed_id"].as_str().unwrap()).unwrap();
    assert_eq!((back["approval_restored"].clone(), back["needs_approval"].clone()), (false.into(), true.into()));
}

#[test]
fn an_invalid_file_can_be_removed_and_restored() {
    let t = Tree::new();
    let file = t.write("agents/broken.toml", "schema_version = 1\n[agent]\nid = \"rmh_x\"\nbogus = 1\n");
    assert_eq!(t.list()["definitions"][0]["state"], "invalid");
    let out = t.remove(&file.to_string_lossy()).unwrap();
    assert!(!file.exists());
    let row = &list_removed_at(&t.agents)["removed"][0];
    assert_eq!(row["file_name"], "broken.toml");
    let back = t.restore(out["removed_id"].as_str().unwrap()).unwrap();
    assert_eq!(back["valid"], false);
    assert!(file.exists());
}

#[test]
fn a_second_removal_in_the_same_second_gets_its_own_folder() {
    let t = Tree::new();
    let file = t.write("agents/rmi_x.toml", &def("rmi_x"));
    let first = t.remove(&file.to_string_lossy()).unwrap()["removed_id"].as_str().unwrap().to_string();
    let file = t.write("agents/rmi_x.toml", &def("rmi_x"));
    let second = t.remove(&file.to_string_lossy()).unwrap()["removed_id"].as_str().unwrap().to_string();
    assert_ne!(first, second);
    assert_eq!(list_removed_at(&t.agents)["removed"].as_array().unwrap().len(), 2);
}

#[test]
fn path_escapes_are_refused_and_nothing_moves() {
    let t = Tree::new();
    let outside = t.write("outside.toml", &def("rmj_out"));
    let nested = t.write("agents/sub/nested.toml", &def("rmj_nested"));
    let inside = t.write("agents/rmj_in.toml", &def("rmj_in"));
    let kept = t.write("agents/rmj_in.keep", "x");
    let abs_outside = outside.to_string_lossy().into_owned();
    let dotdot = format!("{}/../outside.toml", t.agents.display());
    for bad in [
        "../outside.toml",
        "..\\outside.toml",
        "sub/nested.toml",
        "sub\\nested.toml",
        "",
        "   ",
        dotdot.as_str(),
        abs_outside.as_str(),
        &nested.to_string_lossy(),
        "rmj_in.txt",
        ".hidden.toml",
    ] {
        let denied = t.remove(bad).unwrap_err();
        assert!(
            matches!(denied.code, "AGENT_DEF_PATH_REFUSED" | "AGENT_DEF_NOT_OWN_FOLDER" | "AGENT_DEF_NOT_FOUND"),
            "{bad:?} -> {}",
            denied.code
        );
    }
    assert!(outside.exists() && nested.exists() && inside.exists() && kept.exists());
    assert!(!t.agents.join(REMOVED_DIR).exists() || list_removed_at(&t.agents)["removed"].as_array().unwrap().is_empty());
    // a missing file is a plain not-found
    assert_eq!(t.remove("rmj_none.toml").unwrap_err().code, "AGENT_DEF_NOT_FOUND");
    // a bare file name works like the listed path
    assert!(t.remove("rmj_in.toml").is_ok());
}

#[test]
fn a_link_out_of_the_agents_folder_is_refused() {
    let t = Tree::new();
    let outside = t.write("outside.toml", &def("rmk_out"));
    let link = t.agents.join("link.toml");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&outside, &link).is_ok();
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&outside, &link).is_ok();
    if !made {
        eprintln!("symlinks are not available here; skipping");
        return;
    }
    let denied = t.remove(&link.to_string_lossy()).unwrap_err();
    assert_eq!(denied.code, "AGENT_DEF_SYMLINK_REFUSED");
    assert!(outside.exists() && fs::symlink_metadata(&link).is_ok());
}

#[test]
fn a_project_or_extra_directory_definition_is_never_moved() {
    let t = Tree::new();
    let project = t.write("proj/.puppet-master/agents/p.toml", &def("rml_p"));
    let extra = t.write("team/e.toml", &def("rml_e"));
    for file in [&project, &extra] {
        let denied = t.remove(&file.to_string_lossy()).unwrap_err();
        assert_eq!(denied.code, "AGENT_DEF_NOT_OWN_FOLDER");
        assert!(denied.message.starts_with("This agent comes from "), "{}", denied.message);
        assert!(denied.message.ends_with("Remove it there, or turn it off with Revoke."), "{}", denied.message);
        assert!(file.exists());
    }
    assert!(denied_names(&t));
}

fn denied_names(t: &Tree) -> bool {
    !t.agents.join(REMOVED_DIR).exists()
}

#[test]
fn restore_refuses_names_that_leave_the_removed_folder() {
    let t = Tree::new();
    let file = t.write("agents/rmm_x.toml", &def("rmm_x"));
    t.remove(&file.to_string_lossy()).unwrap();
    for bad in ["", "..", "../agents", "a/b", "a\\b", ".removed", ".", "C:\\Windows", "rmm_x-20261016-123456/.."] {
        let denied = t.restore(bad).unwrap_err();
        assert_eq!(denied.code, "AGENT_DEF_PATH_REFUSED", "{bad:?}");
    }
    assert_eq!(t.restore("no-such-removal").unwrap_err().code, "AGENT_DEF_NOT_FOUND");
    // a folder with a record that names a path (not a file name) is not trusted either
    let evil = t.agents.join(REMOVED_DIR).join("evil-1");
    fs::create_dir_all(&evil).unwrap();
    fs::write(
        evil.join("removal.json"),
        "{\"version\":1,\"id\":\"e\",\"label\":\"e\",\"file_name\":\"../evil.toml\",\"removed_at\":\"x\",\"bytes_sha256\":\"\",\"approval\":null}",
    )
    .unwrap();
    assert_eq!(t.restore("evil-1").unwrap_err().code, "AGENT_DEF_NOT_FOUND");
    assert!(list_removed_at(&t.agents)["removed"].as_array().unwrap().iter().all(|r| r["removed_id"] != "evil-1"));
}

#[test]
fn recently_removed_lists_newest_first_with_what_was_kept() {
    let t = Tree::new();
    let a = t.approved_agent("rmn_a");
    remove_at(&t.agents, &t.approvals(), &a.to_string_lossy(), &CODE, "2026-10-16T10:00:00Z").unwrap();
    let b = t.write("agents/rmn_b.toml", &def("rmn_b"));
    remove_at(&t.agents, &t.approvals(), &b.to_string_lossy(), &CODE, "2026-10-16T11:00:00Z").unwrap();
    let listed = list_removed_at(&t.agents);
    let rows = listed["removed"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], "rmn_b");
    assert_eq!((rows[0]["fixtures"].clone(), rows[0]["wizard"].clone(), rows[0]["was_approved"].clone()), (false.into(), false.into(), false.into()));
    assert_eq!(rows[1]["id"], "rmn_a");
    assert_eq!(rows[1]["label"], "Test rmn_a");
    assert_eq!((rows[1]["fixtures"].clone(), rows[1]["wizard"].clone(), rows[1]["was_approved"].clone()), (true.into(), true.into(), true.into()));
}

#[test]
fn removing_one_agent_keeps_the_approval_of_another() {
    let t = Tree::new();
    let a = t.approved_agent("rmo_a");
    let b = t.write("agents/rmo_b.toml", &def("rmo_b"));
    let fp = t.list()["definitions"].as_array().unwrap().iter().find(|d| d["id"] == "rmo_b").unwrap()["fingerprint"].as_str().unwrap().to_string();
    service::approve(&t.inputs(), &t.approvals(), &b.to_string_lossy(), &fp).unwrap();
    t.remove(&a.to_string_lossy()).unwrap();
    let records = Approvals::load(&t.approvals());
    assert_eq!(records.records().len(), 1);
    assert_eq!(records.records()[0].id, "rmo_b");
    assert_eq!(t.state_of("rmo_b").as_deref(), Some("active"));
    assert_eq!(build(&t.inputs(), &records).entries.iter().filter(|e| e.state == DefState::Active).count(), 1);
}

/// A directory link for the tests: a symlink, or on Windows a junction (needs no privilege). `false`
/// when neither can be made here, and the test then says so and returns.
fn make_dir_link(target: &std::path::Path, link: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

impl Tree {
    fn purge(&self, removed_id: &str) -> Result<Value, service::ServiceError> {
        purge_at(&self.agents, removed_id)
    }
}

#[test]
fn delete_for_good_removes_one_entry_and_keeps_the_others() {
    let t = Tree::new();
    let a = t.approved_agent("rnp_a");
    let id_a = t.remove(&a.to_string_lossy()).unwrap()["removed_id"].as_str().unwrap().to_string();
    let b = t.write("agents/rnp_b.toml", &def("rnp_b"));
    let id_b = remove_at(&t.agents, &t.approvals(), &b.to_string_lossy(), &CODE, "2026-10-16T11:00:00Z").unwrap()
        ["removed_id"]
        .as_str()
        .unwrap()
        .to_string();

    let out = t.purge(&id_a).unwrap();
    assert_eq!(out["ok"], true);
    assert_eq!(out["deleted"], 1);
    assert!(!t.agents.join(REMOVED_DIR).join(&id_a).exists(), "the entry and its files are gone");

    let listed = list_removed_at(&t.agents);
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["removed"][0]["removed_id"], id_b.as_str());
    assert!(t.agents.join(REMOVED_DIR).join(&id_b).join("rnp_b.toml").is_file(), "the other entry is untouched");
    // the same entry again is simply not there any more
    assert_eq!(t.purge(&id_a).unwrap_err().code, "AGENT_DEF_NOT_FOUND");
}

#[test]
fn clear_all_deletes_every_entry_and_leaves_other_folders_alone() {
    let t = Tree::new();
    let a = t.approved_agent("rnq_a");
    t.remove(&a.to_string_lossy()).unwrap();
    let b = t.write("agents/rnq_b.toml", &def("rnq_b"));
    remove_at(&t.agents, &t.approvals(), &b.to_string_lossy(), &CODE, "2026-10-16T11:00:00Z").unwrap();
    // a folder and a file in .removed/ that do not have the removal name shape are not ours to delete
    let note = t.write(&format!("agents/{REMOVED_DIR}/notes/readme.txt"), "keep");
    let stray = t.write(&format!("agents/{REMOVED_DIR}/rnq_file-20261016-120000"), "keep");
    assert_eq!(list_removed_at(&t.agents)["total"], 2);

    let out = purge_all_at(&t.agents).unwrap();
    assert_eq!((out["ok"].clone(), out["deleted"].clone(), out["failed"].clone()), (true.into(), 2.into(), 0.into()));
    assert_eq!(list_removed_at(&t.agents)["total"], 0);
    assert!(list_removed_at(&t.agents)["removed"].as_array().unwrap().is_empty());
    assert_eq!(fs::read_to_string(&note).unwrap(), "keep");
    assert_eq!(fs::read_to_string(&stray).unwrap(), "keep");
    // nothing left: still a success, with zero
    assert_eq!(purge_all_at(&t.agents).unwrap()["deleted"], 0);
}

#[test]
fn clear_all_with_no_removed_folder_deletes_nothing() {
    let t = Tree::new();
    let out = purge_all_at(&t.agents).unwrap();
    assert_eq!((out["deleted"].clone(), out["failed"].clone()), (0.into(), 0.into()));
    assert!(!t.agents.join(REMOVED_DIR).exists(), "no folder is created just to clear it");
}

#[test]
fn delete_for_good_refuses_anything_that_is_not_a_removal_name_and_deletes_nothing() {
    let t = Tree::new();
    let file = t.write("agents/rnr_x.toml", &def("rnr_x"));
    let kept_id = t.remove(&file.to_string_lossy()).unwrap()["removed_id"].as_str().unwrap().to_string();
    let note = t.write(&format!("agents/{REMOVED_DIR}/notes/readme.txt"), "keep");
    let outside = t.write("outside/keep.txt", "keep");
    let abs_outside = outside.parent().unwrap().to_string_lossy().into_owned();
    let abs_entry = t.agents.join(REMOVED_DIR).join(&kept_id).to_string_lossy().into_owned();
    for bad in [
        "",
        "   ",
        "..",
        ".",
        "../outside",
        "..\\outside",
        "a/b",
        "a\\b",
        ".hidden-20261016-120000",
        "C:\\Windows",
        abs_outside.as_str(),
        abs_entry.as_str(),
        "notes",
        "rnr_x",
        "rnr_x.toml",
        "rnr_x-20261016-123456/..",
    ] {
        let denied = t.purge(bad).unwrap_err();
        assert_eq!(denied.code, "AGENT_DEF_PATH_REFUSED", "{bad:?}");
    }
    assert!(outside.is_file() && note.is_file(), "nothing outside the entry name was touched");
    assert!(t.agents.join(REMOVED_DIR).join(&kept_id).join("rnr_x.toml").is_file(), "the real entry was not deleted");
    // a name of the right shape that is simply not there is a plain not-found
    assert_eq!(t.purge("rnr_x-20991231-235959").unwrap_err().code, "AGENT_DEF_NOT_FOUND");
    // the clear-all pass keeps the stray folder too (it is not a removal name)
    purge_all_at(&t.agents).unwrap();
    assert!(note.is_file());
}

#[test]
fn delete_for_good_does_not_follow_a_link_out_of_the_entry() {
    let t = Tree::new();
    let secret = t.write("outside/inner/secret.txt", "keep");
    let file = t.write("agents/rns_x.toml", &def("rns_x"));
    let id = t.remove(&file.to_string_lossy()).unwrap()["removed_id"].as_str().unwrap().to_string();
    let entry = t.agents.join(REMOVED_DIR).join(&id);
    let link = entry.join("link-to-outside");
    if !make_dir_link(secret.parent().unwrap(), &link) {
        eprintln!("directory links are not available here; skipping");
        return;
    }

    let out = t.purge(&id).unwrap();
    assert_eq!(out["deleted"], 1);
    assert!(!entry.exists(), "the entry is gone");
    assert_eq!(fs::read_to_string(&secret).unwrap(), "keep", "the folder behind the link is not deleted");
}

#[test]
fn clear_all_unlinks_a_link_named_like_a_removal_and_keeps_its_target() {
    let t = Tree::new();
    let secret = t.write("outside/inner/secret.txt", "keep");
    let root = t.agents.join(REMOVED_DIR);
    fs::create_dir_all(&root).unwrap();
    let link = root.join("rnt_link-20261016-120000");
    if !make_dir_link(secret.parent().unwrap(), &link) {
        eprintln!("directory links are not available here; skipping");
        return;
    }

    let out = purge_all_at(&t.agents).unwrap();
    assert_eq!((out["deleted"].clone(), out["failed"].clone()), (1.into(), 0.into()));
    assert!(fs::symlink_metadata(&link).is_err(), "the link itself is gone");
    assert_eq!(fs::read_to_string(&secret).unwrap(), "keep", "the target is not touched");
}

#[test]
fn clear_all_refuses_a_removed_folder_that_is_itself_a_link() {
    let t = Tree::new();
    let secret = t.write("outside/inner/secret.txt", "keep");
    let root = t.agents.join(REMOVED_DIR);
    if !make_dir_link(secret.parent().unwrap(), &root) {
        eprintln!("directory links are not available here; skipping");
        return;
    }
    assert_eq!(purge_all_at(&t.agents).unwrap_err().code, "AGENT_DEF_PURGE_FAILED");
    assert_eq!(t.purge("rnu_x-20261016-120000").unwrap_err().code, "AGENT_DEF_PURGE_FAILED");
    assert_eq!(fs::read_to_string(&secret).unwrap(), "keep");
}
