//! Point and Fix tests for target: the "To:" list order, greyed panes, and the default pane.

use crate::pane_outbox::PaneKind;
use crate::point::target::{targets, Target, TargetFacts};

// The outbox's own sentences. The file is read from its place so this test fails when the wording
// there changes and the copies in `target.rs` do not follow.
#[path = "../../pane_outbox/model.rs"]
#[allow(dead_code)]
mod outbox_model;

fn pane(id: &str, cwd: &str, last: Option<i64>) -> TargetFacts {
    TargetFacts {
        pane_id: id.into(),
        agent_label: format!("Agent {id}"),
        title: format!("{id} title"),
        cwd: cwd.into(),
        kind: PaneKind::Agent,
        run_controls: false,
        not_ready: None,
        last_output_at: last,
    }
}

fn ids(list: &[Target]) -> Vec<&str> {
    list.iter().map(|t| t.pane_id.as_str()).collect()
}

#[test]
fn opener_comes_first_when_it_can_take_messages() {
    let panes = vec![
        pane("other", "C:/w/other", Some(300)),
        pane("same", "C:/w/app", Some(200)),
        pane("opener", "C:/w/elsewhere", Some(100)),
    ];
    let (list, default) = targets(&panes, Some("opener"), Some("C:/w/app"));
    assert_eq!(ids(&list), ["opener", "same", "other"]);
    assert_eq!(default.as_deref(), Some("opener"));
}

#[test]
fn opener_that_cannot_send_is_not_first_and_not_default() {
    let mut proto = pane("proto", "C:/w/app", Some(500));
    proto.kind = PaneKind::Protocol;
    let panes = vec![pane("other", "C:/w/other", Some(900)), pane("same", "C:/w/app", Some(100)), proto];
    let (list, default) = targets(&panes, Some("proto"), Some("C:/w/app"));
    // Same folder first (newest output first), then the rest. The protocol pane is not the opener's slot.
    assert_eq!(ids(&list), ["proto", "same", "other"]);
    assert!(!list[0].can_send);
    assert_eq!(default.as_deref(), Some("same"));
}

#[test]
fn shells_are_left_out() {
    let mut shell = pane("shell", "C:/w/app", Some(999));
    shell.kind = PaneKind::Shell;
    let panes = vec![shell, pane("agent", "C:/w/other", Some(1))];
    let (list, default) = targets(&panes, Some("shell"), Some("C:/w/app"));
    assert_eq!(ids(&list), ["agent"]);
    // The shell is the opener but cannot be the default; nothing is in the preview's folder.
    assert_eq!(default, None);
}

#[test]
fn protocol_and_run_controlled_panes_are_greyed_with_the_outbox_sentence() {
    let mut proto = pane("proto", "C:/w/a", Some(3));
    proto.kind = PaneKind::Protocol;
    let mut running = pane("running", "C:/w/a", Some(2));
    running.run_controls = true;
    // Same order as the outbox: protocol is checked before a run.
    let mut both = pane("both", "C:/w/a", Some(1));
    both.kind = PaneKind::Protocol;
    both.run_controls = true;
    let panes = vec![proto, running, both, pane("free", "C:/w/a", Some(0))];

    let (list, default) = targets(&panes, None, Some("C:/w/a"));
    let by_id = |id: &str| list.iter().find(|t| t.pane_id == id).unwrap().clone();

    let p = by_id("proto");
    assert!(!p.can_send);
    assert_eq!(p.reason.as_deref(), Some(outbox_model::NOT_SUPPORTED));

    let r = by_id("running");
    assert!(!r.can_send);
    assert_eq!(r.reason.as_deref(), Some(outbox_model::RUN_CONTROLS));

    let b = by_id("both");
    assert_eq!(b.reason.as_deref(), Some(outbox_model::NOT_SUPPORTED));

    let f = by_id("free");
    assert!(f.can_send);
    assert_eq!(f.reason, None);

    // Greyed panes are never the default.
    assert_eq!(default.as_deref(), Some("free"));
}

#[test]
fn default_falls_back_to_the_newest_sendable_pane_in_the_same_folder() {
    let mut running = pane("c", "C:/w/app", Some(900));
    running.run_controls = true;
    let panes = vec![
        pane("a", "C:/w/app", Some(100)),
        pane("b", "C:/w/app", Some(300)),
        running,
        pane("d", "C:/w/other", Some(1000)),
    ];
    let (list, default) = targets(&panes, None, Some("C:/w/app"));
    assert_eq!(ids(&list), ["c", "b", "a", "d"]);
    assert_eq!(default.as_deref(), Some("b"));
}

