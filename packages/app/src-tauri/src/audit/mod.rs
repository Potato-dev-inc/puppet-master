//! A small, append-only, hash-chained audit log: "who did what, when" for the actions that hand
//! power from a person to an agent or a device.
//!
//! Where: `<app data>/audit/YYYY-MM.jsonl`, one JSON object per line, plus `head.json` (the last
//! sequence number and hash). Each line carries `prev` (the hash of the line before it, the
//! genesis value for the very first line) and `hash` (SHA-256 over `prev` and the line's own
//! content). The chain runs across monthly files.
//!
//! What it proves: `verify` recomputes every hash, so an edited line fails at that line, a rewritten
//! line (re-hashed by the attacker) fails at the next one, a removed line fails on the sequence
//! number, and a removed tail fails against `head.json`. It does not stop someone who can rewrite
//! the whole folder; it makes quiet changes loud. Nothing here is a secret store: commands are
//! recorded as program name, length and a short hash, never as text.
//!
//! What is recorded (`Kind`): approvals (who pressed Allow: desktop, device or an agent session),
//! take-over and consent decisions, pairing, `shell_exec`, startup-screen dismissals, agent merges
//! and changes to the two agent-authority settings. Writing never fails the action it records: a
//! failure is logged and the action goes on.

pub mod chain;
#[cfg(test)]
mod tests;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub use chain::{verify_dir, VerifyReport};

/// `prev` of the very first line.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const HEAD_FILE: &str = "head.json";
/// Longest summary kept per line; a runaway string cannot grow the log.
const MAX_SUMMARY_CHARS: usize = 300;

/// What kind of change a line records. The words are stable: the Settings list filters on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Approval,
    TakeOver,
    Consent,
    Pairing,
    ShellExec,
    GateDismissal,
    Merge,
    Setting,
    /// Team ("Heads-up", `crate::team`): `team.sent`, `team.received`, `team.action`,
    /// `team.joined`, `team.role_changed`, `team.key_rotated`.
    TeamSent,
    TeamReceived,
    TeamAction,
    TeamJoined,
    TeamRoleChanged,
    TeamKeyRotated,
    /// Missions (`crate::missions`): `mission.started`, `mission.stopped`, `mission.rollback`,
    /// `mission.tests_accepted`, `mission.tests_undone`, `mission.handed_off`.
    MissionStarted,
    MissionStopped,
    MissionRollback,
    MissionTestsAccepted,
    MissionTestsUndone,
    MissionHandedOff,
    /// Keep going (`crate::handoff::keep_going`): `keep_going.handed_over`.
    KeepGoing,
    /// Text your agents (Telegram, `crate::channels`): `channel.paired`, `channel.unpaired`,
    /// `channel.on`, `channel.off`, `channel.answer`, `channel.message`, `channel.stranger`,
    /// `channel.error`.
    ChannelPaired,
    ChannelUnpaired,
    ChannelOn,
    ChannelOff,
    ChannelAnswer,
    ChannelMessage,
    ChannelStranger,
    ChannelError,
    /// Assign an issue (`crate::issue_intake`): `issue.asked`, `issue.auto_approved`, `issue.started`,
    /// `issue.pr_opened`, `issue.held`, `issue.thrown_away`, `issue.settings`. The detail carries `event`.
    IssueIntake,
    /// Red to Green (`crate::ci_fix`): `ci.fix`. The detail carries `event` (`watched`, `asked`, `started`,
    /// `asked_push`, `held`, `pushed`, `green`, ...).
    CiFix,
    /// Night Shift (`crate::night_shift`): one kind for the night's own lines (start, task, stop).
    NightShift,
    /// Review Desk (`crate::review_desk`): one `review_desk.action` line per Keep, Throw away or unmark the
    /// person did. The detail holds the action, source, key, file count and tier; never a title or a path.
    ReviewDesk,
    /// Focus (`crate::attention::focus`): one kind for its lines (start, change, end). Counts only.
    Focus,
    /// What broke it (`crate::checkpoints::bisect`): the person undid one agent turn.
    WhatBrokeIt,
    /// Branch from here (`crate::branch_from`): `branch.started`.
    Branch,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approval => "approval",
            Self::TakeOver => "take_over",
            Self::Consent => "consent",
            Self::Pairing => "pairing",
            Self::ShellExec => "shell_exec",
            Self::GateDismissal => "gate_dismissal",
            Self::Merge => "merge",
            Self::Setting => "setting",
            Self::TeamSent => "team.sent",
            Self::TeamReceived => "team.received",
            Self::TeamAction => "team.action",
            Self::TeamJoined => "team.joined",
            Self::TeamRoleChanged => "team.role_changed",
            Self::TeamKeyRotated => "team.key_rotated",
            Self::MissionStarted => "mission.started",
            Self::MissionStopped => "mission.stopped",
            Self::MissionRollback => "mission.rollback",
            Self::MissionTestsAccepted => "mission.tests_accepted",
            Self::MissionTestsUndone => "mission.tests_undone",
            Self::MissionHandedOff => "mission.handed_off",
            Self::KeepGoing => "keep_going.handed_over",
            Self::ChannelPaired => "channel.paired",
            Self::ChannelUnpaired => "channel.unpaired",
            Self::ChannelOn => "channel.on",
            Self::ChannelOff => "channel.off",
            Self::ChannelAnswer => "channel.answer",
            Self::ChannelMessage => "channel.message",
            Self::ChannelStranger => "channel.stranger",
            Self::ChannelError => "channel.error",
            Self::IssueIntake => "issue.intake",
            Self::CiFix => "ci.fix",
            Self::NightShift => "night_shift",
            Self::ReviewDesk => "review_desk.action",
            Self::Focus => "focus",
            Self::WhatBrokeIt => "what_broke_it.undone",
            Self::Branch => "branch.started",
        }
    }
}

