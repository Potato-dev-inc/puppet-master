//! T34: per-path consent for project and extra sources, approval hardening (argv snapshot,
//! warnings, binary pin). Everything runs against a temp tree; the real app dirs are never read.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use crate::standby::key::canonical_project;
use crate::tui::declarative::approvals::{ApprovalState, Approvals};
use crate::tui::declarative::loader::{build, load_all, DefState, OwnedInputs};
use crate::tui::declarative::service::{
    approval_warnings, approve, approved_snapshot, consent_at, extra_dir_at, project_scan_at, set_source_at,
};
use crate::tui::declarative::sources::{project_candidates, SourceSettings};
use crate::tui::declarative::{DeclarativeProfile, DefinitionOrigin, Source};

struct Tree {
    root: PathBuf,
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl Tree {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("pm-consent-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(&root).unwrap();
        Self { dirs: vec![root.clone()], root, files: Vec::new() }
    }

    fn dir(&mut self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(&path).unwrap();
        let mut at = path.clone();
        while at != self.root {
            if !self.dirs.contains(&at) {
                self.dirs.push(at.clone());
            }
            at = at.parent().unwrap().to_path_buf();
        }
        path
    }

    fn file(&mut self, rel: &str, text: &str) -> PathBuf {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            let rel_parent = parent.strip_prefix(&self.root).unwrap().to_string_lossy().into_owned();
            if !rel_parent.is_empty() {
                self.dir(&rel_parent);
            }
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
        let mut dirs = self.dirs.clone();
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        for d in dirs {
            let _ = fs::remove_dir(d);
        }
    }
}

const CODE_IDS: [&str; 1] = ["claude"];

/// `cargo` is on PATH while tests run, so the binary resolves without any agent installed.
fn def(id: &str, argv: &str) -> String {
    format!(
        "schema_version = 1\n[agent]\nid = \"{id}\"\nlabel = \"Test {id}\"\n[binary]\nnames = [\"cargo\"]\ninstall_hint = \"get cargo\"\n[headless]\nargv = {argv}\ntask_separator = \"--\"\n[headless.output]\nformat = \"plain_text\"\n"
    )
}

fn owned(settings_file: &Path) -> OwnedInputs {
    let settings = SourceSettings::load(settings_file);
    let mut owned = OwnedInputs::from_settings(&settings, CODE_IDS.to_vec());
    owned.user_dir = None; // never the real app data dir
    owned
}

fn states(owned: &OwnedInputs, approvals: &Path) -> Vec<(String, &'static str, &'static str)> {
    build(&owned.inputs(), &Approvals::load(approvals))
        .entries
        .iter()
        .map(|e| (e.id.clone().unwrap_or_default(), e.source, e.state.as_str()))
        .collect()
}

fn consent(t: &Tree, kind: &str, path: &Path, enabled: bool) {
    consent_at(&t.settings(), &json!({"kind": kind, "path": path.to_string_lossy(), "enabled": enabled})).unwrap();
}

/// Project dirs are stored in canonical spelling (lower-case, `/`), so compare that way.
fn same(a: &Path, b: &Path) -> bool {
    canonical_project(&a.to_string_lossy()) == canonical_project(&b.to_string_lossy())
}

fn fingerprint_of(owned: &OwnedInputs, file: &Path) -> String {
    let (files, _) = load_all(&owned.inputs());
    files.into_iter().find(|f| same(&f.path, file)).and_then(|f| f.fingerprint).expect("file loads")
}

#[test]
fn matrix() {
    let mut t = Tree::new();
    let project = t.dir("work/app");
    let evil = t.file("work/app/.puppet-master/agents/evil.toml", &def("evil_x", "[\"run\"]"));
    let approvals = t.approvals();

    // Nothing consented: the hostile file is not even read.
    assert!(states(&owned(&t.settings()), &approvals).is_empty());
    // Switching the project source on is not consent.
    set_source_at(&t.settings(), &json!({"name": "project", "enabled": true})).unwrap();
    assert!(states(&owned(&t.settings()), &approvals).is_empty());

    // Consent: it is listed, but still inactive until approved.
    consent(&t, "project", &project, true);
    let o = owned(&t.settings());
    assert_eq!(states(&o, &approvals), [("evil_x".to_string(), "project", "needs_approval")]);
    assert!(build(&o.inputs(), &Approvals::load(&approvals)).profiles.is_empty(), "nothing registers before approval");

    // Approval activates it.
    let fp = fingerprint_of(&o, &evil);
    approve(&o.inputs(), &approvals, &evil.to_string_lossy(), &fp).unwrap();
    assert_eq!(states(&o, &approvals), [("evil_x".to_string(), "project", "active")]);
    assert_eq!(build(&o.inputs(), &Approvals::load(&approvals)).profiles.len(), 1);

    // Revoking consent unregisters it, and leaves the approval record alone.
    consent(&t, "project", &project, false);
    let o = owned(&t.settings());
    assert!(states(&o, &approvals).is_empty());
    assert!(build(&o.inputs(), &Approvals::load(&approvals)).profiles.is_empty());
    assert_eq!(Approvals::load(&approvals).records().len(), 1);

    // A clone at a new path asks again: the old consent and approval do not carry over.
    consent(&t, "project", &project, true);
    let clone = t.dir("elsewhere/app-clone");
    let cloned = t.file("elsewhere/app-clone/.puppet-master/agents/evil.toml", &def("evil_x", "[\"run\"]"));
    let o = owned(&t.settings());
    assert_eq!(states(&o, &approvals).len(), 1, "the unconsented clone is not read");
    consent(&t, "project", &clone, true);
    let o = owned(&t.settings());
    let all = build(&o.inputs(), &Approvals::load(&approvals)).entries;
    let clone_row = all.iter().find(|e| same(&e.path, &cloned)).unwrap();
    assert_ne!(clone_row.state, DefState::Active, "the clone is not approved by the original's approval");
    // Without the original the clone alone needs approval.
    consent(&t, "project", &project, false);
    let o = owned(&t.settings());
    assert_eq!(states(&o, &approvals), [("evil_x".to_string(), "project", "needs_approval")]);

    // The settings file is the whole state: it survives a reload and keeps canonical keys.
    let reloaded = SourceSettings::load(&t.settings());
    assert!(reloaded.project_consented(&clone.to_string_lossy()));
    assert!(!reloaded.project_consented(&project.to_string_lossy()));
}

#[test]
fn project_definitions_run_only_in_their_project() {
    let mut t = Tree::new();
    let project = t.dir("work/app");
    let file = t.file("work/app/.puppet-master/agents/mine.toml", &def("mine_x", "[\"run\"]"));
    let approvals = t.approvals();
    consent(&t, "project", &project, true);
    let o = owned(&t.settings());
    let fp = fingerprint_of(&o, &file);
    approve(&o.inputs(), &approvals, &file.to_string_lossy(), &fp).unwrap();

    let (files, _) = load_all(&o.inputs());
    let loaded = files.into_iter().next().unwrap();
    let profile = DeclarativeProfile::new(loaded.definition.clone().unwrap()).unwrap().with_origin(DefinitionOrigin {
        source: Source::Project,
        fingerprint: loaded.fingerprint.clone().unwrap(),
        path: loaded.path.clone(),
        project: loaded.project.clone(),
    });
    let records = Approvals::load(&approvals);
    let here = profile.gate_with(&records, &CODE_IDS, &project.to_string_lossy());
    assert!(here.is_ok(), "{here:?}");
    let other = profile.gate_with(&records, &CODE_IDS, "C:/somewhere/else");
    assert_eq!(other.unwrap_err().code, "AGENT_DEF_NO_CONSENT");
}

#[test]
fn extra_directories_need_their_own_consent() {
    let mut t = Tree::new();
    let dir = t.dir("team-agents");
    t.file("team-agents/shared.toml", &def("shared_x", "[\"run\"]"));
    let approvals = t.approvals();

    extra_dir_at(&t.settings(), &json!({"action": "add", "path": dir.to_string_lossy()})).unwrap();
    set_source_at(&t.settings(), &json!({"name": "extra", "enabled": true})).unwrap();
    assert!(states(&owned(&t.settings()), &approvals).is_empty(), "listed but not consented loads nothing");

    consent(&t, "extra", &dir, true);
    assert_eq!(states(&owned(&t.settings()), &approvals), [("shared_x".to_string(), "extra", "needs_approval")]);

    set_source_at(&t.settings(), &json!({"name": "extra", "enabled": false})).unwrap();
    assert!(states(&owned(&t.settings()), &approvals).is_empty(), "the source switch unregisters it");
    set_source_at(&t.settings(), &json!({"name": "extra", "enabled": true})).unwrap();

    extra_dir_at(&t.settings(), &json!({"action": "remove", "path": dir.to_string_lossy()})).unwrap();
    assert!(states(&owned(&t.settings()), &approvals).is_empty());
    assert!(extra_dir_at(&t.settings(), &json!({"action": "remove", "path": dir.to_string_lossy()})).is_err());
    assert!(consent_at(&t.settings(), &json!({"kind": "extra", "path": dir.to_string_lossy(), "enabled": true})).is_err());
    assert!(set_source_at(&t.settings(), &json!({"name": "bundled", "enabled": false})).is_err());
    assert!(consent_at(&t.settings(), &json!({"kind": "project", "path": "C:/no/such/dir/at/all", "enabled": true})).is_err());
}

#[test]
fn banner_lists_names_without_reading_files() {
    let mut t = Tree::new();
    let project = t.dir("work/app");
    t.file("work/app/.puppet-master/agents/b.toml", "this is { not toml and never parsed");
    t.file("work/app/.puppet-master/agents/a.toml", &def("a_x", "[\"run\"]"));
    t.file("work/app/.puppet-master/agents/notes.txt", "ignored");
    assert_eq!(project_candidates(&project), ["a.toml", "b.toml"]);
    let scan = project_scan_at(&t.settings(), &project.to_string_lossy());
    assert_eq!(scan["files"], json!(["a.toml", "b.toml"]));
    assert_eq!(scan["consented"], false);
    consent(&t, "project", &project, true);
    let scan = project_scan_at(&t.settings(), &project.to_string_lossy());
    assert_eq!(scan["consented"], true);
    assert!(project_candidates(&t.root.join("nope")).is_empty());
}

#[test]
fn a_bad_settings_file_means_defaults() {
    let mut t = Tree::new();
    let path = t.file("agent-definitions.json", "{ not json");
    let settings = SourceSettings::load(&path);
    assert_eq!(settings, SourceSettings::default());
    assert!(settings.active_projects().is_empty() && settings.active_extra().is_empty());
    assert!(settings.sources.user.enabled && !settings.sources.project.enabled && !settings.sources.extra.enabled);
}

#[test]
fn binary_swap_requires_reapproval() {
    let mut t = Tree::new();
    let dir = t.dir("agents");
    let file = t.file("agents/swap.toml", &def("swap_x", "[\"run\"]"));
    let approvals = t.approvals();
    let mut o = owned(&t.settings());
    o.user_dir = Some(dir.clone());
    let fp = fingerprint_of(&o, &file);
    approve(&o.inputs(), &approvals, &file.to_string_lossy(), &fp).unwrap();
    let row = |o: &OwnedInputs| build(&o.inputs(), &Approvals::load(&approvals)).entries.remove(0);
    assert!(!row(&o).binary_changed);

    // Someone put a different `cargo` first on PATH: the record pins another path.
    let mut records = Approvals::load(&approvals);
    let mut record = records.get(&file).unwrap().clone();
    record.binary = "C:/old/place/cargo.exe".into();
    records.approve(record);
    records.save(&approvals).unwrap();
    let swapped = row(&o);
    assert!(swapped.binary_changed, "the listing flags it so the UI can ask for re-approval");
    assert_eq!(swapped.state, DefState::Active, "the fingerprint is unchanged");

    let (files, _) = load_all(&o.inputs());
    let loaded = files.into_iter().next().unwrap();
    let profile = DeclarativeProfile::new(loaded.definition.clone().unwrap()).unwrap().with_origin(DefinitionOrigin {
        source: Source::User,
        fingerprint: loaded.fingerprint.clone().unwrap(),
        path: loaded.path.clone(),
        project: None,
    });
    let err = profile.gate_with(&Approvals::load(&approvals), &CODE_IDS, "C:/work").unwrap_err();
    assert_eq!(err.code, "AGENT_DEF_BINARY_CHANGED");

    // Approving again pins the binary as it resolves now.
    approve(&o.inputs(), &approvals, &file.to_string_lossy(), &fp).unwrap();
    assert!(!row(&o).binary_changed);
    assert!(profile.gate_with(&Approvals::load(&approvals), &CODE_IDS, "C:/work").is_ok());
}

#[test]
fn approval_keeps_the_argv_and_warns_about_risky_flags() {
    let mut t = Tree::new();
    let dir = t.dir("agents");
    let file = t.file("agents/risky.toml", &def("risky_x", "[\"run\"]"));
    let approvals = t.approvals();
    let mut o = owned(&t.settings());
    o.user_dir = Some(dir);
    let fp = fingerprint_of(&o, &file);
    approve(&o.inputs(), &approvals, &file.to_string_lossy(), &fp).unwrap();
    let snapshot = approved_snapshot(&approvals, &file.to_string_lossy());
    assert_eq!(snapshot["headless_argv"], json!(["run", "--", "<task>"]));

    // The file changes: it is stale and the record still shows what was approved.
    fs::write(&file, def("risky_x", "[\"run\", \"--yolo\", \"--force\"]")).unwrap();
    let (files, _) = load_all(&o.inputs());
    let now = files.into_iter().next().unwrap();
    let state = Approvals::load(&approvals).state(&file, now.fingerprint.as_deref().unwrap());
    assert_eq!(state, ApprovalState::Stale);
    let snapshot = approved_snapshot(&approvals, &file.to_string_lossy());
    assert_eq!(snapshot["headless_argv"], json!(["run", "--", "<task>"]));

    let warnings = approval_warnings(now.definition.as_ref().unwrap());
    assert!(warnings.iter().any(|w| w.contains("--yolo")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("--force")), "{warnings:?}");
    assert!(!warnings.iter().any(|w| w.contains("--auto")));

    let env_text = format!("{}[env.set]\nFOO = \"1\"\n", def("risky_x", "[\"run\", \"--auto\", \"--dangerously-skip\"]"));
    fs::write(&file, env_text).unwrap();
    let (files, _) = load_all(&o.inputs());
    let warnings = approval_warnings(files[0].definition.as_ref().unwrap());
    assert!(warnings.iter().any(|w| w.contains("--auto")));
    assert!(warnings.iter().any(|w| w.contains("--dangerously")));
    assert!(warnings.iter().any(|w| w.contains("FOO")), "{warnings:?}");
}
