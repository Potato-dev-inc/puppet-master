use super::*;
use crate::pty::registry::PaneRegistry;

fn p(pid: u32, parent: u32) -> ProcEntry {
    ProcEntry { pid, parent, image: "x.exe".into() }
}

fn claimed(roots: &[u32], exited: bool) -> ClaimedPane {
    ClaimedPane { roots: roots.to_vec(), exited }
}

/// pane root 10 -> agent 11 -> launcher 12 -> shim 13; an unrelated tree 20 -> 21.
fn table() -> Vec<ProcEntry> {
    vec![p(10, 1), p(11, 10), p(12, 11), p(13, 12), p(20, 1), p(21, 20)]
}

#[test]
fn a_hint_is_cut_down_to_plain_characters() {
    assert_eq!(clean_hint("  57c03490-a9bd-4e4e  ").as_deref(), Some("57c03490-a9bd-4e4e"));
    assert_eq!(clean_hint("a\r\nX-Evil: 1").as_deref(), Some("aX-Evil1"), "no line break survives");
    assert_eq!(clean_hint("  \r\n "), None);
    assert_eq!(clean_hint(&"a".repeat(500)).unwrap().len(), 96);
}

#[test]
fn a_process_descends_from_a_pane_through_its_chain_of_parents() {
    let procs = table();
    assert!(descends_from(&procs, &[10], 13));
    assert!(descends_from(&procs, &[10], 10), "the pane's own process counts");
    assert!(!descends_from(&procs, &[10], 21));
    assert!(!descends_from(&procs, &[10], 99), "an unknown process is not inside any pane");
    assert!(!descends_from(&procs, &[0], 0), "pid 0 is nobody's root");
    assert!(descends_from(&procs, &[20, 10], 13), "any of several roots will do");
}

#[test]
fn a_cyclic_process_table_ends_the_walk() {
    let procs = vec![p(1, 2), p(2, 1), p(3, 3)];
    assert!(!descends_from(&procs, &[10], 1));
    assert!(!descends_from(&procs, &[10], 3));
}

#[test]
fn a_hint_that_checks_out_names_the_parent() {
    let procs = table();
    let pane = claimed(&[10], false);
    assert_eq!(accept_hint(Some("pane-a"), "child", Some(&pane), Some(13), Some(&procs)).as_deref(), Some("pane-a"));
}

#[test]
fn a_forged_hint_earns_nothing() {
    let procs = table();
    let pane = claimed(&[10], false);
    // The caller's process is in another pane's tree: it named a pane it does not run in.
    assert_eq!(accept_hint(Some("pane-a"), "child", Some(&pane), Some(21), Some(&procs)), None);
    // No process id was reported, so nothing ties the caller to the pane.
    assert_eq!(accept_hint(Some("pane-a"), "child", Some(&pane), None, Some(&procs)), None);
    // The pane it names does not exist (or is gone).
    assert_eq!(accept_hint(Some("pane-a"), "child", None, Some(13), Some(&procs)), None);
    assert_eq!(accept_hint(Some("pane-a"), "child", Some(&claimed(&[10], true)), Some(13), Some(&procs)), None);
    // No hint, or a pane naming itself as its own parent.
    assert_eq!(accept_hint(None, "child", Some(&pane), Some(13), Some(&procs)), None);
    assert_eq!(accept_hint(Some("child"), "child", Some(&pane), Some(13), Some(&procs)), None);
}

#[test]
fn without_a_process_table_the_pane_existing_is_all_that_can_be_checked() {
    let pane = claimed(&[10], false);
    assert_eq!(accept_hint(Some("pane-a"), "child", Some(&pane), None, None).as_deref(), Some("pane-a"));
    assert_eq!(accept_hint(Some("pane-a"), "child", None, None, None), None);
}

#[test]
fn a_pane_keeps_the_parent_it_was_given() {
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    registry.lock().panes.insert("child".into(), PaneRegistry::test_pane_stub("child"));
    assert!(set_parent(&registry, "child", "first"));
    assert!(!set_parent(&registry, "child", "second"), "set once, kept for the pane's life");
    assert!(!set_parent(&registry, "ghost", "first"), "a pane that is gone changes nothing");
    let info = registry.lock().panes["child"].info();
    assert_eq!(info.parent_pane_id.as_deref(), Some("first"));
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["parent_pane_id"], "first");
}

#[test]
fn a_pane_without_a_parent_serializes_no_parent_field() {
    let info = PaneRegistry::test_pane_stub("solo").info();
    assert!(serde_json::to_value(&info).unwrap().get("parent_pane_id").is_none());
}

#[test]
fn the_claimed_pane_is_read_from_the_live_registry() {
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    registry.lock().panes.insert("parent".into(), PaneRegistry::test_pane_stub("parent"));
    let claim = claimed_pane(&registry, "parent").expect("known pane");
    assert!(!claim.exited);
    assert_eq!(claim.roots.len(), 1);
    assert!(claimed_pane(&registry, "nope").is_none());
}