/// Who did it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Who {
    /// The person, on this computer.
    Desktop,
    /// A paired phone (its id when known).
    Device(Option<String>),
    /// An agent, through its MCP connection (the connection id).
    Agent(String),
    /// A program on this computer with no MCP connection and no pairing (the plain loopback bridge).
    Local,
    /// A teammate (Team), by the first characters of their member key.
    Teammate(String),
    /// A message channel (Text your agents), by its short name, for example `telegram`. The name is
    /// fixed in code, never typed by the other side.
    Channel(String),
}

impl Who {
    fn parts(&self) -> (&'static str, Option<String>) {
        match self {
            Self::Desktop => ("desktop", None),
            Self::Device(id) => ("device", id.clone()),
            // A shortened connection id: enough to tell two agents apart, not a credential.
            Self::Agent(session) => ("agent", Some(session.chars().take(8).collect())),
            Self::Local => ("local", None),
            Self::Teammate(member) => ("teammate", Some(member.chars().take(8).collect())),
            Self::Channel(name) => ("channel", Some(name.clone())),
        }
    }
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub v: u8,
    pub seq: u64,
    pub ts_ms: u64,
    pub kind: String,
    /// `desktop`, `device` or `agent`.
    pub who: String,
    /// The device id, or the first characters of the agent's connection id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// One plain sentence: what happened.
    pub summary: String,
    /// Ids and facts for a bug report. Never command text.
    #[serde(default)]
    pub detail: Value,
    pub prev: String,
    pub hash: String,
}

/// The hash a line must carry: SHA-256 over `prev`, a newline, and the line serialised with an
/// empty `hash`. Writing and verifying both go through this one function.
pub fn entry_hash(entry: &Entry) -> String {
    let mut body = entry.clone();
    body.hash = String::new();
    let text = serde_json::to_string(&body).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(body.prev.as_bytes());
    hasher.update(b"\n");
    hasher.update(text.as_bytes());
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `2026-10` for a Unix time in milliseconds (UTC), so a file name needs no date crate.
pub fn month_of(ts_ms: u64) -> String {
    let days = (ts_ms / 86_400_000) as i64;
    // Howard Hinnant's days-to-civil algorithm (public domain).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Head {
    seq: u64,
    hash: String,
}

/// The newest line, so the next one can point at it.
#[derive(Debug, Clone)]
struct Tip {
    seq: u64,
    hash: String,
    /// Its time (0 when only the head file was there): month files never go backwards from it.
    ts_ms: u64,
}

/// The lock file every writer holds while it reads the tip and appends (review R12 3.4).
const LOCK_FILE: &str = ".append.lock";

/// One log folder. The app uses `global()`; tests make their own in a temp folder.
pub struct AuditLog {
    dir: PathBuf,
    tip: Mutex<Option<Tip>>,
}

impl AuditLog {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, tip: Mutex::new(None) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Append one line. Returns the entry written, or the reason it could not be.
    pub fn append(
        &self,
        kind: Kind,
        who: &Who,
        summary: &str,
        detail: Value,
        ts_ms: u64,
    ) -> Result<Entry, String> {
        use std::io::Write;
        let mut tip = self.tip.lock();
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("could not create {}: {e}", self.dir.display()))?;
        // Another writer (a second app instance, a second log on the same folder) may have appended
        // since this one last did: hold the folder's lock and read the tip from disk every time.
        let lock_path = self.dir.join(LOCK_FILE);
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| format!("could not open {}: {e}", lock_path.display()))?;
        lock.lock().map_err(|e| format!("could not lock {}: {e}", lock_path.display()))?;
        *tip = Some(chain::load_tip(&self.dir));
        let current = tip.as_ref().expect("tip just loaded");
        let (who_kind, actor) = who.parts();
        let mut entry = Entry {
            v: 1,
            seq: current.seq + 1,
            ts_ms,
            kind: kind.as_str().to_string(),
            who: who_kind.to_string(),
            actor,
            summary: summary.chars().take(MAX_SUMMARY_CHARS).collect(),
            detail,
            prev: current.hash.clone(),
            hash: String::new(),
        };
        entry.hash = entry_hash(&entry);
        let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
        // The file follows the chain, not the clock: a clock that moved back across a month boundary
        // keeps appending to the newest month, so the files read in name order stay in seq order.
        let path = self.dir.join(format!("{}.jsonl", month_of(ts_ms.max(current.ts_ms))));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("could not open {}: {e}", path.display()))?;
        file.write_all(format!("{line}\n").as_bytes()).map_err(|e| e.to_string())?;
        let head = serde_json::to_string(&Head { seq: entry.seq, hash: entry.hash.clone() }).unwrap_or_default();
        // The head file is a convenience for spotting a removed tail; a failure to write it is not
        // fatal. Written whole or not at all (temp file and rename): a torn head file read as
        // missing and silently turned the missing-tail check off.
        let _ = crate::secure_file::write_private(&self.dir.join(HEAD_FILE), &head);
        *tip = Some(Tip { seq: entry.seq, hash: entry.hash.clone(), ts_ms: current.ts_ms.max(ts_ms) });
        drop(lock);
        Ok(entry)
    }

    /// The newest `limit` lines, newest first.
    pub fn recent(&self, limit: usize) -> Vec<Entry> {
        chain::read_recent(&self.dir, limit)
    }

    pub fn verify(&self) -> VerifyReport {
        // Not between a writer's line and its head file: hold the same locks a writer holds, or a
        // check that runs beside an append (many tests record lines at once) reads a half-written log.
        let _tip = self.tip.lock();
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.dir.join(LOCK_FILE)).ok();
        if let Some(lock) = &lock {
            let _ = lock.lock();
        }
        verify_dir(&self.dir)
    }
}

