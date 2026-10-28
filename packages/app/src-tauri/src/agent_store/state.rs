//! Card state for one Store entry (SPEC 3.4): which of the seven states the card shows, and why,
//! in one plain sentence.
//!
//! `card_state` is pure: it reads only `Inputs`. `detect` gathers those inputs from this PC (PATH,
//! the registry, the approvals and the Store record). It never runs the program; the version
//! check is `agent_health::check_probe`, called by a later task.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::catalog::CatalogEntry;
use super::record::{self, AgentRecord};
use crate::agent_install::plan::Tools;
use crate::tui;
use crate::tui::declarative::approvals::ApprovalState;
use crate::tui::declarative::loader;
use crate::tui::declarative::parse_definition;
use crate::tui::declarative::resolve::resolve_binary;

/// The `source` the registry gives a built-in (code) profile.
const CODE_SOURCE: &str = "code";

/// The card state, in the snake_case names `lib/agent-store.ts` (`StoreState`) expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardState {
    Available,
    FoundOnPc,
    Ready,
    AddedNoProgram,
    NeedsReview,
    OwnVersion,
    Unsupported,
}

/// One row of the registry for this agent id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredRow {
    /// `"code"` for a built-in profile, otherwise the definition's source name.
    pub source: String,
    /// The definition file's text as it is on disk. `None` for a built-in profile.
    pub text: Option<String>,
    /// The registry says the program is installed.
    pub installed: bool,
}

/// Everything `card_state` needs. `detect` fills it; tests build it by hand.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub tools: Tools,
    /// The registry row for the entry's id, if any.
    pub registered: Option<RegisteredRow>,
    /// The approval of the registered definition file (`None` for a built-in profile).
    pub approval: Option<ApprovalState>,
    /// The Store record's entry for this id, if any.
    pub record: Option<AgentRecord>,
    /// Where the program was found on PATH, if it was.
    pub found_path: Option<PathBuf>,
    /// True on Windows. The Store works on Windows only.
    pub windows: bool,
}

/// The card state and its plain reason, from the state table (SPEC 3.4).
pub fn card_state(entry: &CatalogEntry, inputs: &Inputs) -> (CardState, String) {
    if !inputs.windows {
        return (CardState::Unsupported, "The Agent Store works on Windows only.".into());
    }
    let id = entry.entry.store.id.as_str();
    let found = found(inputs);
    let Some(row) = inputs.registered.as_ref() else {
        return if found {
            (CardState::FoundOnPc, found_reason(entry))
        } else {
            (CardState::Available, available_reason(entry))
        };
    };
    if row.source != CODE_SOURCE && !is_store_file(entry, inputs) {
        let reason = format!("You have your own {id} definition, so the Store leaves it alone.");
        return (CardState::OwnVersion, reason);
    }
    match inputs.approval {
        Some(ApprovalState::Stale) => (
            CardState::NeedsReview,
            "The definition changed after you approved it. Review it in Settings > Agents.".into(),
        ),
        Some(ApprovalState::Unapproved) => (
            CardState::NeedsReview,
            "The definition is not approved yet. Review it in Settings > Agents.".into(),
        ),
        _ if found => (CardState::Ready, "Added and ready. The program is on this PC.".into()),
        _ => (
            CardState::AddedNoProgram,
            "Added to Puppet Master, but the program is not on this PC any more.".into(),
        ),
    }
}

/// Gathers the inputs for one entry from this PC, with the Store record in the app data folder.
pub fn detect(entry: &CatalogEntry) -> Inputs {
    detect_at(entry, &record::default_path())
}

