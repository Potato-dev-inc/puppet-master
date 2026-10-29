//! Review Desk: helper groups (SPEC n33 section 3.4 E). Pure.
//!
//! A pane started by another pane is a helper. Its root is the top of the helper chain. The root
//! and its helpers share one group, labelled `<root title> and helpers`. A pane with no helper link
//! and no helpers has no group. Groups are for display only.

use std::collections::HashSet;

use super::folder::{plain_line, PaneFacts};
use super::super::model::Group;

/// How many helper links the walk follows. A chain deeper than this has no group.
const WALK_MAX: usize = 8;
/// The root title's length in the group label.
const LABEL_TITLE_MAX: usize = 40;

/// The group of `pane_id` among `panes`, or `None` when it is not a helper and has no helpers, or
/// when its chain cannot be walked (a missing parent, a loop, or deeper than `WALK_MAX`).
pub fn group_of(panes: &[PaneFacts], pane_id: &str) -> Option<Group> {
    let pane = find(panes, pane_id)?;
    let root = root_of(panes, pane)?;
    // A helper always has its own link inside the root's helpers, so this only fails for a lone root.
    let has_helpers = panes.iter().any(|other| other.parent.as_deref() == Some(root.id.as_str()));
    if !has_helpers {
        return None;
    }
    Some(Group { root_pane_id: root.id.clone(), label: label_of(&root.title) })
}

fn find<'a>(panes: &'a [PaneFacts], id: &str) -> Option<&'a PaneFacts> {
    panes.iter().find(|pane| pane.id == id)
}

/// Walks up the helper links from `pane` (at most `WALK_MAX` of them). `None` when a parent is not
/// in `panes`, the chain loops back on itself, or it is still going after `WALK_MAX` links.
fn root_of<'a>(panes: &'a [PaneFacts], pane: &'a PaneFacts) -> Option<&'a PaneFacts> {
    let mut current = pane;
    let mut seen: HashSet<&str> = HashSet::new();
    seen.insert(pane.id.as_str());
    for _ in 0..WALK_MAX {
        let Some(parent_id) = current.parent.as_deref() else {
            return Some(current);
        };
        if !seen.insert(parent_id) {
            return None;
        }
        current = find(panes, parent_id)?;
    }
    current.parent.is_none().then_some(current)
}

/// `<root title> and helpers`, the title cut to 40 characters. An empty title reads "Agent".
fn label_of(title: &str) -> String {
    let cut = plain_line(title, LABEL_TITLE_MAX);
    let name = if cut.is_empty() { "Agent" } else { cut.as_str() };
    format!("{name} and helpers")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::status::PaneStatus;
    use std::path::PathBuf;

    fn pane(id: &str, title: &str, parent: Option<&str>) -> PaneFacts {
        PaneFacts {
            id: id.to_string(),
            cwd: PathBuf::from(r"C:\Users\you\app"),
            title: title.to_string(),
            status: PaneStatus::Idle,
            parent: parent.map(String::from),
            alive: true,
        }
    }

    /// `c0` is the root, and `c{k}` is k helper links below it, for k in 0..=depth.
    fn chain(depth: usize) -> Vec<PaneFacts> {
        (0..=depth)
            .map(|k| {
                let parent = (k > 0).then(|| format!("c{}", k - 1));
                PaneFacts {
                    id: format!("c{k}"),
                    cwd: PathBuf::from(r"C:\Users\you\app"),
                    title: format!("Level {k}"),
                    status: PaneStatus::Idle,
                    parent,
                    alive: true,
                }
            })
            .collect()
    }

    #[test]
    fn root_and_helpers_share_the_root_label() {
        let panes = vec![
            pane("root", "Builder", None),
            pane("h1", "Helper one", Some("root")),
            pane("h2", "Helper two", Some("root")),
        ];
        let group = Group { root_pane_id: "root".to_string(), label: "Builder and helpers".to_string() };
        assert_eq!(group_of(&panes, "root"), Some(group.clone()));
        assert_eq!(group_of(&panes, "h1"), Some(group.clone()));
        assert_eq!(group_of(&panes, "h2"), Some(group));
    }

    #[test]
    fn a_lone_pane_has_no_group() {
        let panes = vec![pane("solo", "Solo", None), pane("other", "Other", None)];
        assert_eq!(group_of(&panes, "solo"), None);
    }

    #[test]
    fn an_unknown_pane_has_no_group() {
        assert_eq!(group_of(&[pane("a", "A", None)], "zzz"), None);
    }

    #[test]
    fn a_helper_whose_parent_is_gone_has_no_group() {
        assert_eq!(group_of(&[pane("h", "Helper", Some("gone"))], "h"), None);
    }

    #[test]
    fn a_loop_has_no_group_and_does_not_hang() {
        let panes = vec![pane("a", "A", Some("b")), pane("b", "B", Some("a")), pane("c", "C", Some("c"))];
        assert_eq!(group_of(&panes, "a"), None);
        assert_eq!(group_of(&panes, "b"), None);
        assert_eq!(group_of(&panes, "c"), None, "a pane that is its own parent loops");
    }

    #[test]
    fn eight_links_reach_the_root_and_nine_do_not() {
        let panes = chain(9);
        let eight = group_of(&panes, "c8").expect("eight links are inside the walk");
        assert_eq!(eight.root_pane_id, "c0");
        assert_eq!(eight.label, "Level 0 and helpers");
        assert_eq!(group_of(&panes, "c9"), None, "nine links are past the walk");
        assert_eq!(group_of(&panes, "c1").map(|g| g.root_pane_id), Some("c0".to_string()));
    }

    #[test]
    fn the_root_title_is_cut_to_forty_characters() {
        let panes = vec![pane("root", &"x".repeat(60), None), pane("h", "Helper", Some("root"))];
        let group = group_of(&panes, "h").unwrap();
        assert_eq!(group.label, format!("{} and helpers", "x".repeat(40)));
    }

    #[test]
    fn controls_are_removed_from_the_label_and_an_empty_title_reads_agent() {
        let hidden = vec![pane("r", "Build\u{7}er\u{202E}", None), pane("h", "H", Some("r"))];
        assert_eq!(group_of(&hidden, "r").unwrap().label, "Builder and helpers");

        let blank = vec![pane("r", "\u{202E}\u{7}", None), pane("h", "H", Some("r"))];
        assert_eq!(group_of(&blank, "h").unwrap().label, "Agent and helpers");
    }
}
