//! GEN-1..3: the zero-config generic mode.

use crate::tui::declarative::build_for_test;
use crate::tui::types::SpawnRequest;
use crate::tui::TuiProfile;

const THREE_LINES: &str = r#"
schema_version = 1
[agent]
id = "mycli"
label = "My CLI"
kind = "generic"
experimental = true
[binary]
names = ["mycli-fixture-not-installed"]
"#;

#[test]
fn three_line_definition() {
    let p = build_for_test(THREE_LINES).expect("a generic definition needs no [tui] or [headless]");
    assert!(p.experimental(), "GEN-3");
    let f = p.features();
    assert!(f.dispatchable, "experimental generic agents are dispatchable");
    assert!(!f.read_only_headless && !f.resumable_headless);
    // visible-only: no headless command, no prompt-delivery verification
    assert!(p.headless_command(&crate::tui::types::HeadlessLaunch::new(".", "t")).is_err());
    assert!(!p.verifies_prompt_delivery());
    // shared heuristics answer: nothing is claimed for an empty screen
    assert_eq!(p.busy_marker(""), None);
    assert!(p.detect_prompt("", false).is_none());
    let _ = SpawnRequest { cwd: ".", extra_args: &[], cols: 120, rows: 30, read_only: false };
}

#[test]
fn requires_experimental_for_dispatch() {
    let text = THREE_LINES.replace("experimental = true", "");
    let p = build_for_test(&text).expect("valid");
    assert!(!p.features().dispatchable, "AGENT_NOT_DISPATCHABLE without experimental = true");
    assert!(p.not_dispatchable_reason().contains("experimental"));
    assert!(p.experimental(), "still labelled experimental");
}

#[test]
fn generic_with_a_headless_table_is_rejected() {
    let text = format!("{THREE_LINES}\n[headless]\nargv = [\"run\"]\n[headless.output]\nformat = \"plain_text\"\n");
    let err = build_for_test(&text).err().expect("rejected");
    assert!(err.contains("AGENT_DEF_INCONSISTENT headless"), "{err}");
}

#[test]
fn a_cli_definition_still_needs_a_table_and_is_not_experimental() {
    let text = THREE_LINES.replace("kind = \"generic\"", "").replace("experimental = true", "");
    assert!(build_for_test(&text).err().expect("rejected").contains("AGENT_DEF_INCONSISTENT"));
    let ok = build_for_test(&format!("{}\n[tui]\nargv = []\n", text)).unwrap();
    assert!(!ok.experimental());
}

#[test]
fn descriptor_and_receipt_carry_the_experimental_flag() {
    let p = build_for_test(THREE_LINES).unwrap();
    assert!(crate::tui::types::Descriptor::of(&p).experimental);
}

#[test]
fn add_generic_writes_an_unapproved_valid_file() {
    use crate::tui::declarative::loader::{load_dir, write_generic};
    use crate::tui::declarative::sources::Origin;
    use crate::tui::declarative::validate::Source;
    let dir = std::env::temp_dir().join(format!("pm-generic-{}", std::process::id()));
    let path = write_generic(&dir, "MyCoder.exe", &["claude", "codex"]).unwrap();
    assert_eq!(path.file_name().unwrap(), "mycoder.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("kind = \"generic\"") && text.contains("experimental = true"));
    // valid, and it carries no approval: load_dir alone never approves
    let loaded = load_dir(&dir, Origin::new(Source::User), None, &["claude", "codex"]);
    assert!(loaded.files.iter().all(|f| f.errors.is_empty()), "{:?}", loaded.files.iter().map(|f| &f.errors).collect::<Vec<_>>());
    let approvals = crate::tui::declarative::approvals::Approvals::load(&dir.join("no-approvals.json"));
    let fingerprint = loaded.files[0].fingerprint.clone().expect("fingerprint");
    assert!(
        approvals.state(&path, &fingerprint) == crate::tui::declarative::approvals::ApprovalState::Unapproved,
        "a new generic agent must not be auto-approved"
    );
    // a second add, a path-like name, an unsafe name and a reserved id are refused
    assert!(write_generic(&dir, "mycoder", &["claude"]).unwrap_err().contains("AGENT_DEF_DUPLICATE_ID"));
    assert!(write_generic(&dir, r"..\evil", &["claude"]).is_err());
    assert!(write_generic(&dir, "a b\"; x", &["claude"]).is_err());
    assert!(write_generic(&dir, "claude", &["claude"]).unwrap_err().contains("AGENT_DEF_RESERVED_ID"));
    let _ = std::fs::remove_file(&path);
}
