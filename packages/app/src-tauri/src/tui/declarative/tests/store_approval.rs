//! AS-9: the Agent Store's approval (`approved_by: "store"`) only for a file byte-identical to the embedded
//! copy, and the wizard's approval keeps recording `"wizard"`. Runs against a temp tree; the real app dirs
//! are never read.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::standby::key::canonical_project;
use crate::tui::declarative::approvals::{ApprovalState, Approvals, APPROVED_BY_STORE, APPROVED_BY_WIZARD};
use crate::tui::declarative::loader::{load_all, OwnedInputs};
use crate::tui::declarative::service::{approve, approve_as_store, approve_current_as_wizard, listing_with};
use crate::tui::declarative::sources::SourceSettings;

const CODE_IDS: [&str; 1] = ["claude"];
const CATALOG8: &str = "a1b2c3d4";

struct Tree {
    root: PathBuf,
    files: Vec<PathBuf>,
}

impl Tree {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("pm-store-approval-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(&root).unwrap();
        Self { root, files: Vec::new() }
    }

    fn file(&mut self, rel: &str, text: &str) -> PathBuf {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, text).unwrap();
        self.files.push(path.clone());
        path
    }

    fn settings(&self) -> PathBuf {
        self.root.join("agent-definitions.json")
    }

    fn approvals(&self) -> PathBuf {
        self.root.join("approvals.json")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        for f in self.files.iter().chain([self.settings(), self.approvals()].iter()) {
            let _ = fs::remove_file(f);
        }
        let _ = fs::remove_dir(&self.root);
    }
}

/// `cargo` is on PATH while tests run, so the binary resolves without any agent installed.
fn def(id: &str, argv: &str) -> String {
    format!(
        "schema_version = 1\n[agent]\nid = \"{id}\"\nlabel = \"Test {id}\"\n[binary]\nnames = [\"cargo\"]\ninstall_hint = \"get cargo\"\n[headless]\nargv = {argv}\ntask_separator = \"--\"\n[headless.output]\nformat = \"plain_text\"\n"
    )
}

/// Inputs that read one user folder only (never the real app data dir).
fn owned(t: &Tree, user_dir: &Path) -> OwnedInputs {
    let settings = SourceSettings::load(&t.settings());
    let mut owned = OwnedInputs::from_settings(&settings, CODE_IDS.to_vec());
    owned.user_dir = Some(user_dir.to_path_buf());
    owned
}

fn same(a: &Path, b: &Path) -> bool {
    canonical_project(&a.to_string_lossy()) == canonical_project(&b.to_string_lossy())
}

fn fingerprint_of(owned: &OwnedInputs, file: &Path) -> String {
    let (files, _) = load_all(&owned.inputs());
    files.into_iter().find(|f| same(&f.path, file)).and_then(|f| f.fingerprint).expect("file loads")
}

fn record_for(approvals: &Path, file: &Path) -> crate::tui::declarative::approvals::Approval {
    Approvals::load(approvals).get(file).cloned().expect("a record exists")
}

#[test]
fn store_approval_records_store_and_its_checks() {
    let mut t = Tree::new();
    let dir = t.root.clone();
    let text = def("nimbus_x", "[\"run\"]");
    let file = t.file("nimbus_x.toml", &text);
    let approvals = t.approvals();
    let o = owned(&t, &dir);

    let result = approve_as_store(&o.inputs(), &approvals, &file.to_string_lossy(), &text, CATALOG8).unwrap();
    assert_eq!(result["state"], "active", "{result}");

    let record = record_for(&approvals, &file);
    assert_eq!(record.approved_by.as_deref(), Some(APPROVED_BY_STORE));
    assert_eq!(record.checks, ["Shipped in this app version", "catalog a1b2c3d4"]);
    assert_eq!(record.fingerprint, fingerprint_of(&o, &file), "the fingerprint of the file the Store checked");

    // The listing row says who approved it, next to the number of checks.
    let listed = listing_with(&o.inputs(), &approvals, &SourceSettings::load(&t.settings()));
    let row = listed["definitions"].as_array().unwrap().iter().find(|d| d["id"] == "nimbus_x").expect("row");
    assert_eq!(row["state"], "active");
    assert_eq!(row["approved_by"], APPROVED_BY_STORE);
    assert_eq!(row["approval_checks"], 2);
}

