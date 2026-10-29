//! Review Desk: the views that leave Rust (SPEC n33 sections 3.6 and 3.9). Plain data in camelCase for the desktop
//! window, and the small read-only phone view. Everything here is built from items that were already cleaned
//! (redacted, control characters removed). The phone view cleans again and sends no keys, pane ids, project
//! names, file names or diffs (S5).

use serde::Serialize;

use super::model::{strip_controls, Clean, Item, Keep, Level, Reason, Source, Throw};
use super::risk::level_word;

/// Rows of the file list sent to the window.
pub const ROWS_SHOWN: usize = 200;
/// Items the phone gets.
pub const PHONE_ITEMS: usize = 20;
/// The phone title is cut at this many characters.
pub const PHONE_TITLE_MAX: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub root_pane_id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRowView {
    pub path: String,
    /// `A`, `M` or `D`.
    pub status: char,
    pub added: u32,
    pub deleted: u32,
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesView {
    pub total: u32,
    pub added: u32,
    pub deleted: u32,
    /// At most `ROWS_SHOWN`.
    pub rows: Vec<FileRowView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepView {
    pub kind: &'static str,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThrowView {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// One row of the desk. No project folder and no account: the window needs neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub key: String,
    pub source: Source,
    pub fingerprint: String,
    pub title: String,
    pub agent_label: String,
    pub where_label: String,
    pub pane_id: Option<String>,
    pub finished_at_ms: u64,
    pub turns: u32,
    pub group: Option<GroupView>,
    pub files: FilesView,
    pub level: Level,
    pub level_word: String,
    pub tier: u8,
    pub reasons: Vec<Reason>,
    pub keep: KeepView,
    pub throw: ThrowView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_pane_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_why_not: Option<String>,
    /// Only the file names were checked (the patch was too big or not read): the row says so.
    pub names_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub total: u32,
    pub high: u32,
    pub check: u32,
    pub low: u32,
}

/// `review_desk_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeskView {
    pub items: Vec<ItemView>,
    pub counts: Counts,
    /// One plain line for every source that could not be read.
    pub notes: Vec<String>,
}

/// `review_desk_detail`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailView {
    pub item: ItemView,
    /// Redacted (secrets masked, home folder as `~`), cut at `DETAIL_PATCH_MAX`.
    pub patch: String,
    pub patch_truncated: bool,
    /// Night reason, issue push-check reasons, mission headline.
    pub extra: Vec<String>,
}

/// `review_desk_keep` / `review_desk_throw_away`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActView {
    pub sentence: String,
    /// The item left the desk.
    pub gone: bool,
}

impl ItemView {
    pub fn of(item: &Item) -> ItemView {
        let (fix_pane_id, fix_why_not) = match &item.fix {
            Ok(pane) => (Some(pane.clone()), None),
            Err(why) => (None, Some(why.clone())),
        };
        let rows = item
            .facts
            .files
            .iter()
            .flatten()
            .take(ROWS_SHOWN)
            .map(|row| FileRowView {
                path: row.path.clone(),
                status: row.status,
                added: row.added,
                deleted: row.deleted,
                binary: row.binary,
            })
            .collect();
        ItemView {
            key: item.key.as_str().to_string(),
            source: item.source,
            fingerprint: item.fingerprint.clone(),
            title: item.title.clone(),
            agent_label: item.agent_label.clone(),
            where_label: item.where_label.clone(),
            pane_id: item.pane_id.clone(),
            finished_at_ms: item.finished_at_ms,
            turns: item.turns,
            group: item
                .group
                .as_ref()
                .map(|group| GroupView { root_pane_id: group.root_pane_id.clone(), label: group.label.clone() }),
            files: FilesView {
                total: item.facts.files_total,
                added: item.facts.added,
                deleted: item.facts.deleted,
                rows,
            },
            level: item.risk.level,
            level_word: level_word(item.risk.level).to_string(),
            tier: item.risk.tier,
            reasons: item.risk.reasons.clone(),
            keep: keep_view(&item.keep),
            throw: throw_view(&item.throw),
            fix_pane_id,
            fix_why_not,
            names_only: item.facts.files.is_some() && !item.facts.lines_scanned,
        }
    }
}

fn keep_view(keep: &Keep) -> KeepView {
    let merge = |kind, into: &String| KeepView {
        kind,
        label: "Merge\u{2026}".to_string(),
        into: Some(into.clone()),
        why: None,
    };
    match keep {
        Keep::Merge { into, .. } => merge("merge", into),
        Keep::NightKeep { into, .. } => merge("night_keep", into),
        Keep::IssuePush { .. } => KeepView {
            kind: "issue_push",
            label: "Push as draft\u{2026}".to_string(),
            into: None,
            why: None,
        },
        Keep::Mark { .. } => KeepView { kind: "mark", label: "Looks good".to_string(), into: None, why: None },
        Keep::No { why } => KeepView { kind: "no", label: "Keep".to_string(), into: None, why: Some(why.clone()) },
    }
}