#[test]
fn no_default_without_a_sendable_pane_in_the_same_folder() {
    let panes = vec![pane("far", "C:/w/other", Some(5))];
    let (list, default) = targets(&panes, None, Some("C:/w/app"));
    assert_eq!(ids(&list), ["far"]);
    assert!(!list[0].same_folder);
    assert_eq!(default, None);
    // No preview folder at all: nothing counts as the same folder, and the default is still none.
    let (list, default) = targets(&panes, None, None);
    assert!(!list[0].same_folder);
    assert_eq!(default, None);
}

#[test]
fn rest_is_sorted_by_last_output_newest_first_with_unknown_last() {
    let panes = vec![
        pane("x", "C:/w/a", None),
        pane("y", "C:/w/b", Some(50)),
        pane("z", "C:/w/c", Some(90)),
    ];
    let (list, _) = targets(&panes, None, None);
    assert_eq!(ids(&list), ["z", "y", "x"]);
}

#[test]
fn same_folder_ignores_slashes_and_on_windows_case() {
    let panes = vec![pane("a", "C:\\Users\\you\\App\\", None)];
    let (list, _) = targets(&panes, None, Some("c:/users/you/app"));
    // Case only counts on Windows. Slashes never count.
    assert_eq!(list[0].same_folder, cfg!(windows));

    // The same path with other slashes is the same folder on every OS.
    let (list, _) = targets(&panes, None, Some("C:/Users/you/App"));
    assert!(list[0].same_folder);

    let (list, _) = targets(&panes, None, Some("C:/Users/you/Other"));
    assert!(!list[0].same_folder);

    // An empty folder is never the same as another.
    let empty = vec![pane("e", "", None)];
    let (list, _) = targets(&empty, None, Some(""));
    assert!(!list[0].same_folder);
}

#[test]
fn labels_and_titles_come_from_the_caller() {
    let mut p = pane("k", "C:/w/a", None);
    p.agent_label = "Kilo CLI".into();
    p.title = "Fix the pricing page".into();
    let (list, default) = targets(&[p], None, None);
    assert_eq!(list[0].agent_label, "Kilo CLI");
    assert_eq!(list[0].title, "Fix the pricing page");
    assert!(list[0].can_send);
    assert_eq!(default, None);
}

#[test]
fn a_pane_on_a_dialog_is_greyed_out_with_a_plain_reason_and_is_never_the_default() {
    // Seen live: Codex sat on its own "Update available" dialog, was listed as sendable and was the default.
    let why = outbox_model::WAIT_GATE;
    let mut dialog = pane("dialog", "C:/w/app", Some(900));
    dialog.not_ready = Some(why.to_string());
    let mut no_box = pane("nobox", "C:/w/app", Some(800));
    no_box.not_ready = Some(outbox_model::NOT_READY_BOX.to_string());
    let panes = vec![dialog, no_box, pane("free", "C:/w/app", Some(1))];

    // The dialog pane started the preview, and is the newest: it still cannot be the opener's slot or the default.
    let (list, default) = targets(&panes, Some("dialog"), Some("C:/w/app"));
    let by_id = |id: &str| list.iter().find(|t| t.pane_id == id).unwrap().clone();
    assert!(!by_id("dialog").can_send);
    assert_eq!(by_id("dialog").reason.as_deref(), Some(why));
    assert!(!by_id("nobox").can_send);
    assert_eq!(by_id("nobox").reason.as_deref(), Some(outbox_model::NOT_READY_BOX));
    assert!(by_id("free").can_send);
    assert_eq!(ids(&list), ["dialog", "nobox", "free"], "the dialog pane is not in the opener slot: it sorts with its folder by output");
    assert_eq!(default.as_deref(), Some("free"));

    // Nothing else can take it: no default at all, rather than a pane that is on a dialog.
    let mut only = pane("only", "C:/w/app", Some(5));
    only.not_ready = Some(why.to_string());
    let (_, default) = targets(&[only], Some("only"), Some("C:/w/app"));
    assert_eq!(default, None);
}
