//! Current-turn steering receipts, distinct from follow-up turns.

use crate::operations::{OperationError, OperationSnapshot, OperationStatus};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use uuid::Uuid;

pub const QUEUE_WAIT_TIMEOUT_MS: u64 = 300_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    LiveOrQueue,
    Live,
    Queue,
}

impl DeliveryMode {
    pub fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("live_or_queue") {
            "live" => Self::Live,
            "queue" => Self::Queue,
            _ => Self::LiveOrQueue,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    #[serde(alias = "delivered")]
    Accepted,
    Queued,
    Processed,
    Deferred,
    Unsupported,
    Expired,
    Rejected,
}

impl Disposition {
    #[allow(dead_code)]
    pub const ALL: &'static [&'static str] = &[
        "accepted",
        "queued",
        "processed",
        "deferred",
        "unsupported",
        "expired",
        "rejected",
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Queued => "queued",
            Self::Processed => "processed",
            Self::Deferred => "deferred",
            Self::Unsupported => "unsupported",
            Self::Expired => "expired",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptAction {
    AbortNative,
    StopHeadless,
    SignalTui,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageReceipt {
    pub message_id: String,
    pub idempotency_key: String,
    pub disposition: Disposition,
    pub handle: String,
    pub turn_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteerTarget<'a> {
    pub backend: &'a str,
    pub busy: bool,
    pub waiting_input: bool,
    pub has_pane: bool,
    pub can_live_steer: bool,
}

pub fn steer_target(snapshot: &OperationSnapshot) -> SteerTarget<'_> {
    SteerTarget {
        backend: snapshot.agent_type.as_str(),
        busy: !snapshot.status.terminal(),
        waiting_input: snapshot.status == OperationStatus::WaitingInput,
        has_pane: snapshot.pane_id.is_some(),
        can_live_steer: super::capabilities::for_snapshot(snapshot).live_messages
            && snapshot
                .worker
                .provider_session_id
                .as_deref()
                .is_some_and(|id| !id.is_empty()),
    }
}

/// Decide how a current-turn message is handled. Never claims TUI injection succeeded.
pub fn decide_delivery(mode: DeliveryMode, target: SteerTarget<'_>) -> Disposition {
    if !target.busy {
        return match mode {
            DeliveryMode::Queue => Disposition::Queued,
            DeliveryMode::Live | DeliveryMode::LiveOrQueue => Disposition::Rejected,
        };
    }
    if target.waiting_input && !target.can_live_steer {
        return match mode {
            DeliveryMode::Live => Disposition::Unsupported,
            DeliveryMode::Queue | DeliveryMode::LiveOrQueue => Disposition::Deferred,
        };
    }
    if target.can_live_steer {
        return match mode {
            DeliveryMode::Queue => Disposition::Queued,
            DeliveryMode::Live | DeliveryMode::LiveOrQueue => Disposition::Accepted,
        };
    }
    // Busy TUI or headless CLI: do not inject arbitrary text.
    match mode {
        DeliveryMode::Live => Disposition::Unsupported,
        DeliveryMode::Queue | DeliveryMode::LiveOrQueue => Disposition::Queued,
    }
}

pub fn followup_timeout_expired(
    queued: bool,
    created_at_ms: u64,
    started_at_ms: Option<u64>,
    now_ms: u64,
    timeout_ms: u64,
    queue_timeout_ms: u64,
) -> bool {
    if queued {
        now_ms.saturating_sub(created_at_ms) >= queue_timeout_ms
    } else {
        now_ms.saturating_sub(started_at_ms.unwrap_or(created_at_ms)) >= timeout_ms
    }
}

pub fn interrupt_action(snapshot: &OperationSnapshot) -> Option<InterruptAction> {
    if snapshot.status.terminal() {
        return None;
    }
    let caps = super::capabilities::for_snapshot(snapshot);
    if caps.graceful_interrupt {
        return Some(InterruptAction::AbortNative);
    }
    if !caps.visible_terminal {
        return Some(InterruptAction::StopHeadless);
    }
    Some(InterruptAction::SignalTui)
}

pub fn send_message(
    project: &str,
    handle: &str,
    text: &str,
    snapshot: &OperationSnapshot,
    mode: DeliveryMode,
    idempotency_key: Option<&str>,
) -> Result<MessageReceipt, OperationError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(OperationError::new(
            "INVALID_TASK",
            "message is required",
            false,
        ));
    }
    let key = idempotency_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if let Some(existing) = load_receipt(project, handle, &key) {
        return Ok(existing);
    }
    let disposition = decide_delivery(mode, steer_target(snapshot));
    let receipt = MessageReceipt {
        message_id: Uuid::new_v4().to_string(),
        idempotency_key: key,
        disposition,
        handle: handle.to_string(),
        turn_id: format!("turn-{}", snapshot.turn_index),
        text: text.to_string(),
        result: None,
    };
    store_receipt(project, handle, &receipt)?;
    Ok(receipt)
}