#[test]
fn store_approval_refuses_any_byte_difference() {
    let mut t = Tree::new();
    let dir = t.root.clone();
    let text = def("nimbus_x", "[\"run\"]");
    let approvals = t.approvals();

    // The file on disk was edited after the Store wrote it.
    let file = t.file("nimbus_x.toml", &def("nimbus_x", "[\"run\", \"--auto\"]"));
    let o = owned(&t, &dir);
    let err = approve_as_store(&o.inputs(), &approvals, &file.to_string_lossy(), &text, CATALOG8).unwrap_err();
    assert_eq!(err.code, "STORE_FILE_CHANGED");
    assert!(Approvals::load(&approvals).records().is_empty(), "nothing is recorded for a refused file");

    // Even a trailing newline is a difference: the check is on bytes, not on meaning.
    fs::write(&file, format!("{text}\n")).unwrap();
    let err = approve_as_store(&o.inputs(), &approvals, &file.to_string_lossy(), &text, CATALOG8).unwrap_err();
    assert_eq!(err.code, "STORE_FILE_CHANGED");
    assert!(Approvals::load(&approvals).records().is_empty());

    // A file that is gone is refused the same way, never approved.
    fs::remove_file(&file).unwrap();
    let err = approve_as_store(&o.inputs(), &approvals, &file.to_string_lossy(), &text, CATALOG8).unwrap_err();
    assert_eq!(err.code, "STORE_FILE_CHANGED");
}

#[test]
fn a_store_approval_goes_stale_after_a_later_edit() {
    let mut t = Tree::new();
    let dir = t.root.clone();
    let text = def("nimbus_x", "[\"run\"]");
    let file = t.file("nimbus_x.toml", &text);
    let approvals = t.approvals();
    let o = owned(&t, &dir);
    approve_as_store(&o.inputs(), &approvals, &file.to_string_lossy(), &text, CATALOG8).unwrap();

    fs::write(&file, def("nimbus_x", "[\"run\", \"--force\"]")).unwrap();
    let (files, _) = load_all(&o.inputs());
    let now = files.into_iter().next().unwrap();
    let state = Approvals::load(&approvals).state(&file, now.fingerprint.as_deref().unwrap());
    assert_eq!(state, ApprovalState::Stale);
}

#[test]
fn wizard_approvals_still_record_wizard() {
    let mut t = Tree::new();
    let dir = t.root.clone();
    let file = t.file("wiz_x.toml", &def("wiz_x", "[\"run\"]"));
    let approvals = t.approvals();
    let o = owned(&t, &dir);

    let checks = vec!["check one".to_string(), "check two".to_string()];
    approve_current_as_wizard(&o.inputs(), &approvals, &file.to_string_lossy(), &checks, None).unwrap();
    let record = record_for(&approvals, &file);
    assert_eq!(record.approved_by.as_deref(), Some(APPROVED_BY_WIZARD));
    assert_eq!(record.checks, checks);

    let listed = listing_with(&o.inputs(), &approvals, &SourceSettings::load(&t.settings()));
    let row = listed["definitions"].as_array().unwrap().iter().find(|d| d["id"] == "wiz_x").expect("row");
    assert_eq!(row["approved_by"], APPROVED_BY_WIZARD);
    assert_eq!(row["approval_checks"], 2);
}

#[test]
fn a_persons_approval_has_no_approved_by() {
    let mut t = Tree::new();
    let dir = t.root.clone();
    let file = t.file("mine_x.toml", &def("mine_x", "[\"run\"]"));
    let approvals = t.approvals();
    let o = owned(&t, &dir);
    let fp = fingerprint_of(&o, &file);
    approve(&o.inputs(), &approvals, &file.to_string_lossy(), &fp).unwrap();

    assert_eq!(record_for(&approvals, &file).approved_by, None);
    let listed = listing_with(&o.inputs(), &approvals, &SourceSettings::load(&t.settings()));
    let row = listed["definitions"].as_array().unwrap().iter().find(|d| d["id"] == "mine_x").expect("row");
    assert!(row["approved_by"].is_null(), "{row}");
    assert!(row["approval_checks"].is_null(), "{row}");
}
