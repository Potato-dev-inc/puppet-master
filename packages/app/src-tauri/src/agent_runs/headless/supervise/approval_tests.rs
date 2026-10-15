//! The transcript's Approval entries: one per prompt, each with its own text, whoever parked the
//! run on the dialog first (VERIFY-R8 #1: a run with two approvals showed "Create r8-note.txt" twice;
//! a live re-run showed it once and never the `npm test` card).
//!
//! The actions are what the real detector makes of real Claude Code v2.1.299 dialogs.

use super::super::pane_turn::record_approval;
use crate::agent_runs::transcript;
use crate::operations::OperationSnapshot;
use serde_json::Value;

const CREATE: &str = include_str!("../../../tui/claude/fixtures/create_file_2_1_299.txt");
const BASH: &str = include_str!("../../../tui/claude/fixtures/bash_simple_4_options_2_1_299.txt");

fn action_of(screen: &str) -> Value {
    let prompt = crate::pane_prompt::detect_in_screen("claude", screen, false).expect("a dialog");
    crate::pane_prompt::required_action_from_prompt(prompt)
}

fn run() -> OperationSnapshot {
    let project = std::env::temp_dir().join(format!("pm-approval-{}", uuid::Uuid::new_v4()));
    let request = crate::operations::DelegateWorkRequest {
        project_path: project.to_string_lossy().into_owned(),
        task: "Create a file then run npm test".into(),
        agent_type: "claude".into(),
        pane_id: None,
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 600_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("approval".into()),
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    crate::operations::delegate_work(request, |_| Ok(())).unwrap()
}

fn entries(run: &OperationSnapshot) -> Vec<String> {
    transcript::read(&run.project_path, &run.operation_id, 0)
        .unwrap()
        .chunks
        .into_iter()
        .filter(|chunk| chunk.stream == "permission")
        .map(|chunk| chunk.text)
        .collect()
}

#[test]
fn two_approvals_are_two_entries_each_describing_its_own_prompt() {
    let run = run();
    let (create, bash) = (action_of(CREATE), action_of(BASH));
    assert_ne!(create["prompt_id"], bash["prompt_id"]);
    let (create_text, bash_text) = (
        create["summary"].as_str().unwrap().to_string(),
        bash["summary"].as_str().unwrap().to_string(),
    );
    assert!(bash_text.starts_with("Run command: npm test"), "{bash_text}");
    let mut recorded = None;
    // The first dialog; then the supervisor reads it again before Claude redraws (the stale frame
    // that wrote a second "Create ..." entry in the R8 run).
    assert!(record_approval(&run, &create, &mut recorded));
    assert!(!record_approval(&run, &create, &mut recorded), "the same dialog twice is one entry");
    // The second dialog. The pane observer parks the run on it first, so the run is
    // `waiting_input` with this very action by the time the supervisor looks.
    crate::operations::mark_operation_waiting_input(
        &run.project_path,
        &run.operation_id,
        Some("waiting_input".into()),
        bash.clone(),
        crate::operations::StateSource::Inferred,
    )
    .unwrap();
    let parked = crate::operations::get_operation(&run.project_path, &run.operation_id).unwrap();
    assert_eq!(parked.required_action.as_ref(), Some(&bash), "the observer's write is the supervisor's action");
    assert!(record_approval(&parked, &bash, &mut recorded), "it still needs its entry");
    assert!(!record_approval(&parked, &bash, &mut recorded));
    assert_eq!(entries(&run), [create_text, bash_text]);
}

#[test]
fn the_same_command_asked_again_after_its_dialog_went_away_is_a_new_entry() {
    let run = run();
    let bash = action_of(BASH);
    let mut recorded = None;
    assert!(record_approval(&run, &bash, &mut recorded));
    recorded = None; // a tick saw no dialog on screen (the supervisor clears it then)
    assert!(record_approval(&run, &bash, &mut recorded));
    assert_eq!(entries(&run).len(), 2);
}

// The same Bash approval read at two pane widths (real Claude Code 2.1.299 captures of ONE pending
// dialog: the pane at the phone's 52 columns, then at the desktop's 120). VERIFY-R11 #2: answering
// from the phone's Keys panel re-wrapped the pane, the command text broke at different places, and the
// transcript listed the approval once per width change (live: wrapped, wide, wrapped = 3 entries).
const BASH_52: &str = include_str!("../../../tui/claude/fixtures/bash_approval_52col_2_1_299_live.txt");
const BASH_WIDE: &str = include_str!("../../../tui/claude/fixtures/bash_approval_wide_2_1_299_live.txt");

#[test]
fn one_approval_read_at_two_widths_is_one_prompt() {
    let (narrow, wide) = (action_of(BASH_52), action_of(BASH_WIDE));
    assert_ne!(narrow["command"], wide["command"], "the screens really do read the command differently");
    assert_eq!(narrow["prompt_id"], wide["prompt_id"], "{narrow} vs {wide}");
}

#[test]
fn a_pane_re_wrapping_under_one_approval_writes_one_transcript_entry() {
    let run = run();
    let (narrow, wide) = (action_of(BASH_52), action_of(BASH_WIDE));
    let mut recorded = None;
    // The live order of the supervisor's reads while the Keys panel moved the selection: wrapped,
    // wide, wrapped, wide.
    assert!(record_approval(&run, &narrow, &mut recorded));
    for action in [&wide, &narrow, &wide] {
        assert!(!record_approval(&run, action, &mut recorded), "the same approval is not a new entry");
    }
    assert_eq!(entries(&run).len(), 1, "{:?}", entries(&run));
}
