//! OBS-2 and APR-4 / SEC-4 at run time: the definition a run started from is recorded in its
//! receipt, an edit never rewrites a run that already started, and a new run after an edit or a
//! binary swap is refused with the SPEC 7.4 code.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::fixtures::parked_snapshot;
use crate::operations::OperationStatus;
use crate::tui::declarative::approvals::{Approval, Approvals};
use crate::tui::declarative::loader::load_dir;
use crate::tui::declarative::sources::Origin;
use crate::tui::declarative::validate::Source;
use crate::tui::declarative::{DeclarativeProfile, DefinitionOrigin};
use crate::tui::DefinitionRef;

fn receipt_of(snapshot: crate::operations::OperationSnapshot) -> crate::agent_runs::runtime::RunReceipt {
    crate::agent_runs::runtime::AgentRunView::from(snapshot).receipt.expect("every view carries a receipt")
}

fn reference(fingerprint: &str) -> DefinitionRef {
    DefinitionRef {
        id: "kk".into(),
        source: "user".into(),
        fingerprint: fingerprint.into(),
        binary: Some("C:/bin/kk.exe".into()),
    }
}

#[test]
fn receipt_has_agent_definition_for_declarative() {
    let mut snapshot = parked_snapshot(OperationStatus::Completed, "none");
    snapshot.worker.agent_definition = Some(reference("sha256:aaa"));
    let receipt = receipt_of(snapshot.clone());
    let definition = receipt.agent_definition.clone().expect("declarative run records its definition");
    assert_eq!((definition.id.as_str(), definition.source.as_str()), ("kk", "user"));
    assert_eq!(definition.fingerprint, "sha256:aaa");
    assert_eq!(definition.binary.as_deref(), Some("C:/bin/kk.exe"));
    // it is part of the serialised receipt and of the persisted snapshot
    let json = serde_json::to_value(&receipt).unwrap();
    assert_eq!(json["agent_definition"]["fingerprint"], "sha256:aaa");
    let stored: crate::operations::OperationSnapshot =
        serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).unwrap();
    assert_eq!(stored.worker.agent_definition, snapshot.worker.agent_definition);
    // code profiles record nothing, and an old snapshot without the field still loads
    let plain = receipt_of(parked_snapshot(OperationStatus::Completed, "none"));
    assert!(plain.agent_definition.is_none());
    assert!(serde_json::to_value(&plain).unwrap().get("agent_definition").is_none());
    let mut raw = serde_json::to_value(&snapshot).unwrap();
    raw["worker"].as_object_mut().unwrap().remove("agent_definition");
    let old: crate::operations::OperationSnapshot = serde_json::from_value(raw).unwrap();
    assert!(old.worker.agent_definition.is_none());
}

#[test]
fn run_in_flight_unaffected_by_edit() {
    let mut old = parked_snapshot(OperationStatus::Running, "none");
    old.worker.agent_definition = Some(reference("sha256:before-edit"));
    let mut new = parked_snapshot(OperationStatus::Running, "none");
    new.worker.agent_definition = Some(reference("sha256:after-edit"));
    assert_eq!(
        receipt_of(old).agent_definition.unwrap().fingerprint,
        "sha256:before-edit",
        "the earlier run keeps the fingerprint it started with"
    );
    assert_eq!(receipt_of(new).agent_definition.unwrap().fingerprint, "sha256:after-edit");
}

struct Tree {
    root: PathBuf,
    files: Vec<PathBuf>,
}

impl Tree {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir()
            .join(format!("pm-gate-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&root).unwrap();
        Self { root, files: Vec::new() }
    }
    fn write(&mut self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, text).unwrap();
        if !self.files.contains(&path) {
            self.files.push(path.clone());
        }
        path
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        for file in &self.files {
            let _ = std::fs::remove_file(file);
        }
        let _ = std::fs::remove_dir(&self.root);
    }
}

/// `cargo` is on PATH while tests run, so the binary resolves with no agent installed.
fn definition(argv: &str) -> String {
    format!(
        "schema_version = 1\n[agent]\nid = \"gate_x\"\nlabel = \"Gate\"\n[binary]\nnames = [\"cargo\"]\n[headless]\nargv = [{argv}]\ntask_separator = \"--\"\n[headless.output]\nformat = \"plain_text\"\n"
    )
}

struct Fixture {
    tree: Tree,
    path: PathBuf,
    profile: DeclarativeProfile,
    approvals: Approvals,
    binary: String,
}

fn fixture() -> Fixture {
    let mut tree = Tree::new();
    let path = tree.write("g.toml", &definition("\"run\""));
    let file = load_dir(&tree.root, Origin::new(Source::User), None, &["claude"]).files.remove(0);
    let fingerprint = file.fingerprint.clone().expect("valid definition");
    let profile = DeclarativeProfile::new(file.definition.clone().unwrap()).unwrap().with_origin(DefinitionOrigin {
        source: Source::User,
        fingerprint: fingerprint.clone(),
        path: file.path.clone(),
        project: None,
    });
    let binary = crate::tui::declarative::resolve::resolve_binary(&file.definition.unwrap().binary, &tree.root)
        .expect("cargo resolves")
        .to_string_lossy()
        .into_owned();
    let mut approvals = Approvals::default();
    approvals.approve(Approval {
        id: "gate_x".into(),
        source: "user".into(),
        path: file.path.to_string_lossy().into_owned(),
        fingerprint,
        binary: binary.clone(),
        approved_at: "2026-10-09T00:00:00Z".into(),
        ..Default::default()
    });
    Fixture { tree, path, profile, approvals, binary }
}

fn gate(f: &Fixture) -> Result<Option<DefinitionRef>, crate::tui::GateError> {
    f.profile.gate_with(&f.approvals, &["claude"], &f.tree.root.to_string_lossy())
}

#[test]
fn approved_definition_passes_the_gate_and_yields_the_reference() {
    let f = fixture();
    let reference = gate(&f).unwrap().expect("declarative profile returns a reference");
    assert_eq!(reference.id, "gate_x");
    assert_eq!(reference.source, "user");
    assert!(reference.fingerprint.starts_with("sha256:"));
    assert_eq!(reference.binary.as_deref(), Some(f.binary.as_str()));
}

#[test]
fn stale_approval_blocks_new_run() {
    let mut f = fixture();
    // an executable edit after registration (argv) changes the fingerprint
    f.tree.write("g.toml", &definition("\"run\", \"--yolo\""));
    let error = gate(&f).unwrap_err();
    assert_eq!(error.code, "AGENT_DEF_APPROVAL_STALE");
    // a cosmetic edit (label) keeps it runnable
    let mut f = fixture();
    f.tree.write("g.toml", &definition("\"run\"").replace("label = \"Gate\"", "label = \"Other\""));
    assert!(gate(&f).is_ok());
    let _ = &f.path;
}

#[test]
fn changed_binary_blocks_new_run() {
    let mut f = fixture();
    let mut record = f.approvals.records()[0].clone();
    record.binary = "C:/somewhere/else/cargo.exe".into();
    f.approvals.approve(record);
    let error = gate(&f).unwrap_err();
    assert_eq!(error.code, "AGENT_DEF_BINARY_CHANGED");
}

#[test]
fn revoked_approval_blocks_new_run() {
    let mut f = fixture();
    assert!(f.approvals.revoke("gate_x"));
    assert_eq!(gate(&f).unwrap_err().code, "AGENT_DEF_UNAPPROVED");
}