pub fn pending_texts(project: &str, handle: &str) -> Vec<String> {
    load_all(project, handle)
        .into_iter()
        .filter(|receipt| {
            matches!(
                receipt.disposition,
                Disposition::Queued | Disposition::Deferred
            )
        })
        .map(|receipt| receipt.text)
        .collect()
}

pub fn receipt_value(receipt: &MessageReceipt) -> serde_json::Value {
    let mut value = json!({
        "message_id": receipt.message_id,
        "idempotency_key": receipt.idempotency_key,
        "disposition": receipt.disposition.as_str(),
        "state": receipt.disposition.as_str(),
        "handle": receipt.handle,
        "turn_id": receipt.turn_id,
        "result": receipt.result,
    });
    if receipt.disposition == Disposition::Rejected {
        value["suggestion"] = json!("turn already finished; use followup_task for the next turn");
    }
    value
}

pub fn expire_terminal_steers(project: &str, handle: &str) {
    expire_matching(project, handle, |receipt| {
        matches!(
            receipt.disposition,
            Disposition::Queued | Disposition::Deferred
        )
    });
}

pub fn expire_unsettled_steers(project: &str, handle: &str) {
    expire_matching(project, handle, |receipt| {
        matches!(
            receipt.disposition,
            Disposition::Accepted | Disposition::Queued | Disposition::Deferred
        )
    });
}

fn expire_matching(project: &str, handle: &str, pred: impl Fn(&MessageReceipt) -> bool) {
    let open: Vec<MessageReceipt> = load_all(project, handle)
        .into_iter()
        .filter(pred)
        .collect();
    for mut receipt in open {
        receipt.disposition = Disposition::Expired;
        let _ = update_receipt(project, handle, &receipt);
    }
}

pub fn latest_steer(project: &str, handle: &str) -> Option<MessageReceipt> {
    load_all(project, handle).into_iter().next_back()
}

pub fn mark_open_steers_processed(project: &str, handle: &str, result: &str) {
    let result = result.trim();
    if result.is_empty() {
        return;
    }
    let open: Vec<MessageReceipt> = load_all(project, handle)
        .into_iter()
        .filter(|receipt| {
            matches!(
                receipt.disposition,
                Disposition::Accepted | Disposition::Queued
            ) && receipt.result.is_none()
        })
        .collect();
    for mut receipt in open {
        receipt.disposition = Disposition::Processed;
        receipt.result = Some(result.to_string());
        let _ = update_receipt(project, handle, &receipt);
    }
}

fn store_key(project: &str, handle: &str) -> String {
    format!("{project}\n{handle}")
}

fn path_for(project: &str, handle: &str) -> PathBuf {
    PathBuf::from(project)
        .join(".puppet-master")
        .join("messages")
        .join(format!("{handle}.json"))
}

fn cache() -> &'static Mutex<HashMap<String, Vec<MessageReceipt>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<MessageReceipt>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_all(project: &str, handle: &str) -> Vec<MessageReceipt> {
    let key = store_key(project, handle);
    if let Some(entries) = cache().lock().get(&key) {
        return entries.clone();
    }
    let loaded: Vec<MessageReceipt> = fs::read_to_string(path_for(project, handle))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default();
    cache().lock().insert(key, loaded.clone());
    loaded
}

fn load_receipt(project: &str, handle: &str, idempotency_key: &str) -> Option<MessageReceipt> {
    load_all(project, handle)
        .into_iter()
        .find(|receipt| receipt.idempotency_key == idempotency_key)
}

pub fn find_receipt_by_key(
    project: &str,
    handle: &str,
    idempotency_key: &str,
) -> Option<MessageReceipt> {
    load_receipt(project, handle, idempotency_key)
}

pub fn update_receipt(
    project: &str,
    handle: &str,
    receipt: &MessageReceipt,
) -> Result<(), OperationError> {
    upsert_receipt(project, handle, receipt, true)
}

fn store_receipt(
    project: &str,
    handle: &str,
    receipt: &MessageReceipt,
) -> Result<(), OperationError> {
    upsert_receipt(project, handle, receipt, false)
}

fn upsert_receipt(
    project: &str,
    handle: &str,
    receipt: &MessageReceipt,
    overwrite: bool,
) -> Result<(), OperationError> {
    let key = store_key(project, handle);
    let mut all = cache().lock();
    let entries = all.entry(key).or_insert_with(Vec::new);
    if let Some(existing) = entries
        .iter_mut()
        .find(|existing| existing.idempotency_key == receipt.idempotency_key)
    {
        if !overwrite {
            return Ok(());
        }
        *existing = receipt.clone();
    } else {
        entries.push(receipt.clone());
    }
    let path = path_for(project, handle);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            OperationError::new("STORAGE_ERROR", error.to_string(), true)
        })?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&*entries).map_err(|error| {
            OperationError::new("STORAGE_ERROR", error.to_string(), true)
        })?,
    )
    .map_err(|error| OperationError::new("STORAGE_ERROR", error.to_string(), true))
}