fn throw_view(throw: &Throw) -> ThrowView {
    match throw {
        Throw::Discard { .. } => ThrowView { kind: "discard", why: None },
        Throw::NightThrow { .. } => ThrowView { kind: "night_throw", why: None },
        Throw::IssueThrow { .. } => ThrowView { kind: "issue_throw", why: None },
        Throw::Restore { .. } => ThrowView { kind: "restore", why: None },
        Throw::No { why } => ThrowView { kind: "no", why: Some(why.clone()) },
    }
}

/// The desk for the window: items in the order given, the counts and the notes.
pub fn desk_view(items: &[Item], notes: Vec<String>) -> DeskView {
    let mut counts = Counts { total: u32::try_from(items.len()).unwrap_or(u32::MAX), ..Counts::default() };
    for item in items {
        match item.risk.level {
            Level::High => counts.high += 1,
            Level::Check => counts.check += 1,
            Level::Low => counts.low += 1,
        }
    }
    DeskView { items: items.iter().map(ItemView::of).collect(), counts, notes }
}

// ---------------------------------------------------------------------------------------------- phone

/// One row for the phone: words and numbers only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneItem {
    pub title: String,
    pub level_word: String,
    pub agent_label: String,
    pub where_word: String,
    pub files: u32,
    pub added: u32,
    pub deleted: u32,
}

/// `GET /review-desk` (read-only): no keys, no pane ids, no project names, no paths, no diffs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    pub count: u32,
    pub high: u32,
    pub items: Vec<PhoneItem>,
}

fn where_word(source: Source) -> &'static str {
    match source {
        Source::Copy => "Copy",
        Source::Mission => "Mission",
        Source::Night => "Night Shift",
        Source::Issue => "Issue",
        Source::Folder => "Folder",
    }
}

