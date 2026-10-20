//! The debug file of a wizard run that stopped (SPEC 11.22): `<the wizard's folder>/wizard-debug.json`.
//!
//! "Why did it stop" used to need the benchmark. Now every stop in the app (a question the wizard may not answer, a menu
//! it could not read, an error row, a failed check) can be followed by a `dump` call from the front end, which writes the
//! last looks of the capture and of the visible test (the masked screen, the rows drawn highlighted, the menu the answerer's
//! parser read with its classification), what the answerer decided, the AI looks Rust acted on, and the stop reason. A
//! lead reads the real screen from it next time.
//!
//! Nothing in it is raw: every text goes through `exec::clean` (secrets, the user's folder name, email addresses become
//! `<email>`), the test words are masked, and the whole file passes the email net once more as it is written.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::super::exec;
use super::super::livetest::mask_words;
use super::super::report::WizardError;
use super::super::session::{Session, TestWords};

/// The file name, in the session's folder and (as a copy that outlives the folder) in the wizard's temp root.
pub const FILE: &str = "wizard-debug.json";
pub const COPY: &str = "pm-wizard-last-debug.json";

/// How long the stop reason may be.
const REASON_CHARS: usize = 600;

/// Every string in `value`, cleaned and masked in place.
fn scrub(value: &mut Value, words: &TestWords) {
    match value {
        Value::String(text) => *text = exec::mask_private(&mask_words(&exec::clean(text), words)),
        Value::Array(list) => list.iter_mut().for_each(|v| scrub(v, words)),
        Value::Object(map) => map.values_mut().for_each(|v| scrub(v, words)),
        _ => {}
    }
}

/// The document, before it is scrubbed: what the capture and the test saw, what was decided, and why it stopped.
pub fn document(session: &Session, capture: Option<Value>, reason: &str) -> Value {
    let test_status = session.visible.test_status.lock().clone();
    json!({
        "what": "Puppet Master's Agent Wizard debug file. Redacted: emails are <email>, secrets are removed, the test words are <word-N>.",
        "reason": exec::cut_chars(reason, REASON_CHARS),
        "session": session.id,
        "command": session.command,
        "autonomous": session.autonomous,
        "auto_answer": session.auto_answer,
        "capture": capture,
        "test": {
            "status": if test_status.is_null() { Value::Null } else { test_status },
            "looks": session.visible.test_looks.lock().iter().cloned().collect::<Vec<_>>(),
            "decisions": session.visible.test_decisions.lock().clone(),
        },
        "ai_looks": session.visible.auto.verdicts.lock().clone(),
    })
}

/// Write the file for `session` and return where. `capture` is the capture's last looks (live or kept).
pub fn write(session: &Session, temp_root: &Path, capture: Option<Value>, reason: &str) -> Result<Value, WizardError> {
    let mut doc = document(session, capture, reason);
    scrub(&mut doc, &session.words);
    let text = serde_json::to_string_pretty(&doc).map_err(|e| WizardError::new(500, "WIZARD_IO", format!("cannot write the debug file: {e}")))?;
    let text = exec::mask_private(&text);
    let primary: PathBuf = session.work_dir.join(FILE);
    std::fs::write(&primary, &text).map_err(|e| WizardError::new(500, "WIZARD_IO", format!("cannot write the debug file: {e}")))?;
    // a copy that stays when the session's folder is removed (the wizard window was closed)
    let copy = temp_root.join(COPY);
    let copied = std::fs::write(&copy, &text).is_ok();
    Ok(json!({"path": primary.to_string_lossy(), "copy": copied.then(|| copy.to_string_lossy().into_owned())}))
}
