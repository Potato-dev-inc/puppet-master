use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::super::approvals::{Approval, Approvals};
use super::super::loader::{build, load_dir, DefState, Entry, LoadInputs};
use super::super::sources::Origin;
use super::super::validate::Source;
use super::{refresh, watch_dirs, Refresh, DEBOUNCE};
use crate::tui::declarative::tests::MINIMAL;
use crate::tui::registry::Registry;

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("pm-watch-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(root.join("agents")).unwrap();
        Self { root }
    }
    fn agents(&self) -> PathBuf {
        self.root.join("agents")
    }
    fn approvals(&self) -> PathBuf {
        self.root.join("approvals.json")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if let Ok(entries) = fs::read_dir(self.agents()) {
            for e in entries.flatten() {
                let _ = fs::remove_file(e.path());
            }
        }
        let _ = fs::remove_file(self.approvals());
        let _ = fs::remove_dir(self.agents());
        let _ = fs::remove_dir(&self.root);
    }
}

const CODE_IDS: &[&str] = &["claude", "codex", "cursor_agent", "opencode", "shell"];

fn inputs(dir: &Path) -> LoadInputs<'_> {
    LoadInputs { bundled: &[], user_dir: Some(dir), code_ids: CODE_IDS, projects: &[], extra: &[] }
}

/// Approve whatever currently parses in the tree's agents dir.
fn approve_all(tree: &Tree) {
    let mut approvals = Approvals::load(&tree.approvals());
    for file in load_dir(&tree.agents(), Origin::new(Source::User), None, CODE_IDS).files {
        if let (Some(def), Some(fp)) = (&file.definition, &file.fingerprint) {
            approvals.approve(Approval {
                id: def.agent.id.clone(),
                source: "user".into(),
                path: file.path.to_string_lossy().into_owned(),
                fingerprint: fp.clone(),
                binary: "x".into(),
                approved_at: "2026-01-01T00:00:00Z".into(),
                ..Default::default()
            });
        }
    }
    approvals.save(&tree.approvals()).unwrap();
}

fn ids(registry: &Registry) -> Vec<String> {
    registry.ordered().iter().skip(9).map(|p| p.id().to_string()).collect()
}

fn start(tree: &Tree, registry: &Arc<Registry>, previous: &Arc<Mutex<Vec<Entry>>>) -> super::WatchHandle {
    let (agents, approvals) = (tree.agents(), tree.approvals());
    let (registry, previous) = (registry.clone(), previous.clone());
    watch_dirs(vec![tree.agents()], DEBOUNCE, move || {
        let inputs = inputs(&agents);
        let mut prev = previous.lock().unwrap();
        match refresh(&registry, &inputs, &approvals, &prev) {
            Refresh::KeptPrevious => true,
            Refresh::Applied { entries, .. } => {
                *prev = entries;
                false
            }
        }
    })
    .unwrap()
}

fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> Option<Duration> {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            return Some(start.elapsed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    done().then(|| start.elapsed())
}

#[test]
fn save_updates_within_1s() {
    let tree = Tree::new();
    let registry = Arc::new(Registry::seeded());
    let previous = Arc::new(Mutex::new(Vec::new()));
    let target = tree.agents().join("mycoder.toml");
    fs::write(&target, MINIMAL).unwrap();
    approve_all(&tree);
    fs::remove_file(&target).unwrap();
    let _watch = start(&tree, &registry, &previous);
    assert!(ids(&registry).is_empty());

    // Atomic editor save: write a temp file, then rename it over the target.
    let temp = tree.agents().join("mycoder.toml.tmp");
    fs::write(&temp, MINIMAL).unwrap();
    fs::rename(&temp, &target).unwrap();
    let took = wait_for(Duration::from_secs(1), || ids(&registry) == ["mycoder"]);
    assert!(took.is_some(), "registry did not pick the saved definition up within 1s");
    assert!(build(&inputs(&tree.agents()), &Approvals::load(&tree.approvals())).entries.iter().any(|e| e.state == DefState::Active));

    // Deleting the file unregisters it.
    fs::remove_file(&target).unwrap();
    assert!(wait_for(Duration::from_secs(1), || ids(&registry).is_empty()).is_some());
}

#[test]
fn half_written_keeps_previous() {
    let tree = Tree::new();
    let target = tree.agents().join("mycoder.toml");
    fs::write(&target, MINIMAL).unwrap();
    approve_all(&tree);
    let registry = Arc::new(Registry::seeded());
    let previous = Arc::new(Mutex::new(Vec::new()));
    {
        let mut prev = previous.lock().unwrap();
        let Refresh::Applied { entries, diff } = refresh(&registry, &inputs(&tree.agents()), &tree.approvals(), &prev) else {
            panic!("first refresh must apply")
        };
        assert_eq!(diff.added, ["mycoder"]);
        *prev = entries;
    }
    let before = registry.snapshot();
    let _watch = start(&tree, &registry, &previous);

    // A truncated write (what an editor leaves for a moment): still the previous profile.
    fs::write(&target, &MINIMAL[..MINIMAL.len() / 3]).unwrap();
    std::thread::sleep(Duration::from_millis(700));
    assert!(Arc::ptr_eq(&before, &registry.snapshot()), "registry must not change on a half-written file");
    assert_eq!(ids(&registry), ["mycoder"]);

    // The completed write (new label, so a new fingerprint) arrives while the retry loop runs.
    let updated = MINIMAL.replace("My Coder", "My Coder 2");
    fs::write(&target, &updated).unwrap();
    approve_all(&tree);
    assert!(wait_for(Duration::from_secs(2), || !Arc::ptr_eq(&before, &registry.snapshot())).is_some());
    assert_eq!(ids(&registry), ["mycoder"]);
}

#[test]
fn refresh_reports_added_removed_changed() {
    let tree = Tree::new();
    let target = tree.agents().join("mycoder.toml");
    fs::write(&target, MINIMAL).unwrap();
    approve_all(&tree);
    let registry = Registry::seeded();
    let Refresh::Applied { entries, diff } = refresh(&registry, &inputs(&tree.agents()), &tree.approvals(), &[]) else { panic!() };
    assert_eq!((diff.added.clone(), diff.removed.len(), diff.changed.len()), (vec!["mycoder".to_string()], 0, 0));

    fs::write(&target, MINIMAL.replace("argv = []", "argv = [\"--other\"]")).unwrap();
    approve_all(&tree);
    let Refresh::Applied { entries, diff } = refresh(&registry, &inputs(&tree.agents()), &tree.approvals(), &entries) else { panic!() };
    assert_eq!(diff.changed, ["mycoder"]);
    assert_eq!(diff.invalidated().count(), 1);

    fs::remove_file(&target).unwrap();
    let Refresh::Applied { diff, .. } = refresh(&registry, &inputs(&tree.agents()), &tree.approvals(), &entries) else { panic!() };
    assert_eq!(diff.removed, ["mycoder"]);
}

#[test]
fn watcher_ignores_directories_it_was_not_given() {
    let tree = Tree::new();
    let other = tree.root.join("other");
    fs::create_dir_all(&other).unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let _watch = watch_dirs(vec![tree.agents()], DEBOUNCE, move || {
        counter.fetch_add(1, Ordering::SeqCst);
        false
    })
    .unwrap();
    fs::write(other.join("x.toml"), "x").unwrap();
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    fs::write(tree.agents().join("y.toml"), "y").unwrap();
    assert!(wait_for(Duration::from_secs(1), || hits.load(Ordering::SeqCst) >= 1).is_some());
    let _ = fs::remove_file(other.join("x.toml"));
    let _ = fs::remove_dir(&other);
}