static GLOBAL: Lazy<AuditLog> = Lazy::new(|| AuditLog::new(log_dir()));

fn log_dir() -> PathBuf {
    #[cfg(test)]
    {
        // Tests that go through the app's own recorders write here, never to the real app data.
        std::env::temp_dir().join(format!("pm-audit-test-{}", std::process::id()))
    }
    #[cfg(not(test))]
    {
        crate::app_paths::app_data_dir().join("audit")
    }
}

/// The app's log.
pub fn global() -> &'static AuditLog {
    &GLOBAL
}

/// Record a line in the app's log. Never fails the caller.
pub fn record(kind: Kind, who: Who, summary: &str, detail: Value) {
    if let Err(error) = GLOBAL.append(kind, &who, summary, detail, now_ms()) {
        tracing::warn!(error = %error, kind = kind.as_str(), "audit log write failed");
    }
}

/// A command as the log keeps it: the program name, its length and a short hash, so a line can be
/// matched with the pane's own history without storing text that may hold a token.
pub fn command_fingerprint(command: &str) -> Value {
    let trimmed = command.trim();
    let program = trimmed
        .split_whitespace()
        .next()
        .map(|word| word.rsplit(['/', '\\']).next().unwrap_or(word))
        .unwrap_or("")
        .chars()
        .take(40)
        .collect::<String>();
    let digest = Sha256::digest(trimmed.as_bytes());
    json!({ "program": program, "length": trimmed.chars().count(), "sha256_prefix": hex(&digest[..4]) })
}

/// Device events the pairing store emits (`mobile_pairing::events`) that no call site records
/// itself: a phone's abilities changed (Settings > Phone > Devices). Pairing and removing a phone
/// are recorded where they happen (`bridge/routes/pairing.rs`, `commands/app.rs`), which also know
/// who did it, so they are not recorded twice here. Registered once, when the bridge starts.
pub fn watch_device_events() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        crate::mobile_pairing::events::subscribe(|event| {
            if let crate::mobile_pairing::events::SecurityEvent::DeviceCapsChanged { device_id, caps } = event {
                record(
                    Kind::Pairing,
                    Who::Desktop,
                    "You changed what a paired phone may do",
                    json!({ "device_id": device_id, "caps": caps }),
                );
            }
        });
    });
}

/// The settings change the Settings window made. Called from the Tauri command, with the old and new value.
pub fn record_setting(name: &str, label: &str, from: bool, to: bool) {
    if from == to {
        return;
    }
    let summary = format!("{label}: turned {}", if to { "on" } else { "off" });
    record(Kind::Setting, Who::Desktop, &summary, json!({ "setting": name, "from": from, "to": to }));
}

/// Puppet Master closed a startup screen with the keys its profile declares (an update notice, for
/// example). The screens only a person may pass are never dismissed, so this is the whole list of
/// times Puppet Master pressed a key on its own to get past a dialog.
pub fn note_gate_dismissal(gate_id: &str, what: &str, keys: &str) {
    let shown: Vec<String> = keys
        .chars()
        .map(|c| match c {
            '\r' | '\n' => "Enter".to_string(),
            '\u{1b}' => "Esc".to_string(),
            '\t' => "Tab".to_string(),
            c if c.is_control() => format!("0x{:02x}", c as u32),
            c => c.to_string(),
        })
        .collect();
    record(
        Kind::GateDismissal,
        Who::Local,
        &format!("Puppet Master closed a start-up screen on its own: {what}"),
        json!({ "gate": gate_id, "keys": shown.join(" ") }),
    );
}