/// `detect` with the Store record read from `record_path` (tests point it at a scratch file).
pub(super) fn detect_at(entry: &CatalogEntry, record_path: &Path) -> Inputs {
    let id = entry.entry.store.id.as_str();
    let loaded = loader::last_entry_for(id);
    let registered = tui::descriptors().into_iter().find(|d| d.id == id).map(|d| {
        let text = match (d.source == CODE_SOURCE, loaded.as_ref()) {
            (false, Some(found)) => fs::read_to_string(&found.path).ok(),
            _ => None,
        };
        RegisteredRow {
            source: d.source.to_string(),
            text,
            installed: d.installed,
        }
    });
    let here = std::env::current_dir().unwrap_or_default();
    let found_path = entry
        .definition_text
        .as_deref()
        .and_then(|text| parse_definition(text).ok())
        .and_then(|definition| resolve_binary(&definition.binary, &here).ok());
    let mut record = record::load(record_path);
    Inputs {
        tools: Tools::detect(),
        registered,
        approval: loaded.as_ref().and_then(|found| found.approval),
        record: record.agents.remove(id),
        found_path,
        windows: cfg!(windows),
    }
}

/// The program is on PATH, or the registry says it is installed (built-in profiles).
pub(super) fn found(inputs: &Inputs) -> bool {
    inputs.found_path.is_some() || inputs.registered.as_ref().is_some_and(|row| row.installed)
}

/// The registered definition is the Store's own file: the record says the Store added it, and its
/// bytes equal the copy shipped in this app version. Any other definition with this id is the
/// person's own (`own_version`).
fn is_store_file(entry: &CatalogEntry, inputs: &Inputs) -> bool {
    let added = inputs.record.as_ref().is_some_and(|record| !record.added.is_empty());
    let same_bytes = inputs
        .registered
        .as_ref()
        .is_some_and(|row| row.text.is_some() && row.text == entry.definition_text);
    added && same_bytes
}

fn found_reason(entry: &CatalogEntry) -> String {
    if entry.entry.store.ambiguous_name {
        "The program is on this PC. Its short name is shared with other programs, so confirm it before you add it."
            .into()
    } else {
        "The program is on this PC. Add it to Puppet Master to use it.".into()
    }
}

fn available_reason(entry: &CatalogEntry) -> String {
    if has_windows_install(entry) {
        "Not on this PC yet. Install it from here, or add it once it is installed.".into()
    } else {
        "Not on this PC yet. Install it with the vendor's own steps.".into()
    }
}