/// The phone's view of the desk. Titles and labels are cleaned again with `clean` (strict redaction) and the
/// title is cut at `PHONE_TITLE_MAX` characters; at most `PHONE_ITEMS` rows.
pub fn phone_view(desk: &DeskView, clean: &Clean) -> PhoneView {
    let first_line = |raw: &str, max: usize| -> String {
        let cleaned = clean.apply(raw);
        let line = cleaned.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
        strip_controls(line).trim().chars().take(max).collect::<String>().trim_end().to_string()
    };
    let items = desk
        .items
        .iter()
        .take(PHONE_ITEMS)
        .map(|item| PhoneItem {
            title: first_line(&item.title, PHONE_TITLE_MAX),
            level_word: item.level_word.clone(),
            agent_label: first_line(&item.agent_label, PHONE_TITLE_MAX),
            where_word: where_word(item.source).to_string(),
            files: item.files.total,
            added: item.files.added,
            deleted: item.files.deleted,
        })
        .collect();
    PhoneView { count: desk.counts.total, high: desk.counts.high, items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review_desk::model::{Facts, FileRow, ItemKey, Risk};
    use serde_json::json;

    fn item(key: &str, source: Source, keep: Keep, throw: Throw) -> Item {
        Item {
            key: ItemKey::parse(key).expect("valid key"),
            source,
            fingerprint: "abcdef012345".into(),
            title: "Make the cart total round".into(),
            agent_label: "Fixture agent".into(),
            account: Some("work".into()),
            where_label: "Separate copy of main".into(),
            project: r"C:\Users\you\app".into(),
            pane_id: Some("pane-1".into()),
            finished_at_ms: 100,
            turns: 2,
            group: None,
            facts: Facts {
                files: Some(vec![FileRow { path: "src/a.ts".into(), status: 'M', added: 3, deleted: 1, binary: false }]),
                files_total: 1,
                added: 3,
                deleted: 1,
                lines_scanned: true,
                ..Facts::default()
            },
            risk: Risk { tier: 7, level: Level::Low, reasons: Vec::new() },
            keep,
            throw,
            fix: Ok("pane-1".into()),
        }
    }

    #[test]
    fn the_item_view_uses_camel_case_and_the_words_of_the_spec() {
        let merge = item(
            "copy:h1",
            Source::Copy,
            Keep::Merge { project: r"C:\Users\you\app".into(), handle: "h1".into(), into: "main".into() },
            Throw::Discard { project: r"C:\Users\you\app".into(), handle: "h1".into() },
        );
        let value = serde_json::to_value(ItemView::of(&merge)).unwrap();
        assert_eq!(value["key"], "copy:h1");
        assert_eq!(value["source"], "copy");
        assert_eq!(value["levelWord"], "Low risk");
        assert_eq!(value["keep"], json!({ "kind": "merge", "label": "Merge\u{2026}", "into": "main" }));
        assert_eq!(value["throw"], json!({ "kind": "discard" }));
        assert_eq!(value["fixPaneId"], "pane-1");
        assert_eq!(value["files"]["rows"][0]["status"], "M");
        assert_eq!(value["namesOnly"], false);
        // The window gets neither the project folder nor the account.
        let text = value.to_string();
        assert!(!text.contains("Users") && !text.contains("work"), "{text}");
    }

    #[test]
    fn every_keep_and_throw_has_its_tag_and_label() {
        let cases = [
            (Keep::NightKeep { task_id: "nt-00000001".into(), into: "dev".into() }, "night_keep", "Merge\u{2026}"),
            (Keep::IssuePush { run_id: "ir-000000000001".into(), reasons: vec![] }, "issue_push", "Push as draft\u{2026}"),
            (Keep::Mark { pane_id: "p".into(), through_ms: 1 }, "mark", "Looks good"),
            (Keep::No { why: "Held.".into() }, "no", "Keep"),
        ];
        for (keep, kind, label) in cases {
            let view = keep_view(&keep);
            assert_eq!((view.kind, view.label.as_str()), (kind, label));
        }
        assert_eq!(keep_view(&Keep::No { why: "Held.".into() }).why.as_deref(), Some("Held."));
        assert_eq!(throw_view(&Throw::Restore { pane_id: "p".into(), checkpoint_id: "1".into(), files: 2 }).kind, "restore");
        assert_eq!(throw_view(&Throw::No { why: "Closed.".into() }).why.as_deref(), Some("Closed."));
    }

    #[test]
    fn a_closed_pane_shows_why_ask_to_fix_is_off() {
        let mut closed = item(
            "folder:p:1-1",
            Source::Folder,
            Keep::Mark { pane_id: "p".into(), through_ms: 5 },
            Throw::No { why: "The agent's pane is closed. Use History to undo.".into() },
        );
        closed.pane_id = None;
        closed.fix = Err("The agent's pane is closed.".into());
        let value = serde_json::to_value(ItemView::of(&closed)).unwrap();
        assert!(value.get("fixPaneId").is_none());
        assert_eq!(value["fixWhyNot"], "The agent's pane is closed.");
        assert!(value["paneId"].is_null());
    }

    #[test]
    fn the_rows_are_cut_at_200_and_the_counts_follow_the_levels() {
        let mut big = item(
            "copy:h1",
            Source::Copy,
            Keep::No { why: "x".into() },
            Throw::No { why: "x".into() },
        );
        big.facts.files = Some(
            (0..250)
                .map(|n| FileRow { path: format!("f{n}.ts"), status: 'A', added: 1, deleted: 0, binary: false })
                .collect(),
        );
        big.facts.files_total = 250;
        let mut high = big.clone();
        high.risk.level = Level::High;
        let mut check = big.clone();
        check.risk.level = Level::Check;
        let desk = desk_view(&[high, check, big], vec!["Could not read Night Shift. Its tasks are not listed.".into()]);
        assert_eq!(desk.items[0].files.rows.len(), 200);
        assert_eq!(desk.items[0].files.total, 250);
        assert_eq!(desk.counts, Counts { total: 3, high: 1, check: 1, low: 1 });
        assert_eq!(desk.notes.len(), 1);
    }

    #[test]
    fn the_phone_view_has_words_and_numbers_only() {
        let mut one = item(
            "copy:secret-handle",
            Source::Night,
            Keep::No { why: "x".into() },
            Throw::No { why: "x".into() },
        );
        one.title = format!("{} and a very long tail", "T".repeat(70));
        let mut desk = desk_view(&vec![one.clone(); 25], Vec::new());
        desk.items[0].agent_label = "Fixture agent".into();
        let marker = Clean { text: Box::new(|raw| raw.replace("Fixture", "[clean]")) };
        let phone = phone_view(&desk, &marker);
        assert_eq!(phone.count, 25);
        assert_eq!(phone.items.len(), 20);
        assert_eq!(phone.items[0].title.chars().count(), 60);
        assert_eq!(phone.items[0].agent_label, "[clean] agent");
        assert_eq!(phone.items[0].where_word, "Night Shift");
        let text = serde_json::to_string(&phone).unwrap();
        for forbidden in ["secret-handle", "pane-1", "Users", "src/a.ts", "abcdef012345", "key", "path"] {
            assert!(!text.contains(forbidden), "{forbidden} in {text}");
        }
    }
}
