//! `headless_verified`: may a lead trust this agent's one-task (headless) mode?
//!
//! True only when ALL of these hold for a custom agent:
//! * its definition is the one in force (approved and its program unchanged since approval),
//! * the definition has a `[headless]` table with an output parser,
//! * `<id>.wizard.json` (written by the Agent Wizard) records a live test whose stages `pong` and
//!   `sentence` passed (and none failed), and
//! * the wizard's `definition_sha256` is the SHA-256 of the definition file as it is now, so the
//!   file that was tested is the file that would run. Any later edit, even to a label, lapses it;
//!   a hand-written definition has no test and is never verified.
//!
//! Built-in agents are not judged here: they keep the headless support they declare
//! (`headless_supported`) and report `headless_verified: false` with reason `built_in`.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The verdict and, when not verified, the short reason code (stable, for tools and the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verification {
    pub verified: bool,
    pub reason: Option<&'static str>,
}

impl Verification {
    pub const fn yes() -> Self {
        Self { verified: true, reason: None }
    }

    pub const fn no(reason: &'static str) -> Self {
        Self { verified: false, reason: Some(reason) }
    }

    pub const fn built_in() -> Self {
        Self::no("built_in")
    }

    /// One plain sentence for the Settings window and error messages.
    pub fn explain(&self) -> &'static str {
        match self.reason {
            None => "Verified: the wizard's one-task test passed for this exact definition.",
            Some("no_headless_mode") => "This agent has no one-task (headless) mode.",
            Some("not_approved") => "Approve this agent's definition first.",
            Some("not_tested") => "The wizard's one-task test has not passed for this agent.",
            Some("changed_since_test") => "The definition changed after the wizard tested it. Run the wizard test again.",
            Some("built_in") => "Built-in agents keep their own headless support.",
            Some(_) => "One-task mode is not verified for this agent.",
        }
    }
}

/// What the verdict is computed from; all of it plain data so tests need no app state.
pub struct Inputs<'a> {
    /// The definition is registered and spawnable, with its program unchanged since approval.
    pub in_force: bool,
    /// The definition file as it is on disk now.
    pub file_bytes: Option<&'a [u8]>,
    /// `<id>.wizard.json` as it is on disk now.
    pub wizard_json: Option<&'a str>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn evaluate(inputs: &Inputs<'_>) -> Verification {
    if !inputs.in_force {
        return Verification::no("not_approved");
    }
    let Some(bytes) = inputs.file_bytes else {
        return Verification::no("not_approved");
    };
    let def = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| crate::tui::declarative::schema::parse_definition(text).ok());
    let Some(def) = def else {
        return Verification::no("not_approved");
    };
    if !def.headless.as_ref().is_some_and(|h| h.output.is_some()) {
        return Verification::no("no_headless_mode");
    }
    let Some(wizard) = inputs.wizard_json.and_then(|text| serde_json::from_str::<Value>(text).ok()) else {
        return Verification::no("not_tested");
    };
    if !stages_passed(&wizard) {
        return Verification::no("not_tested");
    }
    if wizard.get("id").and_then(Value::as_str).is_some_and(|id| !id.eq_ignore_ascii_case(&def.agent.id)) {
        return Verification::no("not_tested");
    }
    let tested = wizard.get("definition_sha256").and_then(Value::as_str).unwrap_or("");
    if tested.is_empty() || !tested.eq_ignore_ascii_case(&sha256_hex(bytes)) {
        return Verification::no("changed_since_test");
    }
    Verification::yes()
}

/// The one-task live test: `pong` and `sentence` passed and no stage failed.
fn stages_passed(wizard: &Value) -> bool {
    let Some(stages) = wizard.pointer("/results/stages").and_then(Value::as_array) else {
        return false;
    };
    let status = |name: &str| {
        stages
            .iter()
            .find(|s| s.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|s| s.get("status").and_then(Value::as_str))
    };
    status("pong") == Some("passed")
        && status("sentence") == Some("passed")
        && !stages.iter().any(|s| s.get("status").and_then(Value::as_str) == Some("failed"))
}

/// The verdict for a registered custom agent, from the files on disk right now.
pub fn verification_for(id: &str) -> Verification {
    #[cfg(test)]
    {
        if let Some(verified) = super::store::test_verified(id) {
            return if verified { Verification::yes() } else { Verification::no("not_tested") };
        }
    }
    #[cfg(feature = "dev-fake-tui")]
    {
        if let Some(verified) = e2e::forced(id) {
            return if verified { Verification::yes() } else { Verification::no("not_tested") };
        }
    }
    use crate::tui::declarative::loader::{last_entry_for, DefState};
    let Some(entry) = last_entry_for(id) else {
        return Verification::no("not_approved");
    };
    let in_force = entry.state == DefState::Active && !entry.binary_changed;
    let file_bytes = std::fs::read(&entry.path).ok();
    let stem = entry.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let wizard_json = std::fs::read_to_string(entry.path.with_file_name(format!("{stem}.wizard.json"))).ok();
    evaluate(&Inputs { in_force, file_bytes: file_bytes.as_deref(), wizard_json: wizard_json.as_deref() })
}

/// End-to-end tests (`dev-fake-tui`) run fake agents that have no definition file on disk; they
/// decide the verdict here instead. Never compiled into a release build.
#[cfg(feature = "dev-fake-tui")]
pub mod e2e {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    static FORCED: Mutex<BTreeMap<String, bool>> = Mutex::new(BTreeMap::new());

    /// Force the verdict for `id` (`None` removes it again).
    pub fn force(id: &str, verified: Option<bool>) {
        let mut forced = FORCED.lock().unwrap_or_else(|e| e.into_inner());
        match verified {
            Some(value) => forced.insert(id.to_ascii_lowercase(), value),
            None => forced.remove(&id.to_ascii_lowercase()),
        };
    }

    pub(super) fn forced(id: &str) -> Option<bool> {
        FORCED.lock().unwrap_or_else(|e| e.into_inner()).get(&id.to_ascii_lowercase()).copied()
    }
}