/// True when the shipped definition has a Windows install table (the Install button).
fn has_windows_install(entry: &CatalogEntry) -> bool {
    entry
        .definition_text
        .as_deref()
        .and_then(|text| parse_definition(text).ok())
        .and_then(|definition| definition.install)
        .and_then(|install| install.windows)
        .is_some()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::agent_store::catalog::load;

    /// A valid `entry.toml` for `id`, optionally with an ambiguous short name.
    fn entry_toml(id: &str, ambiguous: bool) -> String {
        format!(
            r#"schema_version = 1

[store]
id = "{id}"
tagline = "A test agent."
tier = "starter"
builtin = false
categories = ["general"]
status = "active"
ambiguous_name = {ambiguous}
first_listed = "2026-10-28"

[vendor]
name = "Test Vendor"
homepage = "https://example.com/{id}"
license = "Apache-2.0"
license_kind = "open_source"

[sign_in]
kind = "none"
summary = "No sign-in needed."

[checked]
date = "2026-10-28"
version = "1.0.0"
how = "vendor_docs"
windows_tested = false
"#
        )
    }

    /// A `definition.toml` for `id` with a Windows npm install.
    fn definition_toml(id: &str) -> String {
        format!(
            "schema_version = 1\n\n[agent]\nid = \"{id}\"\nlabel = \"Test {id}\"\n\n[binary]\nnames = [\"{id}\"]\n\n\
             [install.windows]\nsource = \"https://example.com/{id}\"\nnpm = \"test-{id}-cli\"\n"
        )
    }

    /// One store entry (with its definition) as the catalog loads it.
    fn store_entry(id: &str, ambiguous: bool) -> CatalogEntry {
        let files = [
            (format!("agents/{id}/entry.toml"), entry_toml(id, ambiguous)),
            (format!("agents/{id}/definition.toml"), definition_toml(id)),
        ];
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let loaded = load(&refs);
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        loaded.entries.into_iter().next().expect("the entry loads")
    }

    /// Inputs for a Windows PC with nothing on it.
    fn bare() -> Inputs {
        Inputs {
            tools: Tools::default(),
            registered: None,
            approval: None,
            record: None,
            found_path: None,
            windows: true,
        }
    }

    /// The registry row of the Store's own file (same bytes as the shipped copy).
    fn store_row(entry: &CatalogEntry, installed: bool) -> RegisteredRow {
        RegisteredRow {
            source: "user".into(),
            text: entry.definition_text.clone(),
            installed,
        }
    }

    fn store_record() -> AgentRecord {
        AgentRecord {
            added: "2026-10-28".into(),
            catalog: "1a2b3c4d".into(),
            ..AgentRecord::default()
        }
    }

    fn program() -> Option<PathBuf> {
        Some(PathBuf::from("qwen.exe"))
    }

    /// The Store's own file, added, approved and found: the `ready` case.
    fn ready_inputs(entry: &CatalogEntry) -> Inputs {
        Inputs {
            registered: Some(store_row(entry, true)),
            approval: Some(ApprovalState::Approved),
            record: Some(store_record()),
            found_path: program(),
            ..bare()
        }
    }

    #[test]
    fn row_available_when_the_program_is_not_on_the_pc_and_not_added() {
        let entry = store_entry("qwen", false);
        let (state, reason) = card_state(&entry, &bare());
        assert_eq!(state, CardState::Available);
        assert!(reason.starts_with("Not on this PC yet."), "{reason}");
    }

    #[test]
    fn row_found_on_pc_when_the_program_is_on_path_and_not_added() {
        let entry = store_entry("qwen", false);
        let inputs = Inputs { found_path: program(), ..bare() };
        let (state, reason) = card_state(&entry, &inputs);
        assert_eq!(state, CardState::FoundOnPc);
        assert_eq!(reason, "The program is on this PC. Add it to Puppet Master to use it.");
    }

    #[test]
    fn row_found_on_pc_asks_for_confirmation_when_the_name_is_ambiguous() {
        let entry = store_entry("qwen", true);
        let inputs = Inputs { found_path: program(), ..bare() };
        let (state, reason) = card_state(&entry, &inputs);
        assert_eq!(state, CardState::FoundOnPc);
        assert!(reason.contains("confirm it before you add it"), "{reason}");
    }

    #[test]
    fn row_ready_when_added_approved_and_found() {
        let entry = store_entry("qwen", false);
        let (state, reason) = card_state(&entry, &ready_inputs(&entry));
        assert_eq!(state, CardState::Ready);
        assert_eq!(reason, "Added and ready. The program is on this PC.");
    }

    #[test]
    fn row_added_no_program_when_added_and_the_program_is_gone() {
        let entry = store_entry("qwen", false);
        let inputs = Inputs {
            found_path: None,
            registered: Some(store_row(&entry, false)),
            ..ready_inputs(&entry)
        };
        let (state, reason) = card_state(&entry, &inputs);
        assert_eq!(state, CardState::AddedNoProgram);
        assert!(reason.contains("not on this PC"), "{reason}");
    }

    #[test]
    fn row_needs_review_when_the_approval_is_stale_or_missing() {
        let entry = store_entry("qwen", false);
        for approval in [ApprovalState::Stale, ApprovalState::Unapproved] {
            let inputs = Inputs { approval: Some(approval), ..ready_inputs(&entry) };
            let (state, reason) = card_state(&entry, &inputs);
            assert_eq!(state, CardState::NeedsReview, "{approval:?}");
            assert!(reason.contains("Settings > Agents"), "{reason}");
        }
    }

    #[test]
    fn row_own_version_when_the_registered_definition_has_the_id_but_no_store_record() {
        let entry = store_entry("qwen", false);
        let inputs = Inputs {
            registered: Some(store_row(&entry, true)),
            approval: Some(ApprovalState::Approved),
            record: None,
            found_path: program(),
            ..bare()
        };
        let (state, reason) = card_state(&entry, &inputs);
        assert_eq!(state, CardState::OwnVersion);
        assert_eq!(reason, "You have your own qwen definition, so the Store leaves it alone.");
    }

    #[test]
    fn own_version_when_the_registered_bytes_differ_from_the_shipped_copy() {
        let entry = store_entry("qwen", false);
        let mut row = store_row(&entry, true);
        row.text = Some(format!("{}# edited by the person\n", definition_toml("qwen")));
        let inputs = Inputs { registered: Some(row), ..ready_inputs(&entry) };
        let (state, _) = card_state(&entry, &inputs);
        assert_eq!(state, CardState::OwnVersion);
    }

    #[test]
    fn own_version_when_the_registered_file_is_missing_its_text() {
        let entry = store_entry("qwen", false);
        let mut row = store_row(&entry, true);
        row.text = None;
        let inputs = Inputs { registered: Some(row), ..ready_inputs(&entry) };
        let (state, _) = card_state(&entry, &inputs);
        assert_eq!(state, CardState::OwnVersion);
    }

    #[test]
    fn row_unsupported_off_windows_whatever_else_is_true() {
        let entry = store_entry("qwen", false);
        let inputs = Inputs { windows: false, ..ready_inputs(&entry) };
        let (state, reason) = card_state(&entry, &inputs);
        assert_eq!(state, CardState::Unsupported);
        assert_eq!(reason, "The Agent Store works on Windows only.");
        let bare_off = Inputs { windows: false, ..bare() };
        assert_eq!(card_state(&entry, &bare_off).0, CardState::Unsupported);
    }

    #[test]
    fn a_built_in_profile_is_added_without_a_store_record() {
        let entry = store_entry("qwen", false);
        let row = |installed| RegisteredRow { source: "code".into(), text: None, installed };
        let ready = Inputs { registered: Some(row(true)), ..bare() };
        assert_eq!(card_state(&entry, &ready).0, CardState::Ready);
        let gone = Inputs { registered: Some(row(false)), ..bare() };
        assert_eq!(card_state(&entry, &gone).0, CardState::AddedNoProgram);
    }

    #[test]
    fn state_names_are_the_ones_the_ui_maps() {
        let names: Vec<String> = [
            CardState::Available,
            CardState::FoundOnPc,
            CardState::Ready,
            CardState::AddedNoProgram,
            CardState::NeedsReview,
            CardState::OwnVersion,
            CardState::Unsupported,
        ]
        .iter()
        .map(|s| serde_json::to_value(s).unwrap().as_str().unwrap().to_string())
        .collect();
        assert_eq!(
            names,
            [
                "available",
                "found_on_pc",
                "ready",
                "added_no_program",
                "needs_review",
                "own_version",
                "unsupported"
            ]
        );
    }

    #[test]
    fn every_reason_is_one_plain_sentence() {
        let entry = store_entry("qwen", true);
        let cases = [
            bare(),
            Inputs { found_path: program(), ..bare() },
            ready_inputs(&entry),
            Inputs {
                found_path: None,
                registered: Some(store_row(&entry, false)),
                ..ready_inputs(&entry)
            },
            Inputs { approval: Some(ApprovalState::Stale), ..ready_inputs(&entry) },
            Inputs { record: None, ..ready_inputs(&entry) },
            Inputs { windows: false, ..bare() },
        ];
        for inputs in cases {
            let (_, reason) = card_state(&entry, &inputs);
            assert!(reason.ends_with('.'), "{reason}");
            assert!(reason.chars().next().is_some_and(|c| c.is_ascii_uppercase()), "{reason}");
            assert!(!reason.contains('`') && !reason.contains('\\'), "{reason}");
            assert!(reason.chars().all(|c| !c.is_control()), "{reason}");
        }
    }

    #[test]
    fn detect_on_an_empty_pc_finds_nothing_and_reads_no_record() {
        let entry = store_entry("as10probe", false);
        let missing = std::env::temp_dir().join(format!("pm-as10-no-record-{}.json", std::process::id()));
        let inputs = detect_at(&entry, &missing);
        assert!(inputs.record.is_none());
        assert_eq!(inputs.windows, cfg!(windows));
        assert_eq!(inputs.approval, None);
        // The probe name is not a program on any PATH, so nothing is found.
        assert!(inputs.found_path.is_none());
        assert_eq!(card_state(&entry, &inputs).0, if cfg!(windows) { CardState::Available } else { CardState::Unsupported });
    }
}
